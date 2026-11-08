import { computed, ref } from 'vue'
import { getIPFromIpipnet, getIPFromIpsb } from '@/api/geoip'
import {
  getWechatLatency,
  getBilibiliLatency,
  getGithubLatency,
  getCloudflareLatency,
  getYoutubeLatency,
} from '@/api/latency'

export type CheckStatus = 'idle' | 'scheduled' | 'pending' | 'done' | 'error'

export interface IPResult {
  status: CheckStatus
  ip: string
  location: string[]
  error: string
}

export interface LatencyResult {
  status: CheckStatus
  ms: number
  error: string
}

export const IP_SOURCES = {
  domestic: async () => {
    const res = await getIPFromIpipnet()
    return { ip: res.ip, location: res.location.filter(Boolean) }
  },
  abroad: async () => {
    const res = await getIPFromIpsb()
    return { ip: res.ip ?? '', location: [res.country, res.organization].filter(Boolean) as string[] }
  },
}

export const LATENCY_SITES = {
  wechat: getWechatLatency,
  bilibili: getBilibiliLatency,
  github: getGithubLatency,
  cloudflare: getCloudflareLatency,
  youtube: getYoutubeLatency,
}

export type IPKey = keyof typeof IP_SOURCES
export type LatencyKey = keyof typeof LATENCY_SITES

export const AUTO_CHECK_DELAY_MS = 3000

const ipKeys = Object.keys(IP_SOURCES) as IPKey[]
const latencyKeys = Object.keys(LATENCY_SITES) as LatencyKey[]

const emptyIP = (status: CheckStatus = 'idle'): IPResult => ({ status, ip: '', location: [], error: '' })
const emptyLatency = (status: CheckStatus = 'idle'): LatencyResult => ({ status, ms: -1, error: '' })

function fill<K extends string, T>(keys: K[], make: () => T): Record<K, T> {
  return Object.fromEntries(keys.map((key) => [key, make()])) as Record<K, T>
}

const ip = ref(fill(ipKeys, () => emptyIP()))
const latency = ref(fill(latencyKeys, () => emptyLatency()))

let ipGeneration = 0
let latencyGeneration = 0
let autoTimer: ReturnType<typeof setTimeout> | null = null

const ipBusy = computed(() => ipKeys.some((key) => ip.value[key].status === 'pending'))
const latencyBusy = computed(() => latencyKeys.some((key) => latency.value[key].status === 'pending'))

function describeError(error: unknown): string {
  if (error instanceof SyntaxError) return '返回内容无法识别'
  const text = String((error as { message?: unknown })?.message ?? error ?? '').trim()
  return text || '未知错误'
}

function runScheduledNow(): Promise<void> | null {
  if (!autoTimer) return null
  clearTimeout(autoTimer)
  autoTimer = null
  return checkAll()
}

function checkIP(): Promise<void> {
  const scheduled = runScheduledNow()
  if (scheduled) return scheduled
  if (ipBusy.value) return Promise.resolve()
  const generation = ++ipGeneration
  return Promise.all(ipKeys.map(async (key) => {
    ip.value[key] = emptyIP('pending')
    try {
      const result = await IP_SOURCES[key]()
      if (generation !== ipGeneration) return
      ip.value[key] = result.ip
        ? { status: 'done', ip: result.ip, location: result.location, error: '' }
        : { ...emptyIP('error'), error: '返回内容里没有 IP' }
    } catch (error) {
      if (generation !== ipGeneration) return
      ip.value[key] = { ...emptyIP('error'), error: describeError(error) }
    }
  })).then(() => {})
}

function checkLatency(): Promise<void> {
  const scheduled = runScheduledNow()
  if (scheduled) return scheduled
  if (latencyBusy.value) return Promise.resolve()
  const generation = ++latencyGeneration
  return Promise.all(latencyKeys.map(async (key) => {
    latency.value[key] = emptyLatency('pending')
    try {
      const ms = Math.max(1, Math.round(await LATENCY_SITES[key]()))
      if (generation !== latencyGeneration) return
      latency.value[key] = { status: 'done', ms, error: '' }
    } catch (error) {
      if (generation !== latencyGeneration) return
      latency.value[key] = { status: 'error', ms: 0, error: describeError(error) }
    }
  })).then(() => {})
}

function checkAll(): Promise<void> {
  return Promise.all([checkIP(), checkLatency()]).then(() => {})
}

function reset() {
  if (autoTimer) clearTimeout(autoTimer)
  autoTimer = null
  ipGeneration++
  latencyGeneration++
  ip.value = fill(ipKeys, () => emptyIP())
  latency.value = fill(latencyKeys, () => emptyLatency())
}

function scheduleAutoCheck(delay = AUTO_CHECK_DELAY_MS) {
  reset()
  ip.value = fill(ipKeys, () => emptyIP('scheduled'))
  latency.value = fill(latencyKeys, () => emptyLatency('scheduled'))
  autoTimer = setTimeout(() => {
    autoTimer = null
    void checkAll()
  }, delay)
}

export function useNetworkStore() {
  return {
    ip,
    latency,
    ipBusy,
    latencyBusy,
    checkIP,
    checkLatency,
    checkAll,
    reset,
    scheduleAutoCheck,
  }
}
