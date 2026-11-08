import { EditorView } from '@codemirror/view'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import ConfigEditor from './ConfigEditor.vue'
import { useToastStore } from '@/stores/toast'

const bridge = vi.hoisted(() => ({
  readSingboxConfig: vi.fn(),
  replaceConfig: vi.fn(),
  validateSingboxConfigContent: vi.fn(),
}))

vi.mock('@/bridge/config', () => bridge)

describe('configuration module editing', () => {
  let wrapper: VueWrapper | undefined

  beforeEach(() => {
    bridge.readSingboxConfig.mockResolvedValue(JSON.stringify({
      log: { level: 'info' },
      dns: { servers: [] },
    }, null, 2))
    bridge.replaceConfig.mockReset()
    bridge.replaceConfig.mockResolvedValue(undefined)
  })

  afterEach(() => {
    wrapper?.unmount()
    const { toasts, removeToast } = useToastStore()
    for (const toast of [...toasts.value]) removeToast(toast.id)
  })

  it('keeps saved module edits when saving again and switching modules or editor modes', async () => {
    wrapper = mount(ConfigEditor, {
      props: {
        configPath: 'C:\\app\\config.json',
        singboxPath: 'C:\\app\\sing-box.exe',
        workingDir: 'C:\\app',
      },
    })
    await flushPromises()

    await wrapper.findAll('button').find((button) => button.text() === '分模块编辑')!.trigger('click')
    const editor = EditorView.findFromDOM(wrapper.get('.cm-editor').element as HTMLElement)!
    editor.dispatch({
      changes: { from: 0, to: editor.state.doc.length, insert: '{"level":"debug"}' },
    })
    await nextTick()
    await wrapper.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()

    expect(JSON.parse(bridge.replaceConfig.mock.calls[0][1]).log.level).toBe('debug')
    expect(JSON.parse(editor.state.doc.toString()).level).toBe('debug')
    expect(wrapper.text()).not.toContain('未保存')

    await wrapper.get('.cm-content').trigger('keydown', { key: 's', code: 'KeyS', ctrlKey: true })
    await flushPromises()
    expect(bridge.replaceConfig).toHaveBeenCalledTimes(1)
    expect((wrapper.vm as unknown as { isDirty: () => boolean }).isDirty()).toBe(false)

    await wrapper.get('select').setValue('dns')
    await wrapper.get('select').setValue('log')
    expect(JSON.parse(editor.state.doc.toString()).level).toBe('debug')

    await wrapper.findAll('button').find((button) => button.text() === '整体编辑')!.trigger('click')
    expect(JSON.parse(editor.state.doc.toString()).log.level).toBe('debug')
  })

  it('keeps comments elsewhere in the file when saving one module', async () => {
    bridge.readSingboxConfig.mockResolvedValue([
      '{',
      '  // 日志设置',
      '  "log": { "level": "info" },',
      '  /* DNS 配置 */',
      '  "dns": {',
      '    // 国内解析',
      '    "servers": []',
      '  }',
      '}',
    ].join('\n'))
    wrapper = mount(ConfigEditor, {
      props: { configPath: 'C:\\app\\config.json', singboxPath: 'C:\\app\\sing-box.exe', workingDir: 'C:\\app' },
    })
    await flushPromises()

    await wrapper.findAll('button').find((button) => button.text() === '分模块编辑')!.trigger('click')
    const editor = EditorView.findFromDOM(wrapper.get('.cm-editor').element as HTMLElement)!
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: '{ "level": "debug" }' } })
    await nextTick()
    await wrapper.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()

    const written = bridge.replaceConfig.mock.calls[0][1] as string
    expect(written).toContain('// 日志设置')
    expect(written).toContain('/* DNS 配置 */')
    expect(written).toContain('// 国内解析')
    expect(written).toContain('"level": "debug"')
  })
})
