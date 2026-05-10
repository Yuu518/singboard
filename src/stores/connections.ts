import { ref, computed, onUnmounted, watch } from 'vue'
import { createClashWS } from '@/api/websocket'
import { disconnectAll, disconnectById } from '@/api'
import { useConfigStore } from './config'
import { appVisible } from './appVisible'
import { pushConnectionCount } from './overview'
import type { Connection, ConnectionsSnapshot } from '@/types'
import type ReconnectingWebSocket from 'reconnecting-websocket'

const connections = ref<Connection[]>([])
const downloadTotal = ref(0)
const uploadTotal = ref(0)
const closedConnections = ref<Connection[]>([])
const paused = ref(false)
const filterText = ref('')

let ws: ReconnectingWebSocket | null = null
let prevTraffic = new Map<string, { download: number; upload: number }>()
let refCount = 0

function matchesFilter(c: Connection, q: string): boolean {
  const m = c.metadata
  return (
    (m.host || '').toLowerCase().includes(q) ||
    (m.sniffHost || '').toLowerCase().includes(q) ||
    (m.destinationIP || '').toLowerCase().includes(q) ||
    (m.sourceIP || '').toLowerCase().includes(q) ||
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

const filteredClosedConnections = computed(() => {
  if (!filterText.value) return closedConnections.value
  const q = filterText.value.toLowerCase()
  return closedConnections.value.filter((c) => matchesFilter(c, q))
})

export function useConnectionsStore() {
  const { activeClashApiId } = useConfigStore()

  function start() {
    if (ws) return
    ws = createClashWS('/connections', (data: ConnectionsSnapshot) => {
      if (paused.value) return
      downloadTotal.value = data.downloadTotal
      uploadTotal.value = data.uploadTotal

      const currentIds = new Set(data.connections.map((c) => c.id))
      const closed = connections.value.filter((c) => !currentIds.has(c.id))
      if (closed.length) {
        closedConnections.value = [...closed, ...closedConnections.value].slice(0, 500)
      }

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
    })
  }

  function stop() {
    ws?.close()
    ws = null
    prevTraffic = new Map()
  }

  refCount++
  const unwatchApi = watch(
    () => activeClashApiId.value,
    () => {
      if (ws) {
        stop()
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
  onUnmounted(() => {
    unwatchApi()
    unwatchVisible()
    refCount--
    if (refCount === 0) stop()
  })

  function resetOnRestart() {
    closedConnections.value = []
    connections.value = []
    downloadTotal.value = 0
    uploadTotal.value = 0
  }

  async function closeConnection(id: string) {
    await disconnectById(id)
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
    start,
    stop,
    resetOnRestart,
    closeConnection,
    closeAllConnections,
  }
}
