import { invoke } from '@tauri-apps/api/core'

export interface DetectedRuntimeFiles {
  baseDir: string
  singboxPath?: string
  configPath?: string
  found: boolean
}

export async function readSingboxConfig(path: string): Promise<string> {
  return invoke<string>('read_config', { path })
}

export async function writeSingboxConfig(path: string, content: string): Promise<void> {
  return invoke('write_config', { path, content })
}

export async function validateSingboxConfig(
  singboxPath: string,
  configPath: string,
  workingDir?: string,
): Promise<string> {
  return invoke<string>('validate_config', { singboxPath, configPath, workingDir })
}

export async function validateSingboxConfigContent(
  singboxPath: string,
  configPath: string,
  content: string,
  workingDir?: string,
): Promise<string> {
  return invoke<string>('validate_config_content', { singboxPath, configPath, content, workingDir })
}

export async function getSingboxVersion(singboxPath: string): Promise<string> {
  return invoke<string>('get_singbox_version', { singboxPath })
}

export async function getFileHash(path: string): Promise<string> {
  return invoke<string>('get_file_hash', { path })
}

export async function detectRuntimeFiles(baseDir?: string): Promise<DetectedRuntimeFiles> {
  return invoke<DetectedRuntimeFiles>('detect_runtime_files', { baseDir })
}

export async function getRunningConfigPath(): Promise<string> {
  return invoke<string>('get_running_config_path')
}

export async function getRemoteConfigDir(): Promise<string> {
  return invoke<string>('get_remote_config_dir')
}

export async function getRemoteConfigPath(profileId: string): Promise<string> {
  return invoke<string>('get_remote_config_path', { profileId })
}

export async function deleteFile(path: string): Promise<void> {
  return invoke('delete_file', { path })
}

export async function copyToRunningConfig(sourcePath: string): Promise<string> {
  return invoke<string>('copy_to_running_config', { sourcePath })
}

export async function fetchUrl(url: string): Promise<string> {
  return invoke<string>('fetch_url', { url })
}

export async function srsListProvider(
  workingDir: string,
  configPath: string,
  singboxPath: string,
  tag: string,
): Promise<Array<{ type: string; value: string }>> {
  return invoke<Array<{ type: string; value: string }>>('srs_list_provider', { workingDir, configPath, singboxPath, tag })
}

export async function srsMatchProvider(
  workingDir: string,
  configPath: string,
  singboxPath: string,
  tag: string,
  query: string,
): Promise<boolean> {
  return invoke<boolean>('srs_match_provider', { workingDir, configPath, singboxPath, tag, query })
}
