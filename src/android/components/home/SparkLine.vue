<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, computed } from 'vue'
import * as echarts from 'echarts/core'
import { LineChart } from 'echarts/charts'
import { GridComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { HistoryPoint } from '@/stores/overview'
import { usePageActive } from '@/android/composables/usePageActive'

echarts.use([LineChart, GridComponent, CanvasRenderer])

const props = withDefaults(
  defineProps<{
    data: HistoryPoint[]
    color: string
    min?: number
    labelFormatter?: (v: number) => string
    rightMargin?: number
  }>(),
  {
    min: 1,
  },
)

const pageActive = usePageActive()
const chartEl = ref<HTMLElement>()
let chart: echarts.ECharts | null = null
let resizeOb: ResizeObserver | null = null

const options = computed(() => ({
  animation: false,
  grid: {
    left: 0,
    top: 8,
    right: props.labelFormatter ? (props.rightMargin ?? 50) : 0,
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
    max: (value: { max: number }) => Math.max(value.max, props.min),
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: { show: false },
    axisLabel: props.labelFormatter
      ? {
          show: true,
          inside: false,
          fontSize: 9,
          color: '#9ca3af',
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
      lineStyle: { width: 1.5 },
      data: props.data,
      color: props.color,
      emphasis: { disabled: true },
      areaStyle: {
        color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
          { offset: 0, color: props.color + '60' },
          { offset: 1, color: props.color + '10' },
        ]),
      },
    },
  ],
}))

onMounted(() => {
  if (!chartEl.value) return
  chart = echarts.init(chartEl.value)
  chart.setOption(options.value)

  watch([options, pageActive], () => {
    if (pageActive.value) chart?.setOption(options.value)
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
  <div ref="chartEl" class="w-full h-full" />
</template>
