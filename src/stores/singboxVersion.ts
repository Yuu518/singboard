import { ref } from 'vue'
import { getFileHash, getSingboxVersion } from '@/bridge/config'
import { normalizeVersionText } from '@/utils/format'
import { useConfigStore } from './config'
import { fetchVersion } from '@/api'
import { IS_ANDROID } from '@/platform'

// 版本号按内核文件哈希缓存：启动时先展示缓存值，哈希变化才重新检测
const CACHE_KEY = 'singboard-singbox-version'
const PATH_EDIT_DELAY_MS = 800

export type SingboxVersionStatus = 'cached' | 'detecting' | 'ok' | 'failed' | 'empty'

function loadCache(): { hash: string; version: string } {
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE_KEY) ?? '')
    if (typeof cached?.hash === 'string' && typeof cached?.version === 'string') {
      return cached
    }
  } catch { }
  return { hash: '', version: '' }
}

const singboxVersion = ref(loadCache().version)
const versionStatus = ref<SingboxVersionStatus>('cached')
const versionError = ref('')
let generation = 0
let latestRun: Promise<void> = Promise.resolve()
let scheduleTimer: ReturnType<typeof setTimeout> | null = null

function fail(reason: string) {
  singboxVersion.value = ''
  versionError.value = reason
  versionStatus.value = 'failed'
}

async function runRemoteDetect(token: number) {
  versionStatus.value = 'detecting'
  try {
    const { data } = await fetchVersion()
    if (token !== generation) return
    const raw = typeof data?.version === 'string' ? data.version : ''
    const version = raw ? normalizeVersionText(raw.startsWith('sing-box') ? raw : `sing-box ${raw}`) : ''
    if (!version) {
      fail('Clash API 未返回版本号')
      return
    }
    singboxVersion.value = version
    versionError.value = ''
    versionStatus.value = 'ok'
  } catch {
    if (token === generation) fail('无法连接 Clash API')
  }
}

async function runDetect(token: number) {
  const { config } = useConfigStore()
  if (IS_ANDROID && !config.value.rootMode) {
    await runRemoteDetect(token)
    return
  }
  const path = config.value.singboxPath?.trim()
  if (!path) {
    singboxVersion.value = ''
    versionError.value = ''
    versionStatus.value = 'empty'
    return
  }
  versionStatus.value = 'detecting'
  let hash: string
  try {
    hash = await getFileHash(path)
  } catch {
    if (token === generation) fail('找不到该文件或无法读取')
    return
  }
  if (token !== generation) return
  const cached = loadCache()
  if (cached.hash === hash && cached.version) {
    singboxVersion.value = cached.version
    versionError.value = ''
    versionStatus.value = 'ok'
    return
  }
  try {
    const version = normalizeVersionText(await getSingboxVersion(path))
    if (token !== generation) return
    if (!version) {
      fail('无法从该文件读取版本号')
      return
    }
    singboxVersion.value = version
    versionError.value = ''
    versionStatus.value = 'ok'
    localStorage.setItem(CACHE_KEY, JSON.stringify({ hash, version }))
  } catch {
    if (token === generation) fail('无法运行该文件读取版本号')
  }
}

async function detectVersion(): Promise<void> {
  if (scheduleTimer) {
    clearTimeout(scheduleTimer)
    scheduleTimer = null
  }
  const token = ++generation
  let awaited = runDetect(token)
  latestRun = awaited
  await awaited
  while (latestRun !== awaited) {
    awaited = latestRun
    await awaited
  }
}

function scheduleDetectVersion(delayMs = PATH_EDIT_DELAY_MS) {
  if (scheduleTimer) clearTimeout(scheduleTimer)
  scheduleTimer = setTimeout(() => {
    scheduleTimer = null
    void detectVersion()
  }, delayMs)
}

export function useSingboxVersionStore() {
  return { singboxVersion, versionStatus, versionError, detectVersion, scheduleDetectVersion }
}
