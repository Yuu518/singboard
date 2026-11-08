import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, nextTick, ref } from 'vue'

type Listener = () => void

interface FakeSocket {
  path: string
  onMessage: (data: any) => void
  listeners: Record<string, Listener>
  close: ReturnType<typeof vi.fn>
}

const sockets = vi.hoisted(() => ({ list: [] as FakeSocket[], throwOnCreate: false }))

vi.mock('@/api/websocket', () => ({
  createClashWS: (path: string, onMessage: (data: any) => void) => {
    if (sockets.throwOnCreate) throw new TypeError('Invalid URL')
    const socket: FakeSocket = {
      path,
      onMessage,
      listeners: {},
      close: vi.fn(),
    }
    sockets.list.push(socket)
    return {
      close: socket.close,
      addEventListener: (type: string, listener: Listener) => {
        socket.listeners[type] = listener
      },
    }
  },
}))

const apiConfig = vi.hoisted(() => ({ value: null as any }))

vi.mock('./config', async () => {
  const { ref } = await import('vue')
  apiConfig.value = {
    activeClashApiId: ref('default'),
    clashApiUrl: ref('http://127.0.0.1:9090'),
    clashApiSecret: ref(''),
  }
  return { useConfigStore: () => apiConfig.value }
})

vi.mock('./appVisible', () => ({ appVisible: ref(true) }))

const mounted: Array<() => void> = []

function latest(path: string): FakeSocket {
  const found = sockets.list.filter((socket) => socket.path === path).at(-1)
  if (!found) throw new Error(`no socket for ${path}`)
  return found
}

async function mountStore() {
  const { useOverviewStore } = await import('./overview')
  let store!: ReturnType<typeof useOverviewStore>
  const app = createApp(defineComponent({
    setup() {
      store = useOverviewStore()
      return () => null
    },
  }))
  app.mount(document.createElement('div'))
  mounted.push(() => app.unmount())
  return { store }
}

describe('overview store', () => {
  beforeEach(() => {
    vi.resetModules()
    sockets.list = []
    sockets.throwOnCreate = false
  })

  afterEach(() => {
    mounted.splice(0).forEach((unmount) => unmount())
  })

  it('shows no traffic or memory until the core reports them', async () => {
    const { store } = await mountStore()
    store.start()
    expect(store.socketState.value).toBe('connecting')
    expect(store.currentTraffic.value).toBeNull()
    expect(store.memory.value).toBeNull()

    latest('/traffic').onMessage({ up: 10, down: 20 })
    expect(store.socketState.value).toBe('open')
    expect(store.currentTraffic.value).toEqual({ up: 10, down: 20 })
    expect(store.uploadSpeedHistory.value.at(-1)?.value).toBe(10)
    expect(store.downloadSpeedHistory.value.at(-1)?.value).toBe(20)
  })

  it('skips only the placeholder zero that opens each memory stream', async () => {
    const { store } = await mountStore()
    store.start()
    const memory = latest('/memory')

    memory.onMessage({ inuse: 0, oslimit: 0 })
    expect(store.memory.value).toBeNull()

    memory.onMessage({ inuse: 2048, oslimit: 0 })
    expect(store.memory.value).toEqual({ inuse: 2048, oslimit: 0 })

    memory.onMessage({ inuse: 0, oslimit: 0 })
    expect(store.memory.value).toEqual({ inuse: 0, oslimit: 0 })
    expect(store.memoryHistory.value.at(-1)?.value).toBe(0)

    memory.listeners.open()
    memory.onMessage({ inuse: 0, oslimit: 0 })
    expect(store.memoryHistory.value.at(-1)?.value).toBe(0)
    memory.onMessage({ inuse: 4096, oslimit: 0 })
    expect(store.memory.value?.inuse).toBe(4096)
  })

  it('drops live numbers and counts failures when the link closes', async () => {
    const { store } = await mountStore()
    store.start()
    latest('/traffic').onMessage({ up: 1, down: 2 })
    latest('/memory').onMessage({ inuse: 0 })
    latest('/memory').onMessage({ inuse: 99 })

    latest('/traffic').listeners.close()
    latest('/traffic').listeners.close()
    expect(store.socketState.value).toBe('reconnecting')
    expect(store.socketFailures.value).toBe(2)
    expect(store.currentTraffic.value).toBeNull()
    expect(store.memory.value).toBeNull()

    latest('/traffic').listeners.open()
    expect(store.socketState.value).toBe('open')
    expect(store.socketFailures.value).toBe(0)
  })

  it('clears numbers and history when it stops or resets', async () => {
    const { store } = await mountStore()
    store.start()
    latest('/traffic').onMessage({ up: 5, down: 6 })
    store.stop()
    expect(store.currentTraffic.value).toBeNull()
    expect(store.socketState.value).toBe('idle')

    store.start()
    latest('/traffic').onMessage({ up: 5, down: 6 })
    store.reset()
    expect(store.currentTraffic.value).toBeNull()
    expect(store.uploadSpeedHistory.value.every((point) => point.value === null)).toBe(true)
  })

  it('reconnects and forgets the old endpoint when the API changes', async () => {
    const { store } = await mountStore()
    store.start()
    const first = latest('/traffic')
    first.onMessage({ up: 7, down: 8 })

    apiConfig.value.clashApiUrl.value = 'http://127.0.0.1:9191'
    await nextTick()

    expect(first.close).toHaveBeenCalled()
    expect(latest('/traffic')).not.toBe(first)
    expect(store.currentTraffic.value).toBeNull()
    expect(store.uploadSpeedHistory.value.every((point) => point.value === null)).toBe(true)
    first.onMessage({ up: 70, down: 80 })
    expect(store.currentTraffic.value).toBeNull()
  })

  it('closes the streams while the core is down and starts fresh when it returns', async () => {
    const { store } = await mountStore()
    const { suspendOverview, resumeOverview } = await import('./overview')
    store.start()
    const first = latest('/traffic')
    first.listeners.close()
    first.listeners.close()
    expect(store.socketFailures.value).toBe(2)

    suspendOverview()
    expect(first.close).toHaveBeenCalled()
    expect(store.socketFailures.value).toBe(0)
    expect(store.socketState.value).toBe('idle')
    const count = sockets.list.length
    store.start()
    expect(sockets.list.length).toBe(count)

    resumeOverview()
    expect(sockets.list.length).toBe(count + 2)
    expect(store.socketState.value).toBe('connecting')
    expect(store.socketFailures.value).toBe(0)
  })

  it('does not open streams on resume when no page uses them', async () => {
    const { suspendOverview, resumeOverview } = await import('./overview')
    suspendOverview()
    resumeOverview()
    expect(sockets.list).toHaveLength(0)
  })

  it('starts a fresh history after the window was hidden', async () => {
    const { store } = await mountStore()
    const { appVisible } = await import('./appVisible')
    store.start()
    latest('/traffic').onMessage({ up: 9, down: 9 })
    appVisible.value = false
    await nextTick()
    expect(store.uploadSpeedHistory.value.every((point) => point.value === null)).toBe(true)
    appVisible.value = true
    await nextTick()
    expect(store.socketState.value).toBe('connecting')
  })

  it('handles an API change once even with several store users', async () => {
    const a = await mountStore()
    await mountStore()
    a.store.start()
    const before = sockets.list.length
    apiConfig.value.clashApiSecret.value = 'token'
    await nextTick()
    expect(sockets.list.length - before).toBe(2)
  })

  it('reports an invalid address instead of throwing', async () => {
    sockets.throwOnCreate = true
    const { store } = await mountStore()
    expect(() => store.start()).not.toThrow()
    expect(store.socketState.value).toBe('failed')
  })
})

describe('overviewLinkState', () => {
  it('maps service and socket health to what the page should say', async () => {
    const { overviewLinkState } = await import('./overview')
    const open = { state: 'open', failures: 0 }
    expect(overviewLinkState(false, open, open)).toBe('stopped')
    expect(overviewLinkState(true, open, open)).toBe('live')
    expect(overviewLinkState(true, { state: 'failed', failures: 0 }, open)).toBe('invalid')
    expect(overviewLinkState(true, { state: 'connecting', failures: 0 }, open)).toBe('connecting')
    expect(overviewLinkState(true, { state: 'reconnecting', failures: 2 }, open)).toBe('connecting')
    expect(overviewLinkState(true, open, { state: 'reconnecting', failures: 3 })).toBe('unreachable')
    expect(overviewLinkState(true, open, { state: 'failed', failures: 0 })).toBe('invalid')
  })
})
