import { invoke } from '@tauri-apps/api/core'

export async function isLaunchedHidden(): Promise<boolean> {
  return invoke<boolean>('is_launched_hidden')
}

export async function getAutoLaunch(): Promise<boolean> {
  return invoke<boolean>('get_auto_launch')
}

export async function setAutoLaunch(enabled: boolean): Promise<void> {
  return invoke('set_auto_launch', { enabled })
}
