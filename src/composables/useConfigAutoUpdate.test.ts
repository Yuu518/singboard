import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, ref } from 'vue'
import type { ConfigProfile } from '@/types'

const refresh = vi.hoisted(() => vi.fn())
const store = vi.hoisted(() => ({ value: null as any }))
const toast = vi.hoisted(() => vi.fn())

vi.mock('@/utils/remoteConfig', async (original) => ({
  ...(await original<typeof import('@/utils/remoteConfig')>()),
  refreshRemoteConfig: refresh,
}))
vi.mock('@/bridge/config', () => ({
  getRemoteConfigPath: vi.fn(async (id: string) => `C:/data/${id}.json`),
  hasConfigBackup: vi.fn(async () => false),
}))
vi.mock('@/stores/service', async () => {
  const { ref } = await import('vue')
  return { serviceStatus: ref({ state: 'running' }) }
})
vi.mock('@/stores/toast', () => ({ useToastStore: () => ({ pushToast: toast }) }))
vi.mock('@/stores/config', () => ({ useConfigStore: () => store.value }))

const due: ConfigProfile = {
  id: 'sub',
  name: '机场',
  type: 'remote',
  source: 'https://sub.example.com',
  autoUpdateInterval: 1,
  lastUpdated: '2020-01-01T00:00:00.000Z',
}

async function startAutoUpdate() {
  const { useConfigAutoUpdate } = await import('./useConfigAutoUpdate')
  const app = createApp(defineComponent({
    setup() {
      useConfigAutoUpdate().start()
      return () => null
    },
  }))
  app.mount(document.createElement('div'))
  return () => app.unmount()
}

describe('remote config auto-update', () => {
  beforeEach(() => {
    vi.resetModules()
    refresh.mockReset()
    toast.mockReset()
    refresh.mockResolvedValue({ path: 'C:/data/sub.json', changed: true, appliedToRunning: false })
    store.value = {
      config: ref({ singboxPath: 'C:/sb.exe', workingDir: 'C:/sb' }),
      configProfiles: ref([due]),
      activeConfigProfileId: ref(''),
      updateConfigProfile: vi.fn(),
    }
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('updates a due profile through the shared queue after a startup delay', async () => {
    vi.useFakeTimers()
    const stop = await startAutoUpdate()
    await vi.advanceTimersByTimeAsync(5_000)
    expect(refresh).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(11_000)
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(store.value.updateConfigProfile).toHaveBeenCalledWith('sub', expect.objectContaining({ lastFetchError: '' }))
    stop()
  })

  it('stays quiet when the subscription did not change', async () => {
    refresh.mockResolvedValue({ path: 'C:/data/sub.json', changed: false, appliedToRunning: false })
    vi.useFakeTimers()
    const stop = await startAutoUpdate()
    await vi.advanceTimersByTimeAsync(16_000)
    expect(refresh).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(5_000)
    expect(toast).not.toHaveBeenCalled()
    stop()
  })

  it('catches up on a skipped update once editing ends', async () => {
    const { useConfigOps } = await import('@/stores/configOps')
    const ops = useConfigOps()
    ops.editingProfileId.value = 'sub'
    const stop = await startAutoUpdate()
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(refresh).not.toHaveBeenCalled()

    ops.editingProfileId.value = ''
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledTimes(1))
    stop()
  })

  it('retries a failed update after five minutes and merges failure toasts', async () => {
    refresh.mockRejectedValue(new Error('服务器返回 HTTP 503'))
    store.value.configProfiles.value = [due, { ...due, id: 'sub2', name: '机场 B' }]
    vi.useFakeTimers()
    const stop = await startAutoUpdate()
    await vi.advanceTimersByTimeAsync(40_000)
    expect(refresh).toHaveBeenCalledTimes(2)
    expect(toast).toHaveBeenCalledTimes(1)
    expect(toast.mock.calls[0][0].message).toContain('2 个远程配置自动更新失败')

    await vi.advanceTimersByTimeAsync(5 * 60_000)
    expect(refresh).toHaveBeenCalledTimes(4)
    stop()
  })

  it('does not overwrite a fresh local edit when editing ends', async () => {
    const { useConfigOps } = await import('@/stores/configOps')
    const ops = useConfigOps()
    store.value.configProfiles.value = [{ ...due, lastLocalChangeAt: new Date().toISOString() }]
    ops.editingProfileId.value = 'sub'
    const stop = await startAutoUpdate()
    ops.editingProfileId.value = ''
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(refresh).not.toHaveBeenCalled()
    stop()
  })

  it('skips a profile that is open in the editor', async () => {
    const { useConfigOps } = await import('@/stores/configOps')
    useConfigOps().editingProfileId.value = 'sub'
    const stop = await startAutoUpdate()
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(refresh).not.toHaveBeenCalled()
    stop()
  })
})
