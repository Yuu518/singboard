import { invoke } from '@tauri-apps/api/core'
import { IS_ANDROID } from '@/platform'
import type { ServiceStatus } from '@/types'

export const SERVICE_NAME = 'singboard.service'

export async function queryServiceStatus(): Promise<ServiceStatus> {
  return invoke<ServiceStatus>('service_status')
}

export async function startService(): Promise<void> {
  return invoke('service_start')
}

export async function stopService(): Promise<void> {
  return invoke('service_stop')
}

export async function restartService(): Promise<void> {
  return invoke('service_restart')
}

export async function installService(
  singboxPath: string,
  configPath: string,
  workingDir: string,
  startupDelaySeconds = 0,
): Promise<void> {
  if (IS_ANDROID) return invoke('service_install', { singboxPath, configPath, workingDir })
  return invoke('service_install', { singboxPath, configPath, workingDir, startupDelaySeconds })
}

export async function uninstallService(): Promise<void> {
  return invoke('service_uninstall')
}

// 启动时自动同步；需要变更时通过 UAC 执行，后端合并同次启动的请求。
// 返回 'not_installed' | 'ok' | 'migrated' | 'updated'
export async function syncServiceComponent(): Promise<string> {
  return invoke<string>('service_component_sync')
}

export function isElevationCancelled(error: unknown): boolean {
  return (error instanceof Error ? error.message : String(error)).includes('elevation_cancelled')
}

export async function readServiceErrorLog(): Promise<string> {
  return invoke<string>('service_error_log')
}

export async function startupTaskExists(): Promise<boolean> {
  return invoke<boolean>('service_startup_task_exists')
}

export async function createStartupTask(startupDelaySeconds: number): Promise<void> {
  return invoke('service_create_startup_task', { startupDelaySeconds })
}

export async function deleteStartupTask(): Promise<void> {
  return invoke('service_delete_startup_task')
}

export async function bootHookExists(): Promise<boolean> {
  return invoke<boolean>('service_boot_hook_exists')
}

export async function createBootHook(): Promise<void> {
  return invoke('service_create_boot_hook')
}

export async function deleteBootHook(): Promise<void> {
  return invoke('service_delete_boot_hook')
}
