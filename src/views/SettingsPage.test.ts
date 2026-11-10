import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, nextTick, ref } from 'vue'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))

const service = vi.hoisted(() => ({
  stopService: vi.fn().mockResolvedValue(undefined),
  installService: vi.fn().mockResolvedValue(undefined),
  uninstallService: vi.fn().mockResolvedValue(undefined),
  readServiceErrorLog: vi.fn().mockResolvedValue(''),
  startupTaskExists: vi.fn().mockResolvedValue(false),
  createStartupTask: vi.fn().mockResolvedValue(undefined),
  isElevationCancelled: vi.fn((_error: unknown) => false),
}))
vi.mock('@/bridge/service', () => service)

const status = vi.hoisted(() => ({ state: 'running' as string, ref: null as null | { value: { state: string } } }))
vi.mock('@/stores/service', async () => {
  const { ref: vueRef, computed } = await import('vue')
  const serviceStatus = vueRef({ state: status.state })
  status.ref = serviceStatus
  return {
    serviceStatus,
    useServiceStore: () => ({
      serviceStatus,
      statusText: computed(() => serviceStatus.value.state),
      ready: Promise.resolve(),
      refresh: vi.fn(),
    }),
  }
})
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref(status.state), isRunning: ref(status.state === 'running') }),
}))

const api = vi.hoisted(() => ({
  fetchConfig: vi.fn(),
  patchConfig: vi.fn(),
}))
vi.mock('@/api', () => api)
const route = vi.hoisted(() => ({ hash: '' }))
vi.mock('vue-router', () => ({ useRoute: () => route }))
const socket = vi.hoisted(() => ({ state: 'connecting' }))
vi.mock('@/stores/logs', () => ({
  useLogsStore: () => ({ socketState: ref(socket.state), socketFailures: ref(0) }),
}))
vi.mock('@/bridge/app', () => ({
  getAutoLaunch: vi.fn().mockResolvedValue(false),
  setAutoLaunch: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/bridge/config', () => ({ getRunningConfigPath: vi.fn().mockResolvedValue('C:/config.json') }))
vi.mock('@/utils/coreControl', () => ({ startCore: vi.fn(), restartCore: vi.fn() }))
vi.mock('@/stores/singboxVersion', () => ({
  useSingboxVersionStore: () => ({ singboxVersion: ref('sing-box 1.12.0'), versionStatus: ref('ok'), versionError: ref('') }),
}))
vi.mock('@/stores/proxies', () => ({
  useProxiesStore: () => ({ proxyGroups: ref([]), loadProxies: vi.fn() }),
}))

const background = vi.hoisted(() => ({ blob: null as Blob | null }))
vi.mock('@/stores/background', async () => {
  const { ref: vueRef, computed } = await import('vue')
  const backgroundUrl = vueRef<string | null>('blob:bg')
  return {
    assertBackgroundFile: vi.fn(),
    useBackgroundStore: () => ({
      backgroundUrl,
      hasBackground: computed(() => backgroundUrl.value !== null),
      setBackground: vi.fn(async (blob: Blob) => {
        background.blob = blob
        backgroundUrl.value = 'blob:restored'
      }),
      removeBackground: vi.fn(async () => {
        const removed = background.blob
        backgroundUrl.value = null
        return removed
      }),
    }),
  }
})

const stub = defineComponent({ render: () => null })
vi.mock('@/components/settings/DnsQueryTool.vue', () => ({ default: stub }))
vi.mock('@/components/settings/CoreUpdateCard.vue', () => ({ default: stub }))
vi.mock('@/components/settings/PanelUpdateCard.vue', () => ({ default: stub }))
vi.mock('@/components/settings/BackgroundEditorDialog.vue', () => ({ default: stub }))

async function flush() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await nextTick()
  }
}

async function mountPage(state = 'running') {
  status.state = state
  if (status.ref) status.ref.value = { state }
  vi.resetModules()
  const { default: SettingsPage } = await import('./SettingsPage.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(SettingsPage)
  app.mount(host)
  await flush()
  return { host, unmount: () => { app.unmount(); host.remove() } }
}

function buttonByText(root: ParentNode, text: string): HTMLButtonElement {
  const button = Array.from(root.querySelectorAll('button')).find((element) => element.textContent?.trim() === text)
  if (!button) throw new Error(`no button "${text}"`)
  return button as HTMLButtonElement
}

function dialog() {
  return document.body.querySelector('[role="alertdialog"]') as HTMLElement | null
}

beforeEach(() => {
  localStorage.clear()
  vi.clearAllMocks()
  route.hash = ''
  api.fetchConfig.mockResolvedValue({ data: { mode: 'Rule', 'mode-list': ['Rule', 'Global'] } })
  api.patchConfig.mockResolvedValue({})
  background.blob = new Blob(['x'], { type: 'image/png' })
})

afterEach(() => {
  vi.useRealTimers()
  document.body.innerHTML = ''
})

describe('SettingsPage destructive actions', () => {
  it('asks before uninstalling the service and says the proxy will drop', async () => {
    const { host, unmount } = await mountPage('running')
    buttonByText(host, '卸载服务').click()
    await flush()

    expect(dialog()?.textContent).toContain('核心将停止，代理会中断')
    buttonByText(dialog()!, '取消').click()
    await flush()
    expect(service.uninstallService).not.toHaveBeenCalled()

    buttonByText(host, '卸载服务').click()
    await flush()
    buttonByText(dialog()!, '卸载服务').click()
    await flush()
    expect(service.uninstallService).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('asks before stopping the core', async () => {
    const { host, unmount } = await mountPage('running')
    buttonByText(host, '停止').click()
    await flush()
    expect(dialog()?.textContent).toContain('代理会中断')
    buttonByText(dialog()!, '停止核心').click()
    await flush()
    expect(service.stopService).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('lets a removed test URL mapping come back in place', async () => {
    localStorage.setItem('singboard-config', JSON.stringify({
      groupTestUrls: { A: 'https://a.test', B: 'https://b.test', C: 'https://c.test' },
    }))
    const { host, unmount } = await mountPage()
    const rows = () => Array.from(host.querySelectorAll('.settings-mapping-list > div')).map((row) => row.textContent?.trim() ?? '')

    const rowB = host.querySelector('[data-mapping-group="B"]')!
    ;(rowB.querySelector('[aria-label="删除「B」的测速地址"]') as HTMLButtonElement).click()
    await flush()

    expect(rows()[1]).toContain('已删除「B」的测速地址')
    expect(document.activeElement?.getAttribute('aria-label')).toBe('撤销删除「B」的测速地址')
    expect(host.textContent).toContain('8 秒内可撤销')

    ;(document.activeElement as HTMLButtonElement).click()
    await flush()
    const groups = Array.from(host.querySelectorAll<HTMLElement>('[data-mapping-group]')).map((row) => row.dataset.mappingGroup)
    expect(groups).toEqual(['A', 'B', 'C'])
    expect(JSON.parse(localStorage.getItem('singboard-config')!).groupTestUrls).toEqual({
      A: 'https://a.test', B: 'https://b.test', C: 'https://c.test',
    })
    unmount()
  })

  it('drops the undo for a removed mapping after eight seconds', async () => {
    localStorage.setItem('singboard-config', JSON.stringify({ groupTestUrls: { A: 'https://a.test' } }))
    const { host, unmount } = await mountPage()
    vi.useFakeTimers()
    ;(host.querySelector('[aria-label="删除「A」的测速地址"]') as HTMLButtonElement).click()
    await flush()
    expect(host.querySelector('.settings-mapping-undo')).not.toBeNull()
    vi.advanceTimersByTime(8000)
    await flush()
    expect(host.querySelector('.settings-mapping-undo')).toBeNull()
    unmount()
  })

  it('restores a deleted background from the undo', async () => {
    const original = background.blob
    const { host, unmount } = await mountPage()
    ;(host.querySelector('[aria-label="删除背景"]') as HTMLButtonElement).click()
    await flush()

    expect(host.textContent).toContain('背景已删除，改用系统默认材质。')
    const undo = host.querySelector('[aria-label="撤销删除背景"]') as HTMLButtonElement
    expect(document.activeElement).toBe(undo)
    undo.click()
    await flush()

    expect(background.blob).toBe(original)
    expect(host.querySelector('[aria-label="撤销删除背景"]')).toBeNull()
    expect(host.querySelector('[aria-label="删除背景"]')).not.toBeNull()
    unmount()
  })
})

function setInput(input: HTMLInputElement, value: string) {
  input.value = value
  input.dispatchEvent(new Event('input'))
}

function storedApis() {
  return JSON.parse(localStorage.getItem('singboard-config')!).clashApis as { name: string; url: string }[]
}

describe('SettingsPage Clash API endpoints', () => {
  it('keeps an IPv6 endpoint whole when opening it for editing', async () => {
    localStorage.setItem('singboard-config', JSON.stringify({
      clashApis: [{ id: 'a', name: '本机', url: 'http://[::1]:9090', secret: '' }],
      activeClashApiId: 'a',
    }))
    const { host, unmount } = await mountPage()
    ;(host.querySelector('[aria-label="编辑端点「本机」"]') as HTMLButtonElement).click()
    await flush()
    const inputs = host.querySelectorAll<HTMLInputElement>('form input.settings-mono')
    expect(inputs[0].value).toBe('[::1]')
    expect(inputs[1].value).toBe('9090')
    unmount()
  })

  it('blocks a save with a bad port and points at the field', async () => {
    const { host, unmount } = await mountPage()
    ;(host.querySelector('[aria-label="新增端点"]') as HTMLButtonElement).click()
    await flush()
    const form = host.querySelector('form')!
    const [hostInput, portInput] = form.querySelectorAll<HTMLInputElement>('input.settings-mono')
    setInput(hostInput, '10.0.0.2')
    setInput(portInput, '90a')
    form.dispatchEvent(new Event('submit', { cancelable: true }))
    await flush()

    expect(portInput.getAttribute('aria-invalid')).toBe('true')
    expect(form.textContent).toContain('端口只能是数字')
    expect(document.activeElement).toBe(portInput)
    expect(host.querySelectorAll('select[aria-labelledby="settings-endpoint-label"] option')).toHaveLength(1)

    setInput(portInput, '9090')
    await flush()
    expect(form.textContent).not.toContain('端口只能是数字')
    unmount()
  })

  it('normalizes a pasted address and reports the connection result', async () => {
    const { host, unmount } = await mountPage()
    ;(host.querySelector('[aria-label="新增端点"]') as HTMLButtonElement).click()
    await flush()
    const form = host.querySelector('form')!
    setInput(form.querySelector<HTMLInputElement>('input.settings-mono')!, 'https://[::1]:9443')
    let release!: () => void
    api.fetchConfig.mockImplementationOnce(() => new Promise((resolve) => { release = () => resolve({ data: {} }) }))
    form.dispatchEvent(new Event('submit', { cancelable: true }))
    await flush()

    expect(storedApis().at(-1)).toMatchObject({ name: '端点 2', url: 'https://[::1]:9443' })
    expect(host.querySelector('#settings-endpoint-status')?.textContent).toContain('正在连接 [::1]:9443…')
    expect(host.querySelector('.settings-endpoint-dot')?.classList.contains('is-pending')).toBe(true)

    release()
    await flush()
    expect(host.querySelector('#settings-endpoint-status')?.textContent).toContain('已连接 [::1]:9443')
    expect(host.querySelector('.settings-endpoint-dot')?.classList.contains('is-ok')).toBe(true)
    unmount()
  })

  it('names the reason a saved endpoint cannot be reached', async () => {
    const { host, unmount } = await mountPage()
    ;(host.querySelector('[aria-label^="编辑端点"]') as HTMLButtonElement).click()
    await flush()
    api.fetchConfig.mockRejectedValueOnce(new Error('connect ECONNREFUSED'))
    host.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }))
    await flush()

    const status = host.querySelector('#settings-endpoint-status')!
    expect(status.textContent).toContain('连接失败：connect ECONNREFUSED')
    expect(status.classList.contains('settings-text-danger')).toBe(true)
    expect(host.querySelector('.settings-endpoint-dot')?.classList.contains('is-error')).toBe(true)
    unmount()
  })

  it('lands on the endpoint picker when opened from 检查 API 配置', async () => {
    route.hash = '#clash-api'
    const { host, unmount } = await mountPage()
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(document.activeElement).toBe(host.querySelector('#clash-api select'))
    unmount()
  })
})

describe('SettingsPage startup delay', () => {
  async function openServiceFields(host: HTMLElement) {
    const disclosure = Array.from(host.querySelectorAll<HTMLButtonElement>('.settings-disclosure'))
      .find((button) => button.textContent?.includes('服务参数'))!
    disclosure.click()
    await flush()
    return host.querySelector<HTMLInputElement>('#service-startup-delay')!
  }

  it('waits for an explicit apply and warns about elevation first', async () => {
    service.startupTaskExists.mockResolvedValue(true)
    const { host, unmount } = await mountPage('running')
    const input = await openServiceFields(host)
    setInput(input, '60')
    input.dispatchEvent(new Event('change'))
    input.dispatchEvent(new Event('blur'))
    await flush()

    expect(service.createStartupTask).not.toHaveBeenCalled()
    expect(host.querySelector('#service-startup-delay-hint')?.textContent).toContain('请求管理员授权')

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }))
    await flush()
    expect(service.createStartupTask).toHaveBeenCalledWith(60)
    expect(host.querySelector('#service-startup-delay-hint')?.textContent).toContain('已应用：下次登录 60 秒后启动核心')
    expect(JSON.parse(localStorage.getItem('singboard-config')!).startupDelaySeconds).toBe(60)
    unmount()
  })

  it('keeps the old delay when elevation is cancelled', async () => {
    service.startupTaskExists.mockResolvedValue(true)
    service.createStartupTask.mockRejectedValueOnce('cancelled')
    service.isElevationCancelled.mockImplementation((error: unknown) => error === 'cancelled')
    const { host, unmount } = await mountPage('running')
    const input = await openServiceFields(host)
    setInput(input, '90')
    await flush()
    buttonByText(host, '应用延迟').click()
    await flush()

    expect(host.querySelector('#service-startup-delay-hint')?.textContent).toContain('已取消授权，自启任务仍是 30 秒')
    expect(input.value).toBe('90')
    service.isElevationCancelled.mockImplementation(() => false)
    unmount()
  })

  it('only saves the delay while the service is not installed', async () => {
    service.startupTaskExists.mockResolvedValue(false)
    const { host, unmount } = await mountPage('not_installed')
    const input = await openServiceFields(host)
    expect(host.textContent).toContain('安装服务时写入')
    setInput(input, '4000')
    await flush()
    expect(host.querySelector('#service-startup-delay-hint')?.textContent).toBe('填 0–3600 之间的整数')
    expect(buttonByText(host, '保存延迟').disabled).toBe(true)

    setInput(input, '15')
    await flush()
    buttonByText(host, '保存延迟').click()
    await flush()
    expect(service.createStartupTask).not.toHaveBeenCalled()
    expect(JSON.parse(localStorage.getItem('singboard-config')!).startupDelaySeconds).toBe(15)
    unmount()
  })
})

describe('SettingsPage text settings', () => {
  it('checks the default test URL on blur and explains an empty value', async () => {
    const { host, unmount } = await mountPage()
    const input = host.querySelector<HTMLInputElement>('#settings-latency-url')!
    const feedback = () => host.querySelector('#settings-latency-url-feedback')?.textContent?.trim()

    input.dispatchEvent(new Event('focus'))
    setInput(input, 'www.gstatic.com')
    input.dispatchEvent(new Event('blur'))
    await flush()
    expect(feedback()).toBe('测速地址要以 http:// 或 https:// 开头')
    expect(input.getAttribute('aria-invalid')).toBe('true')

    setInput(input, 'https://cp.cloudflare.com/')
    input.dispatchEvent(new Event('blur'))
    await flush()
    expect(feedback()).toBe('已保存')
    expect(JSON.parse(localStorage.getItem('singboard-config')!).latencyTestUrl).toBe('https://cp.cloudflare.com/')

    setInput(input, '')
    input.dispatchEvent(new Event('blur'))
    await flush()
    expect(feedback()).toBe('已恢复默认地址 https://www.gstatic.com/generate_204')
    expect(input.value).toBe('https://www.gstatic.com/generate_204')
    unmount()
  })

  it('leaves the core start button as the only filled blue action', async () => {
    const { host, unmount } = await mountPage('stopped')
    expect(Array.from(host.querySelectorAll('.btn-route, .btn-primary')).map((button) => button.textContent?.trim())).toEqual(['启动'])
    unmount()
  })
})

describe('SettingsPage start page', () => {
  it('starts on the proxies page until another page is picked', async () => {
    const { host, unmount } = await mountPage()
    const select = host.querySelector<HTMLSelectElement>('#settings-start-page')!
    expect(select.value).toBe('/proxies')
    expect(Array.from(select.options).map((option) => option.textContent)).toEqual(['概览', '代理', '连接', '日志', '规则', '配置', '设置'])

    select.value = '/overview'
    select.dispatchEvent(new Event('change'))
    await flush()
    expect(JSON.parse(localStorage.getItem('singboard-config')!).startPage).toBe('/overview')
    expect(select.value).toBe('/overview')
    unmount()
  })

  it('falls back to the proxies page when the saved start page no longer exists', async () => {
    localStorage.setItem('singboard-config', JSON.stringify({ startPage: '/gone' }))
    const { host, unmount } = await mountPage()
    expect(host.querySelector<HTMLSelectElement>('#settings-start-page')!.value).toBe('/proxies')
    unmount()
  })
})
