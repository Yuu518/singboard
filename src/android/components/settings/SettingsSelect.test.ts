import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import SettingsSelect from './SettingsSelect.vue'

const options = [
  { value: 'stable', label: '稳定版' },
  { value: 'testing', label: '测试版', description: 'pre-release' },
]

describe('SettingsSelect', () => {
  it('shows the selected label and falls back to the placeholder', async () => {
    const wrapper = mount(SettingsSelect, { props: { modelValue: 'testing', options } })
    expect(wrapper.get('.settings-select-value').text()).toBe('测试版')

    await wrapper.setProps({ modelValue: '' })
    expect(wrapper.get('.settings-select-value').text()).toBe('请选择')
    expect(wrapper.get('.settings-select-value').classes()).toContain('is-placeholder')
    wrapper.unmount()
  })

  it('opens a listbox and emits the chosen value', async () => {
    const wrapper = mount(SettingsSelect, { props: { modelValue: 'stable', options }, attachTo: document.body })
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)

    await wrapper.get('.settings-select-trigger').trigger('click')
    const items = wrapper.findAll('[role="option"]')
    expect(items).toHaveLength(2)
    expect(items[0].attributes('aria-selected')).toBe('true')
    expect(items[1].text()).toContain('pre-release')

    await items[1].trigger('click')
    expect(wrapper.emitted('update:modelValue')).toEqual([['testing']])
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('does not emit when the current value is chosen again', async () => {
    const wrapper = mount(SettingsSelect, { props: { modelValue: 'stable', options } })
    await wrapper.get('.settings-select-trigger').trigger('click')
    await wrapper.findAll('[role="option"]')[0].trigger('click')
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    wrapper.unmount()
  })

  it('closes on an outside pointerdown and on Escape', async () => {
    const wrapper = mount(SettingsSelect, { props: { modelValue: 'stable', options }, attachTo: document.body })
    await wrapper.get('.settings-select-trigger').trigger('click')
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }))
    await wrapper.vm.$nextTick()
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)

    await wrapper.get('.settings-select-trigger').trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)
    await wrapper.get('[role="listbox"]').trigger('keydown', { key: 'Escape' })
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('does not open when disabled', async () => {
    const wrapper = mount(SettingsSelect, { props: { modelValue: 'stable', options, disabled: true } })
    await wrapper.get('.settings-select-trigger').trigger('click')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    wrapper.unmount()
  })
})
