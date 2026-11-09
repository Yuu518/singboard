import { describe, expect, it, vi } from 'vitest'
import { computed, createApp, nextTick, ref } from 'vue'
import { PAGE_ACTIVE } from '@/android/composables/usePageActive'

const mocks = vi.hoisted(() => ({
  config: null as any,
  clashApiReady: null as any,
  loadRules: null as any,
  fetchRuleProviders: null as any,
  srsListProvider: null as any,
  getRunningConfigPath: null as any,
  readSingboxConfig: null as any,
}))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/components/common/ServiceNotice.vue', async () => {
  const { h } = await import('vue')
  return { default: { render: () => h('div', { 'data-service-notice': '' }) } }
})
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref('running'), isRunning: ref(true) }),
}))
vi.mock('@/stores/service', () => ({ useServiceStore: () => ({ clashApiReady: mocks.clashApiReady }) }))
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({ config: mocks.config, clashApiUrl: ref('http://127.0.0.1:9090') }),
}))
vi.mock('@/stores/proxies', async () => {
  const { ref } = await import('vue')
  return {
    useProxiesStore: () => ({
      proxyMap: ref({ Proxy: { name: 'Proxy', type: 'Selector', now: 'HK 01', history: [] } }),
    }),
  }
})
vi.mock('@/stores/rules', async () => {
  const { ref } = await import('vue')
  const rules = ref([
    { type: 'default', payload: 'rule_set=[geosite-cn]', proxy: 'route(direct)' },
    { type: 'default', payload: 'domain_suffix=[google.com]', proxy: 'route(Proxy)' },
  ])
  return {
    useRulesStore: () => ({
      rules,
      loading: ref(false),
      loaded: ref(true),
      loadError: ref(''),
      filterText: ref(''),
      loadRules: mocks.loadRules,
    }),
  }
})
vi.mock('@/api', () => ({
  fetchRuleProviders: (...args: unknown[]) => mocks.fetchRuleProviders(...args),
  updateRuleProvider: vi.fn(),
}))
vi.mock('@/bridge/config', () => ({
  srsMatchProvider: vi.fn(() => Promise.resolve(false)),
  srsListProvider: (...args: unknown[]) => mocks.srsListProvider(...args),
  getRunningConfigPath: (...args: unknown[]) => mocks.getRunningConfigPath(...args),
  readSingboxConfig: (...args: unknown[]) => mocks.readSingboxConfig(...args),
}))

const provider = {
  name: 'geosite-cn', behavior: 'binary', format: 'binary', ruleCount: 100, type: 'Rule',
  updatedAt: '2026-11-09T10:00:00Z', vehicleType: 'Remote',
}

async function flush() {
  for (let i = 0; i < 4; i++) await nextTick()
}

async function mountPage(options: { rootMode: boolean; ready?: boolean }) {
  mocks.config = ref({ rootMode: options.rootMode, configPath: '/data/adb/singboard/config.json', workingDir: '/data/adb/singboard', singboxPath: '/data/adb/singboard/sing-box' })
  mocks.clashApiReady = ref(options.ready ?? true)
  mocks.loadRules = vi.fn(() => Promise.resolve(''))
  mocks.fetchRuleProviders = vi.fn(() => Promise.resolve({ data: { providers: { [provider.name]: provider } } }))
  mocks.srsListProvider = vi.fn(() => Promise.resolve([{ type: 'domain_suffix', value: 'cn' }]))
  mocks.getRunningConfigPath = vi.fn(() => Promise.reject(new Error('unavailable on Android')))
  mocks.readSingboxConfig = vi.fn(() => Promise.reject(new Error('unavailable on Android')))
  const { default: RulesPage } = await import('./RulesPage.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(RulesPage)
  app.provide(PAGE_ACTIVE, computed(() => true))
  app.mount(host)
  await flush()
  return { host, unmount: () => { app.unmount(); host.remove(); document.body.innerHTML = '' } }
}

describe('RulesPage on Android', () => {
  it('loads once the Clash API is ready and never asks for the running config', async () => {
    const { unmount } = await mountPage({ rootMode: true, ready: false })
    expect(mocks.loadRules).not.toHaveBeenCalled()

    mocks.clashApiReady.value = true
    await flush()
    expect(mocks.loadRules).toHaveBeenCalled()
    expect(mocks.fetchRuleProviders).toHaveBeenCalled()
    expect(mocks.readSingboxConfig).not.toHaveBeenCalled()
    expect(mocks.getRunningConfigPath).not.toHaveBeenCalled()
    unmount()
  })

  it('shows each rule as a glass card with a readable condition and its resolved outbound', async () => {
    const { host, unmount } = await mountPage({ rootMode: true })
    expect(host.querySelector('[data-service-notice]')).toBeNull()
    const cards = host.querySelectorAll<HTMLElement>('#rules-panel-rules li')
    expect(cards).toHaveLength(2)
    expect(cards[0].classList).toContain('surface-card')
    expect(cards[0].querySelector('.rules-rail')).toBeNull()
    expect(cards[1].textContent).toContain('域名后缀')
    expect(cards[1].querySelector('.rules-dest')!.textContent).toContain('Proxy')
    expect(cards[1].querySelector('.rules-dest')!.textContent).toContain('HK 01')
    expect(host.querySelector('.rules-step--final')).toBeNull()
    expect(host.querySelector('.rules-table-container')!.classList).toContain('under-nav')
    unmount()
  })

  it('opens rule set details in root mode using the configured config path', async () => {
    const { host, unmount } = await mountPage({ rootMode: true })
    const chip = host.querySelector<HTMLButtonElement>('button.rules-set')!
    expect(chip.textContent).toContain('geosite-cn')
    chip.click()
    await flush()
    expect(mocks.srsListProvider).toHaveBeenCalledWith(
      '/data/adb/singboard',
      '/data/adb/singboard/config.json',
      '/data/adb/singboard/sing-box',
      'geosite-cn',
    )
    expect(document.body.querySelector('[role="dialog"]')!.classList.toString()).toContain('glass-popover')
    expect(document.body.querySelector('.glass-scrim')!.classList).toContain('safe-overlay')
    unmount()
  })

  it('keeps rule set contents closed without root access', async () => {
    const { host, unmount } = await mountPage({ rootMode: false })
    expect(host.querySelector('button.rules-set')).toBeNull()
    expect(host.querySelector('span.rules-set')!.textContent).toContain('geosite-cn')
    const providerSearch = host.querySelector<HTMLInputElement>('input[aria-label="按域名或 IP 查找规则提供商"]')!
    expect(providerSearch.disabled).toBe(true)
    expect(providerSearch.placeholder).toBe('开启 root 模式后可按域名或 IP 查找')
    unmount()
  })
})
