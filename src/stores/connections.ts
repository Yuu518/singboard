import { ref, computed, onUnmounted, watch } from 'vue'
import { createClashWS } from '@/api/websocket'
import { disconnectAll, disconnectById } from '@/api'
import { useConfigStore } from './config'
import { appVisible } from './appVisible'
import { pushConnectionCount } from './overview'
import { clashApiReady } from './service'
import { IS_ANDROID } from '@/platform'
import type { Connection, ConnectionsSnapshot } from '@/types'
import type ReconnectingWebSocket from 'reconnecting-websocket'

const connections = ref<Connection[]>([])
const downloadTotal = ref(0)
const uploadTotal = ref(0)
const closedConnections = ref<Connection[]>([])
const filterText = ref('')
const paused = ref(false)
const socketState = ref<'idle' | 'connecting' | 'resuming' | 'reconnecting' | 'open' | 'failed'>('idle')
const socketFailures = ref(0)

let ws: ReconnectingWebSocket | null = null
let prevTraffic = new Map<string, { download: number; upload: number }>()
let refCount = 0
let stoppedAt = 0
let coreSuspended = false

export function matchesFilter(c: Connection, q: string): boolean {
  const m = c.metadata
  return (
    (m.host || '').toLowerCase().includes(q) ||
    (m.sniffHost || '').toLowerCase().includes(q) ||
    (m.destinationIP || '').toLowerCase().includes(q) ||
    (m.sourceIP || '').toLowerCase().includes(q) ||
    String(m.destinationPort ?? '').includes(q) ||
    (m.network || '').toLowerCase().includes(q) ||
    (m.process || '').toLowerCase().includes(q) ||
    (m.processPath || '').toLowerCase().includes(q) ||
    (c.chains || []).join(' ').toLowerCase().includes(q) ||
    (c.rule || '').toLowerCase().includes(q) ||
    (c.rulePayload || '').toLowerCase().includes(q)
  )
}

const filteredConnections = computed(() => {
  if (!filterText.value) return connections.value
  const q = filterText.value.toLowerCase()
  return connections.value.filter((c) => matchesFilter(c, q))
})

export const chainSpeeds = computed(() => {
  const map: Record<string, { down: number; up: number }> = {}
  for (const conn of connections.value) {
    if (!conn.chains) continue
    for (const chain of conn.chains) {
      if (!map[chain]) map[chain] = { down: 0, up: 0 }
      map[chain].down += conn.downloadSpeed ?? 0
      map[chain].up += conn.uploadSpeed ?? 0
    }
  }
  return map
})

const filteredClosedConnections = computed(() => {
  if (!filterText.value) return closedConnections.value
  const q = filterText.value.toLowerCase()
  return closedConnections.value.filter((c) => matchesFilter(c, q))
})

function start() {
  if (ws || coreSuspended || (IS_ANDROID && !clashApiReady.value)) return
  let backgroundSince = connections.value.length > 0 ? stoppedAt : 0
  stoppedAt = 0
  socketState.value = backgroundSince ? 'resuming' : 'connecting'
  socketFailures.value = 0
  let socket: ReconnectingWebSocket
  try {
    socket = createClashWS('/connections', (data: ConnectionsSnapshot) => {
      if (ws !== socket) return
      socketState.value = 'open'
      socketFailures.value = 0
      if (paused.value) return
      downloadTotal.value = data.downloadTotal
      uploadTotal.value = data.uploadTotal

      const currentIds = new Set(data.connections.map((c) => c.id))
      const closed = connections.value.filter((c) => !currentIds.has(c.id))
      if (closed.length) {
        const closedAt = backgroundSince || Date.now()
        const closedInBackground = !!backgroundSince
        const finished = closed.map((c) => ({ ...c, downloadSpeed: 0, uploadSpeed: 0, closedAt, closedInBackground }))
        closedConnections.value = [...finished, ...closedConnections.value].slice(0, 500)
      }
      backgroundSince = 0

      const newTraffic = new Map<string, { download: number; upload: number }>()
      for (const conn of data.connections) {
        const prev = prevTraffic.get(conn.id)
        if (prev) {
          conn.downloadSpeed = Math.max(0, conn.download - prev.download)
          conn.uploadSpeed = Math.max(0, conn.upload - prev.upload)
        } else {
          conn.downloadSpeed = 0
          conn.uploadSpeed = 0
        }
        newTraffic.set(conn.id, { download: conn.download, upload: conn.upload })
      }
      prevTraffic = newTraffic

      connections.value = data.connections
      pushConnectionCount(data.connections.length)
    }, undefined, { latestOnly: true })
  } catch (error) {
    console.error('Failed to connect to the connection stream:', error)
    socketState.value = 'failed'
    return
  }
  ws = socket
  socket.addEventListener('close', () => {
    if (ws !== socket) return
    socketFailures.value++
    socketState.value = 'reconnecting'
  })
}

function stop() {
  const socket = ws
  ws = null
  if (socket) stoppedAt = Date.now()
  socket?.close()
  prevTraffic = new Map()
  socketState.value = 'idle'
  socketFailures.value = 0
}

function resetOnRestart() {
  closedConnections.value = []
  connections.value = []
  downloadTotal.value = 0
  uploadTotal.value = 0
}

export function suspendConnections() {
  coreSuspended = true
  stop()
}

export function resumeConnections() {
  coreSuspended = false
  if (refCount > 0 && appVisible.value) start()
}

export function resetConnectionsOnRestart() {
  resetOnRestart()
}

export function useConnectionsStore() {
  const { activeClashApiId, clashApiUrl, clashApiSecret } = useConfigStore()

  refCount++
  const unwatchApi = watch(
    () => [activeClashApiId.value, clashApiUrl.value, clashApiSecret.value] as const,
    () => {
      if (ws) {
        stop()
        connections.value = []
        start()
      }
    },
  )
  const unwatchVisible = watch(appVisible, (visible) => {
    if (visible) {
      if (!ws && refCount > 0) start()
    } else {
      if (ws) stop()
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

  async function closeConnection(id: string) {
    await disconnectById(id)
  }

  async function closeConnections(ids: string[]): Promise<string[]> {
    const results = await Promise.allSettled(ids.map((id) => disconnectById(id)))
    return ids.filter((_, index) => results[index].status === 'rejected')
  }

  async function closeAllConnections() {
    await disconnectAll()
  }

  return {
    connections,
    filteredConnections,
    closedConnections,
    filteredClosedConnections,
    downloadTotal,
    uploadTotal,
    paused,
    filterText,
    socketState,
    socketFailures,
    start,
    stop,
    resetOnRestart,
    closeConnection,
    closeConnections,
    closeAllConnections,
  }
}
