import { invoke } from '@tauri-apps/api/core'
import type { ServiceStatus } from '@/types'

export async function queryServiceStatus(serviceName: string): Promise<ServiceStatus> {
  return invoke<ServiceStatus>('service_status', { serviceName })
}

export async function startService(serviceName: string): Promise<void> {
  return invoke('service_start', { serviceName })
}

export async function stopService(serviceName: string): Promise<void> {
  return invoke('service_stop', { serviceName })
}

export async function restartService(serviceName: string): Promise<void> {
  return invoke('service_restart', { serviceName })
}

export async function installService(
  serviceName: string,
  singboxPath: string,
  configPath: string,
  workingDir: string,
  startupDelaySeconds: number,
): Promise<void> {
  return invoke('service_install', { serviceName, singboxPath, configPath, workingDir, startupDelaySeconds })
}

export async function uninstallService(serviceName: string): Promise<void> {
  return invoke('service_uninstall', { serviceName })
}

// 同步服务宿主组件(singboard-service.exe):迁移旧注册路径或热换新版本
// 返回 'not_installed' | 'skipped' | 'ok' | 'migrated' | 'updated'
export async function syncServiceComponent(serviceName: string): Promise<string> {
  return invoke<string>('service_component_sync', { serviceName })
}

export async function readServiceErrorLog(serviceName: string): Promise<string> {
  return invoke<string>('service_error_log', { serviceName })
}

export async function startupTaskExists(serviceName: string): Promise<boolean> {
  return invoke<boolean>('service_startup_task_exists', { serviceName })
}

export async function createStartupTask(serviceName: string, startupDelaySeconds: number): Promise<void> {
  return invoke('service_create_startup_task', { serviceName, startupDelaySeconds })
}

export async function deleteStartupTask(serviceName: string): Promise<void> {
  return invoke('service_delete_startup_task', { serviceName })
}
