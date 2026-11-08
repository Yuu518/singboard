import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import ConfigProfileCard from './ConfigProfileCard.vue'
import type { ConfigProfile } from '@/types'

const remote: ConfigProfile = {
  id: 'r1',
  name: '机场 A',
  type: 'remote',
  source: 'https://sub.example.com/link?token=secret',
  lastUpdated: '2026-11-08T02:20:00.000Z',
  autoUpdateInterval: 12,
}

describe('ConfigProfileCard', () => {
  it('marks the active profile with text and aria-current instead of a blue button', () => {
    const wrapper = mount(ConfigProfileCard, { props: { profile: remote, isActive: true } })
    expect(wrapper.attributes('aria-current')).toBe('true')
    expect(wrapper.text()).toContain('使用中')
    expect(wrapper.find('.btn-primary').exists()).toBe(false)
  })

  it('shows only the host of a remote address and labels the update time', () => {
    const wrapper = mount(ConfigProfileCard, { props: { profile: remote, isActive: false } })
    expect(wrapper.text()).toContain('sub.example.com')
    expect(wrapper.text()).not.toContain('token=secret')
    expect(wrapper.text()).toContain('更新于')
    expect(wrapper.text()).toContain('每 12 小时自动更新')
  })

  it('hides an unreadable timestamp instead of printing Invalid Date', () => {
    const wrapper = mount(ConfigProfileCard, {
      props: { profile: { ...remote, type: 'local', source: 'D:\\sb\\a.json', lastUpdated: 'nope' }, isActive: false },
    })
    expect(wrapper.text()).not.toContain('Invalid Date')
    expect(wrapper.text()).not.toContain('保存于')
  })

  it('keeps actions focusable but inert while another one runs', async () => {
    const wrapper = mount(ConfigProfileCard, { props: { profile: remote, isActive: false, locked: true, busy: 'update' } })
    const buttons = wrapper.findAll('button')
    expect(buttons.every((button) => button.attributes('disabled') === undefined)).toBe(true)
    expect(buttons.every((button) => button.attributes('aria-disabled') === 'true')).toBe(true)
    for (const button of buttons) await button.trigger('click')
    expect(Object.keys(wrapper.emitted())).not.toEqual(expect.arrayContaining(['update', 'delete', 'select', 'edit']))
    expect(wrapper.text()).toContain('更新中…')
  })

  it('names every action after its profile', () => {
    const wrapper = mount(ConfigProfileCard, { props: { profile: remote, isActive: false, hasBackup: true } })
    const labels = wrapper.findAll('button').map((button) => button.attributes('aria-label'))
    expect(labels).toEqual([
      '使用「机场 A」',
      '编辑「机场 A」',
      '更新「机场 A」',
      '把「机场 A」换回上一版本',
      '修改「机场 A」的名称和来源',
      '删除「机场 A」',
    ])
  })

  it('names the backup by its time and offers it for local files too', () => {
    const local = { ...remote, type: 'local' as const, source: 'D:\sb\a.json', backupUpdatedAt: '2026-11-07T01:30:00.000Z' }
    const wrapper = mount(ConfigProfileCard, { props: { profile: local, isActive: false, hasBackup: true } })
    const restore = wrapper.findAll('button').find((button) => button.text().startsWith('换回'))!
    expect(restore.text()).toMatch(/^换回 11\/7 .+ 的版本$/)
  })

  it('warns that a local edit to a remote config will be overwritten', () => {
    const edited = { ...remote, lastLocalChangeAt: '2026-11-08T03:00:00.000Z' }
    const wrapper = mount(ConfigProfileCard, { props: { profile: edited, isActive: false } })
    expect(wrapper.text()).toContain('下次自动更新会覆盖')
  })

  it('expands the full failure reason on demand', async () => {
    const failing = { ...remote, lastFetchError: 'sing-box 校验未通过：\ndecode config: unknown field "foo"', lastFetchFailedAt: '2026-11-08T01:00:00.000Z' }
    const wrapper = mount(ConfigProfileCard, { props: { profile: failing, isActive: false } })
    expect(wrapper.text()).toContain('保留现有配置')
    const toggle = wrapper.findAll('button').find((button) => button.text() === '详情')!
    expect(toggle.attributes('aria-expanded')).toBe('false')
    await toggle.trigger('click')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    expect(wrapper.get('pre').text()).toContain('unknown field "foo"')
  })
})
