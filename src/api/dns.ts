import { invoke } from '@tauri-apps/api/core'

export interface DnsRecord {
  record_type: string
  name: string
  ttl: number
  value: string
  ip: string | null
}

export interface DnsQueryResult {
  source: 'system' | 'custom'
  server: string | null
  records: DnsRecord[]
}

export function queryDns(domain: string, recordType: string, server?: string) {
  return invoke<DnsQueryResult>('dns_query', {
    domain,
    recordType,
    server: server?.trim() || null,
  })
}
