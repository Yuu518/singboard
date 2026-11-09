import { afterEach, expect, it, vi } from 'vitest'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

afterEach(() => {
  vi.resetModules()
  localStorage.clear()
})

it('keeps Android paths as live fields and never turns them into config profiles', async () => {
  localStorage.setItem('singboard-config', JSON.stringify({
    rootMode: true,
    workingDir: ' /data/adb/box ',
    singboxPath: '/data/adb/box/bin/sing-box',
    configPath: '/data/adb/box/config.json',
    rulesDir: '/data/adb/box/rules',
    startupDelaySeconds: 45,
  }))
  const { useConfigStore } = await import('./config')
  const { config, configProfiles } = useConfigStore()
  expect(config.value.rootMode).toBe(true)
  expect(config.value.workingDir).toBe('/data/adb/box')
  expect(config.value.configPath).toBe('/data/adb/box/config.json')
  expect(config.value.rulesDir).toBe('/data/adb/box/rules')
  expect(configProfiles.value).toEqual([])
})

it('keeps page swiping off unless it was turned on', async () => {
  localStorage.setItem('singboard-config', JSON.stringify({ rootMode: false }))
  const first = (await import('./config')).useConfigStore()
  expect(first.config.value.pageSwipe).toBe(false)
  first.updateConfig({ pageSwipe: true })

  vi.resetModules()
  const reloaded = (await import('./config')).useConfigStore()
  expect(reloaded.config.value.pageSwipe).toBe(true)
})

it('turns root mode on in the backend at startup and when it changes', async () => {
  const { invoke } = await import('@tauri-apps/api/core')
  localStorage.setItem('singboard-config', JSON.stringify({ rootMode: true }))
  const { rootReady, useConfigStore } = await import('./config')
  await rootReady
  expect(invoke).toHaveBeenCalledWith('set_root_enabled', { enabled: true })
  expect(invoke).not.toHaveBeenCalledWith('set_close_to_tray', expect.anything())
  const { nextTick } = await import('vue')
  useConfigStore().updateConfig({ rootMode: false })
  await nextTick()
  expect(invoke).toHaveBeenCalledWith('set_root_enabled', { enabled: false })
})
