import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const bridge = vi.hoisted(() => ({
  startService: vi.fn(), stopService: vi.fn(), restartService: vi.fn(), installService: vi.fn(),
  validateSingboxConfig: vi.fn(),
}))
vi.mock('@/bridge/service', () => bridge)
vi.mock('@/bridge/config', () => bridge)
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({
  config: ref({
    workingDir: '/data/adb/box',
    singboxPath: '/data/adb/box/bin/sing-box',
    configPath: '/data/adb/box/config.json',
  }),
  refreshConfigFromStorage: vi.fn(),
}) }))
import { restartCore } from './coreControl'

describe('restart with one root operation', () => {
  beforeEach(() => vi.resetAllMocks())
  it('validates configuration, refreshes the service script and submits a single restart', async () => {
    await restartCore()
    expect(bridge.validateSingboxConfig).toHaveBeenCalledExactlyOnceWith('/data/adb/box/bin/sing-box', '/data/adb/box/config.json', '/data/adb/box')
    expect(bridge.installService).toHaveBeenCalledExactlyOnceWith('/data/adb/box/bin/sing-box', '/data/adb/box/config.json', '/data/adb/box')
    expect(bridge.restartService).toHaveBeenCalledExactlyOnceWith()
    expect(bridge.stopService).not.toHaveBeenCalled()
    expect(bridge.startService).not.toHaveBeenCalled()
  })
  it('does not touch the service when configuration is invalid', async () => {
    bridge.validateSingboxConfig.mockRejectedValueOnce(new Error('invalid config'))
    await expect(restartCore()).rejects.toThrow('invalid config')
    expect(bridge.installService).not.toHaveBeenCalled()
    expect(bridge.restartService).not.toHaveBeenCalled()
    expect(bridge.stopService).not.toHaveBeenCalled()
  })
  it('does not retry or split a failed root restart', async () => {
    bridge.restartService.mockRejectedValueOnce('未获得 root 权限')
    await expect(restartCore()).rejects.toBe('未获得 root 权限')
    expect(bridge.restartService).toHaveBeenCalledOnce()
    expect(bridge.startService).not.toHaveBeenCalled()
    expect(bridge.stopService).not.toHaveBeenCalled()
  })
})
