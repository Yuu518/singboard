import { invoke } from '@tauri-apps/api/core'

export interface PanelUpdateInfo {
  currentVersion: string
  latestVersion: string
  hasUpdate: boolean
  outOfSync: boolean
  publishedAt: string
  assetUrl: string
  assetSize: number
  assetDigest: string
}

export interface PanelUpdateProgress {
  phase: 'download' | 'verify' | 'replace'
  downloaded: number
  total: number
}

export async function checkPanelUpdate(): Promise<PanelUpdateInfo> {
  return invoke<PanelUpdateInfo>('check_panel_update')
}

export async function performPanelUpdate(args: {
  assetUrl: string
  assetSize: number
  assetDigest: string
  mirror: string
}): Promise<void> {
  return invoke('perform_panel_update', args)
}

export async function cleanupPanelUpdate(): Promise<void> {
  return invoke('cleanup_panel_update')
}

export function isInstallPermissionRequired(error: unknown): boolean {
  return String(error instanceof Error ? error.message : error).includes('install_permission_required')
}
