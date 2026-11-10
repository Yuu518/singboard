import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick, ref } from 'vue'

const chart = vi.hoisted(() => ({
  options: [] as any[],
  handlers: {} as Record<string, () => void>,
  cleared: 0,
}))

vi.mock('echarts/core', () => ({
  use: vi.fn(),
  init: () => ({
    setOption: (option: any) => chart.options.push(option),
    clear: () => { chart.cleared++ },
    on: (event: string, handler: () => void) => { chart.handlers[event] = handler },
    resize: vi.fn(),
    dispose: vi.fn(),
  }),
}))
vi.mock('echarts/charts', () => ({ SankeyChart: {} }))
vi.mock('echarts/components', () => ({ TooltipComponent: {} }))
vi.mock('echarts/renderers', () => ({ CanvasRenderer: {} }))

const connections = vi.hoisted(() => ({ value: null as any }))
vi.mock('@/stores/connections', () => ({ useConnectionsStore: () => ({ connections: connections.value, start: vi.fn() }) }))
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({ resolvedTheme: ref('light') }) }))

const motion = vi.hoisted(() => ({ reduced: false }))

class FakeResizeObserver {
  observe() {}
  disconnect() {}
}

function conn(id: string, exit: string) {
  return { id, metadata: { sourceIP: '10.0.0.2' }, rule: 'MATCH', rulePayload: '', chains: [exit] }
}

const live = ref(true)

async function mount() {
  const { default: TopologyChart } = await import('./TopologyChart.vue')
  const host = document.createElement('div')
  const app = createApp({ render: () => h(TopologyChart, { live: live.value, waiting: '与核心的连接已中断，恢复后显示连接拓扑' }) })
  app.mount(host)
  await nextTick()
  await nextTick()
  return { host, unmount: () => app.unmount() }
}

const flush = async () => {
  await nextTick()
  await nextTick()
}

const lastSeries = () => chart.options.at(-1)?.series[0]

describe('TopologyChart', () => {
  beforeEach(() => {
    vi.resetModules()
    chart.options = []
    chart.handlers = {}
    chart.cleared = 0
    connections.value = ref([conn('1', 'DIRECT')])
    motion.reduced = false
    live.value = true
    vi.stubGlobal('ResizeObserver', FakeResizeObserver)
    vi.spyOn(window, 'matchMedia').mockImplementation((query: string) => ({
      matches: query.includes('reduced-motion') && motion.reduced,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }) as unknown as MediaQueryList)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('updates with a short transition instead of replaying a staggered entrance', async () => {
    const { unmount } = await mount()
    const series = lastSeries()
    expect(series.animationDurationUpdate).toBeLessThanOrEqual(300)
    expect(series.animationDuration).toBeLessThanOrEqual(300)
    expect(series.animationDelay).toBeUndefined()
    expect(series.nodeGap).toBeGreaterThanOrEqual(8)
    expect(series.data.every((node: any) => typeof node.depth === 'number' && !('layer' in node))).toBe(true)
    unmount()
  })

  it('turns animation off under reduced motion', async () => {
    motion.reduced = true
    const { unmount } = await mount()
    expect(chart.options.at(-1).animation).toBe(false)
    expect(lastSeries().animation).toBe(false)
    expect(lastSeries().animationDurationUpdate).toBe(0)
    unmount()
  })

  it('labels nodes with their names, never the internal column ids', async () => {
    const { unmount } = await mount()
    const series = lastSeries()
    const node = series.data.find((item: any) => item.name === '10.0.0.2')
    expect(node.id).not.toBe(node.name)
    expect(series.label.formatter({ name: node.id, data: node })).toBe('10.0.0.2')
    unmount()
  })

  it('colours each column from the theme palette and shades links between them', async () => {
    const { unmount } = await mount()
    const series = lastSeries()
    const colorOf = (depth: number) => series.data.find((node: any) => node.depth === depth).itemStyle.color
    expect(colorOf(0)).toMatch(/^rgba\(/)
    expect(colorOf(0)).not.toBe(colorOf(3))
    expect(series.lineStyle.color).toBe('gradient')
    expect(JSON.stringify(series)).not.toMatch(/#0A84FF|#30D158|#FF9F0A|#FF6482/i)
    expect(chart.options.at(-1).tooltip.extraCssText).toContain('var(--glass-rgb)')
    unmount()
  })

  it('keeps the user pause when a hover ends', async () => {
    const { host, unmount } = await mount()
    const pause = host.querySelector<HTMLButtonElement>('[data-action="toggle-pause"]')!
    pause.click()
    await flush()
    expect(host.textContent).toContain('已暂停更新')

    const before = chart.options.length
    chart.handlers.showTip()
    chart.handlers.hideTip()
    connections.value.value = [conn('1', 'DIRECT'), conn('2', 'Proxy')]
    await flush()
    expect(chart.options.length).toBe(before)
    expect(host.textContent).toContain('已暂停更新')

    pause.click()
    await flush()
    expect(chart.options.length).toBe(before + 1)
    expect(lastSeries().data.some((node: any) => node.name === 'Proxy')).toBe(true)
    unmount()
  })

  it('holds updates while hovering and applies the latest when the pointer leaves', async () => {
    const { host, unmount } = await mount()
    const before = chart.options.length
    chart.handlers.showTip()
    connections.value.value = [conn('1', 'DIRECT'), conn('2', 'Proxy')]
    await flush()
    expect(chart.options.length).toBe(before)
    expect(host.textContent).not.toContain('已暂停更新')

    chart.handlers.hideTip()
    await flush()
    expect(lastSeries().data.some((node: any) => node.name === 'Proxy')).toBe(true)
    unmount()
  })

  it('names the pause control and reports its state', async () => {
    const { host, unmount } = await mount()
    const pause = host.querySelector<HTMLButtonElement>('[data-action="toggle-pause"]')!
    expect(pause.getAttribute('aria-label')).toBe('暂停更新连接拓扑')
    expect(pause.getAttribute('aria-pressed')).toBe('false')
    pause.click()
    await flush()
    expect(pause.getAttribute('aria-pressed')).toBe('true')
    unmount()
  })

  it('describes the canvas and offers the same data as a list', async () => {
    connections.value.value = [conn('1', 'DIRECT'), conn('2', 'HK 01'), conn('3', 'HK 01')]
    const { host, unmount } = await mount()
    const canvas = host.querySelector('[role="img"]')!
    expect(canvas.getAttribute('aria-label')).toContain('3 条连接经 2 个出口，最多的是 HK 01（2 条）')

    const options = [...host.querySelectorAll<HTMLButtonElement>('[data-action="toggle-view"] [role="radio"]')]
    expect(options.map((option) => option.textContent?.trim())).toEqual(['图', '列表'])
    expect(options[0].getAttribute('aria-checked')).toBe('true')
    options[1].click()
    await flush()
    const list = host.querySelector('[data-topology-list]')!
    const rows = [...list.querySelectorAll('section')[0].querySelectorAll('li')].map((li) => [...li.querySelectorAll('span')].map((span) => span.textContent?.trim()).join(' '))
    expect(rows).toEqual(['HK 01 2 条', 'DIRECT 1 条'])
    expect(list.querySelector<HTMLElement>('.topology-bar > i')!.style.width).toBe('67%')
    expect(host.querySelector('[data-topology-summary]')!.textContent).toContain('3 条连接经 2 个出口')
    expect(list.textContent).toContain('按规则')
    unmount()
  })

  it('drops stale connections and waits while the link is down', async () => {
    const { host, unmount } = await mount()
    expect(lastSeries().data.length).toBeGreaterThan(0)

    live.value = false
    await flush()
    const waiting = host.querySelector('[data-topology-waiting]')!
    expect(waiting.getAttribute('role')).toBe('status')
    expect(waiting.textContent).toContain('恢复后显示连接拓扑')
    expect(host.querySelector('[role="img"]')!.parentElement!.style.display).toBe('none')
    expect(host.querySelector('[data-action="toggle-view"]')).toBeNull()
    expect(chart.cleared).toBeGreaterThan(0)

    connections.value.value = [conn('9', 'Stale')]
    await flush()
    expect(chart.options.at(-1).series[0].data.some((node: any) => node.name === 'Stale')).toBe(false)

    live.value = true
    await flush()
    expect(host.querySelector('[data-topology-waiting]')).toBeNull()
    expect(lastSeries().data.some((node: any) => node.name === 'Stale')).toBe(true)
    unmount()
  })

  it('ignores a user pause while the link is down so old data never lingers', async () => {
    const { host, unmount } = await mount()
    host.querySelector<HTMLButtonElement>('[data-action="toggle-pause"]')!.click()
    await flush()
    const cleared = chart.cleared
    live.value = false
    await flush()
    expect(chart.cleared).toBeGreaterThan(cleared)
    unmount()
  })
})
