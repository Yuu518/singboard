import ReconnectingWebSocket from 'reconnecting-websocket'
import { useConfigStore } from '@/stores/config'

function debounce<T extends (...args: any[]) => void>(fn: T, delay: number): T {
  let timer: ReturnType<typeof setTimeout> | null = null
  return ((...args: any[]) => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => fn(...args), delay)
  }) as unknown as T
}

export function createClashWS(
  path: string,
  onMessage: (data: any) => void,
  params?: Record<string, string>,
  options: { latestOnly?: boolean } = {},
): ReconnectingWebSocket {
  const { clashApiUrl, clashApiSecret } = useConfigStore()

  const base = clashApiUrl.value.replace(/^http/, 'ws')
  const url = new URL(path, base)

  if (clashApiSecret.value) {
    url.searchParams.set('token', clashApiSecret.value)
  }
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      url.searchParams.set(k, v)
    }
  }

  const ws = new ReconnectingWebSocket(url.toString(), [], {
    maxReconnectionDelay: 10000,
    minReconnectionDelay: 1000,
    reconnectionDelayGrowFactor: 1.5,
    maxRetries: Infinity,
  })

  const handler = (event: MessageEvent) => {
    try {
      const data = JSON.parse(event.data)
      onMessage(data)
    } catch {}
  }

  ws.onmessage = options.latestOnly ? debounce(handler, 100) : handler

  return ws
}
