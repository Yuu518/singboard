import { describe, expect, it, vi } from 'vitest'
import { computed, createApp, h, nextTick, ref } from 'vue'

const state = vi.hoisted(() => ({ value: null as any }))

vi.mock('@/stores/network', () => ({ useNetworkStore: () => state.value }))

function makeStore() {
  const ip = ref({
    domestic: { status: 'error', ip: '', location: [], error: '请求超时（30 秒）' },
    abroad: { status: 'done', ip: '2001:0db8:85a3:0000:0000:8a2e:0370:7334', location: ['Japan', 'Example'], error: '' },
  })
  const latency = ref({
    wechat: { status: 'done', ms: 120, error: '' },
    bilibili: { status: 'error', ms: 0, error: '5 秒内无响应' },
    github: { status: 'pending', ms: -1, error: '' },
    cloudflare: { status: 'idle', ms: -1, error: '' },
    youtube: { status: 'done', ms: 950, error: '' },
  })
  const latencyBusy = ref(true)
  return {
    ip,
    latency,
    ipBusy: computed(() => false),
    latencyBusy,
    setLatencyBusy: (value: boolean) => {
      latencyBusy.value = value
    },
    checkAll: vi.fn(() => Promise.resolve()),
  }
}

async function mount(props: Record<string, unknown> = {}) {
  state.value = makeStore()
  const { default: NetworkInfo } = await import('./NetworkInfo.vue')
  const host = document.createElement('div')
  const app = createApp({ render: () => h(NetworkInfo, props) })
  app.mount(host)
  await nextTick()
  return { host, unmount: () => app.unmount() }
}

describe('NetworkInfo', () => {
  it('names the exits by what they measure', async () => {
    const { host, unmount } = await mount()
    const labels = [...host.querySelectorAll('dt')].map((dt) => {
      const copy = dt.cloneNode(true) as HTMLElement
      copy.querySelectorAll('.sr-only').forEach((node) => node.remove())
      return copy.textContent?.replace(/\s+/g, ' ').trim()
    })
    expect(labels.slice(0, 2)).toEqual(['直连出口 ipip.net', '代理出口 ip.sb'])
    expect(host.querySelector('[data-network-hint]')).toBeNull()
    expect(host.querySelector('h2')!.textContent).toBe('网络')
    expect(host.querySelector('h2')!.closest('.surface-card')).toBeNull()
    unmount()
  })

  it('gives a failure reason and never a fake mask', async () => {
    const { host, unmount } = await mount()
    const domestic = host.querySelector('[data-ip-row="domestic"]')!
    expect(domestic.textContent).toContain('检测失败：请求超时（30 秒）')
    expect(domestic.textContent).not.toMatch(/[*•]/)
    expect(domestic.textContent).not.toContain('()')
    unmount()
  })

  it('shows addresses in the mono face so long IPv6 wraps instead of hiding under buttons', async () => {
    const { host, unmount } = await mount()
    const abroad = host.querySelector('[data-ip-row="abroad"] .settings-mono')!
    expect(abroad.className).toContain('break-all')
    expect(abroad.textContent).toMatch(/^[•:]+$/)
    expect(host.querySelector('[data-ip-row="abroad"]')!.textContent).toContain('Japan')
    expect(host.querySelector('[data-ip-row="abroad"]')!.textContent).not.toContain('Example')
    unmount()
  })

  it('reads latency on the shared scale without stray units', async () => {
    const { host, unmount } = await mount()
    const text = (key: string) => host.querySelector(`[data-latency-row="${key}"]`)!.textContent!.replace(/\s+/g, ' ').trim()
    expect(text('wechat')).toBe('120ms')
    expect(text('bilibili')).toBe('5 秒内无响应失败')
    expect(text('github')).toBe('检测中')
    expect(text('cloudflare')).toBe('未测')
    expect(host.textContent).not.toContain('超时ms')
    expect(host.textContent).not.toContain('...ms')
    const dot = (key: string) => host.querySelector(`[data-latency-row="${key}"] .overview-dot`)?.className ?? ''
    expect(dot('wechat')).toContain('bg-success')
    expect(dot('youtube')).toContain('bg-error')
    expect(dot('bilibili')).toContain('bg-error')
    expect(dot('cloudflare')).toContain('border')
    expect(host.querySelector('[data-latency-row="github"] .loading')).not.toBeNull()
    unmount()
  })

  it('names every control after what it acts on', async () => {
    const { host, unmount } = await mount()
    const label = (action: string) => host.querySelector(`[data-action="${action}"]`)!.getAttribute('aria-label')
    expect(label('toggle-ip')).toBe('显示完整出口 IP')
    expect(label('check-all')).toBe('重新检测出口 IP 和访问延迟')
    const check = host.querySelector<HTMLButtonElement>('[data-action="check-all"]')!
    expect(check.textContent?.trim()).toBe('重新检测')
    expect(check.getAttribute('aria-busy')).toBe('true')
    check.click()
    expect(state.value.checkAll).not.toHaveBeenCalled()
    state.value.setLatencyBusy(false)
    await nextTick()
    check.click()
    expect(state.value.checkAll).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('announces results when a check finishes', async () => {
    const { host, unmount } = await mount()
    const region = host.querySelector('[data-network-announcement]')!
    expect(region.getAttribute('aria-live')).toBe('polite')
    state.value.latency.value.github = { status: 'done', ms: 200, error: '' }
    state.value.latency.value.cloudflare = { status: 'done', ms: 90, error: '' }
    state.value.setLatencyBusy(false)
    await nextTick()
    await nextTick()
    expect(region.textContent).toContain('访问延迟检测完成：微信 120ms，Bilibili 失败（5 秒内无响应）')
    unmount()
  })

  it('stops offering checks and routing claims while the core is stopped', async () => {
    const { host, unmount } = await mount({ running: false })
    expect(host.querySelector('[data-action="check-all"]')).toBeNull()
    expect(host.querySelector('[data-action="toggle-ip"]')).toBeNull()
    expect(host.querySelector('[data-network-hint]')!.textContent).toBe('sing-box 未运行，启动后自动检测')
    expect(host.textContent).not.toContain('按 sing-box 路由规则发出')
    unmount()
  })

  it('gives screen readers what each exit row measures', async () => {
    const { host, unmount } = await mount()
    const dt = host.querySelector('dt')!
    expect(dt.querySelector('.sr-only')!.textContent).toContain('国内站点 myip.ipip.net 看到的地址')
    unmount()
  })
})
