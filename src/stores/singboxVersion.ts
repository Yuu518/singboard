import { ref } from 'vue'
import { getFileHash, getSingboxVersion } from '@/bridge/config'
import { normalizeVersionText } from '@/utils/format'
import { useConfigStore } from './config'

// 版本号按内核文件哈希缓存：启动时先展示缓存值，哈希变化才重新检测
const CACHE_KEY = 'singboard-singbox-version'

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
let detecting = false

async function detectVersion() {
  if (detecting) return
  const { config } = useConfigStore()
  const path = config.value.singboxPath?.trim()
  if (!path) {
    singboxVersion.value = ''
    return
  }
  detecting = true
  try {
    const hash = await getFileHash(path)
    const cached = loadCache()
    if (cached.hash === hash && cached.version) {
      singboxVersion.value = cached.version
      return
    }
    const raw = await getSingboxVersion(path)
    const version = normalizeVersionText(raw)
    singboxVersion.value = version
    localStorage.setItem(CACHE_KEY, JSON.stringify({ hash, version }))
  } catch {
    // 检测失败时保留现有显示
  } finally {
    detecting = false
  }
}

export function useSingboxVersionStore() {
  return { singboxVersion, detectVersion }
}
