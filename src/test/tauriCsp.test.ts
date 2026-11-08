import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const config = JSON.parse(readFileSync(resolve(__dirname, '../../src-tauri/tauri.conf.json'), 'utf8'))
const security = config.app.security
const csp: Record<string, string> = security.csp

function sources(directive: string): string[] {
  return (csp[directive] ?? '').split(/\s+/).filter(Boolean)
}

describe('tauri content security policy', () => {
  it('is set', () => {
    expect(csp).toBeTruthy()
    expect(typeof csp).toBe('object')
  })

  it('only runs bundled scripts', () => {
    expect(sources('script-src')).toEqual(["'self'"])
    expect(sources('default-src')).toEqual(["'self'"])
    expect(sources('object-src')).toEqual(["'none'"])
    expect(sources('base-uri')).toEqual(["'none'"])
  })

  it('keeps Tauri IPC and any Clash API host reachable', () => {
    expect(sources('connect-src')).toEqual(expect.arrayContaining(['ipc:', 'http://ipc.localhost', 'http:', 'https:', 'ws:', 'wss:']))
  })

  it('allows the asset protocol, wallpaper blobs and lens data maps', () => {
    expect(sources('img-src')).toEqual(expect.arrayContaining(['asset:', 'http://asset.localhost', 'blob:', 'data:']))
  })

  it('keeps inline styles working by leaving style-src without a nonce', () => {
    expect(sources('style-src')).toContain("'unsafe-inline'")
    expect(security.dangerousDisableAssetCspModification).toEqual(['style-src'])
  })
})
