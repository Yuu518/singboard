import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, defineComponent, nextTick, ref } from 'vue'

type RawLog = { type: string; payload: string }

const socket = vi.hoisted(() => ({
  onMessage: null as ((data: RawLog) => void) | null,
  params: undefined as Record<string, string> | undefined,
  listeners: {} as Record<string, () => void>,
  created: 0,
  close: vi.fn(),
}))

vi.mock('@/api/websocket', () => ({
  createClashWS: (_path: string, onMessage: (data: RawLog) => void, params?: Record<string, string>) => {
    socket.onMessage = onMessage
    socket.params = params
    socket.created++
    return {
      close: socket.close,
      addEventListener: (type: string, listener: () => void) => {
        socket.listeners[type] = listener
      },
    }
  },
}))

vi.mock('@/api', () => ({
  fetchConfig: vi.fn().mockResolvedValue({ data: { 'log-level': 'info' } }),
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

vi.mock('./service', async () => {
  const { ref } = await import('vue')
  return { serviceStatus: ref({ state: 'running' }) }
})

vi.mock('./appVisible', () => ({ appVisible: ref(true) }))

async function startLogs() {
  const { useLogsLifecycle, useLogsStore } = await import('./logs')
  const { appVisible } = await import('./appVisible')
  const host = document.createElement('div')
  const app = createApp(defineComponent({
    setup() {
      useLogsLifecycle().start()
      return () => null
    },
  }))
  app.mount(host)
  await vi.waitFor(() => expect(socket.onMessage).not.toBeNull())
  return { store: useLogsStore(), appVisible, unmount: () => app.unmount() }
}

function emit(count: number, offset = 0, type = 'info') {
  for (let i = 0; i < count; i++) socket.onMessage?.({ type, payload: `line ${offset + i}` })
}

describe('logs store', () => {
  beforeEach(() => {
    vi.resetModules()
    socket.onMessage = null
    socket.params = undefined
    socket.listeners = {}
    socket.created = 0
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('subscribes at the level configured in sing-box', async () => {
    const { store, unmount } = await startLogs()
    expect(socket.params).toEqual({ level: 'info' })
    expect(store.coreLevel.value).toBe('info')
    unmount()
  })

  it('follows the core level by default and resubscribes at any chosen level', async () => {
    const { store, unmount } = await startLogs()
    expect(store.levelFilter.value).toBe('info')

    store.levelChoice.value = 'debug'
    expect(store.levelFilter.value).toBe('debug')
    expect(socket.params).toEqual({ level: 'debug' })
    expect(socket.created).toBe(2)

    store.levelChoice.value = 'info'
    expect(socket.params).toEqual({ level: 'info' })
    expect(socket.created).toBe(3)

    store.levelChoice.value = ''
    expect(store.levelFilter.value).toBe('info')
    expect(socket.created).toBe(3)

    store.levelChoice.value = 'panic'
    expect(socket.params).toEqual({ level: 'panic' })
    unmount()
  })

  it('resubscribes when the core comes back with another level', async () => {
    const { store, unmount } = await startLogs()
    const { fetchConfig } = await import('@/api')
    vi.mocked(fetchConfig).mockResolvedValue({ data: { 'log-level': 'debug' } } as never)

    socket.listeners.open()
    socket.listeners.close()
    socket.onMessage = null
    socket.listeners.open()

    await vi.waitFor(() => expect(socket.params).toEqual({ level: 'debug' }))
    expect(store.coreLevel.value).toBe('debug')
    expect(socket.created).toBe(2)
    vi.mocked(fetchConfig).mockResolvedValue({ data: { 'log-level': 'info' } } as never)
    unmount()
  })

  it('maps core levels onto the filter choices', async () => {
    const { filterLevelFor } = await import('./logs')
    expect(filterLevelFor('warning')).toBe('warn')
    expect(filterLevelFor('panic')).toBe('panic')
    expect(filterLevelFor('fatal')).toBe('fatal')
    expect(filterLevelFor('DEBUG')).toBe('debug')
    expect(filterLevelFor('silent')).toBe('info')
  })

  it('keeps every line of a burst and writes them in one batch', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()

    emit(300)
    expect(store.logs.value).toHaveLength(0)

    vi.advanceTimersByTime(100)
    expect(store.logs.value).toHaveLength(300)
    expect(store.logs.value.map((l) => l.seq)).toEqual(Array.from({ length: 300 }, (_, i) => i + 1))
    unmount()
  })

  it('trims to the newest 5000 lines', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()

    emit(3000)
    vi.advanceTimersByTime(100)
    emit(3000, 3000)
    vi.advanceTimersByTime(100)

    expect(store.logs.value).toHaveLength(5000)
    expect(store.logs.value[0].payload).toBe('line 1000')
    expect(store.logs.value[4999].payload).toBe('line 5999')
    unmount()
  })

  it('keeps earlier lines and marks the switch when the level changes', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()

    emit(2, 0, 'info')
    emit(2, 2, 'warn')
    emit(1, 4, 'error')
    vi.advanceTimersByTime(100)

    store.levelChoice.value = 'warn'
    const shown = store.filteredLogs.value
    expect(shown.slice(0, 3).map((l) => l.payload)).toEqual(['line 2', 'line 3', 'line 4'])
    expect(shown[3].type).toBe('marker')
    expect(shown[3].payload).toContain('日志级别切换为 warn')

    store.levelChoice.value = 'info'
    expect(store.logs.value.filter((l) => l.type !== 'marker')).toHaveLength(5)
    expect(store.filteredLogs.value).toHaveLength(7)
    unmount()
  })

  it('keeps receiving while paused and shows the frozen view', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()

    emit(3)
    vi.advanceTimersByTime(100)
    store.setPaused(true)
    emit(4, 3)
    vi.advanceTimersByTime(100)

    expect(store.filteredLogs.value).toHaveLength(3)
    expect(store.newSincePause.value).toBe(4)

    store.setPaused(false)
    expect(store.filteredLogs.value).toHaveLength(7)
    expect(store.newSincePause.value).toBe(0)
    unmount()
  })

  it('keeps logs while hidden and marks the gap when the window returns', async () => {
    const { store, appVisible, unmount } = await startLogs()
    vi.useFakeTimers()
    socket.listeners.open()

    emit(2)
    vi.advanceTimersByTime(100)
    appVisible.value = false
    await nextTick()
    expect(store.logs.value).toHaveLength(2)
    expect(store.socketState.value).toBe('idle')

    appVisible.value = true
    await nextTick()
    for (let i = 0; i < 5 && socket.created < 2; i++) await Promise.resolve()
    expect(socket.created).toBe(2)
    socket.listeners.open()
    vi.advanceTimersByTime(100)

    const last = store.logs.value[store.logs.value.length - 1]
    expect(store.logs.value).toHaveLength(3)
    expect(last.type).toBe('marker')
    expect(last.payload).toContain('窗口隐藏期间未接收日志')
    unmount()
  })

  it('marks the gap after the core link drops and recovers', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()
    socket.listeners.open()

    emit(1)
    vi.advanceTimersByTime(100)
    socket.listeners.close()
    socket.listeners.open()
    vi.advanceTimersByTime(100)

    expect(store.logs.value.map((l) => l.type)).toEqual(['info', 'marker'])
    expect(store.logs.value[1].payload).toContain('与核心的连接中断')
    unmount()
  })

  it('reports the socket state and counts failed attempts', async () => {
    const { store, unmount } = await startLogs()
    expect(store.socketState.value).toBe('connecting')

    socket.listeners.open()
    expect(store.socketState.value).toBe('open')

    socket.listeners.close()
    socket.listeners.close()
    socket.listeners.close()
    expect(store.socketState.value).toBe('reconnecting')
    expect(store.socketFailures.value).toBe(3)

    socket.listeners.open()
    expect(store.socketState.value).toBe('open')
    expect(store.socketFailures.value).toBe(0)
    unmount()
    expect(store.socketState.value).toBe('idle')
  })

  it('ignores surrounding spaces in the search text', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()

    emit(3)
    vi.advanceTimersByTime(100)
    store.filterText.value = '  line 1 '
    expect(store.filteredLogs.value.map((l) => l.payload)).toEqual(['line 1'])
    store.filterText.value = '   '
    expect(store.filteredLogs.value).toHaveLength(3)
    unmount()
  })

  it('clears including queued lines and restores them in order', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()

    emit(5)
    vi.advanceTimersByTime(100)
    emit(2, 5)
    const removed = store.clear()
    expect(removed).toHaveLength(7)
    expect(store.logs.value).toHaveLength(0)

    emit(1, 7)
    vi.advanceTimersByTime(100)
    store.restore(removed)
    expect(store.logs.value.map((l) => l.payload)).toEqual(Array.from({ length: 8 }, (_, i) => `line ${i}`))
    unmount()
  })

  it('reconnects without clearing when the active API address changes in place', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()
    socket.listeners.open()
    emit(2)
    vi.advanceTimersByTime(100)

    apiConfig.value.clashApiUrl.value = 'http://127.0.0.1:9191'
    await nextTick()
    for (let i = 0; i < 5 && socket.created < 2; i++) await Promise.resolve()

    expect(socket.created).toBe(2)
    expect(store.logs.value.map((l) => l.type)).toEqual(['info', 'info', 'marker'])
    expect(store.logs.value[2].payload).toContain('Clash API 配置已更新')
    unmount()
  })

  it('marks core stop and start and holds the stream while the core is down', async () => {
    const { store, unmount } = await startLogs()
    const { serviceStatus } = await import('./service')
    vi.useFakeTimers()
    socket.listeners.open()
    emit(1)
    vi.advanceTimersByTime(100)

    serviceStatus.value = { state: 'stopped' }
    await nextTick()
    expect(store.socketState.value).toBe('idle')
    expect(store.socketFailures.value).toBe(0)

    serviceStatus.value = { state: 'starting' }
    await nextTick()
    expect(socket.created).toBe(1)

    serviceStatus.value = { state: 'running' }
    await nextTick()
    for (let i = 0; i < 5 && socket.created < 2; i++) await Promise.resolve()
    vi.advanceTimersByTime(100)

    expect(socket.created).toBe(2)
    expect(store.logs.value.map((l) => l.payload)).toEqual([
      'line 0',
      expect.stringContaining('核心已停止'),
      expect.stringContaining('核心已启动'),
    ])
    serviceStatus.value = { state: 'running' }
    unmount()
  })

  it('shows nothing when a filter leaves only gap markers', async () => {
    const { store, unmount } = await startLogs()
    vi.useFakeTimers()
    emit(2, 0, 'info')
    vi.advanceTimersByTime(100)

    store.levelChoice.value = 'error'
    vi.advanceTimersByTime(100)
    expect(store.logs.value.some((l) => l.type === 'marker')).toBe(true)
    expect(store.filteredLogs.value).toEqual([])
    unmount()
  })
})
