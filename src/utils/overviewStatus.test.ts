import { describe, expect, it } from 'vitest'
import { coreNotice, exitLabel, modeLabel, networkCounts, resolveCurrentExit } from './overviewStatus'
import type { Proxy } from '@/types'

function group(name: string, all: string[], now: string, type = 'Selector'): Proxy {
  return { name, type, all, now, history: [] }
}

function node(name: string): Proxy {
  return { name, type: 'Shadowsocks', history: [] }
}

const proxies: Record<string, Proxy> = {
  GLOBAL: group('GLOBAL', ['DIRECT', '自动香港', '节点选择', '全球直连'], '日本 01'),
  节点选择: group('节点选择', ['自动香港', '日本 01'], '自动香港'),
  自动香港: group('自动香港', ['香港 IPLC 01', '香港 02'], '香港 IPLC 01', 'URLTest'),
  全球直连: group('全球直连', ['DIRECT'], 'DIRECT'),
  '香港 IPLC 01': node('香港 IPLC 01'),
  '日本 01': node('日本 01'),
  DIRECT: { name: 'DIRECT', type: 'Direct', history: [] },
}

describe('modeLabel', () => {
  it('speaks the mode in Chinese and keeps unknown modes as they are', () => {
    expect(modeLabel('Rule')).toBe('规则')
    expect(modeLabel('global')).toBe('全局')
    expect(modeLabel('direct')).toBe('直连')
    expect(modeLabel('script')).toBe('script')
    expect(modeLabel(null)).toBe('—')
  })
})

describe('resolveCurrentExit', () => {
  it('follows the first selector group in config order down to the node in rule mode', () => {
    expect(resolveCurrentExit(proxies, 'rule')).toEqual({ node: '香港 IPLC 01', via: ['节点选择', '自动香港'], direct: false })
  })

  it('uses GLOBAL in global mode', () => {
    expect(resolveCurrentExit(proxies, 'global')).toEqual({ node: '日本 01', via: [], direct: false })
  })

  it('reports direct mode without walking groups', () => {
    expect(resolveCurrentExit(proxies, 'direct')).toEqual({ node: 'DIRECT', via: [], direct: true })
  })

  it('marks a selector that resolves to DIRECT as direct', () => {
    const only = { 全球直连: proxies['全球直连'], DIRECT: proxies.DIRECT }
    expect(resolveCurrentExit(only, 'rule')).toEqual({ node: 'DIRECT', via: ['全球直连'], direct: true })
  })

  it('survives loops and missing groups', () => {
    const loop = {
      A: group('A', ['B'], 'B'),
      B: group('B', ['A'], 'A'),
    }
    expect(resolveCurrentExit(loop, 'rule')).toEqual({ node: 'B', via: ['A'], direct: false })
    expect(resolveCurrentExit({}, 'rule')).toBeNull()
  })
})

describe('coreNotice', () => {
  it('stays quiet while live or still connecting', () => {
    expect(coreNotice('live', 'running', 0)).toBeNull()
    expect(coreNotice('connecting', 'running', 1)).toBeNull()
  })

  it('offers the right action for each failure', () => {
    expect(coreNotice('stopped', 'stopped', 0)).toMatchObject({ title: 'sing-box 未运行', action: 'start', alert: false })
    expect(coreNotice('stopped', 'starting', 0)).toMatchObject({ action: null, busy: true })
    expect(coreNotice('stopped', 'not_installed', 0)?.action).toBe('settings')
    expect(coreNotice('unreachable', 'running', 4)).toMatchObject({ action: 'api', alert: true })
    expect(coreNotice('unreachable', 'running', 4)?.detail).toContain('已连续 4 次连接失败')
    expect(coreNotice('invalid', 'running', 0)?.title).toBe('sing-box 连接地址无效')
  })
})

describe('networkCounts', () => {
  it('counts TCP and UDP connections', () => {
    expect(networkCounts([
      { metadata: { network: 'tcp' } },
      { metadata: { network: 'TCP' } },
      { metadata: { network: 'udp' } },
      { metadata: {} },
    ])).toEqual({ tcp: 2, udp: 1 })
  })
})

describe('exitLabel', () => {
  it('only claims a current exit when every connection uses it', () => {
    expect(exitLabel('rule').label).toBe('默认代理')
    expect(exitLabel('Rule').hint).toContain('具体连接可能走别的出口')
    expect(exitLabel(null).label).toBe('默认代理')
    expect(exitLabel('global').label).toBe('当前出口')
    expect(exitLabel('direct').label).toBe('当前出口')
  })
})
