<script setup lang="ts">
import { onMounted, computed } from 'vue'
import { useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import NetworkInfo from '@/components/overview/NetworkInfo.vue'
import TopologyChart from '@/components/overview/TopologyChart.vue'
import SparkLine from '@/components/overview/SparkLine.vue'
import { formatBytes, formatSpeed } from '@/utils/format'

const {
  currentTraffic, memory, uploadSpeedHistory, downloadSpeedHistory, connectionsHistory,
  start: startOverview,
} = useOverviewStore()
const { connections, downloadTotal, uploadTotal, start: startConnections } = useConnectionsStore()

const memoryUsage = computed(() => formatBytes(memory.value.inuse))

const speedLabelFormatter = (v: number) => (v === 0 ? '' : formatSpeed(v))
const connLabelFormatter = (v: number) => (v === 0 ? '' : String(Math.round(v)))

onMounted(async () => {
  startOverview()
  startConnections()
})
</script>

<template>
  <div class="space-y-4">
    <h1 class="text-[26px] leading-tight tracking-tight font-bold">概览</h1>

    <div class="grid grid-cols-3 gap-3">
      <div class="surface-card p-4 flex flex-col gap-1.5">
        <div class="text-xs text-base-content/60 font-semibold tracking-wider">上传</div>
        <div class="text-3xl font-extralight tabular-nums">
          {{ formatSpeed(currentTraffic.up) }}
        </div>
        <div class="h-14 mt-1">
          <SparkLine
            :data="uploadSpeedHistory"
            color="#0A84FF"
            :min="60000"
            :label-formatter="speedLabelFormatter"
          />
        </div>
        <div class="text-xs text-base-content/50">总计 {{ formatBytes(uploadTotal) }}</div>
      </div>
      <div class="surface-card p-4 flex flex-col gap-1.5">
        <div class="text-xs text-base-content/60 font-semibold tracking-wider">下载</div>
        <div class="text-3xl font-extralight tabular-nums">
          {{ formatSpeed(currentTraffic.down) }}
        </div>
        <div class="h-14 mt-1">
          <SparkLine
            :data="downloadSpeedHistory"
            color="#BF5AF2"
            :min="60000"
            :label-formatter="speedLabelFormatter"
          />
        </div>
        <div class="text-xs text-base-content/50">总计 {{ formatBytes(downloadTotal) }}</div>
      </div>
      <div class="surface-card p-4 flex flex-col gap-1.5">
        <div class="text-xs text-base-content/60 font-semibold tracking-wider">
          连接
        </div>
        <div class="text-3xl font-extralight tabular-nums">{{ connections.length }}</div>
        <div class="h-14 mt-1">
          <SparkLine
            :data="connectionsHistory"
            color="#30D158"
            :min="10"
            :label-formatter="connLabelFormatter"
            :right-margin="24"
          />
        </div>
        <div class="text-xs text-base-content/50">内存使用 {{ memoryUsage }}</div>
      </div>
    </div>

    <NetworkInfo />

    <TopologyChart />
  </div>
</template>
