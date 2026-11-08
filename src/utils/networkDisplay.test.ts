import { describe, expect, it } from 'vitest'
import { ipView, latencyBadge, maskIP } from './networkDisplay'
import { latencyColor } from './format'

const latency = (status: any, ms = -1, error = '') => ({ status, ms, error })
const ip = (status: any, extra: Partial<{ ip: string; location: string[]; error: string }> = {}) => ({
  status,
  ip: '',
  location: [],
  error: '',
  ...extra,
})

describe('latencyBadge', () => {
  it('uses the shared 300/800 scale and its colors', () => {
    expect(latencyBadge(latency('done', 120))).toEqual({ text: '120ms', tone: latencyColor(120), busy: false, reason: '' })
    expect(latencyBadge(latency('done', 450)).tone).toBe(latencyColor(450))
    expect(latencyBadge(latency('done', 900)).tone).toBe(latencyColor(900))
  })

  it('shows a failure in red with its reason instead of a fake number', () => {
    const badge = latencyBadge(latency('error', 0, '5 秒内无响应'))
    expect(badge.text).toBe('失败')
    expect(badge.text).not.toContain('ms')
    expect(badge.tone).toBe(latencyColor(0))
    expect(badge.reason).toBe('5 秒内无响应')
  })

  it('stays neutral while untested or running', () => {
    expect(latencyBadge(latency('idle')).text).toBe('未测')
    expect(latencyBadge(latency('pending'))).toMatchObject({ busy: true, tone: latencyColor(-1) })
    expect(latencyBadge(latency('scheduled')).busy).toBe(true)
  })
})

describe('ipView', () => {
  it('shows the full address and location when revealed', () => {
    const view = ipView(ip('done', { ip: '240e:3b7::1', location: ['中国', '广东', '电信'] }), true)
    expect(view).toEqual({ state: 'found', message: '', location: '中国 广东 电信', ip: '240e:3b7::1' })
  })

  it('hides digits and keeps only the country when hidden', () => {
    const view = ipView(ip('done', { ip: '203.0.113.7', location: ['中国', '广东'] }), false)
    expect(view.ip).toBe('•••.•.•••.•')
    expect(view.location).toBe('中国')
  })

  it('never invents a masked address for a failed or missing check', () => {
    const failed = ipView(ip('error', { error: '请求超时（30 秒）' }), false)
    expect(failed).toEqual({ state: 'failed', message: '检测失败：请求超时（30 秒）', location: '', ip: '' })
    expect(ipView(ip('idle'), false)).toMatchObject({ state: 'idle', message: '未检测', ip: '' })
    expect(ipView(ip('pending'), false)).toMatchObject({ state: 'waiting', ip: '' })
    expect(ipView(ip('scheduled'), false).message).toBe('即将自动检测')
  })
})

describe('maskIP', () => {
  it('keeps the shape of IPv4 and IPv6 addresses', () => {
    expect(maskIP('1.2.3.4')).toBe('•.•.•.•')
    expect(maskIP('2001:db8::ff')).toBe('••••:•••::••')
  })
})
