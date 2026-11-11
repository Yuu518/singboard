import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick, ref } from 'vue'

const push = vi.hoisted(() => vi.fn())
const fetchConfig = vi.hoisted(() => vi.fn())
const loadProxies = vi.hoisted(() => vi.fn())
const startCore = vi.hoisted(() => vi.fn())
const profile = vi.hoisted(() => ({ value: null as any }))
const status = vi.hoisted(() => ({ value: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))
vi.mock('@/api', () => ({ fetchConfig }))
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({
    config: ref({ rootMode: false }),
    clashApiUrl: ref('http://127.0.0.1:9090'),
    activeClashApi: ref({ name: '本机', url: 'http://127.0.0.1:9090' }),
    activeConfigProfile: profile.value,
  }),
}))
vi.mock('@/stores/proxies', () => ({
  useProxiesStore: () => ({
    proxyMap: ref({
      GLOBAL: { name: 'GLOBAL', type: 'Selector', all: ['节点选择'], now: '节点选择', history: [] },
      节点选择: { name: '节点选择', type: 'Selector', all: ['香港 01'], now: '香港 01', history: [] },
      '香港 01': { name: '香港 01', type: 'Trojan', history: [{ time: '', delay: 86 }] },
    }),
    getLatency: () => 86,
    loadProxies,
  }),
}))
vi.mock('@/stores/service', () => ({
  useServiceStore: () => ({
    serviceStatus: status.value,
    statusText: ref('运行中'),
    clashApiState: ref('ready'),
    refresh: vi.fn(),
  }),
}))
vi.mock('@/stores/singboxVersion', () => ({ useSingboxVersionStore: () => ({ singboxVersion: ref('1.12.4') }) }))
vi.mock('@/stores/toast', () => ({ useToastStore: () => ({ pushToast: vi.fn() }) }))
vi.mock('@/utils/coreControl', () => ({ startCore }))
vi.mock('@/bridge/service', () => ({ isElevationCancelled: () => false }))
vi.mock('@/android/composables/useCoreActions', () => ({ useCoreActions: () => { throw new Error('desktop must not use core actions') } }))

const now = Date.now()
const history = (values: number[]) => values.map((value, i) => ({ name: now + i * 1000, value }))

async function render(props: Record<string, unknown>) {
  const { default: Hero } = await import('./OverviewHero.vue')
  const host = document.createElement('div')
  const merged: any = {
    serviceState: 'running',
    failures: 0,
    traffic: { up: 2048, down: 3 * 1024 * 1024 },
    totals: { up: 1024 * 1024, down: 20 * 1024 * 1024 },
    downHistory: history([1024, 3 * 1024 * 1024]),
    upHistory: history([512, 2048]),
    ...props,
  }
  const app = createApp({ render: () => h(Hero, merged) })
  app.mount(host)
  await new Promise((resolve) => setTimeout(resolve, 0))
  await nextTick()
  return { host, unmount: () => app.unmount() }
}

const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, ' ').trim() ?? ''

describe('OverviewHero', () => {
  beforeEach(() => {
    push.mockReset()
    startCore.mockReset()
    fetchConfig.mockReset().mockResolvedValue({ data: { mode: 'rule' } })
    loadProxies.mockReset()
    profile.value = ref({ id: 'a', name: '机场 A', type: 'remote', source: 'https://x', autoUpdateInterval: 0, lastUpdated: new Date().toISOString() })
    status.value = ref({ state: 'running', uptimeSeconds: 7 * 60 + 5 })
  })

  it('answers health, exit and speed while live', async () => {
    const { host, unmount } = await render({ link: 'live' })
    expect(text(host.querySelector('[data-hero-state]'))).toBe('运行中 7:05')
    expect(text(host.querySelector('[data-hero-mode]'))).toBe('规则模式，打开设置页')
    expect(text(host.querySelector('[data-hero-meta]'))).toBe('1.12.4')
    expect(text(host.querySelector('[data-hero-exit-label]'))).toBe('默认代理')
    const exit = host.querySelector('[data-hero-exit]')!
    expect(text(exit.querySelector('.hero-node-name'))).toBe('香港 01')
    expect(text(exit.querySelector('.hero-latency'))).toBe('86ms')
    expect(exit.querySelector('.hero-latency .overview-dot')!.className).toContain('bg-success')
    expect(text(exit.querySelector('.hero-via'))).toBe('节点选择')
    expect(exit.querySelector('button')!.getAttribute('title')).toContain('具体连接可能走别的出口')
    expect(text(host.querySelector('[data-rate="down"] .hero-rate-value'))).toBe('3MB/s')
    expect(text(host.querySelector('[data-rate="down"] .hero-rate-total'))).toBe('总计 20 MB')
    expect(text(host.querySelector('[data-rate="up"] .hero-rate-total'))).toBe('总计 1 MB')
    expect(host.querySelector('[data-traffic-wave] path')).not.toBeNull()
    expect(text(host.querySelector('[data-hero-config-name]'))).toBe('机场 A')
    expect(host.querySelector('[data-hero-control]')).toBeNull()
    expect(loadProxies).toHaveBeenCalled()
    unmount()
  })

  it('speaks the speeds with their recent history and opens connections', async () => {
    const { host, unmount } = await render({ link: 'live' })
    const rates = host.querySelector<HTMLButtonElement>('[data-hero-rates] button')!
    expect(rates.getAttribute('aria-label')).toContain('下载 3 MB/s，最近 2 秒峰值 3 MB/s')
    expect(rates.getAttribute('aria-label')).toMatch(/上传 2 KB\/s.*。查看连接$/)
    expect(host.querySelector('[data-hero-rates] .hero-rates-grid')!.getAttribute('aria-hidden')).toBe('true')
    rates.click()
    unmount()
    expect(push).toHaveBeenCalledWith('/connections')
  })

  it('opens the related page from the exit, mode and config', async () => {
    const { host, unmount } = await render({ link: 'live' })
    host.querySelector<HTMLButtonElement>('[data-hero-exit] button')!.click()
    host.querySelector<HTMLButtonElement>('[data-hero-mode]')!.click()
    host.querySelector<HTMLButtonElement>('[data-hero-config]')!.click()
    expect(push.mock.calls.map(([path]) => path)).toEqual(['/proxies', '/settings', '/config'])
    unmount()
  })

  it('replaces the exit with a start action when sing-box is stopped', async () => {
    status.value = ref({ state: 'stopped' })
    const { host, unmount } = await render({ link: 'stopped', serviceState: 'stopped', traffic: null, totals: null })
    const title = host.querySelector('[data-hero-notice-title]')!
    expect(title.getAttribute('role')).toBe('status')
    expect(title.textContent).toBe('sing-box 未运行')
    const notice = host.querySelector('[data-hero-notice]')!
    expect(notice.querySelector('button')?.textContent?.trim()).toBe('启动服务')
    expect(host.querySelector('[data-hero-exit]')).toBeNull()
    expect(text(host.querySelector('[data-rate="down"] .hero-rate-value'))).toBe('—')
    expect(text(host.querySelector('[data-rate="down"] .hero-rate-total'))).toBe('总计 —')
    expect(host.querySelector('[data-traffic-wave] path')).toBeNull()
    expect(text(host.querySelector('[data-hero-config-name]'))).toBe('机场 A')
    expect(fetchConfig).not.toHaveBeenCalled()
    notice.querySelector('button')!.click()
    expect(startCore).toHaveBeenCalled()
    unmount()
  })

  it('names the API address after repeated failures, with only the title in the live region', async () => {
    const { host, unmount } = await render({ link: 'unreachable', failures: 3, traffic: null, totals: null })
    const notice = host.querySelector('[data-hero-notice]')!
    expect(host.querySelectorAll('[role="alert"]')).toHaveLength(1)
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('连不上 sing-box')
    expect(notice.querySelector('[data-hero-notice-detail]')!.closest('[role]')).toBeNull()
    expect(notice.textContent).toContain('已连续 3 次连接失败')
    expect(notice.textContent).toContain('http://127.0.0.1:9090')
    expect(notice.querySelector('button')?.textContent?.trim()).toBe('检查 API 配置')
    expect(host.querySelector('[data-hero-notice-dot]')!.className).toContain('bg-error')
    unmount()
  })

  it('keeps the state line while connecting but hides stale details', async () => {
    const { host, unmount } = await render({ link: 'connecting', traffic: null, totals: null })
    expect(text(host.querySelector('[data-hero-state]'))).toBe('正在连接 sing-box…')
    expect(host.querySelector('[data-hero-connecting]')!.textContent).toBe('正在连接 sing-box…')
    expect(host.querySelector('[data-hero-mode]')).toBeNull()
    expect(text(host.querySelector('[data-hero-exit] .hero-node'))).toBe('—')
    unmount()
  })

  it('asks to pick a config when none is active and flags a failed update', async () => {
    profile.value = ref(undefined)
    const empty = await render({ link: 'live' })
    expect(text(empty.host.querySelector('[data-hero-config]'))).toContain('未选择配置')
    expect(text(empty.host.querySelector('[data-hero-config]'))).toContain('前往配置页选择')
    empty.unmount()

    profile.value = ref({ id: 'b', name: '机场 B', type: 'remote', source: 'https://x', autoUpdateInterval: 0, lastFetchError: 'boom' })
    const failed = await render({ link: 'live' })
    expect(text(failed.host.querySelector('[data-hero-config]'))).toContain('更新失败')
    failed.unmount()
  })

  it('calls the exit the current exit in global mode', async () => {
    fetchConfig.mockResolvedValue({ data: { mode: 'global' } })
    const { host, unmount } = await render({ link: 'live' })
    expect(text(host.querySelector('[data-hero-exit-label]'))).toBe('当前出口')
    expect(text(host.querySelector('[data-hero-mode]'))).toBe('全局模式，打开设置页')
    unmount()
  })

  it('draws a neutral dot when the service state is unknown', async () => {
    const { host, unmount } = await render({ link: 'stopped', serviceState: 'unknown', traffic: null, totals: null })
    expect(host.querySelector('[data-hero-notice-dot]')!.className).toContain('hero-dot-neutral')
    unmount()
  })
})
