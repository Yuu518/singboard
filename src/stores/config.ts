import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { AppConfig, ClashApiProfile, ConfigProfile } from '@/types'

const STORAGE_KEY = 'singboard-config'

function createClashApiId(): string {
  return `api_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
}

function createConfigProfileId(): string {
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
        autoUpdateInterval: typeof item.autoUpdateInterval === 'number' && item.autoUpdateInterval >= 0 ? item.autoUpdateInterval : 0,
      }))
    : []

  // 迁移兼容：旧 configPath 自动创建本地配置
  const legacyConfigPath = normalizeWindowsPath(raw?.configPath)
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
    && configProfiles.some((p) => p.id === raw.activeConfigProfileId)
      ? raw.activeConfigProfileId
      : configProfiles.length > 0 ? configProfiles[0].id : ''

  return {
    clashApis,
    activeClashApiId,
    singboxPath: normalizeWindowsPath(raw?.singboxPath),
    workingDir: normalizeWindowsPath(raw?.workingDir),
    serviceName: typeof raw?.serviceName === 'string' && raw.serviceName ? raw.serviceName : 'sing-box',
    startupDelaySeconds: normalizeStartupDelay(raw?.startupDelaySeconds),
    theme: typeof raw?.theme === 'string' && raw.theme ? raw.theme : 'light',
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
  }
}

function loadConfig(): AppConfig {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved) {
      return normalizeConfig(JSON.parse(saved))
    }
  } catch { }
  return normalizeConfig({})
}

const config = ref<AppConfig>(loadConfig())

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

watch(config, (val) => {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(val))
}, { deep: true })

// 同步 closeToTray 状态到 Rust 后端
invoke('set_close_to_tray', { enabled: config.value.closeToTray }).catch(() => {})
watch(() => config.value.closeToTray, (val) => {
  invoke('set_close_to_tray', { enabled: val }).catch(() => {})
})

export function useConfigStore() {
  const clashApis = computed(() => config.value.clashApis)
  const activeClashApiId = computed(() => config.value.activeClashApiId)
  const activeClashApi = computed(() =>
    config.value.clashApis.find((api) => api.id === config.value.activeClashApiId)
    ?? config.value.clashApis[0],
  )
  const clashApiUrl = computed(() => activeClashApi.value?.url ?? '')
  const clashApiSecret = computed(() => activeClashApi.value?.secret ?? '')
  const serviceName = computed(() => config.value.serviceName)

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

  function addConfigProfile(name: string, type: 'local' | 'remote', source: string, autoUpdateInterval = 0): string {
    const profile: ConfigProfile = {
      id: createConfigProfileId(),
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
      config.value.activeConfigProfileId = config.value.configProfiles.length > 0
        ? config.value.configProfiles[0].id
        : ''
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
  }

  return {
    config,
    resolvedTheme,
    clashApis,
    activeClashApi,
    activeClashApiId,
    clashApiUrl,
    clashApiSecret,
    serviceName,
    configProfiles,
    activeConfigProfileId,
    activeConfigProfile,
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
