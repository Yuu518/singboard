import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { useConfigStore } from './config'
import { restartCore } from '@/utils/coreControl'

const tauri = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }))

const STORAGE_KEY = 'singboard-config'
const store = useConfigStore()
const initialConfig = JSON.parse(JSON.stringify(store.config.value))

function notifyStorage(key: string | null = STORAGE_KEY, storageArea = localStorage) {
  window.dispatchEvent(new StorageEvent('storage', {
    key,
    newValue: key ? storageArea.getItem(key) : null,
    storageArea,
  }))
}

describe('configuration shared by main and tray windows', () => {
  beforeEach(async () => {
    tauri.invoke.mockResolvedValue(undefined)
    store.updateConfig(initialConfig)
    await nextTick()
    tauri.invoke.mockClear()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('receives settings from another window without writing them back', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      ...store.config.value,
      singboxPath: 'C:\\new\\sing-box.exe',
      theme: 'dark',
      closeToTray: true,
    }))
    const write = vi.spyOn(localStorage, 'setItem')

    notifyStorage()
    await nextTick()

    expect(store.config.value.singboxPath).toBe('C:\\new\\sing-box.exe')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
    expect(tauri.invoke).toHaveBeenCalledWith('set_close_to_tray', { enabled: true })
    expect(write).not.toHaveBeenCalled()

    store.config.value.theme = 'light'
    expect(write).toHaveBeenCalledOnce()
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!).theme).toBe('light')
  })

  it.each([STORAGE_KEY, null])('resets settings after removal or clear (%s) without recreating the key', async (key) => {
    store.updateConfig({ singboxPath: 'C:\\app\\sing-box.exe', theme: 'dark', closeToTray: true })
    await nextTick()
    if (key === null) localStorage.clear()
    else localStorage.removeItem(key)
    const write = vi.spyOn(localStorage, 'setItem')

    notifyStorage(key)
    await nextTick()

    expect(store.config.value.singboxPath).toBe('')
    expect(store.config.value.theme).toBe('light')
    expect(store.config.value.closeToTray).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
    expect(write).not.toHaveBeenCalled()
  })

  it('ignores unrelated storage changes and malformed settings', async () => {
    store.updateConfig({ singboxPath: 'C:\\app\\sing-box.exe' })
    await nextTick()
    sessionStorage.setItem(STORAGE_KEY, '{}')
    notifyStorage(STORAGE_KEY, sessionStorage)
    notifyStorage('unrelated')
    localStorage.setItem(STORAGE_KEY, '{broken')
    notifyStorage()
    await nextTick()

    expect(store.config.value.singboxPath).toBe('C:\\app\\sing-box.exe')
    expect(localStorage.getItem(STORAGE_KEY)).toBe('{broken')
  })

  it('uses the latest selected profile when restarting before a storage event is delivered', async () => {
    store.updateConfig({
      singboxPath: 'C:\\old\\sing-box.exe',
      workingDir: 'C:\\old',
      configProfiles: [{ id: 'old', name: 'Old', type: 'local', source: 'C:\\old\\config.json', autoUpdateInterval: 0 }],
      activeConfigProfileId: 'old',
    })
    await nextTick()
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      ...store.config.value,
      singboxPath: 'C:\\new\\sing-box.exe',
      workingDir: 'C:\\new',
      configProfiles: [{ id: 'new', name: 'New', type: 'local', source: 'C:\\new\\config.json', autoUpdateInterval: 0 }],
      activeConfigProfileId: 'new',
    }))

    await restartCore()

    expect(tauri.invoke).toHaveBeenCalledWith('validate_config', {
      singboxPath: 'C:\\new\\sing-box.exe',
      configPath: 'C:\\new\\config.json',
      workingDir: 'C:\\new',
    })
    expect(tauri.invoke).toHaveBeenCalledWith('copy_to_running_config', { sourcePath: 'C:\\new\\config.json' })
    expect(tauri.invoke).toHaveBeenCalledWith('service_restart')
  })

  it('preserves a local settings edit when restarting in the same tick', async () => {
    store.updateConfig({
      singboxPath: 'C:\\app\\sing-box.exe',
      configProfiles: [{ id: 'current', name: 'Current', type: 'local', source: 'C:\\app\\config.json', autoUpdateInterval: 0 }],
      activeConfigProfileId: 'current',
    })

    await restartCore()

    expect(tauri.invoke).toHaveBeenCalledWith('copy_to_running_config', { sourcePath: 'C:\\app\\config.json' })
  })
})
