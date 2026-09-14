import { afterEach, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

const bridge = vi.hoisted(() => ({ queryServiceStatus: vi.fn(), syncServiceComponent: vi.fn() }))
vi.mock('@/bridge/service', () => ({ ...bridge, isElevationCancelled: (e: unknown) => e === 'elevation_cancelled' }))
afterEach(() => { vi.resetAllMocks(); vi.resetModules(); vi.useRealTimers() })

it('automatically syncs once and refreshes uptime after the component is replaced', async () => {
  let finishSync!: (result: string) => void
  bridge.syncServiceComponent.mockReturnValue(new Promise(resolve => { finishSync = resolve }))
  bridge.queryServiceStatus.mockResolvedValue({ state: 'running', pid: 123, uptimeSeconds: null })
  const { useServiceStore } = await import('./service')
  let store!: ReturnType<typeof useServiceStore>
  const component = defineComponent({ setup() { store = useServiceStore(); return () => null } })
  const main = mount(component)
  const anotherConsumer = mount(component)
  await store.ready
  expect(store.serviceStatus.value.state).toBe('running')
  expect(bridge.syncServiceComponent).toHaveBeenCalledExactlyOnceWith()
  bridge.queryServiceStatus.mockResolvedValue({ state: 'running', pid: 456, uptimeSeconds: 2 })
  finishSync('migrated')
  await flushPromises()
  expect(store.serviceStatus.value.uptimeSeconds).toBe(2)
  main.unmount()
  anotherConsumer.unmount()
})

it('keeps polling after UAC cancellation without repeatedly requesting an update', async () => {
  vi.useFakeTimers()
  bridge.syncServiceComponent.mockRejectedValue('elevation_cancelled')
  bridge.queryServiceStatus.mockResolvedValue({ state: 'running', pid: 123, uptimeSeconds: null })
  const { useServiceStore } = await import('./service')
  let store!: ReturnType<typeof useServiceStore>
  const wrapper = mount(defineComponent({ setup() { store = useServiceStore(); return () => null } }))
  await store.ready
  await vi.advanceTimersByTimeAsync(35_000)
  expect(store.serviceStatus.value.state).toBe('running')
  expect(bridge.syncServiceComponent).toHaveBeenCalledOnce()
  expect(bridge.queryServiceStatus.mock.calls.length).toBeGreaterThan(30)
  wrapper.unmount()
})
