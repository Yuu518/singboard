import { describe, expect, it, vi } from 'vitest'
import { computed, createApp, nextTick, ref, shallowRef } from 'vue'
import { PAGE_ACTIVE } from '@/android/composables/usePageActive'
import type { LogEntry } from '@/types'

const store = vi.hoisted(() => ({ value: null as any }))
const service = vi.hoisted(() => ({ running: true }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/components/common/ServiceNotice.vue', async () => {
  const { h } = await import('vue')
  return { default: { render: () => h('div', { 'data-service-notice': '' }) } }
})
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({
    serviceState: ref(service.running ? 'running' : 'stopped'),
    isRunning: ref(service.running),
  }),
}))
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({ clashApiUrl: ref('http://127.0.0.1:9090'), activeClashApiId: ref('default') }),
}))
vi.mock('@/stores/logs', () => ({
  LOG_LEVELS: ['panic', 'fatal', 'error', 'warn', 'info', 'debug', 'trace'],
  useLogsStore: () => store.value,
}))

function makeStore(entries: LogEntry[]) {
  const logs = shallowRef(entries)
  return {
    logs,
    filteredLogs: computed(() => logs.value),
    levelFilter: ref('trace'),
    levelChoice: ref(''),
    coreFilterLevel: ref('info'),
    paused: ref(false),
    newSincePause: ref(0),
    filterText: ref(''),
    socketState: ref('open'),
    socketFailures: ref(0),
    setPaused: vi.fn(),
    clear: vi.fn(() => []),
    restore: vi.fn(),
  }
}

async function mountPage(entries: LogEntry[], options: { running?: boolean; active?: boolean } = {}) {
  service.running = options.running ?? true
  store.value = makeStore(entries)
  const active = ref(options.active ?? true)
  const { default: LogsPage } = await import('./LogsPage.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(LogsPage)
  app.provide(PAGE_ACTIVE, computed(() => active.value))
  app.mount(host)
  await nextTick()
  return { host, active, unmount: () => { app.unmount(); host.remove() } }
}

describe('LogsPage on Android', () => {
  it('renders logs as tappable glass tiles that scroll under the bottom navigation', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'inbound connection from 127.0.0.1', time: '10:00:00', seq: 1 },
      { type: 'marker', payload: '窗口隐藏期间未接收日志（10:00:01–10:00:09）', time: '10:00:10', seq: 2 },
      { type: 'error', payload: 'dial tcp: i/o timeout', time: '10:00:10', seq: 3 },
    ])

    const list = host.querySelector<HTMLElement>('[role="log"]')!
    expect(list.classList).toContain('under-nav')
    expect(list.classList).not.toContain('card-scroll')
    expect(list.parentElement!.classList).not.toContain('surface-card')

    const rows = host.querySelectorAll<HTMLElement>('[data-virtual-key]')
    expect(rows).toHaveLength(3)
    expect(rows[0].getAttribute('role')).toBe('button')
    expect(rows[0].querySelector('.log-tile')!.classList).toContain('surface-fill')
    expect(rows[0].textContent).toContain('10:00:00')
    expect(rows[1].getAttribute('role')).toBeNull()
    expect(rows[1].textContent).toContain('窗口隐藏期间未接收日志')
    expect(rows[2].querySelector('.log-tile')!.className).toContain('bg-error/[0.1]')
    unmount()
  })

  it('shows two lines of a log and expands the whole line on tap', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'a very long line', time: '10:00:00', seq: 1 },
    ])
    const row = host.querySelector<HTMLElement>('[data-virtual-key="1"]')!
    const payload = row.querySelector('.log-tile')!.lastElementChild!
    expect(payload.className).toContain('line-clamp-2')
    expect(row.getAttribute('aria-expanded')).toBe('false')

    row.click()
    await nextTick()
    expect(payload.className).toContain('whitespace-pre-wrap')
    expect(row.getAttribute('aria-expanded')).toBe('true')
    unmount()
  })

  it('leaves the stopped-service state to the Clash API gate', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'one', time: '10:00:00', seq: 1 },
    ], { running: false })
    expect(host.querySelector('[role="log"]')).not.toBeNull()
    expect(host.querySelector('[data-service-notice]')).toBeNull()
    unmount()
  })

  it('holds the shown logs while the page is off screen and catches up when it returns', async () => {
    const { host, active, unmount } = await mountPage([
      { type: 'info', payload: 'one', time: '10:00:00', seq: 1 },
    ], { active: false })

    store.value.logs.value = [
      ...store.value.logs.value,
      { type: 'info', payload: 'two', time: '10:00:01', seq: 2 },
    ]
    await nextTick()
    expect(host.querySelectorAll('[data-virtual-key]')).toHaveLength(1)

    active.value = true
    await nextTick()
    expect(host.querySelectorAll('[data-virtual-key]')).toHaveLength(2)
    unmount()
  })

  it('ignores the desktop search shortcut', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'one', time: '10:00:00', seq: 1 },
    ], { active: false })
    const input = host.querySelector<HTMLInputElement>('input[type="search"]')!
    expect(input.hasAttribute('aria-keyshortcuts')).toBe(false)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, cancelable: true }))
    expect(document.activeElement).not.toBe(input)
    unmount()
  })
})
