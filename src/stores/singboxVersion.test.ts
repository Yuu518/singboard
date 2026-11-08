import { ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const bridge = vi.hoisted(() => ({
  getFileHash: vi.fn(),
  getSingboxVersion: vi.fn(),
}))
const configState = vi.hoisted(() => ({ path: 'C:/core/sing-box.exe' }))

vi.mock('@/bridge/config', () => bridge)
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({ config: ref({ singboxPath: configState.path }) }),
}))

async function loadStore() {
  vi.resetModules()
  const module = await import('./singboxVersion')
  return module.useSingboxVersionStore()
}

beforeEach(() => {
  localStorage.clear()
  bridge.getFileHash.mockReset()
  bridge.getSingboxVersion.mockReset()
  configState.path = 'C:/core/sing-box.exe'
})

afterEach(() => {
  vi.useRealTimers()
})

describe('singbox version store', () => {
  it('clears the previous version when detection fails', async () => {
    localStorage.setItem('singboard-singbox-version', JSON.stringify({ hash: 'h1', version: 'sing-box 1.11.0' }))
    const store = await loadStore()
    expect(store.singboxVersion.value).toBe('sing-box 1.11.0')

    bridge.getFileHash.mockRejectedValueOnce(new Error('not found'))
    await store.detectVersion()

    expect(store.singboxVersion.value).toBe('')
    expect(store.versionStatus.value).toBe('failed')
    expect(store.versionError.value).toBe('找不到该文件或无法读取')
  })

  it('keeps only the newest detection when runs overlap', async () => {
    const store = await loadStore()
    let releaseFirst!: (hash: string) => void
    bridge.getFileHash
      .mockReturnValueOnce(new Promise<string>((resolve) => { releaseFirst = resolve }))
      .mockResolvedValueOnce('new-hash')
    bridge.getSingboxVersion.mockImplementation(async () => 'sing-box version 1.12.0')

    const first = store.detectVersion()
    configState.path = 'C:/other/sing-box.exe'
    const second = store.detectVersion()
    await second
    releaseFirst('old-hash')
    await first

    expect(store.singboxVersion.value).toBe('sing-box 1.12.0')
    expect(bridge.getSingboxVersion).toHaveBeenCalledTimes(1)
    expect(store.versionStatus.value).toBe('ok')
  })

  it('waits for typing to settle before hashing the path', async () => {
    vi.useFakeTimers()
    const store = await loadStore()
    bridge.getFileHash.mockResolvedValue('hash')
    bridge.getSingboxVersion.mockResolvedValue('sing-box 1.12.0')

    store.scheduleDetectVersion()
    vi.advanceTimersByTime(300)
    store.scheduleDetectVersion()
    vi.advanceTimersByTime(300)
    store.scheduleDetectVersion()
    expect(bridge.getFileHash).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(800)
    expect(bridge.getFileHash).toHaveBeenCalledTimes(1)
  })

  it('reports an empty path as not configured rather than failed', async () => {
    configState.path = '  '
    const store = await loadStore()
    await store.detectVersion()
    expect(store.versionStatus.value).toBe('empty')
    expect(store.singboxVersion.value).toBe('')
    expect(bridge.getFileHash).not.toHaveBeenCalled()
  })
})
