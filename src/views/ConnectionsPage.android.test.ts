import { describe, expect, it, vi } from 'vitest'
import { computed, createApp, nextTick, ref } from 'vue'
import { PAGE_ACTIVE } from '@/android/composables/usePageActive'
import type { Connection } from '@/types'

const store = vi.hoisted(() => ({ value: null as any }))

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/components/common/ServiceNotice.vue', async () => {
  const { h } = await import('vue')
  return { default: { render: () => h('div', { 'data-service-notice': '' }) } }
})
vi.mock('@/api/geoip', () => ({ getGeoIPForIP: vi.fn(() => new Promise(() => {})) }))
vi.mock('@/composables/useSettledServiceState', () => ({
  useSettledServiceState: () => ({ serviceState: ref('stopped'), isRunning: ref(false) }),
}))
vi.mock('@/stores/config', () => ({ useConfigStore: () => ({ clashApiUrl: ref('http://127.0.0.1:9090') }) }))
vi.mock('@/stores/connections', () => ({
  matchesFilter: (c: Connection, q: string) => c.metadata.host.includes(q),
  useConnectionsStore: () => store.value,
}))

function conn(id: string, host: string, start: string): Connection {
  return {
    id,
    metadata: {
      network: 'tcp', type: 'tun', sourceIP: '172.19.0.1', destinationIP: '', sourcePort: '50000',
      destinationPort: '443', host, dnsMode: 'normal', process: 'com.android.chrome', processPath: '', sniffHost: '',
    },
    upload: 1024, download: 4096, start, chains: ['HK 01', 'Proxy'], rule: 'rule_set=geolocation-!cn',
    rulePayload: '', downloadSpeed: 2048, uploadSpeed: 0,
  }
}

function makeStore(list: Connection[]) {
  return {
    connections: ref(list),
    closedConnections: ref<Connection[]>([]),
    downloadTotal: ref(4096),
    uploadTotal: ref(1024),
    filterText: ref(''),
    socketState: ref('open'),
    socketFailures: ref(0),
    start: vi.fn(),
    closeConnection: vi.fn(() => Promise.resolve()),
    closeConnections: vi.fn(() => Promise.resolve([])),
    closeAllConnections: vi.fn(() => Promise.resolve()),
  }
}

async function mountPage(list: Connection[], active = true) {
  store.value = makeStore(list)
  const pageActive = ref(active)
  const { default: ConnectionsPage } = await import('./ConnectionsPage.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(ConnectionsPage)
  app.provide(PAGE_ACTIVE, computed(() => pageActive.value))
  app.mount(host)
  await nextTick()
  return { host, pageActive, unmount: () => { app.unmount(); host.remove() } }
}

describe('ConnectionsPage on Android', () => {
  it('shows each connection as a glass card in a list that scrolls under the bottom navigation', async () => {
    const { host, unmount } = await mountPage([
      conn('a', 'www.google.com', '2026-11-09T10:00:00Z'),
      conn('b', 'github.com', '2026-11-09T10:00:05Z'),
    ])
    expect(host.querySelector('[data-service-notice]')).toBeNull()
    expect(host.querySelector('.conn-list')!.classList).toContain('under-nav')
    expect(host.querySelector('#connections-panel')!.classList).not.toContain('surface-card')

    const cards = host.querySelectorAll<HTMLElement>('.conn-card')
    expect(cards).toHaveLength(2)
    expect(cards[0].classList).toContain('surface-card')
    expect(cards[0].textContent).toContain('github.com:443')
    expect(cards[0].textContent).toContain('com.android.chrome')
    expect(cards[0].textContent).toContain('Proxy → HK 01')
    expect(cards[0].querySelector('[data-conn-row]')).not.toBeNull()
    unmount()
  })

  it('disconnects from the card button and marks the card as closing', async () => {
    const { host, unmount } = await mountPage([conn('a', 'www.google.com', '2026-11-09T10:00:00Z')])
    const close = host.querySelector<HTMLButtonElement>('.conn-close')!
    expect(close.getAttribute('aria-label')).toBe('断开 www.google.com')
    close.click()
    await nextTick()
    expect(store.value.closeConnection).toHaveBeenCalledWith('a')
    unmount()
  })

  it('keeps the list still while the page is off screen', async () => {
    const { host, pageActive, unmount } = await mountPage([conn('a', 'www.google.com', '2026-11-09T10:00:00Z')], false)
    store.value.connections.value = [...store.value.connections.value, conn('b', 'github.com', '2026-11-09T10:00:05Z')]
    await nextTick()
    expect(host.querySelectorAll('.conn-card')).toHaveLength(1)

    pageActive.value = true
    await nextTick()
    expect(host.querySelectorAll('.conn-card')).toHaveLength(2)
    unmount()
  })

  it('holds the order while a finger is on the list', async () => {
    vi.useFakeTimers()
    const { host, unmount } = await mountPage([conn('a', 'www.google.com', '2026-11-09T10:00:00Z')])
    const list = host.querySelector<HTMLElement>('.conn-list')!
    list.dispatchEvent(new Event('touchstart'))
    await nextTick()
    store.value.connections.value = [...store.value.connections.value, conn('b', 'github.com', '2026-11-09T10:00:05Z')]
    await nextTick()
    const hosts = () => [...host.querySelectorAll('.conn-host')].map((el) => el.textContent)
    expect(hosts()).toEqual(['www.google.com:443', 'github.com:443'])

    list.dispatchEvent(new Event('touchend'))
    vi.advanceTimersByTime(1500)
    await nextTick()
    expect(hosts()).toEqual(['github.com:443', 'www.google.com:443'])
    vi.useRealTimers()
    unmount()
  })
})
