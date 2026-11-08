export type EndpointTone = 'ok' | 'pending' | 'error' | 'idle'

export type EndpointProbe =
  | { state: 'idle' }
  | { state: 'probing' }
  | { state: 'ok' }
  | { state: 'error'; reason: string }

export type EndpointSocketState = 'idle' | 'connecting' | 'reconnecting' | 'open' | 'failed'

export interface EndpointStatusInput {
  probe: EndpointProbe
  socketState: EndpointSocketState
  socketFailures: number
  coreRunning: boolean
  address: string
}

export interface EndpointStatus {
  tone: EndpointTone
  text: string
}

export function describeEndpointStatus(input: EndpointStatusInput): EndpointStatus {
  const { probe, socketState, socketFailures, coreRunning, address } = input
  if (probe.state === 'probing') return { tone: 'pending', text: `正在连接 ${address}…` }
  if (socketState === 'open' || probe.state === 'ok') return { tone: 'ok', text: `已连接 ${address}` }
  if (probe.state === 'error') return { tone: 'error', text: `连接失败：${probe.reason}` }
  if (!coreRunning) return { tone: 'idle', text: `核心未运行，启动后会连接 ${address}` }
  if (socketState === 'failed' || socketFailures >= 3) return { tone: 'error', text: `无法连接 ${address}，请检查地址和密钥` }
  if (socketState === 'reconnecting') return { tone: 'pending', text: `连接中断，正在重连 ${address}…` }
  if (socketState === 'connecting') return { tone: 'pending', text: `正在连接 ${address}…` }
  return { tone: 'idle', text: address }
}
