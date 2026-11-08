import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, ref } from 'vue'

type Listener = () => void

interface FakeSocket {
  onMessage: (data: any) => void
  listeners: Record<string, Listener>
  close: ReturnType<typeof vi.fn>
}

const sockets = vi.hoisted(() => ({ list: [] as FakeSocket[], throwOnCreate: false }))

vi.mock('@/api/websocket', () => ({
  createClashWS: (_path: string, onMessage: (data: any) => void) => {
    if (sockets.throwOnCreate) throw new TypeError('Invalid URL')
    const socket: FakeSocket = { onMessage, listeners: {}, close: vi.fn() }
    sockets.list.push(socket)
    return {
      close: socket.close,
      addEventListener: (type: string, listener: Listener) => {
        socket.listeners[type] = listener
      },
    }
  },
}))

vi.mock('@/api', () => ({ disconnectAll: vi.fn(), disconnectById: vi.fn() }))

vi.mock('./config', async () => {
  const { ref } = await import('vue')
  const api = { activeClashApiId: ref('default'), clashApiUrl: ref('http://127.0.0.1:9090'), clashApiSecret: ref('') }
  return { useConfigStore: () => api }
})

vi.mock('./appVisible', () => ({ appVisible: ref(true) }))
vi.mock('./overview', () => ({ pushConnectionCount: vi.fn() }))

const mounted: Array<() => void> = []

async function mountStore() {
  const { useConnectionsStore } = await import('./connections')
  let store!: ReturnType<typeof useConnectionsStore>
  const app = createApp(defineComponent({
    setup() {
      store = useConnectionsStore()
      return () => null
    },
  }))
  app.mount(document.createElement('div'))
  mounted.push(() => app.unmount())
  return store
}

const snapshot = (ids: string[]) => ({
  downloadTotal: 10,
  uploadTotal: 5,
  connections: ids.map((id) => ({ id, download: 0, upload: 0, metadata: {}, chains: ['DIRECT'], rule: 'MATCH' })),
})

describe('connections store lifecycle', () => {
  beforeEach(() => {
    vi.resetModules()
    sockets.list = []
    sockets.throwOnCreate = false
  })

  afterEach(() => {
    mounted.splice(0).forEach((unmount) => unmount())
  })

  it('stops counting failures while the core is down and reconnects fresh when it returns', async () => {
    const store = await mountStore()
    const { suspendConnections, resumeConnections } = await import('./connections')
    store.start()
    sockets.list[0].listeners.close()
    sockets.list[0].listeners.close()
    expect(store.socketFailures.value).toBe(2)

    suspendConnections()
    expect(sockets.list[0].close).toHaveBeenCalled()
    expect(store.socketFailures.value).toBe(0)
    store.start()
    expect(sockets.list).toHaveLength(1)

    resumeConnections()
    expect(sockets.list).toHaveLength(2)
    expect(store.socketFailures.value).toBe(0)
  })

  it('ignores events from a socket that was already replaced', async () => {
    const store = await mountStore()
    const { suspendConnections, resumeConnections } = await import('./connections')
    store.start()
    const old = sockets.list[0]
    suspendConnections()
    resumeConnections()
    old.listeners.close()
    old.onMessage(snapshot(['stale']))
    expect(store.socketFailures.value).toBe(0)
    expect(store.connections.value).toEqual([])

    sockets.list[1].onMessage(snapshot(['a']))
    expect(store.connections.value.map((c) => c.id)).toEqual(['a'])
  })

  it('does not reconnect when no page is using connections', async () => {
    const { suspendConnections, resumeConnections } = await import('./connections')
    suspendConnections()
    resumeConnections()
    expect(sockets.list).toHaveLength(0)
  })

  it('clears data on a core restart without a store user', async () => {
    const store = await mountStore()
    store.start()
    sockets.list[0].onMessage(snapshot(['a']))
    const { resetConnectionsOnRestart } = await import('./connections')
    resetConnectionsOnRestart()
    expect(store.connections.value).toEqual([])
    expect(store.downloadTotal.value).toBe(0)
  })

  it('reports an invalid address instead of throwing', async () => {
    sockets.throwOnCreate = true
    const store = await mountStore()
    expect(() => store.start()).not.toThrow()
    expect(store.socketState.value).toBe('failed')
  })
})
