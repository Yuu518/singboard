import { escapeHtml } from '@/utils/html'
import type { Connection } from '@/types'

export const TOPOLOGY_LAYERS = ['来源 IP', '规则', '策略组', '出口'] as const

export const DEFAULT_MAX_PER_LAYER = 8

export interface TopologyNode {
  id: string
  name: string
  depth: number
  nodeType: string
  count: number
  merged: number
}

export interface TopologyLink {
  source: string
  target: string
  value: number
  originalValue: number
}

export interface TopologyData {
  nodes: TopologyNode[]
  links: TopologyLink[]
  connectionCount: number
}

function nodeKey(depth: number, name: string) {
  return `${depth}\u0000${name}`
}

function connectionPath(conn: Connection): Array<{ depth: number; name: string }> | null {
  const chains = conn.chains || []
  if (chains.length === 0) return null
  const rule = conn.rulePayload ? `${conn.rule}: ${conn.rulePayload}` : conn.rule || '未知规则'
  const path = [
    { depth: 0, name: conn.metadata?.sourceIP || '未知来源' },
    { depth: 1, name: rule },
  ]
  const exit = chains[0]
  const entry = chains[chains.length - 1]
  if (entry !== exit) path.push({ depth: 2, name: entry })
  path.push({ depth: 3, name: exit })
  return path
}

export function buildTopology(connections: readonly Connection[], maxPerLayer = DEFAULT_MAX_PER_LAYER): TopologyData {
  const paths = connections.map(connectionPath).filter((path): path is NonNullable<typeof path> => !!path)
  const counts = new Map<string, { depth: number; name: string; count: number }>()
  for (const path of paths) {
    for (const step of path) {
      const key = nodeKey(step.depth, step.name)
      const entry = counts.get(key)
      if (entry) entry.count++
      else counts.set(key, { ...step, count: 1 })
    }
  }

  const remap = new Map<string, string>()
  const nodes: TopologyNode[] = []
  for (let depth = 0; depth < TOPOLOGY_LAYERS.length; depth++) {
    const layer = [...counts.entries()]
      .filter(([, entry]) => entry.depth === depth)
      .sort(([, a], [, b]) => b.count - a.count || a.name.localeCompare(b.name))
    const limit = Math.max(1, maxPerLayer)
    const keep = layer.length > limit ? layer.slice(0, limit - 1) : layer
    const rest = layer.slice(keep.length)
    const kept = keep
      .map(([key, entry]) => {
        remap.set(key, key)
        return { id: key, name: entry.name, depth, nodeType: TOPOLOGY_LAYERS[depth], count: entry.count, merged: 0 }
      })
      .sort((a, b) => a.name.localeCompare(b.name))
    nodes.push(...kept)
    if (rest.length > 0) {
      const otherId = nodeKey(depth, '\u0000other')
      rest.forEach(([key]) => remap.set(key, otherId))
      nodes.push({
        id: otherId,
        name: `其他 ${rest.length} 个`,
        depth,
        nodeType: TOPOLOGY_LAYERS[depth],
        count: rest.reduce((sum, [, entry]) => sum + entry.count, 0),
        merged: rest.length,
      })
    }
  }

  const linkCounts = new Map<string, number>()
  for (const path of paths) {
    for (let i = 1; i < path.length; i++) {
      const source = remap.get(nodeKey(path[i - 1].depth, path[i - 1].name))!
      const target = remap.get(nodeKey(path[i].depth, path[i].name))!
      const key = `${source}\u0001${target}`
      linkCounts.set(key, (linkCounts.get(key) || 0) + 1)
    }
  }
  const links = [...linkCounts.entries()].map(([key, count]) => {
    const [source, target] = key.split('\u0001')
    return { source, target, value: Math.log10(count + 1) * 10, originalValue: count }
  })

  return { nodes, links, connectionCount: paths.length }
}

export function topologyHeight(data: TopologyData, { min = 160, max = 320, row = 28, padding = 24 } = {}): number {
  const perLayer = new Map<number, number>()
  for (const node of data.nodes) perLayer.set(node.depth, (perLayer.get(node.depth) || 0) + 1)
  const tallest = Math.max(0, ...perLayer.values())
  return Math.min(max, Math.max(min, tallest * row + padding))
}

export interface TopologyTooltipNode {
  id: string
  name: string
  nodeType?: string
}

export interface TopologyTooltipParams {
  dataType?: string
  data?: {
    name?: unknown
    nodeType?: unknown
    count?: unknown
    source?: unknown
    target?: unknown
    value?: unknown
    originalValue?: unknown
  }
}

export function topologyTooltip(params: TopologyTooltipParams, nodes: readonly TopologyTooltipNode[]): string {
  const data = params.data ?? {}
  if (params.dataType === 'node') {
    const count = data.count != null ? `<br/>连接数: ${escapeHtml(data.count)}` : ''
    return `${escapeHtml(data.name)}<br/>类型: ${escapeHtml(data.nodeType || '未知')}${count}`
  }
  if (params.dataType === 'edge') {
    const src = nodes.find((n) => n.id === data.source)
    const tgt = nodes.find((n) => n.id === data.target)
    const count = escapeHtml(data.originalValue || data.value)
    if (src && tgt) return `${escapeHtml(src.name)} → ${escapeHtml(tgt.name)}<br/>连接数: ${count}`
    return `连接数: ${count}`
  }
  return ''
}

export function topologyRanking(data: TopologyData, depth: number): TopologyNode[] {
  return data.nodes
    .filter((node) => node.depth === depth)
    .sort((a, b) => Number(!!a.merged) - Number(!!b.merged) || b.count - a.count || a.name.localeCompare(b.name))
}

export function topologySummary(data: TopologyData): string {
  if (data.connectionCount === 0) return '暂无连接'
  const exits = data.nodes.filter((node) => node.depth === 3)
  const exitCount = exits.reduce((sum, node) => sum + (node.merged || 1), 0)
  const top = topologyRanking(data, 3)[0]
  const lead = top ? `，最多的是 ${top.name}（${top.count} 条）` : ''
  return `${data.connectionCount} 条连接经 ${exitCount} 个出口${lead}`
}
