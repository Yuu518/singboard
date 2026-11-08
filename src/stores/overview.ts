import { ref, onUnmounted, watch } from 'vue'
import { createClashWS } from '@/api/websocket'
import { useConfigStore } from './config'
import { appVisible } from './appVisible'
import type { TrafficData, MemoryData } from '@/types'
import type ReconnectingWebSocket from 'reconnecting-websocket'

export interface HistoryPoint {
  name: number
  value: number
}

const MAX_POINTS = 60
const initHistory = (): HistoryPoint[] =>
  new Array(MAX_POINTS).fill(0).map((_, i) => ({ name: i, value: 0 }))

export const uploadSpeedHistory = ref<HistoryPoint[]>(initHistory())
export const downloadSpeedHistory = ref<HistoryPoint[]>(initHistory())
export const memoryHistory = ref<HistoryPoint[]>(initHistory())
export const connectionsHistory = ref<HistoryPoint[]>(initHistory())

const currentTraffic = ref<TrafficData>({ up: 0, down: 0 })
const memory = ref<MemoryData>({ inuse: 0, oslimit: 0 })

let trafficWs: ReconnectingWebSocket | null = null
let memoryWs: ReconnectingWebSocket | null = null
let refCount = 0

export function pushConnectionCount(count: number) {
  const ts = Date.now()
  connectionsHistory.value = [
    ...connectionsHistory.value,
    { name: ts, value: count },
  ].slice(-MAX_POINTS)
}

export function useOverviewStore() {
  const { activeClashApiId, clashApiUrl, clashApiSecret } = useConfigStore()

  function start() {
    if (!trafficWs) {
      trafficWs = createClashWS('/traffic', (data: TrafficData) => {
        currentTraffic.value = data
        const ts = Date.now()

        uploadSpeedHistory.value = [
          ...uploadSpeedHistory.value,
          { name: ts, value: data.up },
        ].slice(-MAX_POINTS)

        downloadSpeedHistory.value = [
          ...downloadSpeedHistory.value,
          { name: ts, value: data.down },
        ].slice(-MAX_POINTS)
      }, undefined, { latestOnly: true })
    }
    if (!memoryWs) {
      memoryWs = createClashWS('/memory', (data: MemoryData) => {
        if (data.inuse === 0) return
        memory.value = data
        const ts = Date.now()

        memoryHistory.value = [
          ...memoryHistory.value,
          { name: ts, value: data.inuse },
        ].slice(-MAX_POINTS)
      }, undefined, { latestOnly: true })
    }
  }

  function stop() {
    trafficWs?.close()
    trafficWs = null
    memoryWs?.close()
    memoryWs = null
  }

  refCount++
  const unwatchApi = watch(
    () => [activeClashApiId.value, clashApiUrl.value, clashApiSecret.value] as const,
    ([id], [previousId]) => {
      if (trafficWs || memoryWs) {
        stop()
        if (id !== previousId) {
          uploadSpeedHistory.value = initHistory()
          downloadSpeedHistory.value = initHistory()
          memoryHistory.value = initHistory()
          connectionsHistory.value = initHistory()
        }
        start()
      }
    },
  )
  const unwatchVisible = watch(appVisible, (visible) => {
    if (visible) {
      if (!trafficWs && !memoryWs && refCount > 0) start()
    } else {
      if (trafficWs || memoryWs) stop()
    }
  })
  onUnmounted(() => {
    unwatchApi()
    unwatchVisible()
    refCount--
    if (refCount === 0) stop()
  })

  function resetHistory() {
    uploadSpeedHistory.value = initHistory()
    downloadSpeedHistory.value = initHistory()
    memoryHistory.value = initHistory()
    connectionsHistory.value = initHistory()
  }

  return {
    uploadSpeedHistory,
    downloadSpeedHistory,
    memoryHistory,
    connectionsHistory,
    currentTraffic,
    memory,
    start,
    stop,
    resetHistory,
  }
}
