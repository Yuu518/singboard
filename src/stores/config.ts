import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setRootEnabled } from '@/bridge/app'
import { IS_ANDROID } from '@/platform'
import type { AppConfig, ClashApiProfile, ConfigProfile } from '@/types'
import { applyGlassTint, normalizeGlassTint } from '@/utils/glassTint'

const STORAGE_KEY = 'singboard-config'

export const SERVICE_HOME = '/data/adb/singboard'

function createClashApiId(): string {
  return `api_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
}

export function createConfigProfileId(): string {
  return `cfg_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
}

function createClashApiProfile(name: string, url: string, secret: string): ClashApiProfile {
  return {
    id: createClashApiId(),
    name,
    url,
    secret,
  }
}

function normalizeConfig(raw: any): AppConfig {
  const normalizeWindowsPath = (value: unknown): string => {
    if (typeof value !== 'string') return ''
    if (value.startsWith('\\\\?\\UNC\\')) return `\\\\${value.slice(8)}`
    if (value.startsWith('\\\\?\\')) return value.slice(4)
    return value
  }
  const normalizePath = (value: unknown): string => (typeof value === 'string' ? value.trim() : '')
  const normalizeStartupDelay = (value: unknown): number => {
    const delay = typeof value === 'number' ? value : Number(value)
    if (!Number.isFinite(delay)) return 30
    return Math.min(3600, Math.max(0, Math.round(delay)))
  }

  const savedProfiles = Array.isArray(raw?.clashApis)
    ? raw.clashApis
      .filter((item: any) => item && typeof item === 'object')
      .map((item: any): ClashApiProfile => ({
        id: typeof item.id === 'string' && item.id ? item.id : createClashApiId(),
        name: typeof item.name === 'string' && item.name.trim() ? item.name.trim() : 'API',
        url: typeof item.url === 'string' && item.url.trim() ? item.url.trim() : 'http://127.0.0.1:9090',
        secret: typeof item.secret === 'string' ? item.secret : '',
      }))
    : []

  const legacyUrl = typeof raw?.clashApiUrl === 'string' && raw.clashApiUrl.trim()
    ? raw.clashApiUrl.trim()
    : 'http://127.0.0.1:9090'
  const legacySecret = typeof raw?.clashApiSecret === 'string' ? raw.clashApiSecret : ''

  const clashApis: ClashApiProfile[] = savedProfiles.length > 0
    ? savedProfiles
    : [createClashApiProfile('默认 API', legacyUrl, legacySecret)]

  const activeClashApiId =
    typeof raw?.activeClashApiId === 'string'
    && clashApis.some((api) => api.id === raw.activeClashApiId)
      ? raw.activeClashApiId
      : clashApis[0].id

  // 配置文件列表
  const configProfiles: ConfigProfile[] = Array.isArray(raw?.configProfiles)
    ? raw.configProfiles
      .filter((item: any) => item && typeof item === 'object' && typeof item.id === 'string')
      .map((item: any): ConfigProfile => ({
        id: item.id,
        name: typeof item.name === 'string' && item.name.trim() ? item.name.trim() : '未命名配置',
        type: item.type === 'remote' ? 'remote' : 'local',
        source: typeof item.source === 'string' ? normalizeWindowsPath(item.source) : '',
        ...(typeof item.lastUpdated === 'string' ? { lastUpdated: item.lastUpdated } : {}),
        ...(typeof item.lastFetchError === 'string' && item.lastFetchError ? { lastFetchError: item.lastFetchError } : {}),
        ...(typeof item.lastFetchFailedAt === 'string' ? { lastFetchFailedAt: item.lastFetchFailedAt } : {}),
        ...(typeof item.lastLocalChangeAt === 'string' && item.lastLocalChangeAt ? { lastLocalChangeAt: item.lastLocalChangeAt } : {}),
        ...(typeof item.backupUpdatedAt === 'string' && item.backupUpdatedAt ? { backupUpdatedAt: item.backupUpdatedAt } : {}),
        autoUpdateInterval: typeof item.autoUpdateInterval === 'number' && item.autoUpdateInterval >= 0
          ? Math.min(576, Math.round(item.autoUpdateInterval))
          : 0,
      }))
    : []

  // 迁移兼容：旧 configPath 自动创建本地配置
  const legacyConfigPath = IS_ANDROID ? '' : normalizeWindowsPath(raw?.configPath)
  if (configProfiles.length === 0 && legacyConfigPath) {
    configProfiles.push({
      id: createConfigProfileId(),
      name: '默认配置',
      type: 'local',
      source: legacyConfigPath,
      autoUpdateInterval: 0,
    })
  }

  const activeConfigProfileId =
    typeof raw?.activeConfigProfileId === 'string'
    && (raw.activeConfigProfileId === '' || configProfiles.some((p) => p.id === raw.activeConfigProfileId))
      ? raw.activeConfigProfileId
      : configProfiles.length > 0 ? configProfiles[0].id : ''

  return {
    clashApis,
    activeClashApiId,
    singboxPath: IS_ANDROID ? normalizePath(raw?.singboxPath) : normalizeWindowsPath(raw?.singboxPath),
    coreInstallPath: IS_ANDROID ? normalizePath(raw?.coreInstallPath) : '',
    configPath: IS_ANDROID ? normalizePath(raw?.configPath) : '',
    rulesDir: IS_ANDROID ? normalizePath(raw?.rulesDir) : '',
    workingDir: IS_ANDROID ? normalizePath(raw?.workingDir) : normalizeWindowsPath(raw?.workingDir),
    startupDelaySeconds: normalizeStartupDelay(raw?.startupDelaySeconds),
    theme: ['auto', 'light', 'dark'].includes(raw?.theme) ? raw.theme : 'light',
    glassTint: normalizeGlassTint(raw?.glassTint, raw?.glassMode),
    latencyTestUrl: typeof raw?.latencyTestUrl === 'string' && raw.latencyTestUrl
      ? raw.latencyTestUrl
      : 'https://www.gstatic.com/generate_204',
    ipv6TestEnabled: typeof raw?.ipv6TestEnabled === 'boolean' ? raw.ipv6TestEnabled : false,
    groupTestUrls: raw?.groupTestUrls && typeof raw.groupTestUrls === 'object' && !Array.isArray(raw.groupTestUrls)
      ? Object.fromEntries(
          Object.entries(raw.groupTestUrls as Record<string, unknown>).filter((e): e is [string, string] => typeof e[1] === 'string' && e[1].length > 0)
        )
      : {},
    configProfiles,
    activeConfigProfileId,
    closeToTray: typeof raw?.closeToTray === 'boolean' ? raw.closeToTray : false,
    rootMode: IS_ANDROID && typeof raw?.rootMode === 'boolean' ? raw.rootMode : false,
    setupCompleted: typeof raw?.setupCompleted === 'boolean' ? raw.setupCompleted : false,
    pageSwipe: typeof raw?.pageSwipe === 'boolean' ? raw.pageSwipe : false,
    coreUpdateSource: raw?.coreUpdateSource === 'ref1nd' || raw?.coreUpdateSource === 'custom'
      ? raw.coreUpdateSource
      : 'official',
    coreUpdateCustomRepo: typeof raw?.coreUpdateCustomRepo === 'string' ? raw.coreUpdateCustomRepo.trim() : '',
    coreUpdateChannel: raw?.coreUpdateChannel === 'testing' ? 'testing' : 'stable',
    coreUpdateMirror: typeof raw?.coreUpdateMirror === 'string' ? raw.coreUpdateMirror.trim() : '',
    panelAutoCheckUpdate: typeof raw?.panelAutoCheckUpdate === 'boolean' ? raw.panelAutoCheckUpdate : true,
  }
}

let storedConfig: string | null = null

function loadConfig(): AppConfig {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    storedConfig = saved
    if (saved) {
      return normalizeConfig(JSON.parse(saved))
    }
  } catch { }
  return normalizeConfig({})
}

const config = ref<AppConfig>(loadConfig())
let syncingStorage = false

function refreshConfigFromStorage() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === storedConfig) return
    const nextConfig = normalizeConfig(saved ? JSON.parse(saved) : {})
    storedConfig = saved
    syncingStorage = true
    config.value = nextConfig
  } catch {
  } finally {
    syncingStorage = false
  }
}

window.addEventListener('storage', (event) => {
  if (event.storageArea !== localStorage) return
  if (event.key !== STORAGE_KEY && event.key !== null) return
  refreshConfigFromStorage()
})

// auto 主题：跟随系统明亮/暗黑模式
const systemDarkQuery = window.matchMedia('(prefers-color-scheme: dark)')
const systemDark = ref(systemDarkQuery.matches)
systemDarkQuery.addEventListener('change', (e) => {
  systemDark.value = e.matches
})

const resolvedTheme = computed(() =>
  config.value.theme === 'auto'
    ? (systemDark.value ? 'dark' : 'light')
    : config.value.theme,
)

function applyTheme(theme: string) {
  document.documentElement.setAttribute('data-theme', theme)
}

applyTheme(resolvedTheme.value)

watch(resolvedTheme, (val) => {
  applyTheme(val)
})

applyGlassTint(config.value.glassTint)

watch(() => config.value.glassTint, (val) => {
  applyGlassTint(val)
})

watch(config, (val) => {
  if (syncingStorage) return
  const saved = JSON.stringify(val)
  localStorage.setItem(STORAGE_KEY, saved)
  storedConfig = saved
}, { deep: true, flush: 'sync' })

// 同步 closeToTray 状态到 Rust 后端
if (!IS_ANDROID) {
  invoke('set_close_to_tray', { enabled: config.value.closeToTray }).catch(() => {})
  watch(() => config.value.closeToTray, (val) => {
    invoke('set_close_to_tray', { enabled: val }).catch(() => {})
  })
}

export const rootReady: Promise<void> = IS_ANDROID
  ? setRootEnabled(config.value.rootMode).catch(() => {})
  : Promise.resolve()

if (IS_ANDROID) {
  watch(() => config.value.rootMode, (val) => {
    setRootEnabled(val).catch(() => {})
  })
}

export function useConfigStore() {
  const clashApis = computed(() => config.value.clashApis)
  const activeClashApiId = computed(() => config.value.activeClashApiId)
  const activeClashApi = computed(() =>
    config.value.clashApis.find((api) => api.id === config.value.activeClashApiId)
    ?? config.value.clashApis[0],
  )
  const clashApiUrl = computed(() => activeClashApi.value?.url ?? '')
  const clashApiSecret = computed(() => activeClashApi.value?.secret ?? '')
  const clashApiEndpoint = computed(() => `${clashApiUrl.value}\n${clashApiSecret.value}`)

  const configProfiles = computed(() => config.value.configProfiles)
  const activeConfigProfileId = computed(() => config.value.activeConfigProfileId)
  const activeConfigProfile = computed(() =>
    config.value.configProfiles.find((p) => p.id === config.value.activeConfigProfileId),
  )

  function updateConfig(partial: Partial<AppConfig>) {
    config.value = normalizeConfig({ ...config.value, ...partial })
  }

  function setActiveClashApi(id: string) {
    if (config.value.clashApis.some((api) => api.id === id)) {
      config.value.activeClashApiId = id
    }
  }

  function addClashApi(name: string, url: string, secret: string): string {
    const profile = createClashApiProfile(name, url, secret)
    config.value.clashApis.push(profile)
    return profile.id
  }

  function updateActiveClashApi(partial: Partial<Omit<ClashApiProfile, 'id'>>) {
    const current = activeClashApi.value
    if (!current) return
    if (typeof partial.name === 'string') current.name = partial.name
    if (typeof partial.url === 'string') current.url = partial.url
    if (typeof partial.secret === 'string') current.secret = partial.secret
  }

  function removeClashApi(id: string): boolean {
    if (config.value.clashApis.length <= 1) return false
    const index = config.value.clashApis.findIndex((api) => api.id === id)
    if (index === -1) return false
    const removingActive = config.value.activeClashApiId === id
    config.value.clashApis.splice(index, 1)
    if (removingActive) {
      config.value.activeClashApiId = config.value.clashApis[0].id
    }
    return true
  }

  function setSingleClashApi(url: string, secret: string, name = '默认 API') {
    const profile = createClashApiProfile(name, url, secret)
    config.value.clashApis = [profile]
    config.value.activeClashApiId = profile.id
  }

  function addConfigProfile(name: string, type: 'local' | 'remote', source: string, autoUpdateInterval = 0, id = createConfigProfileId()): string {
    const profile: ConfigProfile = {
      id,
      name,
      type,
      source,
      lastUpdated: new Date().toISOString(),
      autoUpdateInterval,
    }
    config.value.configProfiles.push(profile)
    return profile.id
  }

  function removeConfigProfile(id: string): boolean {
    const index = config.value.configProfiles.findIndex((p) => p.id === id)
    if (index === -1) return false
    config.value.configProfiles.splice(index, 1)
    if (config.value.activeConfigProfileId === id) {
      config.value.activeConfigProfileId = ''
    }
    return true
  }

  function setActiveConfigProfile(id: string) {
    if (config.value.configProfiles.some((p) => p.id === id)) {
      config.value.activeConfigProfileId = id
    }
  }

  function updateConfigProfile(id: string, partial: Partial<Omit<ConfigProfile, 'id'>>) {
    const profile = config.value.configProfiles.find((p) => p.id === id)
    if (!profile) return
    if (typeof partial.name === 'string') profile.name = partial.name
    if (typeof partial.source === 'string') profile.source = partial.source
    if (typeof partial.type === 'string') profile.type = partial.type
    if (typeof partial.lastUpdated === 'string') profile.lastUpdated = partial.lastUpdated
    if (typeof partial.autoUpdateInterval === 'number') profile.autoUpdateInterval = partial.autoUpdateInterval
    for (const key of ['lastLocalChangeAt', 'backupUpdatedAt'] as const) {
      if (!(key in partial)) continue
      const value = partial[key]
      if (value) profile[key] = value
      else delete profile[key]
    }
    if ('lastFetchError' in partial) {
      if (partial.lastFetchError) {
        profile.lastFetchError = partial.lastFetchError
        profile.lastFetchFailedAt = partial.lastFetchFailedAt ?? new Date().toISOString()
      } else {
        delete profile.lastFetchError
        delete profile.lastFetchFailedAt
      }
    }
  }

  return {
    config,
    resolvedTheme,
    clashApis,
    activeClashApi,
    activeClashApiId,
    clashApiUrl,
    clashApiSecret,
    clashApiEndpoint,
    configProfiles,
    activeConfigProfileId,
    activeConfigProfile,
    refreshConfigFromStorage,
    updateConfig,
    setActiveClashApi,
    addClashApi,
    updateActiveClashApi,
    removeClashApi,
    setSingleClashApi,
    addConfigProfile,
    removeConfigProfile,
    setActiveConfigProfile,
    updateConfigProfile,
  }
}
