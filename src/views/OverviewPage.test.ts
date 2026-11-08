import { describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, ref } from 'vue'

const push = vi.hoisted(() => vi.fn())

vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))
vi.mock('@/components/overview/NetworkInfo.vue', async () => ({ default: (await import('vue')).defineComponent({ render: () => null }) }))
vi.mock('@/components/overview/TopologyChart.vue', async () => ({ default: (await import('vue')).defineComponent({ render: () => null }) }))
vi.mock('@/components/overview/SparkLine.vue', async () => ({ default: (await import('vue')).defineComponent({ render: () => null }) }))
vi.mock('@/components/overview/OverviewStatusBand.vue', async () => ({ default: (await import('vue')).defineComponent({ render: () => null }) }))
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref('running'), isRunning: ref(true) }),
}))
vi.mock('@/stores/appVisible', async () => ({ appVisible: (await import('vue')).ref(true) }))
vi.mock('@/stores/config', async () => ({ useConfigStore: () => ({}) }))
vi.mock('@/stores/overview', async () => {
  const actual = await vi.importActual<typeof import('@/stores/overview')>('@/stores/overview')
  const now = Date.now()
  return {
    overviewLinkState: actual.overviewLinkState,
    useOverviewStore: () => ({
      currentTraffic: ref({ up: 2048, down: 4096 }),
      memory: ref({ inuse: 1, oslimit: 0 }),
      uploadSpeedHistory: ref([{ name: now, value: 1024 }, { name: now + 1000, value: 3072 }]),
      downloadSpeedHistory: ref([{ name: now, value: 4096 }]),
      connectionsHistory: ref([{ name: now, value: 2 }]),
      socketState: ref('open'),
      socketFailures: ref(0),
      start: vi.fn(),
    }),
  }
})
vi.mock('@/stores/connections', () => ({
  useConnectionsStore: () => ({
    connections: ref([{ metadata: { network: 'tcp' } }, { metadata: { network: 'udp' } }]),
    downloadTotal: ref(0),
    uploadTotal: ref(0),
    socketState: ref('open'),
    socketFailures: ref(0),
    start: vi.fn(),
  }),
}))

describe('OverviewPage', () => {
  it('lets every metric card open the connections page with a spoken summary', async () => {
    const { default: OverviewPage } = await import('./OverviewPage.vue')
    const host = document.createElement('div')
    const app = createApp(OverviewPage)
    app.mount(host)
    await nextTick()

    const links = [...host.querySelectorAll<HTMLButtonElement>('.overview-metric-link')]
    expect(links).toHaveLength(3)
    expect(links[0].getAttribute('aria-label')).toBe('上传 2 KB/s，最近 2 秒峰值 3 KB/s，平均 2 KB/s。查看连接')
    expect(links[2].getAttribute('aria-label')).toContain('连接 2 条，TCP 1，UDP 1')
    expect(host.querySelector('.overview-metric-body')!.getAttribute('aria-hidden')).toBe('true')
    expect(host.textContent).not.toContain('内存')
    expect(host.innerHTML).not.toContain('tracking-wider')

    links[1].click()
    expect(push).toHaveBeenCalledWith('/connections')
    app.unmount()
  })
})
