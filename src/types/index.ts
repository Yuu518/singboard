
export interface ProxyExtra {
  history: LatencyHistory[]
  alive: boolean
}

export interface Proxy {
  name: string
  type: string
  all?: string[]
  now?: string
  history: LatencyHistory[]
  udp?: boolean
  xudp?: boolean
  extra?: Record<string, ProxyExtra>
  testUrl?: string
}

export interface LatencyHistory {
  time: string
  delay: number
}

export interface ProxyGroup {
  name: string
  type: string
  all: string[]
  now: string
  history: LatencyHistory[]
}

export interface ProxiesData {
  proxies: Record<string, Proxy>
}

export interface Rule {
  type: string
  payload: string
  proxy: string
  size?: number
}

export interface RulesData {
  rules: Rule[]
}

export interface Connection {
  id: string
  metadata: ConnectionMetadata
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
  downloadSpeed: number
  uploadSpeed: number
}

export interface ConnectionMetadata {
  network: string
  type: string
  sourceIP: string
  destinationIP: string
  sourcePort: string
  destinationPort: string
  host: string
  dnsMode: string
  process: string
  processPath: string
  sniffHost: string
}

export interface ConnectionsSnapshot {
  downloadTotal: number
  uploadTotal: number
  connections: Connection[]
  memory: number
}

export interface TrafficData {
  up: number
  down: number
}

export interface MemoryData {
  inuse: number
  oslimit: number
}

export interface LogEntry {
  type: string
  payload: string
  time?: string
}

export interface ClashConfig {
  port: number
  'socks-port': number
  'mixed-port': number
  'redir-port': number
  'tproxy-port': number
  mode: string
  'mode-list'?: string[]
  modes?: string[]
  'log-level': string
  'allow-lan': boolean
}

export interface ServiceStatus {
  state: 'running' | 'stopped' | 'starting' | 'stopping' | 'not_installed' | 'unknown'
  pid?: number
  uptimeSeconds?: number
}

export interface ProxyProvider {
  name: string
  proxies: Proxy[]
  testUrl: string
  updatedAt: string
  vehicleType: string
  subscriptionInfo?: {
    Download?: number
    Upload?: number
    Total?: number
    Expire?: number
  }
}

export interface RuleProvider {
  behavior: string
  format: string
  name: string
  ruleCount: number
  type: string
  updatedAt: string
  vehicleType: string
}

export interface RuleProviderDetail extends RuleProvider {
  rules: Array<{ payload: string; proxy?: string; type?: string }>
}

export interface ClashApiProfile {
  id: string
  name: string
  url: string
  secret: string
}

export interface ConfigProfile {
  id: string
  name: string
  type: 'local' | 'remote'
  source: string
  lastUpdated?: string
  autoUpdateInterval: number // 小时，0 = 不自动更新
}

export interface AppConfig {
  clashApis: ClashApiProfile[]
  activeClashApiId: string
  singboxPath: string
  workingDir: string
  serviceName: string
  startupDelaySeconds: number
  theme: string
  latencyTestUrl: string
  ipv6TestEnabled: boolean
  groupTestUrls: Record<string, string>
  configProfiles: ConfigProfile[]
  activeConfigProfileId: string
  closeToTray: boolean
  coreUpdateSource: 'official' | 'ref1nd' | 'custom'
  coreUpdateCustomRepo: string
  coreUpdateChannel: 'stable' | 'testing'
  coreUpdateMirror: string
}
