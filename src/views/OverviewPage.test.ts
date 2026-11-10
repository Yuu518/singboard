import { describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, ref } from 'vue'

const seen = vi.hoisted(() => ({ hero: null as any, stats: null as any, network: null as any, topology: null as any }))

function capture(key: 'hero' | 'stats' | 'network' | 'topology') {
  return async () => {
    const { defineComponent } = await import('vue')
    return {
      default: defineComponent({
        inheritAttrs: false,
        setup(_, { attrs }) {
          seen[key] = attrs
          return () => null
        },
      }),
    }
  }
}

vi.mock('@/components/overview/OverviewHero.vue', capture('hero'))
vi.mock('@/components/overview/OverviewStats.vue', capture('stats'))
vi.mock('@/components/overview/NetworkInfo.vue', capture('network'))
vi.mock('@/components/overview/TopologyChart.vue', capture('topology'))
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref('running'), isRunning: ref(true) }),
}))
vi.mock('@/stores/appVisible', async () => ({ appVisible: (await import('vue')).ref(true) }))
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({ config: ref({ rootMode: false }) }) }))
vi.mock('@/stores/service', () => ({
  cpuHistory: ref([]),
  useServiceStore: () => ({ serviceStatus: ref({ state: 'running', cpuPercent: 4, memoryBytes: 1024 }) }),
}))
vi.mock('@/stores/overview', async () => {
  const actual = await vi.importActual<typeof import('@/stores/overview')>('@/stores/overview')
  const now = Date.now()
  return {
    overviewLinkState: actual.overviewLinkState,
    useOverviewStore: () => ({
      currentTraffic: ref({ up: 2048, down: 4096 }),
      memory: ref({ inuse: 42 * 1024 * 1024, oslimit: 0 }),
      uploadSpeedHistory: ref([{ name: now, value: 1024 }]),
      downloadSpeedHistory: ref([{ name: now, value: 4096 }]),
      connectionsHistory: ref([{ name: now, value: 2 }]),
      memoryHistory: ref([{ name: now, value: 42 }]),
      socketState: ref('open'),
      socketFailures: ref(0),
      start: vi.fn(),
    }),
  }
})
vi.mock('@/stores/connections', () => ({
  useConnectionsStore: () => ({
    connections: ref([{ metadata: { network: 'tcp' } }, { metadata: { network: 'udp' } }]),
    downloadTotal: ref(9000),
    uploadTotal: ref(3000),
    socketState: ref('open'),
    socketFailures: ref(0),
    start: vi.fn(),
  }),
}))

describe('OverviewPage', () => {
  it('feeds one hero, one stats row, the network checks and the topology from live streams', async () => {
    const { default: OverviewPage } = await import('./OverviewPage.vue')
    const host = document.createElement('div')
    const app = createApp(OverviewPage)
    app.mount(host)
    await nextTick()

    expect(host.querySelector('h1')!.textContent).toBe('概览')
    expect(seen.hero.link).toBe('live')
    expect(seen.hero.traffic).toEqual({ up: 2048, down: 4096 })
    expect(seen.hero.totals).toEqual({ up: 3000, down: 9000 })
    expect(seen.stats.connections).toEqual({ count: 2, tcp: 1, udp: 1 })
    expect(seen.stats.memory).toBe(42 * 1024 * 1024)
    expect(seen.stats['show-cpu']).toBe(false)
    expect(seen.stats.cpu).toBeNull()
    expect(seen.stats['resident-memory']).toBeNull()
    expect(seen.network.running).toBe(true)
    expect(seen.topology.live).toBe(true)
    app.unmount()
  })
})
