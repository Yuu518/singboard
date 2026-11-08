<script setup lang="ts">
import { onMounted, computed } from 'vue'
import { useRouter } from 'vue-router'
import { overviewLinkState, useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import { useSettledServiceState } from '@/composables/useSettledServiceState'
import NetworkInfo from '@/components/overview/NetworkInfo.vue'
import TopologyChart from '@/components/overview/TopologyChart.vue'
import SparkLine from '@/components/overview/SparkLine.vue'
import OverviewStatusBand from '@/components/overview/OverviewStatusBand.vue'
import { formatBytes, formatSpeed } from '@/utils/format'
import { historySummary, networkCounts } from '@/utils/overviewStatus'

const {
  currentTraffic, memory, uploadSpeedHistory, downloadSpeedHistory, connectionsHistory,
  socketState: trafficSocketState, socketFailures: trafficFailures,
  start: startOverview,
} = useOverviewStore()
const {
  connections, downloadTotal, uploadTotal,
  socketState: connectionsSocketState, socketFailures: connectionsFailures,
  start: startConnections,
} = useConnectionsStore()
const { serviceState, isRunning } = useSettledServiceState()

const link = computed(() => overviewLinkState(
  isRunning.value,
  { state: trafficSocketState.value, failures: trafficFailures.value },
  { state: connectionsSocketState.value, failures: connectionsFailures.value },
))
const linkFailures = computed(() => Math.max(trafficFailures.value, connectionsFailures.value))

const traffic = computed(() => (isRunning.value ? currentTraffic.value : null))
const connectionsLive = computed(() => isRunning.value && connectionsSocketState.value === 'open')
const counts = computed(() => networkCounts(connections.value))
const topologyWaiting = computed(() => {
  switch (link.value) {
    case 'stopped':
      return 'sing-box 运行后显示连接拓扑'
    case 'connecting':
      return '正在连接核心…'
    default:
      return '与核心的连接已中断，恢复后显示连接拓扑'
  }
})

const router = useRouter()
const speedText = (v: number) => formatSpeed(Math.round(v))
const countText = (v: number) => `${Math.round(v)} 条`

const metricLabels = computed(() => ({
  up: traffic.value
    ? `上传 ${formatSpeed(traffic.value.up)}，${historySummary(uploadSpeedHistory.value, speedText)}。查看连接`
    : '上传速度暂无数据。查看连接',
  down: traffic.value
    ? `下载 ${formatSpeed(traffic.value.down)}，${historySummary(downloadSpeedHistory.value, speedText)}。查看连接`
    : '下载速度暂无数据。查看连接',
  connections: connectionsLive.value
    ? `连接 ${connections.value.length} 条，TCP ${counts.value.tcp}，UDP ${counts.value.udp}，${historySummary(connectionsHistory.value, countText)}。查看连接`
    : '连接数暂无数据。查看连接',
}))

function openConnections() {
  void router.push('/connections')
}

const speedLabelFormatter = (v: number) => (v === 0 ? '' : formatSpeed(v))
const connLabelFormatter = (v: number) => (v === 0 ? '' : String(Math.round(v)))

onMounted(async () => {
  startOverview()
  startConnections()
})
</script>

<template>
  <div class="overview-page space-y-3">
    <h1 class="text-[26px] leading-tight tracking-tight font-bold">概览</h1>

    <OverviewStatusBand :link="link" :service-state="serviceState" :failures="linkFailures" :memory="memory" />

    <div class="grid grid-cols-3 gap-3">
      <div class="surface-card glass-hover overview-metric" data-metric="up">
        <button type="button" class="overview-metric-link" :aria-label="metricLabels.up" @click="openConnections"></button>
        <div class="overview-metric-body" aria-hidden="true">
          <div class="overview-metric-label">上传</div>
          <div class="overview-metric-value" :class="{ 'text-base-content/40': !traffic }">
            {{ traffic ? formatSpeed(traffic.up) : '—' }}
          </div>
          <div class="h-14" :class="{ invisible: !traffic }">
            <SparkLine :data="uploadSpeedHistory" :min="60000" :label-formatter="speedLabelFormatter" />
          </div>
          <div class="overview-metric-foot">总计 <span class="tabular-nums">{{ connectionsLive ? formatBytes(uploadTotal) : '—' }}</span></div>
        </div>
      </div>
      <div class="surface-card glass-hover overview-metric" data-metric="down">
        <button type="button" class="overview-metric-link" :aria-label="metricLabels.down" @click="openConnections"></button>
        <div class="overview-metric-body" aria-hidden="true">
          <div class="overview-metric-label">下载</div>
          <div class="overview-metric-value" :class="{ 'text-base-content/40': !traffic }">
            {{ traffic ? formatSpeed(traffic.down) : '—' }}
          </div>
          <div class="h-14" :class="{ invisible: !traffic }">
            <SparkLine :data="downloadSpeedHistory" :min="60000" :label-formatter="speedLabelFormatter" />
          </div>
          <div class="overview-metric-foot">总计 <span class="tabular-nums">{{ connectionsLive ? formatBytes(downloadTotal) : '—' }}</span></div>
        </div>
      </div>
      <div class="surface-card glass-hover overview-metric" data-metric="connections">
        <button type="button" class="overview-metric-link" :aria-label="metricLabels.connections" @click="openConnections"></button>
        <div class="overview-metric-body" aria-hidden="true">
          <div class="overview-metric-label">连接</div>
          <div class="overview-metric-value" :class="{ 'text-base-content/40': !connectionsLive }">
            {{ connectionsLive ? connections.length : '—' }}
          </div>
          <div class="h-14" :class="{ invisible: !connectionsLive }">
            <SparkLine :data="connectionsHistory" :min="10" :label-formatter="connLabelFormatter" :right-margin="28" />
          </div>
          <div class="overview-metric-foot">
            <template v-if="connectionsLive">TCP <span class="tabular-nums">{{ counts.tcp }}</span> · UDP <span class="tabular-nums">{{ counts.udp }}</span></template>
            <template v-else>—</template>
          </div>
        </div>
      </div>
    </div>

    <NetworkInfo :running="isRunning" />

    <TopologyChart :live="connectionsLive" :waiting="topologyWaiting" />
  </div>
</template>

<style scoped>
.overview-page {
  container-name: overview;
  container-type: inline-size;
}

.overview-metric {
  position: relative;
  min-width: 0;
  cursor: pointer;
  padding: 14px 16px;
}

.overview-metric-body {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
  pointer-events: none;
}

.overview-metric-link {
  position: absolute;
  inset: 0;
  z-index: 1;
  border-radius: inherit;
}

.overview-metric-link:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}

.overview-metric-label {
  font-size: 12px;
  font-weight: 500;
  color: oklch(var(--bc) / 0.65);
}

.overview-metric-value {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 16px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  line-height: 1.4;
}

.overview-metric-foot {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: oklch(var(--bc) / 0.6);
}
</style>
