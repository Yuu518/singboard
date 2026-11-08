export type EndpointProtocol = 'http' | 'https'

export interface EndpointAddress {
  protocol: EndpointProtocol
  host: string
  port: string
}

export type EndpointField = 'host' | 'port'

export type EndpointBuildResult =
  | { ok: true; url: string; address: EndpointAddress }
  | { ok: false; field: EndpointField; error: string }

const PROTOCOL_PATTERN = /^([a-z][a-z0-9+.-]*):\/\//i

function isProtocol(value: string): value is EndpointProtocol {
  return value === 'http' || value === 'https'
}

function looksLikeBareIpv6(host: string): boolean {
  return (host.match(/:/g)?.length ?? 0) >= 2 && /^[0-9a-f:.%]+$/i.test(host)
}

export function parseApiUrl(url: string): EndpointAddress {
  const trimmed = url.trim()
  try {
    const parsed = new URL(trimmed)
    const protocol = parsed.protocol.replace(/:$/, '')
    if (isProtocol(protocol) && parsed.hostname) {
      return { protocol, host: parsed.hostname, port: parsed.port }
    }
  } catch { }
  return { protocol: 'http', host: trimmed, port: '' }
}

export function hostForDisplay(url: string): string {
  const { host, port } = parseApiUrl(url)
  return port ? `${host}:${port}` : host
}

export function buildApiUrl(input: EndpointAddress): EndpointBuildResult {
  let protocol: EndpointProtocol = input.protocol
  let host = input.host.trim()
  let port = input.port.trim()

  if (!host) return { ok: false, field: 'host', error: '填写主机地址，例如 127.0.0.1' }

  const prefix = host.match(PROTOCOL_PATTERN)
  if (prefix) {
    const scheme = prefix[1].toLowerCase()
    if (!isProtocol(scheme)) return { ok: false, field: 'host', error: `不支持 ${scheme}://，只能用 http 或 https` }
    protocol = scheme
    let parsed: URL
    try {
      parsed = new URL(host)
    } catch {
      return { ok: false, field: 'host', error: '主机地址无效' }
    }
    if (parsed.pathname !== '/' || parsed.search || parsed.hash || parsed.username || parsed.password) {
      return { ok: false, field: 'host', error: '主机地址里不能带路径、参数或账号' }
    }
    host = parsed.hostname
    if (parsed.port) {
      if (port && port !== parsed.port) return { ok: false, field: 'port', error: `主机里写的端口是 ${parsed.port}，和这里不一致` }
      port = parsed.port
    }
  } else if (looksLikeBareIpv6(host)) {
    host = `[${host}]`
  } else {
    const bracketed = host.match(/^(\[[^\]]+\])(?::(\d*))?$/)
    const named = host.match(/^([^:[\]]+):(\d*)$/)
    const split = bracketed ?? named
    if (split) {
      host = split[1]
      const inline = split[2] ?? ''
      if (inline) {
        if (port && port !== inline) return { ok: false, field: 'port', error: `主机里写的端口是 ${inline}，和这里不一致` }
        port = inline
      }
    }
  }

  if (/[\s/?#@\\]/.test(host)) return { ok: false, field: 'host', error: '主机地址里不能有空格、斜杠、? # @' }

  if (port) {
    if (!/^\d+$/.test(port)) return { ok: false, field: 'port', error: '端口只能是数字' }
    const value = Number(port)
    if (value < 1 || value > 65535) return { ok: false, field: 'port', error: '端口范围是 1–65535' }
    port = String(value)
  }

  let normalizedHost: string
  try {
    const parsed = new URL(`${protocol}://${host}${port ? `:${port}` : ''}`)
    normalizedHost = parsed.hostname
    if (!normalizedHost) throw new Error('empty')
  } catch {
    return { ok: false, field: 'host', error: '主机地址无效' }
  }

  const address: EndpointAddress = { protocol, host: normalizedHost, port }
  return { ok: true, url: `${protocol}://${normalizedHost}${port ? `:${port}` : ''}`, address }
}
