import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'

const tauri = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(() => Promise.resolve(null)),
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: tauri.listen }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: () => Promise.resolve('3.2.8') }))

import CoreUpdateCard from './CoreUpdateCard.vue'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'

const release = {
  version: 'v1.15.0',
  prerelease: true,
  publishedAt: '2026-11-06T13:14:00Z',
  assetName: 'sing-box-windows-amd64.zip',
  assetUrl: 'https://example.invalid/sing-box.zip',
  assetSize: 123,
  assetDigest: '',
}

function body() {
  return new DOMWrapper(document.body)
}

describe('core update flow', () => {
  const wrappers: VueWrapper[] = []

  beforeEach(() => {
    const { config } = useConfigStore()
    config.value.singboxPath = 'C:\\sing-box\\sing-box.exe'
    config.value.coreUpdateSource = 'official'
    config.value.coreUpdateMirror = 'https://mirror.example.com/'
    tauri.invoke.mockReset()
  })

  afterEach(() => {
    for (const wrapper of wrappers.splice(0)) wrapper.unmount()
    useConfigStore().config.value.coreUpdateMirror = ''
    const toasts = useToastStore()
    for (const toast of [...toasts.toasts.value]) toasts.removeToast(toast.id)
    document.body.innerHTML = ''
  })

  it('shows the release as structured details with a changelog link and a mirror warning', async () => {
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case 'check_core_update': return Promise.resolve(release)
        case 'get_singbox_version': return Promise.resolve('sing-box version 1.14.0')
        case 'get_file_hash': return Promise.resolve('hash')
        default: return Promise.resolve(null)
      }
    })
    const wrapper = mount(CoreUpdateCard)
    wrappers.push(wrapper)
    await wrapper.get('button').trigger('click')
    await flushPromises()

    const dialog = body().get('[role="alertdialog"]')
    const details = dialog.findAll('dt').map((term) => term.text())
    expect(details).toEqual(['当前', '最新', '发布时间', '来源', '下载经由'])
    expect(dialog.text()).toContain('1.15.0 · 测试版')
    expect(dialog.text()).toContain('SagerNet/sing-box · 稳定版通道')
    expect(dialog.get('[role="note"]').text()).toContain('没有提供文件校验值')

    const link = dialog.findAll('button').find((button) => button.text().includes('查看更新日志'))!
    await link.trigger('click')
    expect(tauri.invoke).toHaveBeenCalledWith('open_release_page', {
      url: 'https://github.com/SagerNet/sing-box/releases/tag/v1.15.0',
    })
  })

  it('keeps an update in progress visible after leaving the page and blocks a second run', async () => {
    let finish!: (value: unknown) => void
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case 'check_core_update': return Promise.resolve(release)
        case 'get_singbox_version': return Promise.resolve('sing-box version 1.14.0')
        case 'get_file_hash': return Promise.resolve('hash')
        case 'perform_core_update': return new Promise((resolve) => { finish = resolve })
        default: return Promise.resolve(null)
      }
    })
    const first = mount(CoreUpdateCard)
    await first.get('button').trigger('click')
    await flushPromises()
    await body().get('.btn-primary').trigger('click')
    await flushPromises()
    first.unmount()

    const second = mount(CoreUpdateCard)
    wrappers.push(second)
    await flushPromises()
    expect(second.find('.settings-update-progress').exists()).toBe(true)
    expect(second.get('.settings-update-status').text()).toContain('正在更新核心')
    expect(second.get('button').attributes('disabled')).toBeDefined()

    await second.get('button').trigger('click')
    expect(tauri.invoke.mock.calls.filter(([command]) => command === 'check_core_update')).toHaveLength(1)

    finish({ version: '1.15.0', restarted: true })
    await flushPromises()
    expect(second.find('.settings-update-progress').exists()).toBe(false)
    expect(tauri.invoke.mock.calls.filter(([command]) => command === 'perform_core_update')).toHaveLength(1)
  })

  it('validates the mirror prefix on blur and says where it applies', async () => {
    tauri.invoke.mockResolvedValue(null)
    const wrapper = mount(CoreUpdateCard, { attachTo: document.body })
    wrappers.push(wrapper)
    const input = wrapper.get('#core-update-mirror')
    await input.setValue('mirror.example.com')
    await input.trigger('blur')
    expect(wrapper.get('#core-update-mirror-feedback').text()).toBe('镜像前缀要以 http:// 或 https:// 开头')
    expect(useConfigStore().config.value.coreUpdateMirror).toBe('https://mirror.example.com/')

    await input.setValue('https://other.example.com/')
    await input.trigger('blur')
    expect(useConfigStore().config.value.coreUpdateMirror).toBe('https://other.example.com/')
    expect(wrapper.get('#core-update-mirror-feedback').text()).toBe('已保存，核心和面板更新都会经过这个镜像')
  })

  it('turns a raw transport error into advice', async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === 'check_core_update') return Promise.reject('请求 GitHub API 失败: error sending request for url (https://api.github.com/x)')
      return Promise.resolve(null)
    })
    const wrapper = mount(CoreUpdateCard)
    wrappers.push(wrapper)
    await wrapper.get('button').trigger('click')
    await flushPromises()
    const toast = useToastStore().toasts.value.at(-1)
    expect(toast?.message).toBe('检查核心更新失败：网络连接失败，请检查网络，或设置下载镜像后重试')
  })
})
