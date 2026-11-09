import { useConfigStore } from '@/stores/config'
import { validateSingboxConfig } from '@/bridge/config'
import { installService, startService, restartService } from '@/bridge/service'

function currentPaths() {
  const { config, refreshConfigFromStorage } = useConfigStore()
  refreshConfigFromStorage()
  const { workingDir, singboxPath, configPath } = config.value
  if (!workingDir) throw new Error('请先在设置中指定 sing-box 工作目录')
  if (!singboxPath) throw new Error('未在工作目录中找到 sing-box 核心，可在「设置 → 更新」中下载')
  if (!configPath) throw new Error('未在工作目录中找到配置文件')
  return { workingDir, singboxPath, configPath }
}

export async function ensureServiceInstalled(): Promise<void> {
  const paths = currentPaths()
  await installService(paths.singboxPath, paths.configPath, paths.workingDir)
}

export async function prepareCoreStart(): Promise<void> {
  const paths = currentPaths()
  try {
    await validateSingboxConfig(paths.singboxPath, paths.configPath, paths.workingDir)
  } catch (e: any) {
    throw new Error('配置文件校验失败:\n' + (e?.message || e))
  }
  await installService(paths.singboxPath, paths.configPath, paths.workingDir)
}

export async function startCore(): Promise<void> {
  await prepareCoreStart()
  await startService()
}

export async function restartCore(): Promise<void> {
  await prepareCoreStart()
  await restartService()
}
