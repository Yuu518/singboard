import { describe, expect, it } from 'vitest'
import { buildApiUrl, hostForDisplay, parseApiUrl } from './clashEndpoint'

const build = (host: string, port = '', protocol: 'http' | 'https' = 'http') => buildApiUrl({ protocol, host, port })

describe('parseApiUrl', () => {
  it('splits a plain address', () => {
    expect(parseApiUrl('http://127.0.0.1:9090')).toEqual({ protocol: 'http', host: '127.0.0.1', port: '9090' })
    expect(parseApiUrl('https://clash.example.com')).toEqual({ protocol: 'https', host: 'clash.example.com', port: '' })
  })

  it('keeps an IPv6 host whole instead of cutting it at the first colon', () => {
    expect(parseApiUrl('http://[::1]:9090')).toEqual({ protocol: 'http', host: '[::1]', port: '9090' })
    expect(parseApiUrl('http://[fe80::1]')).toEqual({ protocol: 'http', host: '[fe80::1]', port: '' })
  })

  it('falls back to the raw text for something that is not a URL', () => {
    expect(parseApiUrl('not a url')).toEqual({ protocol: 'http', host: 'not a url', port: '' })
  })

  it('shows host and port for display', () => {
    expect(hostForDisplay('http://[::1]:9090')).toBe('[::1]:9090')
  })
})

describe('buildApiUrl', () => {
  it('builds a normal address', () => {
    expect(build('127.0.0.1', '9090')).toMatchObject({ ok: true, url: 'http://127.0.0.1:9090' })
    expect(build('Clash.Example.com', '', 'https')).toMatchObject({ ok: true, url: 'https://clash.example.com' })
  })

  it('takes the protocol and port out of a pasted URL', () => {
    expect(build('https://10.0.0.2:9443', '')).toMatchObject({
      ok: true,
      url: 'https://10.0.0.2:9443',
      address: { protocol: 'https', host: '10.0.0.2', port: '9443' },
    })
    expect(build('http://127.0.0.1/', '9090')).toMatchObject({ ok: true, url: 'http://127.0.0.1:9090' })
  })

  it('accepts host:port typed into the host field', () => {
    expect(build('127.0.0.1:9090')).toMatchObject({ ok: true, url: 'http://127.0.0.1:9090' })
  })

  it('brackets IPv6 hosts', () => {
    expect(build('::1', '9090')).toMatchObject({ ok: true, url: 'http://[::1]:9090' })
    expect(build('[::1]', '9090')).toMatchObject({ ok: true, url: 'http://[::1]:9090' })
    expect(build('[::1]:9090')).toMatchObject({ ok: true, url: 'http://[::1]:9090' })
    expect(build('http://[::1]:9090')).toMatchObject({ ok: true, address: { host: '[::1]', port: '9090' } })
  })

  it('rejects ports that are not numbers in range', () => {
    expect(build('127.0.0.1', '90a')).toEqual({ ok: false, field: 'port', error: '端口只能是数字' })
    expect(build('127.0.0.1', '-1')).toMatchObject({ ok: false, field: 'port' })
    expect(build('127.0.0.1', '0')).toEqual({ ok: false, field: 'port', error: '端口范围是 1–65535' })
    expect(build('127.0.0.1', '70000')).toMatchObject({ ok: false, field: 'port' })
  })

  it('rejects conflicting ports', () => {
    expect(build('127.0.0.1:9090', '9091')).toMatchObject({ ok: false, field: 'port' })
    expect(build('http://127.0.0.1:9090', '9091')).toMatchObject({ ok: false, field: 'port' })
  })

  it('rejects hosts that are empty, carry paths, or use another scheme', () => {
    expect(build('  ')).toMatchObject({ ok: false, field: 'host' })
    expect(build('ws://127.0.0.1')).toMatchObject({ ok: false, field: 'host', error: '不支持 ws://，只能用 http 或 https' })
    expect(build('http://127.0.0.1:9090/ui')).toMatchObject({ ok: false, field: 'host' })
    expect(build('127.0.0.1/ui')).toMatchObject({ ok: false, field: 'host' })
    expect(build('my host')).toMatchObject({ ok: false, field: 'host' })
  })
})
