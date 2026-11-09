import { ref, onUnmounted, watch } from 'vue'
import { createClashWS } from '@/api/websocket'
import { useConfigStore } from './config'
import { appVisible } from './appVisible'
import { clashApiReady } from './service'
import { IS_ANDROID } from '@/platform'
import type { TrafficData, MemoryData } from '@/types'
import type ReconnectingWebSocket from 'reconnecting-websocket'

export interface HistoryPoint {
  name: number
  value: number | null
}

export type OverviewSocketState = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'failed'

export type OverviewLinkState = 'stopped' | 'connecting' | 'live' | 'unreachable' | 'invalid'

export interface SocketHealth {
  state: string
  failures: number
}

export const UNREACHABLE_AFTER_FAILURES = 3

export function overviewLinkState(running: boolean, traffic: SocketHealth, connections: SocketHealth): OverviewLinkState {
  if (!running) return 'stopped'
  if (traffic.state === 'failed' || connections.state === 'failed') return 'invalid'
  if (traffic.state === 'open' && connections.state === 'open') return 'live'
  if (Math.max(traffic.failures, connections.failures) >= UNREACHABLE_AFTER_FAILURES) return 'unreachable'
  return 'connecting'
}

const MAX_POINTS = 60
const initHistory = (): HistoryPoint[] =>
  new Array(MAX_POINTS).fill(0).map((_, i) => ({ name: i, value: null }))

export const uploadSpeedHistory = ref<HistoryPoint[]>(initHistory())
export const downloadSpeedHistory = ref<HistoryPoint[]>(initHistory())
export const memoryHistory = ref<HistoryPoint[]>(initHistory())
export const connectionsHistory = ref<HistoryPoint[]>(initHistory())

const currentTraffic = ref<TrafficData | null>(null)
const memory = ref<MemoryData | null>(null)
const socketState = ref<OverviewSocketState>('idle')
const socketFailures = ref(0)

let trafficWs: ReconnectingWebSocket | null = null
let memoryWs: ReconnectingWebSocket | null = null
let refCount = 0
let handledApiKey = ''
let coreSuspended = false

function append(history: typeof uploadSpeedHistory, value: number) {
  history.value = [...history.value, { name: Date.now(), value }].slice(-MAX_POINTS)
}

export function pushConnectionCount(count: number) {
  append(connectionsHistory, count)
}

function markOpen() {
  socketState.value = 'open'
  socketFailures.value = 0
}

function closeSockets() {
  const traffic = trafficWs
  const mem = memoryWs
  trafficWs = null
  memoryWs = null
  traffic?.close()
  mem?.close()
}

function start() {
  if (trafficWs || memoryWs || coreSuspended || (IS_ANDROID && !clashApiReady.value)) return
  socketState.value = 'connecting'
  socketFailures.value = 0
  try {
    const traffic = createClashWS('/traffic', (data: TrafficData) => {
      if (trafficWs !== traffic) return
      markOpen()
      const up = Number(data?.up) || 0
      const down = Number(data?.down) || 0
      currentTraffic.value = { up, down }
      append(uploadSpeedHistory, up)
      append(downloadSpeedHistory, down)
    }, undefined, { latestOnly: true })
    trafficWs = traffic
    traffic.addEventListener('open', () => {
      if (trafficWs === traffic) markOpen()
    })
    traffic.addEventListener('close', () => {
      if (trafficWs !== traffic) return
      socketFailures.value++
      socketState.value = 'reconnecting'
      currentTraffic.value = null
      memory.value = null
    })

    let awaitingFirstSample = true
    const mem = createClashWS('/memory', (data: MemoryData) => {
      if (memoryWs !== mem) return
      const inuse = Number(data?.inuse) || 0
      if (awaitingFirstSample) {
        awaitingFirstSample = false
        if (inuse === 0) return
      }
      memory.value = { inuse, oslimit: Number(data?.oslimit) || 0 }
      append(memoryHistory, inuse)
    }, undefined, { latestOnly: true })
    memoryWs = mem
    mem.addEventListener('open', () => {
      awaitingFirstSample = true
    })
    mem.addEventListener('close', () => {
      if (memoryWs === mem) memory.value = null
    })
  } catch (error) {
    console.error('Failed to connect to the traffic stream:', error)
    closeSockets()
    socketState.value = 'failed'
    currentTraffic.value = null
    memory.value = null
  }
}

function stop() {
  closeSockets()
  socketState.value = 'idle'
  socketFailures.value = 0
  currentTraffic.value = null
  memory.value = null
}

function reset() {
  uploadSpeedHistory.value = initHistory()
  downloadSpeedHistory.value = initHistory()
  memoryHistory.value = initHistory()
  connectionsHistory.value = initHistory()
  currentTraffic.value = null
  memory.value = null
}

export function suspendOverview() {
  coreSuspended = true
  stop()
}

export function resumeOverview() {
  coreSuspended = false
  if (refCount > 0 && appVisible.value) start()
}

export function resetOverview() {
  reset()
}

export function useOverviewStore() {
  const { activeClashApiId, clashApiUrl, clashApiSecret } = useConfigStore()

  refCount++
  const unwatchApi = watch(
    () => [activeClashApiId.value, clashApiUrl.value, clashApiSecret.value] as const,
    (api) => {
      const key = api.join('\n')
      if (key === handledApiKey) return
      handledApiKey = key
      const active = !!(trafficWs || memoryWs) || socketState.value === 'failed'
      stop()
      reset()
      if (active) start()
    },
  )
  const unwatchVisible = watch(appVisible, (visible) => {
    if (visible) {
      if (!trafficWs && !memoryWs && refCount > 0) start()
    } else {
      if (trafficWs || memoryWs) stop()
      reset()
    }
  })
  const unwatchReady = IS_ANDROID
    ? watch(clashApiReady, (ready) => {
      if (!ready) stop()
      else if (appVisible.value && refCount > 0) start()
    })
    : () => {}
  onUnmounted(() => {
    unwatchApi()
    unwatchVisible()
    unwatchReady()
    refCount--
    if (refCount === 0) stop()
  })

  return {
    uploadSpeedHistory,
    downloadSpeedHistory,
    memoryHistory,
    connectionsHistory,
    currentTraffic,
    memory,
    socketState,
    socketFailures,
    start,
    stop,
    reset,
  }
}
