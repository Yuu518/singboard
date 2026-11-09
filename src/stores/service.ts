import { ref, computed, onUnmounted, watch } from 'vue'
import { queryServiceStatus, syncServiceComponent, isElevationCancelled } from '@/bridge/service'
import { fetchVersion } from '@/api'
import { rootReady, useConfigStore } from '@/stores/config'
import { appVisible } from '@/stores/appVisible'
import { IS_ANDROID } from '@/platform'
import type { ServiceStatus } from '@/types'

const DESKTOP_POLL_MS = 1000
const ROOT_POLL_MS = 2000
const REMOTE_POLL_MS = 3000
const API_RETRY_MS = 500

export type ClashApiState = 'down' | 'waiting' | 'ready' | 'auth_failed'
export type ClashApiOutcome = Exclude<ClashApiState, 'waiting'>

export const serviceStatus = ref<ServiceStatus>({ state: 'unknown' })
export const clashApiState = ref<ClashApiState>('down')
export const clashApiReady = computed(() => clashApiState.value === 'ready')

const CPU_POINTS = 60
const initCpuHistory = () => new Array(CPU_POINTS).fill(0).map((_, i) => ({ name: i, value: 0 }))
export const cpuHistory = ref<{ name: number; value: number }[]>(initCpuHistory())

function recordCpu(status: ServiceStatus) {
  if (typeof status.cpuPercent !== 'number') return
  cpuHistory.value = [...cpuHistory.value, { name: Date.now(), value: status.cpuPercent }].slice(-CPU_POINTS)
}

let pollTimer: ReturnType<typeof setTimeout> | null = null
let refCount = 0
let componentSynced = false
let polling = false
let pollGeneration = 0

const { config, clashApiEndpoint } = useConfigStore()

let firstPollResolve: (() => void) | null = null
const firstPollReady = new Promise<void>((resolve) => { firstPollResolve = resolve })

function isUnauthorized(error: unknown): boolean {
  return (error as { response?: { status?: number } } | null)?.response?.status === 401
}

async function probeRemote(): Promise<ServiceStatus> {
  try {
    await fetchVersion()
    return { state: 'running' }
  } catch (error) {
    return { state: isUnauthorized(error) ? 'running' : 'stopped' }
  }
}

function usesRemoteProbe(): boolean {
  return IS_ANDROID && !config.value.rootMode
}

async function poll() {
  if (polling) return
  polling = true
  const generation = pollGeneration
  try {
    await rootReady
    const next = usesRemoteProbe() ? await probeRemote() : await queryServiceStatus()
    if (generation === pollGeneration) {
      serviceStatus.value = next
      recordCpu(next)
      syncClashApi()
    }
  } catch {
    if (generation === pollGeneration) {
      serviceStatus.value = { state: 'unknown' }
      syncClashApi()
    }
  } finally {
    polling = false
    if (firstPollResolve) {
      firstPollResolve()
      firstPollResolve = null
    }
  }
}

function pollDelay(): number {
  if (!IS_ANDROID) return DESKTOP_POLL_MS
  return config.value.rootMode ? ROOT_POLL_MS : REMOTE_POLL_MS
}

function schedule() {
  if (pollTimer) clearTimeout(pollTimer)
  pollTimer = null
  if (refCount === 0) return
  pollTimer = setTimeout(async () => {
    if (!IS_ANDROID || appVisible.value) await poll()
    schedule()
  }, pollDelay())
}

function syncComponentOnce() {
  if (componentSynced) return
  if (!IS_ANDROID) {
    componentSynced = true
    // The backend shares this result across the main and tray WebViews,
    // including cancellation, so startup requests never duplicate UAC prompts.
    void syncServiceComponent()
      .catch((e) => {
        if (!isElevationCancelled(e)) console.warn('[service] 服务组件同步失败:', e)
      })
      .finally(poll)
    return
  }
  if (!config.value.rootMode) return
  componentSynced = true
  void rootReady
    .then(() => syncServiceComponent())
    .catch((e) => console.warn('[service] 服务脚本同步失败:', e))
}

async function refresh() {
  pollGeneration++
  polling = false
  await poll()
}

let apiGeneration = 0
let apiInstance: string | null = null
let apiSuspended = false

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function untilVisible(): Promise<void> {
  if (appVisible.value) return Promise.resolve()
  return new Promise((resolve) => {
    const stop = watch(appVisible, (visible) => {
      if (!visible) return
      stop()
      resolve()
    })
  })
}

async function waitClashApi(generation: number) {
  clashApiState.value = 'waiting'
  while (generation === apiGeneration) {
    await untilVisible()
    if (generation !== apiGeneration) return
    try {
      await fetchVersion()
      if (generation === apiGeneration) clashApiState.value = 'ready'
      return
    } catch (error) {
      if (generation !== apiGeneration) return
      if (isUnauthorized(error)) {
        clashApiState.value = 'auth_failed'
        return
      }
    }
    await sleep(API_RETRY_MS)
  }
}

function resetClashApi() {
  apiGeneration++
  apiInstance = null
  clashApiState.value = 'down'
}

function syncClashApi() {
  if (!IS_ANDROID || apiSuspended) return
  const status = serviceStatus.value
  if (status.state !== 'running') {
    if (apiInstance !== null || clashApiState.value !== 'down') resetClashApi()
    return
  }
  const instance = String(status.pid ?? '')
  if (instance === apiInstance) return
  apiInstance = instance
  void waitClashApi(++apiGeneration)
}

export function suspendClashApi() {
  apiSuspended = true
  resetClashApi()
}

export function resumeClashApi() {
  apiSuspended = false
  syncClashApi()
}

export function retryClashApi() {
  if (apiSuspended) return
  apiInstance = null
  syncClashApi()
}

export function whenClashApiSettled(): Promise<ClashApiOutcome> {
  if (clashApiState.value !== 'waiting') return Promise.resolve(clashApiState.value)
  return new Promise((resolve) => {
    const stop = watch(clashApiState, (state) => {
      if (state === 'waiting') return
      stop()
      resolve(state)
    })
  })
}

if (IS_ANDROID) {
  watch(
    () => config.value.rootMode,
    () => {
      serviceStatus.value = { state: 'unknown' }
      cpuHistory.value = initCpuHistory()
      syncComponentOnce()
      void refresh()
    },
  )

  watch([() => serviceStatus.value.state, () => serviceStatus.value.pid], syncClashApi)

  watch(clashApiEndpoint, () => {
    if (!config.value.rootMode) void refresh()
    retryClashApi()
  })

  watch(appVisible, (visible) => {
    if (visible && refCount > 0) void poll()
  })
}

// 估算核心本轮启动时间(毫秒时间戳);未在运行或无 uptime 信息时返回 null
export function getCoreStartTimestamp(): number | null {
  const status = serviceStatus.value
  if (status.state !== 'running' || typeof status.uptimeSeconds !== 'number') return null
  return Date.now() - status.uptimeSeconds * 1000
}

function stateLabels(): Record<string, string> {
  if (!IS_ANDROID) {
    return {
      running: '运行中',
      stopped: '已停止',
      starting: '启动中',
      stopping: '停止中',
      not_installed: '未安装',
      unknown: '未知',
    }
  }
  const rootMode = config.value.rootMode
  return {
    running: rootMode ? '运行中' : '已连接',
    stopped: rootMode ? '已停止' : '未连接',
    starting: '启动中',
    stopping: '停止中',
    not_installed: '未启动',
    unknown: rootMode ? '未知' : '检测中',
  }
}

export function useServiceStore() {
  if (refCount === 0) {
    syncComponentOnce()
    void poll()
  }
  refCount++
  if (refCount === 1) schedule()

  onUnmounted(() => {
    refCount--
    if (refCount === 0 && pollTimer) {
      clearTimeout(pollTimer)
      pollTimer = null
    }
  })

  const statusText = computed(() => stateLabels()[serviceStatus.value.state] || '未知')

  return {
    serviceStatus,
    statusText,
    clashApiState,
    clashApiReady,
    ready: firstPollReady,
    refresh,
  }
}
