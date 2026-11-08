import { EditorView } from '@codemirror/view'
import { undo } from '@codemirror/commands'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import ConfigEditor from './ConfigEditor.vue'
import { useToastStore } from '@/stores/toast'

const bridge = vi.hoisted(() => ({
  readSingboxConfig: vi.fn(),
  replaceConfig: vi.fn(),
  validateSingboxConfigContent: vi.fn(),
}))

vi.mock('@/bridge/config', () => bridge)

const SOURCE = [
  '{',
  '  // 日志',
  '  "log": { "level": "info" },',
  '  "dns": { "servers": [] }',
  '}',
].join('\n')

type EditorVm = { isDirty: () => boolean }

describe('editor dirty state and save checks', () => {
  let wrapper: VueWrapper | undefined

  beforeEach(() => {
    bridge.readSingboxConfig.mockResolvedValue(SOURCE)
    bridge.replaceConfig.mockReset()
    bridge.replaceConfig.mockResolvedValue(undefined)
    bridge.validateSingboxConfigContent.mockReset()
    bridge.validateSingboxConfigContent.mockResolvedValue('Configuration is valid')
  })

  afterEach(() => {
    wrapper?.unmount()
    document.body.innerHTML = ''
    const { toasts, removeToast } = useToastStore()
    for (const toast of [...toasts.value]) removeToast(toast.id)
  })

  async function mountEditor(validateBeforeSave = false) {
    wrapper = mount(ConfigEditor, {
      props: { configPath: 'C:\\app\\config.json', singboxPath: 'C:\\app\\sing-box.exe', workingDir: 'C:\\app', validateBeforeSave },
      attachTo: document.body,
    })
    await flushPromises()
    const editor = EditorView.findFromDOM(wrapper.get('.cm-editor').element as HTMLElement)!
    const vm = wrapper.vm as unknown as EditorVm
    return { editor, vm }
  }

  async function toModules() {
    await wrapper!.findAll('button').find((button) => button.text() === '分模块编辑')!.trigger('click')
  }

  function replaceAll(editor: EditorView, text: string) {
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: text } })
  }

  it('counts a comment-only module change as unsaved and writes it', async () => {
    const { editor, vm } = await mountEditor()
    await toModules()
    expect(vm.isDirty()).toBe(false)

    replaceAll(editor, '{ "level": "info" } // 只改注释')
    await nextTick()
    expect(vm.isDirty()).toBe(true)

    await wrapper!.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()
    expect(bridge.replaceConfig.mock.calls[0][1]).toContain('// 只改注释')
    expect(vm.isDirty()).toBe(false)
  })

  it('stays clean after a module save and a switch back to the whole file', async () => {
    const { editor, vm } = await mountEditor()
    await toModules()
    replaceAll(editor, '{ "level": "warn" }')
    await nextTick()
    await wrapper!.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()

    await wrapper!.findAll('button').find((button) => button.text() === '整体编辑')!.trigger('click')
    await nextTick()
    expect(vm.isDirty()).toBe(false)
    expect(editor.state.doc.toString()).toContain('// 日志')
  })

  it('does not count merely viewing a module that is missing from the file', async () => {
    const { vm } = await mountEditor()
    await toModules()
    const select = wrapper!.get('select')
    const missing = Array.from((select.element as HTMLSelectElement).options).map((o) => o.value).find((v) => v !== 'log' && v !== 'dns')
    if (missing) {
      await select.setValue(missing)
      await nextTick()
    }
    expect(vm.isDirty()).toBe(false)
  })

  it('asks before saving the active config when sing-box rejects it, and saves nothing on cancel', async () => {
    bridge.validateSingboxConfigContent.mockRejectedValue(new Error('decode config: unknown field "foo"'))
    const { editor } = await mountEditor(true)
    replaceAll(editor, SOURCE.replace('"info"', '"debug"'))
    await nextTick()

    await wrapper!.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()
    const dialog = document.querySelector('[role="alertdialog"]')!
    expect(dialog.textContent).toContain('unknown field')

    const cancel = Array.from(dialog.querySelectorAll('button')).find((b) => b.textContent?.trim() === '取消')!
    cancel.click()
    await flushPromises()
    expect(bridge.replaceConfig).not.toHaveBeenCalled()
  })

  it('cannot undo the file loading or a module switch', async () => {
    const { editor, vm } = await mountEditor()
    expect(undo(editor)).toBe(false)
    expect(editor.state.doc.toString()).toBe(SOURCE)

    await toModules()
    await wrapper!.get('select').setValue('dns')
    await nextTick()
    const dnsText = editor.state.doc.toString()
    undo(editor)
    expect(editor.state.doc.toString()).toBe(dnsText)
    expect(vm.isDirty()).toBe(false)
  })

  it('reports whether the active config passed the check when it saves', async () => {
    const { editor } = await mountEditor(true)
    replaceAll(editor, SOURCE.replace('"info"', '"debug"'))
    await nextTick()
    await wrapper!.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()
    expect(wrapper!.emitted('saved')).toEqual([['passed']])
  })

  it('stops at a syntax error before running sing-box', async () => {
    const { editor } = await mountEditor(true)
    replaceAll(editor, '{ "log": ')
    await nextTick()
    await wrapper!.get('button[title="保存（Ctrl+S）"]').trigger('click')
    await flushPromises()
    expect(bridge.validateSingboxConfigContent).not.toHaveBeenCalled()
    expect(bridge.replaceConfig).not.toHaveBeenCalled()
  })
})
