import type { OverviewLinkState } from '@/stores/overview'
import type { Proxy, ServiceStatus } from '@/types'

const MODE_LABELS: Record<string, string> = {
  rule: '规则',
  global: '全局',
  direct: '直连',
}

export function modeLabel(mode: string | null | undefined): string {
  if (!mode) return '—'
  return MODE_LABELS[mode.toLowerCase()] ?? mode
}

export interface CurrentExit {
  node: string
  via: string[]
  direct: boolean
}

function isSelector(proxy: Proxy | undefined): boolean {
  return !!proxy && proxy.type.toLowerCase() === 'selector' && !!proxy.all?.length
}

function walk(start: string, proxies: Record<string, Proxy>): string[] {
  const path = [start]
  const seen = new Set(path)
  let current = proxies[start]
  while (current?.now && !seen.has(current.now)) {
    path.push(current.now)
    seen.add(current.now)
    current = proxies[current.now]
  }
  return path
}

export function resolveCurrentExit(proxies: Record<string, Proxy>, mode: string | null | undefined): CurrentExit | null {
  const normalized = (mode ?? '').toLowerCase()
  if (normalized === 'direct') return { node: 'DIRECT', via: [], direct: true }
  let start: string | undefined
  if (normalized === 'global' && proxies.GLOBAL) {
    start = 'GLOBAL'
  } else {
    const order = proxies.GLOBAL?.all ?? Object.keys(proxies)
    start = order.find((name) => name !== 'GLOBAL' && isSelector(proxies[name]))
  }
  if (!start) return null
  const path = walk(start, proxies)
  const node = path[path.length - 1]
  if (!node || node === start && proxies[start]?.all?.length) return null
  return {
    node,
    via: path.slice(0, -1).filter((name) => name !== 'GLOBAL'),
    direct: node.toUpperCase() === 'DIRECT',
  }
}

export type CoreNoticeAction = 'start' | 'settings' | 'api' | null

export interface CoreNotice {
  title: string
  detail: string
  action: CoreNoticeAction
  busy: boolean
  alert: boolean
}

export function coreNotice(link: OverviewLinkState, serviceState: ServiceStatus['state'], failures: number): CoreNotice | null {
  switch (link) {
    case 'live':
    case 'connecting':
      return null
    case 'unreachable':
      return {
        title: '连不上 sing-box',
        detail: `已连续 ${failures} 次连接失败，正在自动重试。连上之前这里不显示数字。`,
        action: 'api',
        busy: false,
        alert: true,
      }
    case 'invalid':
      return { title: 'sing-box 连接地址无效', detail: '请在设置里修改 Clash API 地址。', action: 'api', busy: false, alert: true }
  }
  switch (serviceState) {
    case 'starting':
      return { title: '正在启动 sing-box…', detail: '核心启动后自动显示实时数据。', action: null, busy: true, alert: false }
    case 'stopping':
      return { title: '正在停止 sing-box…', detail: '服务停止后可以在这里重新启动。', action: null, busy: true, alert: false }
    case 'not_installed':
      return { title: '尚未安装 singboard 服务', detail: '在设置中安装服务后，即可启动 sing-box。', action: 'settings', busy: false, alert: false }
    case 'unknown':
      return { title: '无法获取服务状态', detail: '可以在设置中查看服务状态和错误日志。', action: 'settings', busy: false, alert: false }
    default:
      return { title: 'sing-box 未运行', detail: '启动服务后显示实时流量、出口和内存。', action: 'start', busy: false, alert: false }
  }
}

export function networkCounts(connections: readonly { metadata?: { network?: string } }[]) {
  let tcp = 0
  let udp = 0
  for (const conn of connections) {
    const network = conn.metadata?.network?.toLowerCase()
    if (network === 'tcp') tcp++
    else if (network === 'udp') udp++
  }
  return { tcp, udp }
}

export function historySummary(points: readonly { name: number; value: number | null }[], format: (value: number) => string): string {
  const samples = points
    .filter((point) => point.name > 1e12 && point.value !== null)
    .map((point) => point.value as number)
  if (samples.length === 0) return '还没有历史数据'
  const peak = Math.max(...samples)
  const average = samples.reduce((sum, value) => sum + value, 0) / samples.length
  return `最近 ${samples.length} 秒峰值 ${format(peak)}，平均 ${format(average)}`
}

export function exitLabel(mode: string | null | undefined): { label: string; hint: string } {
  const normalized = (mode ?? '').toLowerCase()
  if (normalized === 'global') return { label: '当前出口', hint: '全局模式下所有流量都走这个节点' }
  if (normalized === 'direct') return { label: '当前出口', hint: '直连模式下所有流量都不经过代理' }
  return { label: '默认代理', hint: '规则模式下按规则分流，这里是第一个代理组当前选中的节点，具体连接可能走别的出口' }
}
