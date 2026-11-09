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

export async function showMainWindow(): Promise<void> {
  return invoke('show_main_window')
}

export async function quitApp(): Promise<void> {
  return invoke('quit_app')
}

export async function setWindowMaterial(dark: boolean): Promise<boolean> {
  return invoke<boolean>('set_window_material', { dark })
}

export interface SystemInsets {
  top: number
  right: number
  bottom: number
  left: number
}

export async function checkRootAccess(): Promise<boolean> {
  return invoke<boolean>('root_check')
}

export async function setRootEnabled(enabled: boolean): Promise<void> {
  return invoke('set_root_enabled', { enabled })
}

export async function setSystemBars(dark: boolean, color?: string): Promise<void> {
  return invoke('set_system_bars', { dark, color })
}

export async function getSystemInsets(): Promise<SystemInsets | null> {
  return invoke<SystemInsets | null>('system_insets')
}
