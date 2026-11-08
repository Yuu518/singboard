import { formatLatency, latencyColor } from '@/utils/format'
import type { IPResult, LatencyResult } from '@/stores/network'

export interface LatencyBadge {
  text: string
  tone: string
  busy: boolean
  reason: string
}

export function latencyBadge(result: LatencyResult): LatencyBadge {
  switch (result.status) {
    case 'scheduled':
    case 'pending':
      return { text: '检测中', tone: latencyColor(-1), busy: true, reason: '' }
    case 'done':
      return { text: formatLatency(result.ms), tone: latencyColor(result.ms), busy: false, reason: '' }
    case 'error':
      return { text: formatLatency(0), tone: latencyColor(0), busy: false, reason: result.error }
    default:
      return { text: formatLatency(-1), tone: latencyColor(-1), busy: false, reason: '' }
  }
}

export function maskIP(ip: string): string {
  return ip.replace(/[0-9a-fA-F]/g, '•')
}

export type IPViewState = 'idle' | 'waiting' | 'failed' | 'found'

export interface IPView {
  state: IPViewState
  message: string
  location: string
  ip: string
}

export function ipView(result: IPResult, revealed: boolean): IPView {
  switch (result.status) {
    case 'scheduled':
      return { state: 'waiting', message: '即将自动检测', location: '', ip: '' }
    case 'pending':
      return { state: 'waiting', message: '检测中…', location: '', ip: '' }
    case 'error':
      return { state: 'failed', message: result.error ? `检测失败：${result.error}` : '检测失败', location: '', ip: '' }
    case 'done':
      return {
        state: 'found',
        message: '',
        location: (revealed ? result.location : result.location.slice(0, 1)).join(' '),
        ip: revealed ? result.ip : maskIP(result.ip),
      }
    default:
      return { state: 'idle', message: '未检测', location: '', ip: '' }
  }
}

export function ipAnnouncement(rows: { label: string; result: IPResult }[], revealed: boolean): string {
  const parts = rows.map(({ label, result }) => {
    const view = ipView(result, revealed)
    if (view.state === 'found') return `${label} ${[view.location, revealed ? view.ip : ''].filter(Boolean).join(' ') || '已获取'}`
    return `${label} ${view.message}`
  })
  return `出口 IP 检测完成：${parts.join('；')}`
}

export function latencyAnnouncement(rows: { label: string; result: LatencyResult }[]): string {
  const parts = rows.map(({ label, result }) => {
    const badge = latencyBadge(result)
    return badge.reason ? `${label} ${badge.text}（${badge.reason}）` : `${label} ${badge.text}`
  })
  return `访问延迟检测完成：${parts.join('，')}`
}
