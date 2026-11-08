import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick, ref } from 'vue'

const push = vi.hoisted(() => vi.fn())
const fetchConfig = vi.hoisted(() => vi.fn())
const loadProxies = vi.hoisted(() => vi.fn())
const profile = vi.hoisted(() => ({ value: null as any }))
const status = vi.hoisted(() => ({ value: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))
vi.mock('@/api', () => ({ fetchConfig }))
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({ clashApiUrl: ref('http://127.0.0.1:9090'), activeConfigProfile: profile.value }),
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
vi.mock('@/stores/service', () => ({ useServiceStore: () => ({ serviceStatus: status.value, refresh: vi.fn() }) }))
vi.mock('@/stores/toast', () => ({ useToastStore: () => ({ pushToast: vi.fn() }) }))
vi.mock('@/utils/coreControl', () => ({ startCore: vi.fn() }))
vi.mock('@/bridge/service', () => ({ isElevationCancelled: () => false }))

async function render(props: Record<string, unknown>) {
  const { default: Band } = await import('./OverviewStatusBand.vue')
  const host = document.createElement('div')
  const merged: any = { serviceState: 'running', failures: 0, memory: { inuse: 50 * 1024 * 1024, oslimit: 0 }, ...props }
  const app = createApp({ render: () => h(Band, merged) })
  app.mount(host)
  await new Promise((resolve) => setTimeout(resolve, 0))
  await nextTick()
  return { host, unmount: () => app.unmount() }
}

const cell = (host: HTMLElement, name: string) => host.querySelector(`[data-band-cell="${name}"]`) as HTMLElement

describe('OverviewStatusBand', () => {
  beforeEach(() => {
    push.mockReset()
    fetchConfig.mockReset().mockResolvedValue({ data: { mode: 'rule' } })
    loadProxies.mockReset()
    profile.value = ref({ id: 'a', name: '机场 A', type: 'remote', source: 'https://x', autoUpdateInterval: 0, lastUpdated: new Date().toISOString() })
    status.value = ref({ state: 'running', uptimeSeconds: 7 * 60 + 5 })
  })

  it('shows uptime, memory, mode, exit and config while live', async () => {
    const { host, unmount } = await render({ link: 'live' })
    expect(cell(host, 'core').textContent).toContain('运行中')
    expect(cell(host, 'core').textContent).toContain('7:05')
    expect(cell(host, 'core').textContent).toContain('50 MB')
    expect(cell(host, 'mode').textContent).toContain('规则')
    expect(cell(host, 'exit').querySelector('.overview-band-label')!.textContent).toBe('默认代理')
    expect(cell(host, 'exit').getAttribute('title')).toContain('具体连接可能走别的出口')
    expect(cell(host, 'exit').textContent).toContain('香港 01')
    expect(cell(host, 'exit').textContent).toContain('86ms')
    expect(cell(host, 'exit').textContent).toContain('经 节点选择')
    expect(cell(host, 'config').textContent).toContain('机场 A')
    expect(cell(host, 'config').textContent).toContain('更新于 刚刚')
    expect(loadProxies).toHaveBeenCalled()
    unmount()
  })

  it('opens the related page from each cell', async () => {
    const { host, unmount } = await render({ link: 'live' })
    cell(host, 'exit').click()
    cell(host, 'config').click()
    cell(host, 'mode').click()
    expect(push.mock.calls.map(([path]) => path)).toEqual(['/proxies', '/config', '/settings'])
    unmount()
  })

  it('replaces the core cell with a start action when sing-box is stopped', async () => {
    status.value = ref({ state: 'stopped' })
    const { host, unmount } = await render({ link: 'stopped', serviceState: 'stopped', memory: null })
    const notice = host.querySelector('[data-band-notice]')!
    const title = notice.querySelector('[data-band-notice-title]')!
    expect(title.getAttribute('role')).toBe('status')
    expect(title.textContent).toBe('sing-box 未运行')
    expect(notice.querySelector('button')?.textContent?.trim()).toBe('启动服务')
    expect(cell(host, 'core')).toBeNull()
    expect(cell(host, 'mode').textContent).toContain('—')
    expect(cell(host, 'exit').textContent).toContain('—')
    expect(cell(host, 'config').textContent).toContain('机场 A')
    expect(fetchConfig).not.toHaveBeenCalled()
    unmount()
  })

  it('names the API address after repeated failures', async () => {
    const { host, unmount } = await render({ link: 'unreachable', failures: 3 })
    const notice = host.querySelector('[data-band-notice]')!
    const title = notice.querySelector('[role="alert"]')!
    expect(title.textContent).toBe('连不上 sing-box')
    expect(notice.querySelectorAll('[role="alert"]')).toHaveLength(1)
    expect(notice.querySelector('[data-band-notice-detail]')!.closest('[role]')).toBeNull()
    expect(notice.textContent).toContain('已连续 3 次连接失败')
    expect(notice.textContent).toContain('http://127.0.0.1:9090')
    expect(notice.querySelector('button')?.textContent?.trim()).toBe('检查 API 配置')
    unmount()
  })

  it('keeps the core cell while connecting but hides stale details', async () => {
    const { host, unmount } = await render({ link: 'connecting' })
    expect(cell(host, 'core').textContent).toContain('正在连接 sing-box')
    expect(cell(host, 'core').querySelector('[role]')).toBeNull()
    expect(host.querySelector('[data-band-connecting]')!.textContent).toBe('正在连接 sing-box…')
    expect(cell(host, 'core').textContent).not.toContain('MB')
    expect(cell(host, 'exit').textContent).toContain('—')
    unmount()
  })

  it('asks to pick a config when none is active', async () => {
    profile.value = ref(undefined)
    const { host, unmount } = await render({ link: 'live' })
    expect(cell(host, 'config').textContent).toContain('未选择')
    expect(cell(host, 'config').textContent).toContain('前往配置页选择')
    unmount()
  })

  it('truncates a long config name inside its own cell and keeps the full name as a tooltip', async () => {
    const long = '一个非常非常长的远程订阅配置名称用来测试状态带不会溢出到相邻单元格'
    profile.value = ref({ id: 'a', name: long, type: 'local', source: 'C:/a.json', autoUpdateInterval: 0 })
    const { host, unmount } = await render({ link: 'live' })
    const name = host.querySelector('[data-band-config-name]')!
    expect(name.className).toContain('truncate')
    expect(name.getAttribute('title')).toBe(long)
    expect(name.parentElement!.className).toContain('overview-band-value')
    unmount()
  })

  it('calls the exit the current exit in global mode', async () => {
    fetchConfig.mockResolvedValue({ data: { mode: 'global' } })
    const { host, unmount } = await render({ link: 'live' })
    expect(cell(host, 'exit').querySelector('.overview-band-label')!.textContent).toBe('当前出口')
    expect(cell(host, 'mode').textContent).toContain('全局')
    unmount()
  })

  it('marks an unreachable core red, not with the slow orange', async () => {
    const { host, unmount } = await render({ link: 'unreachable', failures: 3 })
    const dot = host.querySelector('[data-band-notice-dot]')!
    expect(dot.className).toContain('bg-error')
    expect(dot.className).not.toContain('bg-warning')
    expect(host.textContent).not.toContain('不显示过期数字')
    unmount()
  })

  it('draws a neutral dot when the service state is unknown', async () => {
    const { host, unmount } = await render({ link: 'stopped', serviceState: 'unknown', memory: null })
    expect(host.querySelector('[data-band-notice-dot]')!.className).toContain('status-dot-neutral')
    unmount()
  })
})
