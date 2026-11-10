<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted, watch, nextTick } from 'vue'
import * as echarts from 'echarts/core'
import { SankeyChart } from 'echarts/charts'
import { TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { useConnectionsStore } from '@/stores/connections'
import { useConfigStore } from '@/stores/config'
import { buildTopology, topologyHeight, topologyRanking, topologySummary, topologyTooltip, type TopologyData, type TopologyTooltipParams } from '@/utils/topology'
import { CHART_TOOLTIP_CSS, inkColor, readChartInk, readTopologyColors, useReducedMotion } from '@/utils/chartTheme'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import { IS_ANDROID } from '@/platform'

echarts.use([SankeyChart, TooltipComponent, CanvasRenderer])

const VIEW_OPTIONS = [
  { value: 'chart', label: '图' },
  { value: 'list', label: '列表' },
] as const
const NODE_WIDTH = 8
const UPDATE_MS = 240
const EMPTY: TopologyData = { nodes: [], links: [], connectionCount: 0 }

const props = withDefaults(defineProps<{ live?: boolean; waiting?: string }>(), { live: true, waiting: '' })

const { connections, start } = useConnectionsStore()
const { resolvedTheme } = useConfigStore()
const reducedMotion = useReducedMotion()
const chartEl = ref<HTMLElement>()
const userPaused = ref(false)
const hoverPaused = ref(false)
const paused = computed(() => userPaused.value || hoverPaused.value)
const width = ref(0)
const view = ref<'chart' | 'list'>(IS_ANDROID ? 'list' : 'chart')
const listView = computed(() => view.value === 'list')
let myChart: echarts.ECharts | null = null
let resizeOb: ResizeObserver | null = null
let shown: TopologyData = { nodes: [], links: [], connectionCount: 0 }

const topology = computed(() => (props.live ? buildTopology(connections.value || []) : EMPTY))
const displayed = ref<TopologyData>(topology.value)
const chartHeight = computed(() => topologyHeight(displayed.value))
const summary = computed(() => topologySummary(displayed.value))
const exitRanking = computed(() => topologyRanking(displayed.value, 3))
const ruleRanking = computed(() => topologyRanking(displayed.value, 1))
function share(count: number) {
  const total = displayed.value.connectionCount
  return total ? `${Math.max(2, Math.round((count / total) * 100))}%` : '0%'
}
const labelWidth = computed(() => {
  if (!width.value) return 120
  return Math.min(180, Math.max(56, Math.floor((width.value - NODE_WIDTH * 4) / 4) - 8))
})

function buildOptions(data: TopologyData) {
  const ink = readChartInk()
  const palette = readTopologyColors()
  const animate = !reducedMotion.value
  return {
    backgroundColor: 'transparent',
    animation: animate,
    tooltip: {
      trigger: 'item' as const,
      triggerOn: 'mousemove' as const,
      backgroundColor: 'transparent',
      borderWidth: 0,
      padding: [8, 12],
      textStyle: { fontSize: 12 },
      transitionDuration: animate ? 0.2 : 0,
      extraCssText: CHART_TOOLTIP_CSS,
      formatter: (params: TopologyTooltipParams) => topologyTooltip(params, shown.nodes),
    },
    series: [
      {
        type: 'sankey' as const,
        left: 0,
        top: 8,
        bottom: 8,
        right: labelWidth.value + 6,
        layoutIterations: 0,
        data: data.nodes.map((node) => ({
          ...node,
          itemStyle: { color: inkColor(palette[node.depth] ?? ink, node.merged ? 0.35 : 0.75) },
          label: { color: inkColor(ink, node.merged ? 0.5 : 0.72) },
        })),
        links: data.links,
        emphasis: { focus: 'trajectory' as const, lineStyle: { opacity: 0.5 } },
        lineStyle: { color: 'gradient' as const, opacity: 0.16, curveness: 0.5 },
        itemStyle: { borderWidth: 0 },
        label: {
          fontSize: 12,
          width: labelWidth.value,
          overflow: 'truncate' as const,
          ellipsis: '…',
          formatter: (params: { name?: string; data?: { name?: string } }) => params.data?.name ?? params.name ?? '',
        },
        nodeGap: 12,
        nodeWidth: NODE_WIDTH,
        nodeAlign: 'left' as const,
        animation: animate,
        animationDuration: animate ? UPDATE_MS : 0,
        animationDurationUpdate: animate ? UPDATE_MS : 0,
        animationEasingUpdate: 'cubicOut' as const,
      },
    ],
  }
}

function render() {
  if (!myChart) return
  shown = displayed.value
  if (shown.nodes.length === 0) {
    myChart.clear()
    return
  }
  myChart.setOption(buildOptions(shown), { notMerge: false, lazyUpdate: true })
}

function refresh() {
  if (paused.value && props.live) return
  displayed.value = topology.value
  void nextTick(render)
}

watch(() => props.live, (live) => {
  hoverPaused.value = false
  if (!live) myChart?.dispatchAction?.({ type: 'hideTip' })
  displayed.value = topology.value
  void nextTick(render)
})

watch(topology, refresh)
watch(paused, (isPaused) => {
  if (!isPaused) refresh()
})
watch([resolvedTheme, reducedMotion, labelWidth], () => void nextTick(render))

onMounted(() => {
  start()
  if (!chartEl.value) return
  myChart = echarts.init(chartEl.value)
  render()
  myChart.on('showTip', () => { hoverPaused.value = true })
  myChart.on('hideTip', () => { hoverPaused.value = false })
  myChart.on('globalout', () => { hoverPaused.value = false })
  resizeOb = new ResizeObserver((entries) => {
    width.value = entries[0]?.contentRect.width ?? width.value
    myChart?.resize()
  })
  resizeOb.observe(chartEl.value)
})

onUnmounted(() => {
  resizeOb?.disconnect()
  resizeOb = null
  myChart?.dispose()
  myChart = null
})
</script>

<template>
  <section class="overview-section" aria-labelledby="topology-title">
    <div class="overview-section-head">
      <h2 id="topology-title">连接拓扑</h2>
      <span class="overview-section-sub" data-topology-summary>
        <template v-if="props.live">{{ userPaused ? `已暂停更新 · ${summary}` : summary }}</template>
      </span>
      <div v-if="props.live" class="overview-section-actions">
        <SegmentedControl v-model="view" :options="VIEW_OPTIONS" label="连接拓扑的显示方式" data-action="toggle-view" />
        <button
          type="button"
          class="topology-pause"
          aria-label="暂停更新连接拓扑"
          :title="userPaused ? '继续更新连接拓扑' : '暂停更新连接拓扑'"
          :aria-pressed="userPaused"
          data-action="toggle-pause"
          @click="userPaused = !userPaused"
        >
          <svg v-if="!userPaused" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <path d="M9 5.5v13M15 5.5v13" />
          </svg>
          <svg v-else aria-hidden="true" viewBox="0 0 24 24" fill="currentColor">
            <path d="M7 5.65c0-.86.92-1.4 1.67-.99l10.4 6.35a1.13 1.13 0 0 1 0 1.97L8.67 19.34A1.13 1.13 0 0 1 7 18.35V5.65z" />
          </svg>
        </button>
      </div>
    </div>
    <div class="surface-card topology-card">
      <div
        v-if="!props.live"
        class="flex min-h-[140px] items-center justify-center px-6 text-center text-sm text-base-content/65"
        role="status"
        data-topology-waiting
      >
        {{ props.waiting || '暂时拿不到连接数据' }}
      </div>
      <div v-show="props.live && !listView" class="relative">
        <div
          ref="chartEl"
          class="w-full"
          role="img"
          :aria-label="`连接拓扑图：${summary}。切换到“列表”可按出口和规则查看`"
          :style="{ height: chartHeight + 'px' }"
        />
        <div
          v-if="displayed.nodes.length === 0"
          class="absolute inset-0 flex items-center justify-center text-base-content/55 text-sm"
        >
          暂无连接数据
        </div>
      </div>
      <div v-if="props.live && listView" class="topology-list" data-topology-list>
        <p v-if="displayed.nodes.length === 0" class="py-6 text-center text-sm text-base-content/55">暂无连接数据</p>
        <template v-else>
          <section v-for="group in [{ title: '按出口', rows: exitRanking }, { title: '按规则', rows: ruleRanking }]" :key="group.title" class="min-w-0">
            <h3 class="topology-list-title">{{ group.title }}</h3>
            <ul>
              <li v-for="row in group.rows" :key="row.id" class="topology-row">
                <span class="truncate" :class="{ 'text-base-content/60': row.merged }" :title="row.name">{{ row.name }}</span>
                <span class="topology-count">{{ row.count }} 条</span>
                <i class="topology-bar" aria-hidden="true"><i :style="{ width: share(row.count) }"></i></i>
              </li>
            </ul>
          </section>
        </template>
      </div>
    </div>
  </section>
</template>

<style scoped>
.topology-card {
  padding: 12px 14px 12px 18px;
}

.topology-pause {
  display: inline-grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border-radius: 9999px;
  background: var(--fill);
  color: oklch(var(--bc) / 0.7);
  transition: background-color 0.15s ease;
}

.topology-pause:hover {
  background: var(--fill-strong);
}

.topology-pause[aria-pressed='true'] {
  background: color-mix(in oklab, var(--selection-ink) 13%, transparent);
  color: var(--selection-ink);
}

.topology-pause:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}

.topology-pause svg {
  width: 14px;
  height: 14px;
}

.topology-list {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 14px 32px;
  padding: 4px 4px 2px 0;
}

.topology-list-title {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 500;
  color: oklch(var(--bc) / 0.55);
}

.topology-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: baseline;
  column-gap: 12px;
  padding: 5px 0 9px;
  font-size: 13px;
}

.topology-count {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: oklch(var(--bc) / 0.55);
}

.topology-bar {
  grid-column: 1 / -1;
  display: block;
  height: 3px;
  margin-top: 6px;
  overflow: hidden;
  border-radius: 9999px;
  background: var(--fill);
}

.topology-bar > i {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: rgb(var(--chart-ink-rgb) / 0.4);
}

@container overview (min-width: 640px) {
  .topology-list {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (prefers-reduced-motion: reduce) {
  .topology-pause {
    transition: none;
  }
}
</style>
