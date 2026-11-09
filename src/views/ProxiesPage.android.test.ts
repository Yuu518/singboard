import { describe, expect, it, vi } from 'vitest'
import { computed, createApp, nextTick, ref } from 'vue'
import { PAGE_ACTIVE } from '@/android/composables/usePageActive'

const mocks = vi.hoisted(() => ({
  clashApiReady: null as any,
  loadProxies: null as any,
  fetchProxyProviders: null as any,
  startConnections: null as any,
}))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/components/common/ServiceNotice.vue', async () => {
  const { h } = await import('vue')
  return { default: { render: () => h('div', { 'data-service-notice': '' }) } }
})
vi.mock('@/stores/appVisible', async () => {
  const { ref } = await import('vue')
  return { appVisible: ref(true) }
})
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref('stopped'), isRunning: ref(false) }),
}))
vi.mock('@/stores/service', () => ({
  useServiceStore: () => ({ serviceStatus: ref({ state: 'running' }), clashApiReady: mocks.clashApiReady }),
}))
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({ config: ref({ ipv6TestEnabled: false }), clashApiUrl: ref('http://127.0.0.1:9090') }),
}))
vi.mock('@/stores/connections', async () => {
  const { ref } = await import('vue')
  return { chainSpeeds: ref({}), useConnectionsStore: () => ({ start: mocks.startConnections }) }
})
vi.mock('@/api', () => ({
  fetchProxyProviders: (...args: unknown[]) => mocks.fetchProxyProviders(...args),
  updateProxyProvider: vi.fn(),
  healthCheckProvider: vi.fn(),
}))
vi.mock('@/stores/proxies', async () => {
  const { ref } = await import('vue')
  const groups = ref([{ name: 'Proxy', type: 'Selector', all: ['HK 01', 'JP 01'], now: 'HK 01', history: [] }])
  return {
    useProxiesStore: () => ({
      proxyGroups: groups,
      loading: ref(false),
      loaded: ref(true),
      loadError: ref(''),
      loadProxies: mocks.loadProxies,
      switchProxy: vi.fn(),
      testGroupNodes: vi.fn(),
      testNodeLatency: vi.fn(),
      getProxy: (name: string) => (name === 'Proxy' ? groups.value[0] : { name, type: 'Hysteria2', history: [] }),
      getLatency: () => 80,
      getLatencyTime: () => '',
      getTestUrl: () => '',
      isIPv6: () => false,
    }),
  }
})

async function mountPage(ready: boolean) {
  mocks.clashApiReady = ref(ready)
  mocks.loadProxies = vi.fn(() => Promise.resolve())
  mocks.fetchProxyProviders = vi.fn(() => Promise.resolve({ data: { providers: {} } }))
  mocks.startConnections = vi.fn()
  localStorage.setItem('singboard-expanded-groups', JSON.stringify(['Proxy']))
  const { default: ProxiesPage } = await import('./ProxiesPage.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(ProxiesPage)
  app.provide(PAGE_ACTIVE, computed(() => true))
  app.mount(host)
  await nextTick()
  return { host, unmount: () => { app.unmount(); host.remove(); localStorage.clear() } }
}

describe('ProxiesPage on Android', () => {
  it('waits for the Clash API before loading proxies, providers and group speeds', async () => {
    const { unmount } = await mountPage(false)
    expect(mocks.loadProxies).not.toHaveBeenCalled()
    expect(mocks.fetchProxyProviders).not.toHaveBeenCalled()

    mocks.clashApiReady.value = true
    await nextTick()
    expect(mocks.loadProxies).toHaveBeenCalledWith(false)
    expect(mocks.fetchProxyProviders).toHaveBeenCalled()
    expect(mocks.startConnections).toHaveBeenCalled()

    mocks.clashApiReady.value = false
    await nextTick()
    mocks.clashApiReady.value = true
    await nextTick()
    expect(mocks.loadProxies).toHaveBeenLastCalledWith(true)
    unmount()
  })

  it('lays nodes out in narrow tiles inside a panel that scrolls under the bottom navigation', async () => {
    const { host, unmount } = await mountPage(true)
    expect(host.querySelector('[data-service-notice]')).toBeNull()
    const panel = host.querySelector<HTMLElement>('#proxy-panel-groups')!
    expect(panel.classList).toContain('under-nav')
    expect(panel.classList).not.toContain('pb-2')

    const grid = host.querySelector<HTMLElement>('[data-group-panel="Proxy"]')!
    expect(grid.style.gridTemplateColumns).toContain('128px')
    const tiles = grid.querySelectorAll<HTMLElement>('[data-node]')
    expect(tiles).toHaveLength(2)
    expect(tiles[0].classList).toContain('min-w-0')
    expect(tiles[0].classList).not.toContain('min-w-[180px]')
    unmount()
  })

  it('gives the search field the full width without the desktop shortcut', async () => {
    const { host, unmount } = await mountPage(true)
    const input = host.querySelector<HTMLInputElement>('input[type="search"]')!
    expect(input.classList).toContain('w-full')
    expect(input.hasAttribute('aria-keyshortcuts')).toBe(false)
    unmount()
  })
})
