<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted, watch, nextTick } from 'vue'
import * as echarts from 'echarts/core'
import { SankeyChart } from 'echarts/charts'
import { TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { useConnectionsStore } from '@/stores/connections'
import { useConfigStore } from '@/stores/config'

echarts.use([SankeyChart, TooltipComponent, CanvasRenderer])

const { connections, start } = useConnectionsStore()
const { resolvedTheme } = useConfigStore()
const chartEl = ref<HTMLElement>()
const isPaused = ref(false)
let myChart: echarts.ECharts | null = null
let resizeOb: ResizeObserver | null = null

const layerColors = ['#6a6fc5', '#a8d4a0', '#fddb8a', '#f2a0a0']
const darkThemes = new Set(['dark', 'dracula'])
const labelColor = computed(() => (darkThemes.has(resolvedTheme.value) ? '#ffffff' : '#000000'))

const sankeyData = computed(() => {
  const conns = connections.value
  if (!conns || conns.length === 0) return { nodes: [], links: [] }

  const nodeMap = new Map<string, number>()
  const linkMap = new Map<string, number>()
  const layerMap = new Map<string, number>()
  const nodeTypeMap = new Map<string, string>()
  let idx = 0

  const addNode = (name: string, layer: number, type: string) => {
    if (!nodeMap.has(name)) {
      nodeMap.set(name, idx++)
      layerMap.set(name, layer)
      nodeTypeMap.set(name, type)
    }
    return nodeMap.get(name)!
  }

  conns.forEach((conn) => {
    const sourceIP = conn.metadata.sourceIP || 'unknown'
    const rulePayload = conn.rulePayload
      ? `${conn.rule}: ${conn.rulePayload}`
      : conn.rule
    const chains = conn.chains || []
    if (chains.length === 0) return

    const chainLast = chains[chains.length - 1]
    const chainFirst = chains[0]

    const srcNode = addNode(sourceIP, 0, '来源 IP')
    const ruleNode = addNode(rulePayload, 1, '规则匹配')

    if (chainFirst === chainLast) {
      const exitNode = addNode(chainFirst, 3, '代理出口')
      const l1 = `${srcNode}-${ruleNode}`
      const l2 = `${ruleNode}-${exitNode}`
      linkMap.set(l1, (linkMap.get(l1) || 0) + 1)
      linkMap.set(l2, (linkMap.get(l2) || 0) + 1)
    } else {
      const entryNode = addNode(chainLast, 2, '代理入口')
      const exitNode = addNode(chainFirst, 3, '代理出口')
      const l1 = `${srcNode}-${ruleNode}`
      const l2 = `${ruleNode}-${entryNode}`
      const l3 = `${entryNode}-${exitNode}`
      linkMap.set(l1, (linkMap.get(l1) || 0) + 1)
      linkMap.set(l2, (linkMap.get(l2) || 0) + 1)
      linkMap.set(l3, (linkMap.get(l3) || 0) + 1)
    }
  })

  const initialNodes = Array.from(nodeMap.entries()).map(([name, id]) => ({
    id,
    name,
    nodeType: nodeTypeMap.get(name) || '',
    layer: layerMap.get(name) || 0,
    itemStyle: { color: layerColors[layerMap.get(name) || 0] },
  }))

  const byLayer = new Map<number, typeof initialNodes>()
  initialNodes.forEach((n) => {
    if (!byLayer.has(n.layer)) byLayer.set(n.layer, [])
    byLayer.get(n.layer)!.push(n)
  })

  const idMap = new Map<number, number>()
  const sorted: typeof initialNodes = []
  let newId = 0
  Array.from(byLayer.keys()).sort((a, b) => a - b).forEach((layer) => {
    const nodes = byLayer.get(layer)!
    nodes.sort((a, b) => a.name.localeCompare(b.name))
    nodes.forEach((n) => {
      idMap.set(n.id, newId)
      sorted.push({ ...n, id: newId })
      newId++
    })
  })

  const links = Array.from(linkMap.entries()).map(([link, value]) => {
    const [oldSrc, oldTgt] = link.split('-').map(Number)
    return {
      source: idMap.get(oldSrc)!,
      target: idMap.get(oldTgt)!,
      value: Math.log10(value + 1) * 10,
      originalValue: value,
    }
  })

  return { nodes: sorted, links }
})

const options = computed(() => ({
  backgroundColor: 'transparent',
  tooltip: {
    trigger: 'item' as const,
    triggerOn: 'mousemove' as const,
    formatter: (params: any) => {
      if (params.dataType === 'node') {
        return `${params.data.name}<br/>类型: ${params.data.nodeType || '未知'}`
      }
      if (params.dataType === 'edge') {
        const src = sankeyData.value.nodes.find((n) => n.id === params.data.source)
        const tgt = sankeyData.value.nodes.find((n) => n.id === params.data.target)
        const count = params.data.originalValue || params.data.value
        if (src && tgt) return `${src.name} → ${tgt.name}<br/>连接数: ${count}`
        return `连接数: ${count}`
      }
      return ''
    },
  },
  series: [
    {
      type: 'sankey' as const,
      layout: 'none' as const,
      data: sankeyData.value.nodes,
      links: sankeyData.value.links,
      emphasis: { focus: 'trajectory' as const },
      lineStyle: { color: 'gradient' as const, curveness: 0.5 },
      itemStyle: { borderWidth: 0 },
      label: {
        color: labelColor.value,
        fontSize: 11,
        formatter: (params: { name: string }) => {
          return params.name.length > 30 ? params.name.substring(0, 30) + '...' : params.name
        },
      },
      nodeGap: 4,
      nodeWidth: 15,
      nodeAlign: 'left' as const,
      animation: true,
      animationDuration: 1000,
      animationEasing: 'cubicOut' as const,
      animationDelay: (idx: number) => idx * 50,
    },
  ],
}))

function updateChart() {
  if (!myChart || isPaused.value) return
  if (sankeyData.value.nodes.length > 0) {
    myChart.setOption(options.value)
  } else {
    myChart.clear()
  }
}

onMounted(() => {
  start()
  if (chartEl.value) {
    myChart = echarts.init(chartEl.value)
    myChart.setOption(options.value)

    myChart.on('showTip', () => { isPaused.value = true })
    myChart.on('hideTip', () => { isPaused.value = false })
  }

  watch(sankeyData, () => updateChart(), { deep: true })
  watch(resolvedTheme, () => updateChart())

  resizeOb = new ResizeObserver(() => {
    myChart?.resize()
  })
  if (chartEl.value) resizeOb.observe(chartEl.value)
})

onUnmounted(() => {
  resizeOb?.disconnect()
  resizeOb = null
  myChart?.dispose()
  myChart = null
})
</script>

<template>
  <div class="bg-base-200 rounded-lg p-4 space-y-3">
    <div class="flex items-center justify-between">
      <h2 class="text-sm font-semibold">连接拓扑</h2>
      <button class="btn btn-ghost btn-xs btn-circle" @click="isPaused = !isPaused">
        <svg v-if="!isPaused" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25v13.5m-7.5-13.5v13.5" />
        </svg>
        <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4">
          <path stroke-linecap="round" stroke-linejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.348a1.125 1.125 0 010 1.971l-11.54 6.347a1.125 1.125 0 01-1.667-.985V5.653z" />
        </svg>
      </button>
    </div>
    <div class="relative">
      <div ref="chartEl" class="h-80 w-full" />
      <div
        v-if="sankeyData.nodes.length === 0"
        class="absolute inset-0 flex items-center justify-center text-base-content/40 text-sm"
      >
        暂无连接数据
      </div>
    </div>
  </div>
</template>
