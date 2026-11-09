import { afterEach, expect, it, vi } from 'vitest'
import { h, ref } from 'vue'
import { mount } from '@vue/test-utils'
import type { ClashApiState } from '@/stores/service'

const state = vi.hoisted(() => ({ status: null as any, api: null as any, config: null as any }))
vi.mock('@/stores/service', () => ({ useServiceStore: () => ({ serviceStatus: state.status, clashApiState: state.api }) }))
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({ config: state.config }) }))
afterEach(() => { vi.resetModules() })

async function mountGate(serviceState: string, apiState: ClashApiState, rootMode = true) {
  state.status = ref({ state: serviceState })
  state.api = ref(apiState)
  state.config = ref({ rootMode })
  const { default: ClashApiGate } = await import('./ClashApiGate.vue')
  return mount(ClashApiGate, {
    props: { subject: '规则信息' },
    slots: { default: () => h('div', { 'data-content': '' }, 'content') },
  })
}

function contentInert(wrapper: Awaited<ReturnType<typeof mountGate>>) {
  return wrapper.get('.clash-gate-content').attributes('inert') !== undefined
}

it('blurs the page behind a stopped mask and blocks interaction', async () => {
  const wrapper = await mountGate('stopped', 'down')
  expect(wrapper.get('[data-gate]').attributes('data-gate')).toBe('stopped')
  expect(wrapper.text()).toContain('服务未运行')
  expect(wrapper.text()).toContain('请先启动 sing-box 服务以查看规则信息')
  expect(wrapper.find('[data-content]').exists()).toBe(true)
  expect(contentInert(wrapper)).toBe(true)
})

it('names the Clash API when the panel is not in root mode', async () => {
  const wrapper = await mountGate('stopped', 'down', false)
  expect(wrapper.text()).toContain('未连接到 Clash API')
})

it('shows the starting mask while the core runs but the Clash API is not ready', async () => {
  const wrapper = await mountGate('running', 'waiting')
  expect(wrapper.get('[data-gate]').attributes('data-gate')).toBe('starting')
  expect(wrapper.text()).toContain('服务启动中')
  expect(wrapper.find('.loading-spinner').exists()).toBe(true)
  expect(contentInert(wrapper)).toBe(true)
})

it('shows the secret mismatch mask on a rejected secret', async () => {
  const wrapper = await mountGate('running', 'auth_failed')
  expect(wrapper.get('[data-gate]').attributes('data-gate')).toBe('auth')
  expect(wrapper.text()).toContain('Clash API 密钥不匹配')
})

it('removes the mask and restores interaction once the Clash API is ready', async () => {
  const wrapper = await mountGate('running', 'waiting')
  state.api.value = 'ready'
  await wrapper.vm.$nextTick()
  expect(wrapper.find('[data-gate]').exists()).toBe(false)
  expect(wrapper.find('[data-content]').exists()).toBe(true)
  expect(contentInert(wrapper)).toBe(false)
})
