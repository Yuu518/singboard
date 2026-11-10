<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import MiniLine from './MiniLine.vue'
import { formatBytes } from '@/utils/format'
import { historySummary, splitRate } from '@/utils/overviewStatus'
import type { SparkPoint } from '@/utils/sparkPath'

const props = defineProps<{
  connections: { count: number; tcp: number; udp: number } | null
  connectionsHistory: readonly SparkPoint[]
  memory: number | null
  memoryHistory: readonly SparkPoint[]
  residentMemory: number | null
  cpu: number | null
  cpuHistory: readonly SparkPoint[]
  showCpu: boolean
  showResident: boolean
}>()

const router = useRouter()

const memoryValue = computed(() => (props.memory === null ? null : splitRate(formatBytes(props.memory))))
const memorySub = computed(() => {
  if (!props.showResident) return ['运行时占用']
  return props.residentMemory === null ? [] : ['进程常驻', formatBytes(props.residentMemory)]
})
const countText = (v: number) => `${Math.round(v)} 条`
const connectionsLabel = computed(() => {
  const c = props.connections
  if (!c) return '连接数暂无数据。查看连接'
  return `连接 ${c.count} 条，TCP ${c.tcp}，UDP ${c.udp}，${historySummary(props.connectionsHistory, countText)}。查看连接`
})
</script>

<template>
  <section class="surface-card overview-stats" :class="{ 'has-cpu': showCpu }" aria-label="连接与资源">
    <div class="overview-stat" data-stat="connections">
      <button type="button" class="overview-cover overview-stat-cover" :aria-label="connectionsLabel" @click="router.push('/connections')"></button>
      <div class="overview-cover-body overview-stat-body" aria-hidden="true">
        <span class="overview-stat-label">连接</span>
        <span class="overview-stat-value" :class="{ 'overview-dim': !connections }">{{ connections ? connections.count : '—' }}</span>
        <span class="overview-stat-sub">
          <template v-if="connections"><span>TCP {{ connections.tcp }}</span><span>UDP {{ connections.udp }}</span></template>
        </span>
      </div>
      <MiniLine class="overview-stat-line" :data="connectionsHistory" :floor="10" :live="!!connections" />
    </div>

    <div class="overview-stat" data-stat="memory">
      <div class="overview-stat-body">
        <span class="overview-stat-label">内存</span>
        <span class="overview-stat-value" :class="{ 'overview-dim': !memoryValue }">
          <template v-if="memoryValue">{{ memoryValue.value }}<small>{{ memoryValue.unit }}</small></template>
          <template v-else>—</template>
        </span>
        <span class="overview-stat-sub">
          <template v-if="memoryValue"><span v-for="part in memorySub" :key="part">{{ part }}</span></template>
        </span>
      </div>
      <MiniLine class="overview-stat-line" :data="memoryHistory" :floor="16 * 1024 * 1024" :live="memory !== null" />
    </div>

    <div v-if="showCpu" class="overview-stat" data-stat="cpu">
      <div class="overview-stat-body">
        <span class="overview-stat-label">CPU</span>
        <span class="overview-stat-value" :class="{ 'overview-dim': cpu === null }">
          <template v-if="cpu !== null">{{ cpu.toFixed(1) }}<small>%</small></template>
          <template v-else>—</template>
        </span>
        <span class="overview-stat-sub"><span v-if="cpu !== null">占全部核心</span></span>
      </div>
      <MiniLine class="overview-stat-line" :data="cpuHistory" :floor="10" :live="cpu !== null" />
    </div>
  </section>
</template>

<style scoped>
.overview-stats {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin-top: 12px;
  overflow: hidden;
}

.overview-stats.has-cpu {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.overview-stat {
  position: relative;
  display: flex;
  min-width: 0;
  flex-direction: column;
  padding: 12px 14px 0;
}

.overview-stat + .overview-stat::before {
  content: '';
  position: absolute;
  top: 14px;
  bottom: 14px;
  left: 0;
  border-left: 0.5px solid var(--hairline);
}

.overview-stat-cover {
  inset: 4px;
}

.overview-stat-body {
  display: flex;
  min-width: 0;
  flex-direction: column;
  padding-bottom: 8px;
}

.overview-stat-label {
  font-size: 12px;
  color: oklch(var(--bc) / 0.55);
}

.overview-stat-value {
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 18px;
  font-weight: 500;
  letter-spacing: -0.01em;
  font-variant-numeric: tabular-nums;
}

.overview-stat-value small {
  margin-left: 3px;
  font-size: 12.5px;
  font-weight: 400;
  color: oklch(var(--bc) / 0.5);
}

.overview-stat-sub {
  min-height: 1.45em;
  font-size: 11.5px;
  line-height: 1.45;
  color: oklch(var(--bc) / 0.5);
  font-variant-numeric: tabular-nums;
}

.overview-stat-sub > span {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.overview-stat-line {
  position: relative;
  z-index: 1;
  margin: auto -14px 0;
  pointer-events: none;
}

@container overview (min-width: 640px) {
  .overview-stat {
    padding: 14px 18px 0;
  }

  .overview-stat-value {
    font-size: 19px;
  }

  .overview-stat-sub {
    font-size: 12px;
  }

  .overview-stat-sub > span {
    display: inline;
  }

  .overview-stat-sub > span + span::before {
    content: ' · ';
  }

  .overview-stat-line {
    margin: auto -18px 0;
  }
}
</style>
