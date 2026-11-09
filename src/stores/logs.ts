import { ref, shallowRef, computed, onUnmounted, watch } from 'vue'
import { createClashWS } from '@/api/websocket'
import { fetchConfig } from '@/api'
import { useConfigStore } from './config'
import { appVisible } from './appVisible'
import { clashApiReady, serviceStatus } from './service'
import { IS_ANDROID } from '@/platform'
import type { LogEntry } from '@/types'
import type ReconnectingWebSocket from 'reconnecting-websocket'

const MAX_LOGS = 5000
const FLUSH_INTERVAL = 100

export const LEVEL_RANK: Record<string, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  warning: 3,
  error: 4,
  fatal: 5,
  panic: 6,
}

export const LOG_LEVELS = ['panic', 'fatal', 'error', 'warn', 'info', 'debug', 'trace']
const DEFAULT_CORE_LEVEL = 'info'

export function filterLevelFor(level: string): string {
  const normalized = level.toLowerCase()
  if (normalized === 'warning') return 'warn'
  return LOG_LEVELS.includes(normalized) ? normalized : DEFAULT_CORE_LEVEL
}

const logs = shallowRef<LogEntry[]>([])
const chosenLevel = ref('')
const coreLevel = ref('')
const coreFilterLevel = computed(() => coreLevel.value || DEFAULT_CORE_LEVEL)
const levelFilter = computed(() => chosenLevel.value || coreFilterLevel.value)
const levelChoice = computed({
  get: () => chosenLevel.value,
  set: (choice: string) => {
    const previous = levelFilter.value
    chosenLevel.value = choice
    if (levelFilter.value !== previous) resubscribe(levelFilter.value)
  },
})
const paused = ref(false)
const pausedLogs = shallowRef<LogEntry[]>([])
const filterText = ref('')
const socketState = ref<'idle' | 'connecting' | 'reconnecting' | 'open' | 'failed'>('idle')
const socketFailures = ref(0)

let ws: ReconnectingWebSocket | null = null
let nextLogSequence = 1
let connectionGeneration = 0
let lifecycleOwners = 0
let pendingLogs: LogEntry[] = []
let flushTimer: ReturnType<typeof setTimeout> | null = null
let pausedSeq = 0
let gap: { since: number; reason: 'hidden' | 'socket' } | null = null

function lastSeq(entries: LogEntry[]): number {
  return entries.length ? entries[entries.length - 1].seq ?? 0 : 0
}

const shownLogs = computed(() => (paused.value ? pausedLogs.value : logs.value))

const filteredLogs = computed(() => {
  const min = LEVEL_RANK[levelFilter.value] ?? 0
  const q = filterText.value.trim().toLowerCase()
  if (!q && min === 0) return shownLogs.value
  const kept = shownLogs.value.filter((l) => {
    if (l.type === 'marker') return !q
    if ((LEVEL_RANK[l.type.toLowerCase()] ?? min) < min) return false
    return !q || l.payload.toLowerCase().includes(q)
  })
  return kept.some((l) => l.type !== 'marker') ? kept : []
})

const newSincePause = computed(() => (paused.value ? Math.max(0, lastSeq(logs.value) - pausedSeq) : 0))

function clockTime(at: number): string {
  return new Date(at).toTimeString().slice(0, 8)
}

function flushLogs(): void {
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = null
  if (pendingLogs.length === 0) return
  const merged = logs.value.concat(pendingLogs)
  pendingLogs = []
  logs.value = merged.length > MAX_LOGS ? merged.slice(-MAX_LOGS) : merged
}

function enqueueLog(entry: LogEntry): void {
  entry.time = clockTime(Date.now())
  entry.seq = nextLogSequence++
  pendingLogs.push(entry)
  if (!flushTimer) flushTimer = setTimeout(flushLogs, FLUSH_INTERVAL)
}

function dropPendingLogs(): void {
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = null
  pendingLogs = []
}

function openGap(reason: 'hidden' | 'socket'): void {
  if (!gap && socketState.value === 'open') gap = { since: Date.now(), reason }
}

function closeGap(): void {
  if (!gap) return
  const range = `${clockTime(gap.since)}–${clockTime(Date.now())}`
  const text = gap.reason === 'hidden' ? `窗口隐藏期间未接收日志（${range}）` : `与核心的连接中断，期间的日志未收到（${range}）`
  gap = null
  if (logs.value.length === 0 && pendingLogs.length === 0) return
  enqueueLog({ type: 'marker', payload: text })
}

async function loadCoreLevel(): Promise<string> {
  try {
    const { data } = await fetchConfig()
    return filterLevelFor(data['log-level'] || DEFAULT_CORE_LEVEL)
  } catch {
    return coreLevel.value || DEFAULT_CORE_LEVEL
  }
}

const CORE_DOWN = new Set(['stopped', 'stopping', 'starting'])

function shouldConnect(): boolean {
  return lifecycleOwners > 0
    && appVisible.value
    && !CORE_DOWN.has(serviceStatus.value.state)
    && (!IS_ANDROID || clashApiReady.value)
}

function hasLogs(): boolean {
  return logs.value.length > 0 || pendingLogs.length > 0
}

let connectingGeneration = -1

async function connect(refreshCoreLevel = true): Promise<void> {
  if (!shouldConnect() || ws || connectingGeneration === connectionGeneration) return

  const generation = connectionGeneration
  socketState.value = 'connecting'
  if (refreshCoreLevel) {
    connectingGeneration = generation
    const fetched = await loadCoreLevel()
    if (connectingGeneration === generation) connectingGeneration = -1
    if (generation !== connectionGeneration || !shouldConnect() || ws) return
    coreLevel.value = fetched
  }
  const level = levelFilter.value

  try {
    let recheckLevel = false
    const socket = createClashWS('/logs', (data: LogEntry) => {
      if (generation !== connectionGeneration || ws !== socket) return
      enqueueLog(data)
    }, { level })
    ws = socket
    socket.addEventListener('open', () => {
      if (ws !== socket) return
      socketState.value = 'open'
      socketFailures.value = 0
      closeGap()
      if (!recheckLevel) return
      recheckLevel = false
      void loadCoreLevel().then((next) => {
        if (ws !== socket || next === coreLevel.value) return
        coreLevel.value = next
        if (!chosenLevel.value) restart()
      })
    })
    socket.addEventListener('close', () => {
      if (ws !== socket) return
      recheckLevel = true
      openGap('socket')
      socketFailures.value++
      socketState.value = 'reconnecting'
    })
  } catch (error) {
    console.error('Failed to connect to the log stream:', error)
    socketState.value = 'failed'
  }
}

function disconnect(): void {
  connectionGeneration++
  const socket = ws
  ws = null
  socket?.close()
  flushLogs()
  socketState.value = 'idle'
  socketFailures.value = 0
}

function restart(): void {
  disconnect()
  void connect()
}

function resubscribe(level: string): void {
  if (!ws && connectingGeneration !== connectionGeneration) return
  if (logs.value.length > 0 || pendingLogs.length > 0) {
    enqueueLog({ type: 'marker', payload: `日志级别切换为 ${level}，之后的日志按新级别接收` })
  }
  disconnect()
  void connect(false)
}

function setPaused(on: boolean): void {
  if (paused.value === on) return
  if (on) {
    flushLogs()
    pausedLogs.value = logs.value
    pausedSeq = lastSeq(logs.value)
  } else {
    pausedLogs.value = []
  }
  paused.value = on
}

function clear(): LogEntry[] {
  setPaused(false)
  flushLogs()
  const removed = logs.value
  logs.value = []
  return removed
}

function restore(removed: LogEntry[]): void {
  if (removed.length === 0) return
  flushLogs()
  const merged = removed.concat(logs.value)
  logs.value = merged.length > MAX_LOGS ? merged.slice(-MAX_LOGS) : merged
}

export function useLogsLifecycle() {
  const { activeClashApiId, clashApiUrl, clashApiSecret } = useConfigStore()
  let active = false

  const unwatchApi = watch(
    () => [activeClashApiId.value, clashApiUrl.value, clashApiSecret.value] as const,
    ([id], [previousId]) => {
      if (id !== previousId) {
        coreLevel.value = ''
        chosenLevel.value = ''
        gap = null
        setPaused(false)
        disconnect()
        dropPendingLogs()
        logs.value = []
        if (active) void connect()
        return
      }
      gap = null
      if (logs.value.length > 0 || pendingLogs.length > 0) {
        enqueueLog({ type: 'marker', payload: `Clash API 配置已更新，重新连接（${clockTime(Date.now())}）` })
      }
      disconnect()
      if (active) void connect()
    },
  )
  const unwatchService = watch(
    () => serviceStatus.value.state,
    (state, previous) => {
      if (!active) return
      if (CORE_DOWN.has(state)) {
        if (previous === 'running' && hasLogs()) enqueueLog({ type: 'marker', payload: `核心已停止（${clockTime(Date.now())}）` })
        gap = null
        disconnect()
        return
      }
      if (state === 'running' && previous && CORE_DOWN.has(previous)) {
        if (hasLogs()) enqueueLog({ type: 'marker', payload: `核心已启动（${clockTime(Date.now())}）` })
        disconnect()
      }
      void connect()
    },
  )
  const unwatchVisible = watch(appVisible, (visible) => {
    if (!active) return
    if (visible) void connect()
    else {
      openGap('hidden')
      disconnect()
    }
  })

  const unwatchReady = IS_ANDROID
    ? watch(clashApiReady, (ready) => {
      if (!active) return
      if (ready) void connect()
      else disconnect()
    })
    : () => {}

  function start(): void {
    if (active) return
    active = true
    lifecycleOwners++
    void connect()
  }

  function stop(): void {
    if (!active) return
    active = false
    lifecycleOwners--
    if (lifecycleOwners === 0) disconnect()
  }

  onUnmounted(() => {
    unwatchApi()
    unwatchService()
    unwatchVisible()
    unwatchReady()
    stop()
  })

  return { start, stop }
}

export function useLogsStore() {
  return {
    logs: shownLogs,
    filteredLogs,
    levelFilter,
    levelChoice,
    coreLevel,
    coreFilterLevel,
    paused,
    newSincePause,
    filterText,
    socketState,
    socketFailures,
    setPaused,
    clear,
    restore,
  }
}
