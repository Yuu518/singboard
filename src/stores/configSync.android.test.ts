import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { useConfigStore } from './config'
import { restartCore } from '@/android/utils/coreControl'

const tauri = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }))

const STORAGE_KEY = 'singboard-config'
const store = useConfigStore()
const initialConfig = JSON.parse(JSON.stringify(store.config.value))

const oldPaths = {
  workingDir: '/data/adb/old',
  singboxPath: '/data/adb/old/bin/sing-box',
  configPath: '/data/adb/old/config.json',
}

const newPaths = {
  workingDir: '/data/adb/new',
  singboxPath: '/data/adb/new/sing-box',
  configPath: '/data/adb/new/conf/config.json',
}

function notifyStorage(key: string | null = STORAGE_KEY, storageArea = localStorage) {
  window.dispatchEvent(new StorageEvent('storage', {
    key,
    newValue: key ? storageArea.getItem(key) : null,
    storageArea,
  }))
}

describe('configuration kept in sync with storage events', () => {
  beforeEach(async () => {
    tauri.invoke.mockResolvedValue(undefined)
    store.updateConfig(initialConfig)
    await nextTick()
    tauri.invoke.mockClear()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('receives settings from another context without writing them back', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      ...store.config.value,
      ...newPaths,
      theme: 'dark',
    }))
    const write = vi.spyOn(localStorage, 'setItem')

    notifyStorage()
    await nextTick()

    expect(store.config.value.singboxPath).toBe(newPaths.singboxPath)
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
    expect(write).not.toHaveBeenCalled()

    store.config.value.theme = 'light'
    expect(write).toHaveBeenCalledOnce()
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!).theme).toBe('light')
  })

  it.each([STORAGE_KEY, null])('resets settings after removal or clear (%s) without recreating the key', async (key) => {
    store.updateConfig({ ...oldPaths, theme: 'dark', rootMode: true })
    await nextTick()
    if (key === null) localStorage.clear()
    else localStorage.removeItem(key)
    const write = vi.spyOn(localStorage, 'setItem')

    notifyStorage(key)
    await nextTick()

    expect(store.config.value.singboxPath).toBe('')
    expect(store.config.value.workingDir).toBe('')
    expect(store.config.value.theme).toBe('light')
    expect(store.config.value.rootMode).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
    expect(write).not.toHaveBeenCalled()
  })

  it('ignores unrelated storage changes and malformed settings', async () => {
    store.updateConfig({ singboxPath: oldPaths.singboxPath })
    await nextTick()
    sessionStorage.setItem(STORAGE_KEY, '{}')
    notifyStorage(STORAGE_KEY, sessionStorage)
    notifyStorage('unrelated')
    localStorage.setItem(STORAGE_KEY, '{broken')
    notifyStorage()
    await nextTick()

    expect(store.config.value.singboxPath).toBe(oldPaths.singboxPath)
    expect(localStorage.getItem(STORAGE_KEY)).toBe('{broken')
  })

  it('uses the latest detected paths when restarting before a storage event is delivered', async () => {
    store.updateConfig(oldPaths)
    await nextTick()
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...store.config.value, ...newPaths }))

    await restartCore()

    expect(tauri.invoke).toHaveBeenCalledWith('validate_config', {
      singboxPath: newPaths.singboxPath,
      configPath: newPaths.configPath,
      workingDir: newPaths.workingDir,
    })
    expect(tauri.invoke).toHaveBeenCalledWith('service_install', newPaths)
    expect(tauri.invoke).toHaveBeenLastCalledWith('service_restart')
  })

  it('refuses to start without a detected config file', async () => {
    store.updateConfig({ ...oldPaths, configPath: '' })
    await nextTick()

    await expect(restartCore()).rejects.toThrow('未在工作目录中找到配置文件')
    expect(tauri.invoke).not.toHaveBeenCalledWith('service_restart')
  })
})
