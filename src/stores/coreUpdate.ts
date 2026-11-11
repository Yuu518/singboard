import { computed, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { checkCoreUpdate, performCoreUpdate, probeAssetCoreHash, type CoreUpdateInfo, type CoreUpdateProgress } from '@/bridge/coreUpdate'
import { getFileHash } from '@/bridge/config'
import { isElevationCancelled } from '@/bridge/service'
import { failureMessage } from '@/utils/humanError'
import { useConfigStore } from './config'
import { useSingboxVersionStore } from './singboxVersion'
import { useToastStore } from './toast'

// "资产 digest → 资产内 sing-box.exe 哈希"的缓存（只留最近一条），
// 版本号相同时用本地 exe 哈希与之比对，识别上游重建或本地被手动替换
const INSTALL_RECORD_KEY = 'singboard-core-install-record-v2'

export const CORE_REPOS: Record<string, string> = {
  official: 'SagerNet/sing-box',
  ref1nd: 'reF1nd/sing-box-releases',
}

interface InstallRecord {
  assetDigest: string
  exeHash: string
}

export type CoreCheckOutcome = 'up-to-date' | 'update' | 'out-of-sync'

function saveInstallRecord(record: InstallRecord) {
  localStorage.setItem(INSTALL_RECORD_KEY, JSON.stringify(record))
}

function loadInstallRecord(): InstallRecord | null {
  try {
    const record = JSON.parse(localStorage.getItem(INSTALL_RECORD_KEY) ?? '')
    if (typeof record?.assetDigest === 'string' && typeof record?.exeHash === 'string') {
      return record
    }
  } catch { }
  return null
}

const checking = ref(false)
const verifying = ref(false)
const updating = ref(false)
const latest = ref<CoreUpdateInfo | null>(null)
const outOfSync = ref(false)
const progress = ref<CoreUpdateProgress | null>(null)
let sourceWatchStarted = false

void listen<CoreUpdateProgress>('core-update-progress', (event) => {
  if (updating.value || verifying.value) progress.value = event.payload
}).catch(() => { })

function currentRepo(): string {
  const { config } = useConfigStore()
  return config.value.coreUpdateSource === 'custom'
    ? config.value.coreUpdateCustomRepo.trim()
    : CORE_REPOS[config.value.coreUpdateSource] ?? ''
}

function versionNumber(text: string): string {
  return text.match(/\d+\.\d+\S*/)?.[0] ?? ''
}

function ensureSourceWatch() {
  if (sourceWatchStarted) return
  sourceWatchStarted = true
  const { config } = useConfigStore()
  watch(
    () => [config.value.coreUpdateSource, config.value.coreUpdateCustomRepo, config.value.coreUpdateChannel],
    () => {
      if (checking.value || updating.value) return
      latest.value = null
      outOfSync.value = false
    },
  )
}

// 版本号相同时判断本地核心与上游最新资产是否不一致（上游重建或本地被手动替换）。
// 缓存命中直接比对；未命中则下载资产解压计算 exe 哈希（每个 digest 只下载一次）
async function isLocalOutOfSync(info: CoreUpdateInfo): Promise<boolean> {
  const { config } = useConfigStore()
  const path = config.value.singboxPath.trim()
  if (!info.assetDigest || !path) return false
  let localHash: string
  try {
    localHash = await getFileHash(path)
  } catch {
    return false
  }
  const record = loadInstallRecord()
  if (record && record.assetDigest === info.assetDigest) {
    return localHash !== record.exeHash
  }
  verifying.value = true
  try {
    const exeHash = await probeAssetCoreHash({
      assetUrl: info.assetUrl,
      assetSize: info.assetSize,
      assetDigest: info.assetDigest,
      mirror: config.value.coreUpdateMirror,
    })
    saveInstallRecord({ assetDigest: info.assetDigest, exeHash })
    return localHash !== exeHash
  } finally {
    verifying.value = false
    progress.value = null
  }
}

async function check(): Promise<CoreCheckOutcome | null> {
  ensureSourceWatch()
  if (checking.value || updating.value) return null
  const { config } = useConfigStore()
  const { pushToast } = useToastStore()
  const { detectVersion, singboxVersion } = useSingboxVersionStore()
  const repo = currentRepo()
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) {
    pushToast({ message: 'GitHub 仓库格式应为 owner/repo，例如 SagerNet/sing-box', type: 'error' })
    return null
  }
  checking.value = true
  try {
    const [info] = await Promise.all([
      checkCoreUpdate(repo, config.value.coreUpdateChannel, config.value.githubToken),
      detectVersion(),
    ])
    const checkedVersion = info.version.replace(/^v/, '')
    const updateAvailable = checkedVersion !== versionNumber(singboxVersion.value)
    const checkedOutOfSync = updateAvailable ? false : await isLocalOutOfSync(info)
    latest.value = info
    outOfSync.value = checkedOutOfSync
    if (updateAvailable) return 'update'
    if (checkedOutOfSync) return 'out-of-sync'
    pushToast({ message: `当前已是最新版本（${checkedVersion}）`, type: 'info' })
    return 'up-to-date'
  } catch (error) {
    pushToast({ message: failureMessage('检查核心更新失败', error), type: 'error' }, 6000)
    return null
  } finally {
    checking.value = false
  }
}

async function update(): Promise<boolean> {
  const info = latest.value
  if (!info || updating.value || checking.value) return false
  const { config } = useConfigStore()
  const { pushToast } = useToastStore()
  const { detectVersion } = useSingboxVersionStore()
  if (!config.value.singboxPath.trim()) {
    pushToast({ message: '先在“服务参数”里设置 sing-box 可执行文件', type: 'error' })
    return false
  }
  updating.value = true
  progress.value = null
  const assetDigest = info.assetDigest
  try {
    const result = await performCoreUpdate({
      assetUrl: info.assetUrl,
      assetSize: info.assetSize,
      assetDigest,
      mirror: config.value.coreUpdateMirror,
      singboxPath: config.value.singboxPath,
    })
    pushToast({ message: `核心已更新到 ${result.version}${result.restarted ? '，服务已重启' : ''}`, type: 'info' })
    latest.value = null
    outOfSync.value = false
    try {
      if (assetDigest) {
        saveInstallRecord({ assetDigest, exeHash: await getFileHash(config.value.singboxPath) })
      } else {
        localStorage.removeItem(INSTALL_RECORD_KEY)
      }
    } catch { }
    await detectVersion()
    return true
  } catch (error) {
    if (!isElevationCancelled(error)) pushToast({ message: failureMessage('核心更新失败', error), type: 'error' }, 8000)
    return false
  } finally {
    updating.value = false
    progress.value = null
  }
}

const releaseUrl = computed(() => {
  const repo = currentRepo()
  const tag = latest.value?.version
  return repo && tag ? `https://github.com/${repo}/releases/tag/${tag}` : ''
})

export function useCoreUpdateStore() {
  ensureSourceWatch()
  return { checking, verifying, updating, latest, outOfSync, progress, releaseUrl, currentRepo, check, update }
}
