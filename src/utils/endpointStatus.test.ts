import { describe, expect, it } from 'vitest'
import { describeEndpointStatus, type EndpointStatusInput } from './endpointStatus'

const base: EndpointStatusInput = {
  probe: { state: 'idle' },
  socketState: 'idle',
  socketFailures: 0,
  coreRunning: true,
  address: '127.0.0.1:9090',
}

const describe_ = (patch: Partial<EndpointStatusInput>) => describeEndpointStatus({ ...base, ...patch })

describe('describeEndpointStatus', () => {
  it('shows a save in progress before anything else', () => {
    expect(describe_({ probe: { state: 'probing' }, socketState: 'open' })).toEqual({ tone: 'pending', text: '正在连接 127.0.0.1:9090…' })
  })

  it('reports connected from the live stream or a successful check', () => {
    expect(describe_({ socketState: 'open' }).tone).toBe('ok')
    expect(describe_({ probe: { state: 'ok' }, socketState: 'connecting' })).toEqual({ tone: 'ok', text: '已连接 127.0.0.1:9090' })
  })

  it('names the reason a check failed until the stream recovers', () => {
    expect(describe_({ probe: { state: 'error', reason: '认证失败 (401)' }, socketState: 'reconnecting' }))
      .toEqual({ tone: 'error', text: '连接失败：认证失败 (401)' })
    expect(describe_({ probe: { state: 'error', reason: 'x' }, socketState: 'open' }).tone).toBe('ok')
  })

  it('stays neutral while the core is stopped', () => {
    expect(describe_({ coreRunning: false, socketState: 'idle' })).toEqual({ tone: 'idle', text: '核心未运行，启动后会连接 127.0.0.1:9090' })
  })

  it('turns red after repeated stream failures', () => {
    expect(describe_({ socketState: 'reconnecting', socketFailures: 1 }).tone).toBe('pending')
    expect(describe_({ socketState: 'reconnecting', socketFailures: 3 }).tone).toBe('error')
    expect(describe_({ socketState: 'failed' }).tone).toBe('error')
  })
})
