import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (error: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

const calls = vi.hoisted(() => ({
  ipip: [] as Deferred<any>[],
  ipsb: [] as Deferred<any>[],
  ping: [] as Deferred<number>[],
}))

vi.mock('@/api/geoip', () => ({
  getIPFromIpipnet: () => {
    const d = deferred<any>()
    calls.ipip.push(d)
    return d.promise
  },
  getIPFromIpsb: () => {
    const d = deferred<any>()
    calls.ipsb.push(d)
    return d.promise
  },
}))

vi.mock('@/api/latency', () => {
  const ping = () => {
    const d = deferred<number>()
    calls.ping.push(d)
    return d.promise
  }
  return {
    getWechatLatency: ping,
    getBilibiliLatency: ping,
    getGithubLatency: ping,
    getCloudflareLatency: ping,
    getYoutubeLatency: ping,
  }
})

async function load() {
  const mod = await import('./network')
  return { ...mod, store: mod.useNetworkStore() }
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('network store', () => {
  beforeEach(() => {
    vi.resetModules()
    calls.ipip = []
    calls.ipsb = []
    calls.ping = []
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('checks both exits at the same time and keeps the reason for a failure', async () => {
    const { store } = await load()
    const done = store.checkIP()
    expect(calls.ipip).toHaveLength(1)
    expect(calls.ipsb).toHaveLength(1)
    expect(store.ip.value.domestic.status).toBe('pending')
    expect(store.ip.value.abroad.status).toBe('pending')
    expect(store.ipBusy.value).toBe(true)

    calls.ipsb[0].resolve({ ip: '2001:db8::1', country: 'Japan', organization: 'Example' })
    calls.ipip[0].reject(new Error('请求超时（30 秒）'))
    await done

    expect(store.ip.value.abroad).toEqual({ status: 'done', ip: '2001:db8::1', location: ['Japan', 'Example'], error: '' })
    expect(store.ip.value.domestic).toEqual({ status: 'error', ip: '', location: [], error: '请求超时（30 秒）' })
    expect(store.ipBusy.value).toBe(false)
  })

  it('treats a reply without an address as a failure', async () => {
    const { store } = await load()
    const done = store.checkIP()
    calls.ipip[0].resolve({ ip: '', location: [] })
    calls.ipsb[0].reject(new SyntaxError('Unexpected token'))
    await done
    expect(store.ip.value.domestic.error).toBe('返回内容里没有 IP')
    expect(store.ip.value.abroad.error).toBe('返回内容无法识别')
  })

  it('records latency in whole milliseconds and failures as zero with a reason', async () => {
    const { store } = await load()
    const done = store.checkLatency()
    expect(calls.ping).toHaveLength(5)
    calls.ping[0].resolve(41.6)
    calls.ping[1].reject(new Error('5 秒内无响应'))
    calls.ping.slice(2).forEach((d) => d.resolve(120))
    await done
    expect(store.latency.value.wechat).toEqual({ status: 'done', ms: 42, error: '' })
    expect(store.latency.value.bilibili).toEqual({ status: 'error', ms: 0, error: '5 秒内无响应' })
  })

  it('ignores a second click while a check is running', async () => {
    const { store } = await load()
    void store.checkLatency()
    void store.checkLatency()
    expect(calls.ping).toHaveLength(5)
  })

  it('runs the scheduled check once, after the warm-up delay', async () => {
    vi.useFakeTimers()
    const { store, AUTO_CHECK_DELAY_MS } = await load()
    store.scheduleAutoCheck()
    expect(store.ip.value.domestic.status).toBe('scheduled')
    expect(store.latency.value.github.status).toBe('scheduled')
    expect(calls.ipip).toHaveLength(0)

    vi.advanceTimersByTime(AUTO_CHECK_DELAY_MS)
    expect(calls.ipip).toHaveLength(1)
    expect(calls.ipsb).toHaveLength(1)
    expect(calls.ping).toHaveLength(5)
  })

  it('runs everything now when the user checks before the scheduled run', async () => {
    vi.useFakeTimers()
    const { store, AUTO_CHECK_DELAY_MS } = await load()
    store.scheduleAutoCheck()
    void store.checkIP()
    expect(calls.ipip).toHaveLength(1)
    expect(calls.ping).toHaveLength(5)
    vi.advanceTimersByTime(AUTO_CHECK_DELAY_MS * 2)
    expect(calls.ipip).toHaveLength(1)
    expect(calls.ping).toHaveLength(5)
  })

  it('discards results that arrive after the core stopped', async () => {
    const { store } = await load()
    void store.checkIP()
    store.reset()
    calls.ipip[0].resolve({ ip: '1.2.3.4', location: ['中国'] })
    calls.ipsb[0].resolve({ ip: '5.6.7.8', country: 'US', organization: '' })
    await flush()
    expect(store.ip.value.domestic.status).toBe('idle')
    expect(store.ip.value.abroad.ip).toBe('')
  })

  it('cancels a pending scheduled check on reset', async () => {
    vi.useFakeTimers()
    const { store, AUTO_CHECK_DELAY_MS } = await load()
    store.scheduleAutoCheck()
    store.reset()
    vi.advanceTimersByTime(AUTO_CHECK_DELAY_MS)
    expect(calls.ipip).toHaveLength(0)
    expect(store.ip.value.domestic.status).toBe('idle')
  })
})

describe('describePingError', () => {
  it('turns the raw timeout into words', async () => {
    const { describePingError } = await vi.importActual<typeof import('@/api/latency')>('@/api/latency')
    expect(describePingError('timeout')).toBe('5 秒内无响应')
    expect(describePingError(new Error('connection refused'))).toBe('connection refused')
    expect(describePingError('')).toBe('5 秒内无响应')
  })
})
