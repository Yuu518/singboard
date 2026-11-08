<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, computed, nextTick } from 'vue'
import * as echarts from 'echarts/core'
import { LineChart } from 'echarts/charts'
import { GridComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { HistoryPoint } from '@/stores/overview'
import { useConfigStore } from '@/stores/config'
import { inkColor, readChartInk } from '@/utils/chartTheme'

echarts.use([LineChart, GridComponent, CanvasRenderer])

const props = withDefaults(
  defineProps<{
    data: HistoryPoint[]
    min?: number
    labelFormatter?: (v: number) => string
    rightMargin?: number
  }>(),
  {
    min: 1,
  },
)

const { resolvedTheme } = useConfigStore()
const chartEl = ref<HTMLElement>()
const ink = ref(readChartInk())
let chart: echarts.ECharts | null = null
let resizeOb: ResizeObserver | null = null

const options = computed(() => ({
  animation: false,
  grid: {
    left: 0,
    top: 8,
    right: props.labelFormatter ? (props.rightMargin ?? 64) : 0,
    bottom: 0,
  },
  xAxis: {
    type: 'category' as const,
    show: false,
    boundaryGap: false,
  },
  yAxis: {
    type: 'value' as const,
    show: !!props.labelFormatter,
    position: 'right' as const,
    splitNumber: 2,
    min: 0,
    max: (value: { max: number }) => (Number.isFinite(value.max) ? Math.max(value.max, props.min) : props.min),
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: { show: false },
    axisLabel: props.labelFormatter
      ? {
          show: true,
          inside: false,
          fontSize: 12,
          color: inkColor(ink.value, 0.6),
          margin: 4,
          formatter: (value: number) => (value === 0 ? '' : props.labelFormatter!(value)),
        }
      : { show: false },
  },
  series: [
    {
      type: 'line' as const,
      symbol: 'none',
      smooth: true,
      lineStyle: { width: 1.5, color: inkColor(ink.value, 0.55) },
      data: props.data,
      color: inkColor(ink.value, 0.55),
      emphasis: { disabled: true },
      areaStyle: { color: inkColor(ink.value, 0.07) },
    },
  ],
}))

watch(resolvedTheme, () => {
  void nextTick(() => {
    ink.value = readChartInk()
  })
})

onMounted(() => {
  if (!chartEl.value) return
  chart = echarts.init(chartEl.value)
  chart.setOption(options.value)

  watch(options, () => {
    chart?.setOption(options.value)
  })

  resizeOb = new ResizeObserver(() => chart?.resize())
  resizeOb.observe(chartEl.value)
})

onUnmounted(() => {
  resizeOb?.disconnect()
  chart?.dispose()
})
</script>

<template>
  <div ref="chartEl" class="w-full h-full" aria-hidden="true" />
</template>
