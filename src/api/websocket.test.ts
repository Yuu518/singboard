import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const sockets = vi.hoisted(() => [] as Array<{ url: string; onmessage: ((event: MessageEvent) => void) | null }>)

vi.mock('reconnecting-websocket', () => ({
  default: class {
    url: string
    onmessage: ((event: MessageEvent) => void) | null = null
    constructor(url: string) {
      this.url = url
      sockets.push(this)
    }
    close() {}
  },
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({
    clashApiUrl: ref('http://127.0.0.1:9090'),
    clashApiSecret: ref(''),
  }),
}))

import { createClashWS } from './websocket'

function send(payload: unknown) {
  sockets[0].onmessage?.({ data: JSON.stringify(payload) } as MessageEvent)
}

describe('createClashWS', () => {
  beforeEach(() => {
    sockets.length = 0
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('delivers every message of a burst by default', () => {
    const received: number[] = []
    createClashWS('/logs', (data: { n: number }) => received.push(data.n), { level: 'debug' })

    for (let i = 0; i < 50; i++) send({ n: i })

    expect(received).toEqual(Array.from({ length: 50 }, (_, i) => i))
    expect(sockets[0].url).toContain('level=debug')
  })

  it('keeps only the latest message of a burst when asked', () => {
    const received: number[] = []
    createClashWS('/traffic', (data: { n: number }) => received.push(data.n), undefined, { latestOnly: true })

    for (let i = 0; i < 50; i++) send({ n: i })
    expect(received).toEqual([])

    vi.advanceTimersByTime(100)
    expect(received).toEqual([49])
  })
})
