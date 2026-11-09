import { describe, expect, it } from 'vitest'
import { parseRuleAction } from './ruleDisplay'

describe('parseRuleAction', () => {
  it('extracts the outbound from route and bypass actions', () => {
    expect(parseRuleAction('route(Proxy)')).toMatchObject({ kind: 'route', outbound: 'Proxy', args: [] })
    expect(parseRuleAction('route(香港 (HK),override-port=443)')).toMatchObject({
      kind: 'route',
      outbound: '香港 (HK)',
      args: ['override-port=443'],
    })
    expect(parseRuleAction('bypass(direct-out)')).toMatchObject({ kind: 'route', outbound: 'direct-out' })
  })

  it('classifies direct and reject actions', () => {
    expect(parseRuleAction('direct')).toMatchObject({ kind: 'direct', raw: 'direct' })
    expect(parseRuleAction('direct(bind_interface=wlan0)')).toMatchObject({ kind: 'direct' })
    expect(parseRuleAction('reject')).toMatchObject({ kind: 'reject', raw: 'reject' })
    expect(parseRuleAction('reject(drop)')).toMatchObject({ kind: 'reject', args: ['drop'] })
  })

  it('treats non-routing actions as plain actions', () => {
    expect(parseRuleAction('sniff')).toMatchObject({ kind: 'action', name: 'sniff' })
    expect(parseRuleAction('sniff(tls,http)')).toMatchObject({ kind: 'action', raw: 'sniff(tls,http)' })
    expect(parseRuleAction('hijack-dns')).toMatchObject({ kind: 'action', name: 'hijack-dns' })
    expect(parseRuleAction('resolve(local)')).toMatchObject({ kind: 'action', name: 'resolve' })
    expect(parseRuleAction('bypass()')).toMatchObject({ kind: 'action', name: 'bypass' })
  })

  it('falls back to a bare outbound tag', () => {
    expect(parseRuleAction('Proxy')).toMatchObject({ kind: 'route', outbound: 'Proxy' })
    expect(parseRuleAction('🇯🇵 Tokyo')).toMatchObject({ kind: 'route', outbound: '🇯🇵 Tokyo' })
  })
})
