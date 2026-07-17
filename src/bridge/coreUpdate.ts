import { invoke } from '@tauri-apps/api/core'

export interface CoreUpdateInfo {
  version: string
  prerelease: boolean
  publishedAt: string
  assetName: string
  assetUrl: string
  assetSize: number
  /** GitHub 资产 SHA-256（"sha256:..."），个别源缺失时为空串 */
  assetDigest: string
}

export interface CoreUpdateResult {
  version: string
  restarted: boolean
}

export interface CoreUpdateProgress {
  phase: 'download' | 'extract' | 'replace' | 'restart'
  downloaded: number
  total: number
}

export async function checkCoreUpdate(repo: string, channel: string): Promise<CoreUpdateInfo> {
  return invoke<CoreUpdateInfo>('check_core_update', { repo, channel })
}

export async function performCoreUpdate(args: {
  assetUrl: string
  assetSize: number
  mirror: string
  singboxPath: string
  serviceName: string
}): Promise<CoreUpdateResult> {
  return invoke<CoreUpdateResult>('perform_core_update', args)
}
