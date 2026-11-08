import { beforeEach, describe, expect, it, vi } from 'vitest'

const bridge = vi.hoisted(() => ({
  fetchUrl: vi.fn(),
  getRemoteConfigPath: vi.fn(),
  validateSingboxConfigContent: vi.fn(),
  replaceConfig: vi.fn(),
  copyToRunningConfig: vi.fn(),
}))

vi.mock('@/bridge/config', () => bridge)

import { assertConfigShape, firstLine, normalizeUpdateInterval, refreshRemoteConfig } from './remoteConfig'
import type { ConfigProfile } from '@/types'

const profile: ConfigProfile = {
  id: 'abc',
  name: '机场',
  type: 'remote',
  source: 'https://example.com/sub',
  autoUpdateInterval: 0,
}

const context = { singboxPath: 'C:/sing-box.exe', workingDir: 'C:/sb', active: false }

describe('refreshRemoteConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    bridge.fetchUrl.mockResolvedValue('{ "outbounds": [] }')
    bridge.getRemoteConfigPath.mockResolvedValue('C:/data/abc.json')
    bridge.validateSingboxConfigContent.mockResolvedValue('Configuration is valid')
    bridge.replaceConfig.mockResolvedValue(true)
    bridge.copyToRunningConfig.mockResolvedValue({ path: 'C:/data/running-config.json', changed: true })
  })

  it('downloads with the sing-box user agent, validates, then replaces', async () => {
    const result = await refreshRemoteConfig(profile, context)

    expect(bridge.fetchUrl).toHaveBeenCalledWith('https://example.com/sub', 'sing-box')
    expect(bridge.validateSingboxConfigContent).toHaveBeenCalledWith('C:/sing-box.exe', 'C:/data/abc.json', '{ "outbounds": [] }', 'C:/sb')
    expect(bridge.replaceConfig).toHaveBeenCalledWith('C:/data/abc.json', '{ "outbounds": [] }')
    expect(bridge.copyToRunningConfig).not.toHaveBeenCalled()
    expect(result).toEqual({ path: 'C:/data/abc.json', changed: true, appliedToRunning: false })
  })

  it('never writes when the download is not a config', async () => {
    bridge.fetchUrl.mockResolvedValue('<html>订阅已过期</html>')

    await expect(refreshRemoteConfig(profile, context)).rejects.toThrow('不是 sing-box 配置')
    expect(bridge.replaceConfig).not.toHaveBeenCalled()
  })

  it('never writes when sing-box rejects the config', async () => {
    bridge.validateSingboxConfigContent.mockRejectedValue(new Error('decode config: unknown field'))

    await expect(refreshRemoteConfig(profile, context)).rejects.toThrow('sing-box 校验未通过：decode config: unknown field')
    expect(bridge.replaceConfig).not.toHaveBeenCalled()
  })

  it('applies the active profile to the running config after replacing', async () => {
    const result = await refreshRemoteConfig(profile, { ...context, active: true })

    expect(bridge.copyToRunningConfig).toHaveBeenCalledWith('C:/data/abc.json')
    expect(bridge.replaceConfig.mock.invocationCallOrder[0]).toBeLessThan(bridge.copyToRunningConfig.mock.invocationCallOrder[0])
    expect(result.appliedToRunning).toBe(true)
  })

  it('reports nothing new when the subscription did not change', async () => {
    bridge.replaceConfig.mockResolvedValue(false)
    bridge.copyToRunningConfig.mockResolvedValue({ path: 'C:/data/running-config.json', changed: false })

    const result = await refreshRemoteConfig(profile, { ...context, active: true })
    expect(result).toEqual({ path: 'C:/data/abc.json', changed: false, appliedToRunning: false })
  })

  it('gives up without writing when the profile is deleted mid-download', async () => {
    let exists = true
    bridge.fetchUrl.mockImplementation(async () => {
      exists = false
      return '{ "outbounds": [] }'
    })

    await expect(refreshRemoteConfig(profile, { ...context, stillExists: () => exists })).rejects.toThrow('配置已被删除')
    expect(bridge.replaceConfig).not.toHaveBeenCalled()
    expect(bridge.copyToRunningConfig).not.toHaveBeenCalled()
  })

  it('checks whether the profile is still active right before applying', async () => {
    let active = true
    bridge.replaceConfig.mockImplementation(async () => {
      active = false
    })

    const result = await refreshRemoteConfig(profile, { ...context, active: () => active })
    expect(bridge.copyToRunningConfig).not.toHaveBeenCalled()
    expect(result.appliedToRunning).toBe(false)
  })

  it('does not touch the running config without a sing-box path to check it', async () => {
    const result = await refreshRemoteConfig(profile, { ...context, singboxPath: '', active: true })

    expect(bridge.validateSingboxConfigContent).not.toHaveBeenCalled()
    expect(bridge.replaceConfig).toHaveBeenCalled()
    expect(bridge.copyToRunningConfig).not.toHaveBeenCalled()
    expect(result.appliedToRunning).toBe(false)
  })
})

describe('config shape and messages', () => {
  it('accepts JSONC objects and rejects other payloads', () => {
    expect(() => assertConfigShape('{\n  // comment\n  "log": {},\n}')).not.toThrow()
    expect(() => assertConfigShape('proxies:\n  - name: a')).toThrow()
    expect(() => assertConfigShape('[1, 2]')).toThrow()
    expect(() => assertConfigShape('dm1lc3M6Ly8=')).toThrow()
  })

  it('keeps auto-update intervals at whole hours of at least one', () => {
    expect(normalizeUpdateInterval(0)).toBe(0)
    expect(normalizeUpdateInterval(-3)).toBe(0)
    expect(normalizeUpdateInterval('')).toBe(0)
    expect(normalizeUpdateInterval(0.01)).toBe(1)
    expect(normalizeUpdateInterval(5.6)).toBe(6)
    expect(normalizeUpdateInterval('12')).toBe(12)
    expect(normalizeUpdateInterval(720)).toBe(576)
  })

  it('takes the first meaningful line of a long error', () => {
    expect(firstLine('\n  sing-box 校验未通过：decode config\n  at line 3')).toBe('sing-box 校验未通过：decode config')
  })
})
