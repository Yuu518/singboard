import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const bridge = vi.hoisted(() => ({
  startService: vi.fn(), stopService: vi.fn(), restartService: vi.fn(),
  validateSingboxConfig: vi.fn(), copyToRunningConfig: vi.fn(),
}))
vi.mock('@/bridge/service', () => bridge)
vi.mock('@/bridge/config', () => ({ ...bridge,
  getRunningConfigPath: vi.fn().mockResolvedValue('C:\\app\\running.json'),
  getRemoteConfigPath: vi.fn(),
}))
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({
  config: ref({ singboxPath: 'C:\\app\\sing-box.exe', workingDir: 'C:\\app', activeConfigProfileId: null }),
  configProfiles: ref([]),
}) }))
import { restartCore } from './coreControl'

describe('restart with one authorization', () => {
  beforeEach(() => vi.resetAllMocks())
  it('validates configuration and submits a single restart operation', async () => {
    await restartCore()
    expect(bridge.validateSingboxConfig).toHaveBeenCalledOnce()
    expect(bridge.restartService).toHaveBeenCalledExactlyOnceWith()
    expect(bridge.stopService).not.toHaveBeenCalled()
    expect(bridge.startService).not.toHaveBeenCalled()
  })
  it('does not stop the service when configuration is invalid', async () => {
    bridge.validateSingboxConfig.mockRejectedValueOnce(new Error('invalid config'))
    await expect(restartCore()).rejects.toThrow('invalid config')
    expect(bridge.restartService).not.toHaveBeenCalled()
    expect(bridge.stopService).not.toHaveBeenCalled()
  })
  it('does not retry or split a cancelled elevated restart', async () => {
    bridge.restartService.mockRejectedValueOnce('elevation_cancelled')
    await expect(restartCore()).rejects.toBe('elevation_cancelled')
    expect(bridge.restartService).toHaveBeenCalledOnce()
    expect(bridge.startService).not.toHaveBeenCalled()
    expect(bridge.stopService).not.toHaveBeenCalled()
  })
})
