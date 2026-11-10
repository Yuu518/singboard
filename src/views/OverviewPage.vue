<script setup lang="ts">
import { computed, onMounted, type ComputedRef } from 'vue'
import { overviewLinkState, useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import { cpuHistory, useServiceStore } from '@/stores/service'
import { useConfigStore } from '@/stores/config'
import { useSettledServiceState } from '@/composables/useSettledServiceState'
import { usePageActive } from '@/android/composables/usePageActive'
import { IS_ANDROID } from '@/platform'
import OverviewHero from '@/components/overview/OverviewHero.vue'
import OverviewStats from '@/components/overview/OverviewStats.vue'
import NetworkInfo from '@/components/overview/NetworkInfo.vue'
import TopologyChart from '@/components/overview/TopologyChart.vue'
import { networkCounts } from '@/utils/overviewStatus'

const {
  currentTraffic, memory, uploadSpeedHistory, downloadSpeedHistory, connectionsHistory, memoryHistory,
  socketState: trafficSocketState, socketFailures: trafficFailures,
  start: startOverview,
} = useOverviewStore()
const {
  connections, downloadTotal, uploadTotal,
  socketState: connectionsSocketState, socketFailures: connectionsFailures,
  start: startConnections,
} = useConnectionsStore()
const { serviceState, isRunning } = useSettledServiceState()
const { serviceStatus } = useServiceStore()
const { config } = useConfigStore()
const pageActive = usePageActive()

function whileActive<T>(getter: () => T): ComputedRef<T> {
  let held: { value: T } | null = null
  return computed(() => {
    if (pageActive.value || !held) held = { value: getter() }
    return held.value
  })
}

const rootMode = computed(() => IS_ANDROID && config.value.rootMode)

const link = computed(() => overviewLinkState(
  isRunning.value,
  { state: trafficSocketState.value, failures: trafficFailures.value },
  { state: connectionsSocketState.value, failures: connectionsFailures.value },
))
const linkFailures = computed(() => Math.max(trafficFailures.value, connectionsFailures.value))
const trafficLive = computed(() => isRunning.value && trafficSocketState.value === 'open')
const connectionsLive = computed(() => isRunning.value && connectionsSocketState.value === 'open')

const traffic = whileActive(() => (isRunning.value ? currentTraffic.value : null))
const totals = whileActive(() => (connectionsLive.value ? { up: uploadTotal.value, down: downloadTotal.value } : null))
const downHistory = whileActive(() => downloadSpeedHistory.value)
const upHistory = whileActive(() => uploadSpeedHistory.value)

const connectionStats = whileActive(() => {
  if (!connectionsLive.value) return null
  const counts = networkCounts(connections.value)
  return { count: connections.value.length, ...counts }
})
const connectionsTrend = whileActive(() => connectionsHistory.value)
const memoryValue = whileActive(() => (trafficLive.value && memory.value ? memory.value.inuse : null))
const memoryTrend = whileActive(() => memoryHistory.value)
const residentMemory = whileActive(() => {
  const bytes = serviceStatus.value.memoryBytes
  return rootMode.value && isRunning.value && typeof bytes === 'number' ? bytes : null
})
const cpu = whileActive(() => {
  const percent = serviceStatus.value.cpuPercent
  return rootMode.value && isRunning.value && typeof percent === 'number' ? percent : null
})
const cpuTrend = whileActive(() => cpuHistory.value)

const topologyWaiting = computed(() => {
  switch (link.value) {
    case 'stopped':
      return 'sing-box 运行后显示连接拓扑'
    case 'connecting':
      return '正在连接 sing-box…'
    default:
      return '与 sing-box 的连接已中断，恢复后显示连接拓扑'
  }
})

onMounted(() => {
  startOverview()
  startConnections()
})
</script>

<template>
  <div class="overview-page">
    <h1 class="text-[26px] leading-tight tracking-tight font-bold mb-3">概览</h1>

    <OverviewHero
      :link="link"
      :service-state="serviceState"
      :failures="linkFailures"
      :traffic="traffic"
      :totals="totals"
      :down-history="downHistory"
      :up-history="upHistory"
    />

    <OverviewStats
      :connections="connectionStats"
      :connections-history="connectionsTrend"
      :memory="memoryValue"
      :memory-history="memoryTrend"
      :resident-memory="residentMemory"
      :cpu="cpu"
      :cpu-history="cpuTrend"
      :show-cpu="rootMode"
      :show-resident="rootMode"
    />

    <NetworkInfo :running="isRunning" />

    <TopologyChart :live="connectionsLive" :waiting="topologyWaiting" />
  </div>
</template>

<style scoped>
.overview-page {
  container-name: overview;
  container-type: inline-size;
  padding-bottom: 4px;
}
</style>
