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

/** 下载资产并解压，返回其中 sing-box.exe 的 SHA-256（用于同版本一致性校验） */
export async function probeAssetExeHash(args: {
  assetUrl: string
  assetSize: number
  mirror: string
}): Promise<string> {
  return invoke<string>('probe_asset_exe_hash', args)
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
