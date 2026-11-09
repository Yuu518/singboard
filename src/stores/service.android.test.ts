import { afterEach, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

const bridge = vi.hoisted(() => ({ queryServiceStatus: vi.fn(), syncServiceComponent: vi.fn() }))
const api = vi.hoisted(() => ({ fetchVersion: vi.fn() }))
vi.mock('@/bridge/service', () => bridge)
vi.mock('@/api', () => api)
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))
afterEach(() => { vi.resetAllMocks(); vi.resetModules(); vi.useRealTimers(); localStorage.clear() })

function mountStore(useServiceStore: () => any) {
  let store!: ReturnType<typeof useServiceStore>
  const wrapper = mount(defineComponent({ setup() { store = useServiceStore(); return () => null } }))
  return { wrapper, store }
}

it('reports a reachable remote Clash API as running without touching root', async () => {
  api.fetchVersion.mockResolvedValue({ data: { version: 'sing-box 1.12.0' } })
  const { useServiceStore } = await import('./service')
  const { wrapper, store } = mountStore(useServiceStore)
  await store.ready
  expect(store.serviceStatus.value.state).toBe('running')
  expect(store.statusText.value).toBe('已连接')
  expect(bridge.queryServiceStatus).not.toHaveBeenCalled()
  expect(bridge.syncServiceComponent).not.toHaveBeenCalled()

  api.fetchVersion.mockRejectedValue(new Error('offline'))
  await store.refresh()
  expect(store.serviceStatus.value.state).toBe('stopped')
  expect(store.statusText.value).toBe('未连接')
  wrapper.unmount()
})

it('queries the root service and syncs the service script once in root mode', async () => {
  localStorage.setItem('singboard-config', JSON.stringify({ rootMode: true }))
  bridge.syncServiceComponent.mockResolvedValue('ok')
  bridge.queryServiceStatus.mockResolvedValue({ state: 'running', pid: 123, uptimeSeconds: 5 })
  api.fetchVersion.mockResolvedValue({ data: { version: 'sing-box 1.12.0' } })
  const { useServiceStore } = await import('./service')
  const main = mountStore(useServiceStore)
  const another = mountStore(useServiceStore)
  await main.store.ready
  await flushPromises()
  expect(main.store.serviceStatus.value).toEqual({ state: 'running', pid: 123, uptimeSeconds: 5 })
  expect(main.store.statusText.value).toBe('运行中')
  expect(bridge.syncServiceComponent).toHaveBeenCalledOnce()
  expect(main.store.clashApiState.value).toBe('ready')
  expect(api.fetchVersion).toHaveBeenCalledOnce()
  main.wrapper.unmount()
  another.wrapper.unmount()
})

it('keeps polling while visible and a failed script sync is not retried', async () => {
  vi.useFakeTimers()
  localStorage.setItem('singboard-config', JSON.stringify({ rootMode: true }))
  bridge.syncServiceComponent.mockRejectedValue('未获得 root 权限')
  bridge.queryServiceStatus.mockResolvedValue({ state: 'stopped' })
  const { useServiceStore } = await import('./service')
  const { wrapper, store } = mountStore(useServiceStore)
  await store.ready
  await vi.advanceTimersByTimeAsync(21_000)
  expect(store.serviceStatus.value.state).toBe('stopped')
  expect(bridge.syncServiceComponent).toHaveBeenCalledOnce()
  expect(bridge.queryServiceStatus.mock.calls.length).toBeGreaterThanOrEqual(10)
  wrapper.unmount()
})

function rootRunning(pid = 7) {
  localStorage.setItem('singboard-config', JSON.stringify({ rootMode: true }))
  bridge.syncServiceComponent.mockResolvedValue('ok')
  bridge.queryServiceStatus.mockResolvedValue({ state: 'running', pid })
}

const refused = () => Object.assign(new Error('Network Error'), { code: 'ERR_NETWORK' })

it('keeps waiting while the Clash API is not listening yet and becomes ready once it answers', async () => {
  vi.useFakeTimers()
  rootRunning()
  api.fetchVersion
    .mockRejectedValueOnce(refused())
    .mockRejectedValueOnce(refused())
    .mockResolvedValue({ data: { version: 'sing-box 1.12.0' } })
  const { useServiceStore, whenClashApiSettled } = await import('./service')
  const { wrapper, store } = mountStore(useServiceStore)
  await store.ready
  expect(store.clashApiState.value).toBe('waiting')
  const settled = whenClashApiSettled()
  await vi.advanceTimersByTimeAsync(1000)
  await expect(settled).resolves.toBe('ready')
  expect(store.clashApiReady.value).toBe(true)
  expect(api.fetchVersion).toHaveBeenCalledTimes(3)
  wrapper.unmount()
})

it('stops retrying on a rejected secret and waits again after the endpoint changes', async () => {
  vi.useFakeTimers()
  rootRunning()
  api.fetchVersion.mockRejectedValue({ response: { status: 401 } })
  const { useServiceStore } = await import('./service')
  const { useConfigStore } = await import('@/stores/config')
  const { wrapper, store } = mountStore(useServiceStore)
  await store.ready
  await vi.advanceTimersByTimeAsync(0)
  expect(store.clashApiState.value).toBe('auth_failed')
  await vi.advanceTimersByTimeAsync(1500)
  expect(api.fetchVersion).toHaveBeenCalledOnce()

  api.fetchVersion.mockResolvedValue({ data: { version: 'sing-box 1.12.0' } })
  useConfigStore().updateActiveClashApi({ secret: 'fixed' })
  await vi.advanceTimersByTimeAsync(0)
  expect(store.clashApiState.value).toBe('ready')
  wrapper.unmount()
})

it('settles as down when the core exits before the Clash API comes up', async () => {
  vi.useFakeTimers()
  rootRunning()
  api.fetchVersion.mockRejectedValue(refused())
  const { useServiceStore, whenClashApiSettled } = await import('./service')
  const { wrapper, store } = mountStore(useServiceStore)
  await store.ready
  const settled = whenClashApiSettled()
  bridge.queryServiceStatus.mockResolvedValue({ state: 'stopped' })
  await store.refresh()
  await expect(settled).resolves.toBe('down')
  expect(store.clashApiState.value).toBe('down')
  const calls = api.fetchVersion.mock.calls.length
  await vi.advanceTimersByTimeAsync(2000)
  expect(api.fetchVersion.mock.calls.length).toBe(calls)
  wrapper.unmount()
})

it('ignores a probe that finishes after the core action suspended the Clash API', async () => {
  rootRunning()
  let answer!: (value: unknown) => void
  api.fetchVersion.mockReturnValue(new Promise((resolve) => { answer = resolve }))
  const { useServiceStore, suspendClashApi, resumeClashApi } = await import('./service')
  const { wrapper, store } = mountStore(useServiceStore)
  await store.ready
  expect(store.clashApiState.value).toBe('waiting')
  suspendClashApi()
  answer({ data: { version: 'sing-box 1.12.0' } })
  await flushPromises()
  expect(store.clashApiState.value).toBe('down')

  api.fetchVersion.mockResolvedValue({ data: { version: 'sing-box 1.12.0' } })
  resumeClashApi()
  await flushPromises()
  expect(store.clashApiState.value).toBe('ready')
  wrapper.unmount()
})
