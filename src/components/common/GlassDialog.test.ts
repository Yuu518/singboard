import { afterEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import GlassDialog from './GlassDialog.vue'

describe('GlassDialog', () => {
  let wrapper: VueWrapper | undefined

  afterEach(() => {
    wrapper?.unmount()
    document.body.innerHTML = ''
  })

  function mountDialog(props: { open: boolean; busy?: boolean }) {
    wrapper = mount(GlassDialog, {
      props: { title: '添加远程配置', ...props },
      slots: { default: '<input id="first" /><button type="button">取消</button>' },
      attachTo: document.body,
    })
    return wrapper
  }

  it('is a labelled modal dialog that focuses its first field', async () => {
    mountDialog({ open: true })
    await nextTick()
    await nextTick()
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    const title = document.getElementById(dialog.getAttribute('aria-labelledby')!)
    expect(title?.textContent).toBe('添加远程配置')
    expect(document.activeElement?.id).toBe('first')
  })

  it('closes on Escape unless busy', async () => {
    const dialog = mountDialog({ open: true })
    await nextTick()
    const panel = document.querySelector('[role="dialog"]')!
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(dialog.emitted('close')).toHaveLength(1)

    await dialog.setProps({ busy: true })
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(dialog.emitted('close')).toHaveLength(1)
  })

  it('keeps Tab inside the dialog', async () => {
    mountDialog({ open: true })
    await nextTick()
    await nextTick()
    const panel = document.querySelector('[role="dialog"]')!
    const cancel = panel.querySelector('button')!
    cancel.focus()
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }))
    expect(document.activeElement?.id).toBe('first')
  })
})
