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

echarts.use([SankeyChart, TooltipComponent, CanvasRenderer])

const NODE_WIDTH = 10
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
const listView = ref(false)
let myChart: echarts.ECharts | null = null
let resizeOb: ResizeObserver | null = null
let shown: TopologyData = { nodes: [], links: [], connectionCount: 0 }

const topology = computed(() => (props.live ? buildTopology(connections.value || []) : EMPTY))
const displayed = ref<TopologyData>(topology.value)
const chartHeight = computed(() => topologyHeight(displayed.value))
const summary = computed(() => topologySummary(displayed.value))
const exitRanking = computed(() => topologyRanking(displayed.value, 3))
const ruleRanking = computed(() => topologyRanking(displayed.value, 1))
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
          itemStyle: { color: inkColor(palette[node.depth] ?? ink, node.merged ? 0.4 : 0.9) },
          label: { color: inkColor(ink, node.merged ? 0.6 : 0.8) },
        })),
        links: data.links,
        emphasis: { focus: 'trajectory' as const, lineStyle: { opacity: 0.6 } },
        lineStyle: { color: 'gradient' as const, opacity: 0.28, curveness: 0.5 },
        itemStyle: { borderWidth: 0 },
        label: {
          fontSize: 12,
          width: labelWidth.value,
          overflow: 'truncate' as const,
          ellipsis: '…',
          formatter: (params: { name?: string; data?: { name?: string } }) => params.data?.name ?? params.name ?? '',
        },
        nodeGap: 10,
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
  <section class="surface-card p-4 space-y-3" aria-labelledby="topology-title">
    <div class="flex items-center justify-between gap-2">
      <h2 id="topology-title" class="text-sm font-semibold">
        连接拓扑
        <span v-if="userPaused && props.live" class="ml-1 text-xs font-normal text-base-content/65">已暂停更新</span>
      </h2>
      <div v-if="props.live" class="flex items-center gap-1">
        <button
          type="button"
          class="btn btn-ghost btn-xs px-2 font-medium"
          data-action="toggle-list"
          @click="listView = !listView"
        >
          {{ listView ? '显示图表' : '显示列表' }}
        </button>
        <button
          type="button"
          class="btn btn-ghost btn-xs btn-circle"
          aria-label="暂停更新连接拓扑"
          :title="userPaused ? '继续更新连接拓扑' : '暂停更新连接拓扑'"
          :aria-pressed="userPaused"
          data-action="toggle-pause"
          @click="userPaused = !userPaused"
        >
          <svg v-if="!userPaused" aria-hidden="true" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25v13.5m-7.5-13.5v13.5" />
          </svg>
          <svg v-else aria-hidden="true" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.348a1.125 1.125 0 010 1.971l-11.54 6.347a1.125 1.125 0 01-1.667-.985V5.653z" />
          </svg>
        </button>
      </div>
    </div>
    <div
      v-if="!props.live"
      class="flex min-h-[160px] items-center justify-center px-6 text-center text-sm text-base-content/65"
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
        :aria-label="`连接拓扑图：${summary}。选择“显示列表”可按出口和规则查看`"
        :style="{ height: chartHeight + 'px' }"
      />
      <div
        v-if="displayed.nodes.length === 0"
        class="absolute inset-0 flex items-center justify-center text-base-content/55 text-sm"
      >
        暂无连接数据
      </div>
    </div>
    <div v-if="props.live && listView" class="topology-list grid grid-cols-1 gap-3" data-topology-list>
      <p v-if="displayed.nodes.length === 0" class="py-6 text-center text-sm text-base-content/55">暂无连接数据</p>
      <template v-else>
        <p class="text-xs text-base-content/65 topology-list-summary">{{ summary }}</p>
        <section v-for="group in [{ title: '按出口', rows: exitRanking }, { title: '按规则', rows: ruleRanking }]" :key="group.title" class="min-w-0">
          <h3 class="mb-1 text-xs font-medium text-base-content/70">{{ group.title }}</h3>
          <ul class="text-sm">
            <li
              v-for="row in group.rows"
              :key="row.id"
              class="flex min-w-0 items-center justify-between gap-3 border-b-[0.5px] border-[color:var(--hairline)] py-1.5 last:border-b-0"
            >
              <span class="truncate" :class="{ 'text-base-content/65': row.merged }" :title="row.name">{{ row.name }}</span>
              <span class="shrink-0 text-xs tabular-nums text-base-content/65">{{ row.count }} 条</span>
            </li>
          </ul>
        </section>
      </template>
    </div>
  </section>
</template>

<style scoped>
@container overview (min-width: 640px) {
  .topology-list {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .topology-list-summary {
    grid-column: 1 / -1;
  }
}
</style>
