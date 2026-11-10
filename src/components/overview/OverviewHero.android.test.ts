import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick, ref } from 'vue'

const push = vi.hoisted(() => vi.fn())
const run = vi.hoisted(() => vi.fn())
const busy = vi.hoisted(() => ({ value: null as any }))
const rootMode = vi.hoisted(() => ({ value: true }))
const apiState = vi.hoisted(() => ({ value: 'ready' }))
const status = vi.hoisted(() => ({ value: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))
vi.mock('@/api', () => ({ fetchConfig: vi.fn(() => Promise.resolve({ data: { mode: 'rule' } })) }))
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({
    config: ref({ rootMode: rootMode.value, workingDir: '/data/adb/sing-box' }),
    clashApiUrl: ref('http://192.168.1.1:9090'),
    activeClashApi: ref({ name: '路由器', url: 'http://192.168.1.1:9090' }),
    activeConfigProfile: ref(undefined),
  }),
}))
vi.mock('@/stores/proxies', () => ({
  useProxiesStore: () => ({ proxyMap: ref({}), getLatency: () => -1, loadProxies: vi.fn() }),
}))
vi.mock('@/stores/service', () => ({
  useServiceStore: () => ({
    serviceStatus: status.value,
    statusText: ref(rootMode.value ? '运行中' : '已连接'),
    clashApiState: ref(apiState.value),
    refresh: vi.fn(),
  }),
}))
vi.mock('@/stores/singboxVersion', () => ({ useSingboxVersionStore: () => ({ singboxVersion: ref('1.12.4') }) }))
vi.mock('@/stores/toast', () => ({ useToastStore: () => ({ pushToast: vi.fn() }) }))
vi.mock('@/utils/coreControl', () => ({ startCore: vi.fn() }))
vi.mock('@/bridge/service', () => ({ isElevationCancelled: () => false }))
vi.mock('@/android/composables/useCoreActions', () => ({ useCoreActions: () => ({ busy: busy.value, run }) }))

async function render(props: Record<string, unknown>) {
  const { default: Hero } = await import('./OverviewHero.vue')
  const host = document.createElement('div')
  const merged: any = {
    serviceState: 'running',
    failures: 0,
    traffic: { up: 0, down: 0 },
    totals: { up: 0, down: 0 },
    downHistory: [],
    upHistory: [],
    ...props,
  }
  const app = createApp({ render: () => h(Hero, merged) })
  app.mount(host)
  await new Promise((resolve) => setTimeout(resolve, 0))
  await nextTick()
  return { host, unmount: () => app.unmount() }
}

const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, ' ').trim() ?? ''

describe('OverviewHero on Android', () => {
  beforeEach(() => {
    vi.resetModules()
    push.mockReset()
    run.mockReset()
    busy.value = ref('')
    rootMode.value = true
    apiState.value = 'ready'
    status.value = ref({ state: 'running', uptimeSeconds: 65 })
  })

  it('puts restart and power next to the state in root mode, with the core version', async () => {
    const { host, unmount } = await render({ link: 'live' })
    expect(host.querySelector('[data-hero-config]')).toBeNull()
    expect(text(host.querySelector('[data-hero-meta]'))).toBe('· 1.12.4')
    host.querySelector<HTMLButtonElement>('[data-hero-control="restart"]')!.click()
    host.querySelector<HTMLButtonElement>('[data-hero-control="stop"]')!.click()
    expect(run.mock.calls.map(([action]) => action)).toEqual(['restart', 'stop'])
    unmount()
  })

  it('starts sing-box from the notice when the core is stopped', async () => {
    status.value = ref({ state: 'stopped' })
    const { host, unmount } = await render({ link: 'stopped', serviceState: 'stopped', traffic: null, totals: null })
    expect(host.querySelector('[data-hero-control]')).toBeNull()
    const button = host.querySelector<HTMLButtonElement>('[data-hero-notice] button')!
    expect(button.textContent?.trim()).toBe('启动 sing-box')
    button.click()
    expect(run).toHaveBeenCalledWith('start')
    unmount()
  })

  it('shows the endpoint and no controls for a remote core', async () => {
    rootMode.value = false
    status.value = ref({ state: 'running' })
    const { host, unmount } = await render({ link: 'live' })
    expect(text(host.querySelector('[data-hero-state]'))).toBe('已连接')
    expect(text(host.querySelector('[data-hero-meta]'))).toBe('· 路由器')
    expect(host.querySelector('[data-hero-control]')).toBeNull()
    unmount()
  })

  it('sends a remote panel that lost its core to the API settings', async () => {
    rootMode.value = false
    status.value = ref({ state: 'stopped' })
    const { host, unmount } = await render({ link: 'stopped', serviceState: 'stopped', traffic: null, totals: null })
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('连不上 sing-box')
    expect(text(host.querySelector('[data-hero-notice]'))).toContain('http://192.168.1.1:9090')
    host.querySelector<HTMLButtonElement>('[data-hero-notice] button')!.click()
    expect(push).toHaveBeenCalledWith('/settings')
    unmount()
  })

  it('names a wrong secret instead of waiting forever', async () => {
    apiState.value = 'auth_failed'
    const { host, unmount } = await render({ link: 'connecting', traffic: null, totals: null })
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('Clash API 密钥不匹配')
    expect(host.querySelector('[data-hero-connecting]')!.textContent).toBe('')
    unmount()
  })
})
