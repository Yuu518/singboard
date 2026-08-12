import { ref, computed, onUnmounted, watch } from 'vue'
import { createClashWS } from '@/api/websocket'
import { fetchConfig } from '@/api'
import { useConfigStore } from './config'
import { appVisible } from './appVisible'
import type { LogEntry } from '@/types'
import type ReconnectingWebSocket from 'reconnecting-websocket'

const MAX_LOGS = 5000

const logs = ref<LogEntry[]>([])
const logLevel = ref('')
const paused = ref(false)
const filterText = ref('')

let ws: ReconnectingWebSocket | null = null
let connectionGeneration = 0
let lifecycleOwners = 0
let pendingConnect: { generation: number; promise: Promise<void> } | null = null

const filteredLogs = computed(() => {
  if (!filterText.value) return logs.value
  const q = filterText.value.toLowerCase()
  return logs.value.filter((l) => l.payload.toLowerCase().includes(q))
})

function shouldConnect(): boolean {
  return lifecycleOwners > 0 && appVisible.value
}

async function connect(): Promise<void> {
  if (!shouldConnect() || ws) return

  const generation = connectionGeneration
  if (pendingConnect?.generation === generation) {
    return pendingConnect.promise
  }

  const promise = (async () => {
    let level = logLevel.value
    if (!level) {
      try {
        const { data } = await fetchConfig()
        level = data['log-level'] || 'info'
      } catch {
        level = 'info'
      }
    }

    if (generation !== connectionGeneration || !shouldConnect()) return
    logLevel.value = level

    try {
      let socket: ReconnectingWebSocket
      socket = createClashWS('/logs', (data: LogEntry) => {
        if (
          generation !== connectionGeneration
          || ws !== socket
          || !appVisible.value
          || paused.value
        ) return

        data.time = new Date().toLocaleTimeString()
        logs.value.push(data)
        if (logs.value.length > MAX_LOGS) {
          logs.value = logs.value.slice(-MAX_LOGS)
        }
      }, { level })

      if (generation !== connectionGeneration || !shouldConnect()) {
        socket.close()
        return
      }
      ws = socket
    } catch (error) {
      console.error('Failed to connect to the log stream:', error)
    }
  })()

  const pending = { generation, promise }
  pendingConnect = pending
  void promise.then(
    () => {
      if (pendingConnect === pending) pendingConnect = null
    },
    () => {
      if (pendingConnect === pending) pendingConnect = null
    },
  )
  return promise
}

function disconnect(): void {
  connectionGeneration++
  const socket = ws
  ws = null
  socket?.close()
}

function restart(): void {
  disconnect()
  if (shouldConnect()) void connect()
}

function clear(): void {
  logs.value = []
}

function changeLevel(level: string): void {
  if (logLevel.value === level) return
  logLevel.value = level
  clear()
  restart()
}

export function useLogsLifecycle() {
  const { activeClashApiId } = useConfigStore()
  let active = false

  const unwatchApi = watch(
    () => activeClashApiId.value,
    () => {
      logLevel.value = ''
      clear()
      if (active) restart()
    },
  )
  const unwatchVisible = watch(appVisible, (visible) => {
    if (!active) return
    if (visible) void connect()
    else {
      disconnect()
      clear()
    }
  })

  function start(): void {
    if (active) return
    active = true
    lifecycleOwners++
    if (appVisible.value) void connect()
  }

  function stop(): void {
    if (!active) return
    active = false
    lifecycleOwners--
    if (lifecycleOwners === 0) disconnect()
  }

  onUnmounted(() => {
    unwatchApi()
    unwatchVisible()
    stop()
  })

  return { start, stop }
}

export function useLogsStore() {
  return {
    logs,
    filteredLogs,
    logLevel,
    paused,
    filterText,
    clear,
    changeLevel,
  }
}
