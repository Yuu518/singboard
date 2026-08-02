import { useConfigStore } from '@/stores/config'
import {
  copyToRunningConfig,
  getRemoteConfigPath,
  getRunningConfigPath,
  validateSingboxConfig,
} from '@/bridge/config'
import { startService, stopService } from '@/bridge/service'

// 激活配置的源文件路径；未选用任何配置时返回 null
export async function resolveActiveConfigPath(): Promise<string | null> {
  const { config, configProfiles } = useConfigStore()
  const activeId = config.value.activeConfigProfileId
  if (!activeId) return null
  const profile = configProfiles.value.find((p) => p.id === activeId)
  if (!profile) return null
  if (profile.type === 'local') return profile.source
  return await getRemoteConfigPath(profile.id)
}

// 仅同步激活配置到 running-config，不做校验
export async function syncActiveConfigToRunning(): Promise<void> {
  const sourcePath = await resolveActiveConfigPath()
  if (!sourcePath) return
  await copyToRunningConfig(sourcePath)
}

// 启动核心前的准备：校验激活配置并同步到 running-config。
// 核心读取的始终是 running-config，源文件可能在面板之外被改动，
// 因此每次启动都要重新同步，否则会用到上一次的旧配置。
export async function prepareCoreStart(): Promise<void> {
  const { config } = useConfigStore()
  const { singboxPath, workingDir } = config.value
  if (!singboxPath) throw new Error('请先配置 sing-box 路径')

  try {
    const activeConfigPath = await resolveActiveConfigPath()
    if (activeConfigPath) {
      await validateSingboxConfig(singboxPath, activeConfigPath, workingDir)
      await copyToRunningConfig(activeConfigPath)
    } else {
      const runningConfigPath = await getRunningConfigPath()
      await validateSingboxConfig(singboxPath, runningConfigPath, workingDir)
    }
  } catch (e: any) {
    throw new Error('配置文件校验或同步失败:\n' + (e?.message || e))
  }
}

// 启动核心：先同步配置再拉起服务
export async function startCore(serviceName: string): Promise<void> {
  await prepareCoreStart()
  await startService(serviceName)
}

// 重启核心：配置校验失败时不停服务，保持当前连接可用
export async function restartCore(serviceName: string): Promise<void> {
  await prepareCoreStart()
  await stopService(serviceName)
  await new Promise((r) => setTimeout(r, 500))
  await startService(serviceName)
}
