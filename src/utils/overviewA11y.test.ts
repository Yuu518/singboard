import { describe, expect, it } from 'vitest'
import { historySummary } from './overviewStatus'
import { ipAnnouncement, latencyAnnouncement } from './networkDisplay'
import { buildTopology, topologyRanking, topologySummary } from './topology'
import type { Connection } from '@/types'

describe('historySummary', () => {
  it('summarises only real samples', () => {
    const now = Date.now()
    const points = [{ name: 0, value: 0 }, { name: now, value: 10 }, { name: now + 1000, value: 30 }]
    expect(historySummary(points, (v) => `${v}`)).toBe('最近 2 秒峰值 30，平均 20')
    expect(historySummary([{ name: 1, value: 0 }], String)).toBe('还没有历史数据')
    expect(historySummary([{ name: now, value: null }, { name: now + 1, value: 4 }], String)).toBe('最近 1 秒峰值 4，平均 4')
  })
})

describe('network announcements', () => {
  it('reads IP results without the address while it is hidden', () => {
    const rows = [
      { label: '直连出口', result: { status: 'done' as const, ip: '1.2.3.4', location: ['中国', '广东'], error: '' } },
      { label: '代理出口', result: { status: 'error' as const, ip: '', location: [], error: '请求超时（30 秒）' } },
    ]
    expect(ipAnnouncement(rows, false)).toBe('出口 IP 检测完成：直连出口 中国；代理出口 检测失败：请求超时（30 秒）')
    expect(ipAnnouncement(rows, true)).toContain('直连出口 中国 广东 1.2.3.4')
  })

  it('reads latency with failure reasons', () => {
    expect(latencyAnnouncement([
      { label: '微信', result: { status: 'done', ms: 42, error: '' } },
      { label: 'GitHub', result: { status: 'error', ms: 0, error: '5 秒内无响应' } },
    ])).toBe('访问延迟检测完成：微信 42ms，GitHub 失败（5 秒内无响应）')
  })
})

describe('topology text alternative', () => {
  const conn = (exit: string, i: number) =>
    ({ id: String(i), metadata: { sourceIP: `10.0.0.${i}` }, rule: 'MATCH', rulePayload: '', chains: [exit] }) as unknown as Connection

  it('summarises connections, exits and the busiest exit', () => {
    const data = buildTopology([conn('A', 1), conn('A', 2), conn('B', 3)])
    expect(topologySummary(data)).toBe('3 条连接经 2 个出口，最多的是 A（2 条）')
    expect(topologySummary(buildTopology([]))).toBe('暂无连接')
  })

  it('counts merged exits and lists them last', () => {
    const data = buildTopology(Array.from({ length: 5 }, (_, i) => conn(`E${i}`, i)), 3)
    expect(topologySummary(data)).toContain('经 5 个出口')
    expect(topologyRanking(data, 3).at(-1)?.name).toBe('其他 3 个')
  })
})
