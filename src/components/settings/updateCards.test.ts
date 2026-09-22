import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'

const tauri = vi.hoisted(() => ({
  getVersion: vi.fn(() => Promise.resolve('3.2.8')),
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(() => Promise.resolve(null)),
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@tauri-apps/api/app', () => ({ getVersion: tauri.getVersion }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: tauri.listen }))

import CoreUpdateCard from './CoreUpdateCard.vue'
import PanelUpdateCard from './PanelUpdateCard.vue'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

function clearToasts() {
  const store = useToastStore()
  for (const toast of [...store.toasts.value]) store.removeToast(toast.id)
}

describe('update cards', () => {
  const wrappers: VueWrapper[] = []
  let originalSingboxPath = ''

  beforeEach(() => {
    tauri.invoke.mockReset()
    tauri.invoke.mockImplementation(() => Promise.resolve(null))
    clearToasts()
    originalSingboxPath = useConfigStore().config.value.singboxPath
    localStorage.removeItem('singboard-core-install-record')
    localStorage.removeItem('singboard-core-install-record-v2')
  })

  afterEach(() => {
    for (const wrapper of wrappers.splice(0)) wrapper.unmount()
    useConfigStore().config.value.singboxPath = originalSingboxPath
    localStorage.removeItem('singboard-core-install-record')
    localStorage.removeItem('singboard-core-install-record-v2')
    clearToasts()
  })

  it('keeps the panel check state visible and renders an unshadowed child spinner', async () => {
    const pending = deferred<unknown>()
    tauri.invoke.mockImplementation((command: string) => {
      if (command === 'check_panel_update') return pending.promise
      return Promise.resolve(null)
    })

    const wrapper = mount(PanelUpdateCard)
    wrappers.push(wrapper)
    await flushPromises()
    expect(wrapper.get('.settings-update-status').text()).toContain('尚未检查上游版本')

    const button = wrapper.get('button')
    await button.trigger('click')

    expect.soft(button.classes()).not.toContain('loading')
    expect.soft(button.find('.loading.loading-spinner').exists()).toBe(true)
    expect.soft(wrapper.get('.settings-update-status').text()).toContain('正在检查上游版本')
    expect.soft(wrapper.get('.settings-update-status').attributes('aria-live')).toBe('polite')

    pending.resolve({
      currentVersion: '3.2.8',
      latestVersion: '3.2.8',
      hasUpdate: false,
      outOfSync: false,
      publishedAt: '2026-08-12T00:00:00Z',
      assetUrl: '',
      assetSize: 0,
      assetDigest: '',
    })
    await flushPromises()

    const status = wrapper.get('.settings-update-status').text()
    expect.soft(status).toContain('已是最新版本')
    expect.soft(status).toContain('3.2.8')
    expect.soft(status).not.toContain('尚未检查上游版本')
  })

  it('does not label the current core release as an available update', async () => {
    const { config } = useConfigStore()
    config.value.singboxPath = 'C:\\sing-box\\sing-box.exe'
    const pending = deferred<unknown>()

    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case 'check_core_update':
          return pending.promise
        case 'get_file_hash':
          return Promise.resolve('local-hash')
        case 'get_singbox_version':
          return Promise.resolve('sing-box version 1.14.0-beta.13')
        default:
          return Promise.resolve(null)
      }
    })

    const wrapper = mount(CoreUpdateCard)
    wrappers.push(wrapper)
    const button = wrapper.get('button')
    await button.trigger('click')

    expect.soft(button.classes()).not.toContain('loading')
    expect.soft(button.find('.loading.loading-spinner.settings-update-spinner').exists()).toBe(true)
    expect.soft(wrapper.get('.settings-update-status').text()).toContain('正在检查上游版本')
    expect.soft(wrapper.get('.settings-update-status').attributes('aria-live')).toBe('polite')

    pending.resolve({
      version: 'v1.14.0-beta.13',
      prerelease: true,
      publishedAt: '2026-08-12T00:00:00Z',
      assetName: 'sing-box-windows-amd64.zip',
      assetUrl: 'https://example.invalid/sing-box.zip',
      assetSize: 123,
      assetDigest: '',
    })
    await flushPromises()

    const status = wrapper.get('.settings-update-status').text()
    expect.soft(status).toContain('已是最新版本')
    expect.soft(status).toContain('1.14.0-beta.13')
    expect.soft(status).not.toContain('可用版本')
  })

  it('passes the upstream digest to the probe and ignores legacy unverified install records', async () => {
    const { config } = useConfigStore()
    config.value.singboxPath = 'C:\\sing-box\\sing-box.exe'
    const assetDigest = `sha256:${'ab'.repeat(32)}`
    const info = {
      version: 'v1.14.0-beta.13',
      prerelease: true,
      publishedAt: '2026-08-12T00:00:00Z',
      assetName: 'sing-box-windows-amd64.zip',
      assetUrl: 'https://example.invalid/sing-box.zip',
      assetSize: 123,
      assetDigest,
    }
    localStorage.setItem('singboard-core-install-record', JSON.stringify({
      assetDigest,
      exeHash: 'local-hash',
    }))
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case 'check_core_update': return Promise.resolve(info)
        case 'get_file_hash':
        case 'probe_asset_exe_hash': return Promise.resolve('local-hash')
        case 'get_singbox_version': return Promise.resolve('sing-box version 1.14.0-beta.13')
        default: return Promise.resolve(null)
      }
    })

    const wrapper = mount(CoreUpdateCard)
    wrappers.push(wrapper)
    await wrapper.get('button').trigger('click')
    await flushPromises()

    expect(tauri.invoke).toHaveBeenCalledWith('probe_asset_exe_hash', {
      assetUrl: info.assetUrl,
      assetSize: info.assetSize,
      assetDigest,
      mirror: config.value.coreUpdateMirror,
    })
    expect(wrapper.get('.settings-update-status').text()).toContain('已是最新版本')
    expect(wrapper.find('.btn-primary').exists()).toBe(false)
  })

  it('passes the checked release digest when installing a core update', async () => {
    const { config } = useConfigStore()
    config.value.singboxPath = 'C:\\sing-box\\sing-box.exe'
    const info = {
      version: 'v1.15.0',
      prerelease: false,
      publishedAt: '2026-08-12T00:00:00Z',
      assetName: 'sing-box-windows-amd64.zip',
      assetUrl: 'https://example.invalid/sing-box.zip',
      assetSize: 123,
      assetDigest: `sha256:${'cd'.repeat(32)}`,
    }
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case 'check_core_update': return Promise.resolve(info)
        case 'get_file_hash': return Promise.resolve('local-hash')
        case 'get_singbox_version': return Promise.resolve('sing-box version 1.14.0-beta.13')
        case 'perform_core_update': return Promise.resolve({ version: '1.15.0', restarted: true })
        default: return Promise.resolve(null)
      }
    })

    const wrapper = mount(CoreUpdateCard)
    wrappers.push(wrapper)
    await wrapper.get('button').trigger('click')
    await flushPromises()
    await wrapper.get('.btn-primary').trigger('click')
    await flushPromises()

    expect(tauri.invoke).toHaveBeenCalledWith('perform_core_update', {
      assetUrl: info.assetUrl,
      assetSize: info.assetSize,
      assetDigest: info.assetDigest,
      mirror: config.value.coreUpdateMirror,
      singboxPath: config.value.singboxPath,
    })
  })
})
