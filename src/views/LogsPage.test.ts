import { describe, expect, it, vi } from 'vitest'
import { computed, createApp, nextTick, ref, shallowRef } from 'vue'
import type { LogEntry } from '@/types'

const store = vi.hoisted(() => ({ value: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref('running'), isRunning: ref(true) }),
}))
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({ clashApiUrl: ref('http://127.0.0.1:9090') }) }))
vi.mock('@/stores/logs', () => ({
  LOG_LEVELS: ['panic', 'fatal', 'error', 'warn', 'info', 'debug', 'trace'],
  useLogsStore: () => store.value,
}))

function makeStore(entries: LogEntry[]) {
  const logs = shallowRef(entries)
  const levelFilter = ref('trace')
  return {
    logs,
    filteredLogs: computed(() => logs.value),
    levelFilter,
    levelChoice: ref(''),
    coreLevel: ref('info'),
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

async function mountPage(entries: LogEntry[]) {
  store.value = makeStore(entries)
  const { default: LogsPage } = await import('./LogsPage.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(LogsPage)
  app.mount(host)
  await nextTick()
  return { host, unmount: () => { app.unmount(); host.remove() } }
}

describe('LogsPage', () => {
  it('renders each log as a single row with a level badge and readable gap markers', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'inbound connection from 127.0.0.1', time: '10:00:00', seq: 1 },
      { type: 'marker', payload: '窗口隐藏期间未接收日志（10:00:01–10:00:09）', time: '10:00:10', seq: 2 },
      { type: 'error', payload: 'dial tcp: i/o timeout', time: '10:00:10', seq: 3 },
    ])

    const rows = host.querySelectorAll('[data-virtual-key]')
    expect(rows).toHaveLength(3)
    expect(rows[0].textContent).toContain('10:00:00')
    expect(rows[0].textContent).toContain('info')
    expect(rows[1].getAttribute('role')).toBeNull()
    expect(rows[1].textContent).toContain('窗口隐藏期间未接收日志')
    expect(rows[0].getAttribute('role')).toBe('button')
    expect(rows[2].className).toContain('bg-error/[0.06]')
    unmount()
  })

  it('expands a row on click and collapses it again', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'a very long line', time: '10:00:00', seq: 1 },
    ])
    const row = host.querySelector<HTMLElement>('[data-virtual-key="1"]')!
    const payload = row.lastElementChild!
    expect(payload.className).toContain('truncate')

    row.click()
    await nextTick()
    expect(payload.className).toContain('whitespace-pre-wrap')

    row.click()
    await nextTick()
    expect(payload.className).toContain('truncate')
    unmount()
  })

  it('focuses search with Ctrl+F and clears it with Escape', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'one', time: '10:00:00', seq: 1 },
    ])
    const input = host.querySelector<HTMLInputElement>('input[type="search"]')!
    expect(input.getAttribute('aria-label')).toBe('搜索日志')

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, cancelable: true }))
    expect(document.activeElement).toBe(input)

    store.value.filterText.value = 'abc'
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(store.value.filterText.value).toBe('')
    unmount()
  })

  it('moves through rows with the keyboard and expands with Enter', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'first', time: '10:00:00', seq: 1 },
      { type: 'marker', payload: 'gap', time: '10:00:01', seq: 2 },
      { type: 'warn', payload: 'second', time: '10:00:02', seq: 3 },
    ])
    const list = host.querySelector<HTMLElement>('[role="log"]')!
    expect(list.getAttribute('aria-live')).toBe('off')
    expect(list.tabIndex).toBe(0)

    list.focus()
    await nextTick()
    await nextTick()
    expect(document.activeElement?.getAttribute('data-virtual-key')).toBe('1')
    expect(list.tabIndex).toBe(-1)

    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
    await nextTick()
    await nextTick()
    const row = document.activeElement as HTMLElement
    expect(row.getAttribute('data-virtual-key')).toBe('3')
    expect(row.getAttribute('role')).toBe('button')
    expect(row.getAttribute('aria-expanded')).toBe('false')

    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await nextTick()
    expect(row.getAttribute('aria-expanded')).toBe('true')
    expect(row.lastElementChild!.className).toContain('whitespace-pre-wrap')
    unmount()
  })

  it('shows the line count without gap markers', async () => {
    const { host, unmount } = await mountPage([
      { type: 'info', payload: 'first', time: '10:00:00', seq: 1 },
      { type: 'marker', payload: 'gap', time: '10:00:01', seq: 2 },
      { type: 'warn', payload: 'second', time: '10:00:02', seq: 3 },
    ])
    expect(host.querySelector('h1')!.textContent).toContain('2')
    expect(host.querySelector('h1')!.textContent).not.toContain('3')
    unmount()
  })
})
