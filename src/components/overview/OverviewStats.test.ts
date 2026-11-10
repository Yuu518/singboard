import { describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick } from 'vue'

const push = vi.hoisted(() => vi.fn())

vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const now = Date.now()
const history = (values: number[]) => values.map((value, i) => ({ name: now + i * 1000, value }))

async function render(props: Record<string, unknown> = {}) {
  const { default: Stats } = await import('./OverviewStats.vue')
  const host = document.createElement('div')
  const merged: any = {
    connections: { count: 128, tcp: 112, udp: 16 },
    connectionsHistory: history([120, 128]),
    memory: 42 * 1024 * 1024,
    memoryHistory: history([40 * 1024 * 1024, 42 * 1024 * 1024]),
    residentMemory: null,
    cpu: null,
    cpuHistory: [],
    showCpu: false,
    showResident: false,
    ...props,
  }
  const app = createApp({ render: () => h(Stats, merged) })
  app.mount(host)
  await nextTick()
  return { host, unmount: () => app.unmount() }
}

const cell = (host: HTMLElement, name: string) => host.querySelector(`[data-stat="${name}"]`) as HTMLElement
const parts = (el: Element) => [...el.querySelectorAll('.overview-stat-sub > span')].map((span) => span.textContent?.trim())

describe('OverviewStats', () => {
  it('splits secondary details into separate lines so narrow phones never cut them', async () => {
    const { host, unmount } = await render({
      showCpu: true,
      showResident: true,
      residentMemory: 61.8 * 1024 * 1024,
      cpu: 3.24,
      cpuHistory: history([2, 3]),
    })
    expect(parts(cell(host, 'connections'))).toEqual(['TCP 112', 'UDP 16'])
    expect(parts(cell(host, 'memory'))).toEqual(['进程常驻', '61.8 MB'])
    expect(cell(host, 'cpu').querySelector('.overview-stat-value')!.textContent?.replace(/\s+/g, '')).toBe('3.2%')
    expect(host.querySelector('.overview-stats')!.className).toContain('has-cpu')
    unmount()
  })

  it('leaves out CPU where the platform cannot measure it', async () => {
    const { host, unmount } = await render()
    expect(cell(host, 'cpu')).toBeNull()
    expect(parts(cell(host, 'memory'))).toEqual(['运行时占用'])
    unmount()
  })

  it('opens connections with a spoken summary', async () => {
    const { host, unmount } = await render()
    const button = cell(host, 'connections').querySelector('button')!
    expect(button.getAttribute('aria-label')).toBe('连接 128 条，TCP 112，UDP 16，最近 2 秒峰值 128 条，平均 124 条。查看连接')
    expect(cell(host, 'connections').querySelector('.overview-stat-body')!.getAttribute('aria-hidden')).toBe('true')
    button.click()
    expect(push).toHaveBeenCalledWith('/connections')
    unmount()
  })

  it('shows a dash and no trend when the numbers are not live', async () => {
    const { host, unmount } = await render({ connections: null, memory: null })
    expect(cell(host, 'connections').querySelector('.overview-stat-value')!.textContent?.trim()).toBe('—')
    expect(cell(host, 'memory').querySelector('.overview-stat-value')!.textContent?.trim()).toBe('—')
    expect(host.querySelector('[data-mini-line] svg')).toBeNull()
    expect(parts(cell(host, 'memory'))).toEqual([])
    unmount()
  })
})
