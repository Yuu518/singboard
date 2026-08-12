<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { useProxiesStore } from '@/stores/proxies'
import {
  stopService,
  installService,
  uninstallService,
  readServiceErrorLog,
  startupTaskExists,
  createStartupTask,
} from '@/bridge/service'
import { startCore, restartCore } from '@/utils/coreControl'
import { getRunningConfigPath } from '@/bridge/config'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { getAutoLaunch, setAutoLaunch } from '@/bridge/app'
import { open } from '@tauri-apps/plugin-dialog'
import { patchConfig, fetchConfig } from '@/api'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import OverflowingText from '@/components/common/OverflowingText.vue'
import DnsQueryTool from '@/components/settings/DnsQueryTool.vue'
import CoreUpdateCard from '@/components/settings/CoreUpdateCard.vue'
import PanelUpdateCard from '@/components/settings/PanelUpdateCard.vue'

const {
  config,
  updateConfig,
  clashApis,
  activeClashApi,
  activeClashApiId,
  setActiveClashApi,
  addClashApi,
  updateActiveClashApi,
  removeClashApi,
} = useConfigStore()
const { serviceStatus, statusText, refresh } = useServiceStore()
const { pushToast } = useToastStore()
const confirmDialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)

const settingsRootRef = ref<HTMLElement | null>(null)
const activeSection = ref('core')
const settingsSections = [
  { id: 'core', label: '核心与服务', hint: '运行与模式' },
  { id: 'backends', label: '后端', hint: '控制端点' },
  { id: 'network', label: '网络测试', hint: '测速与 DNS' },
  { id: 'application', label: '应用', hint: '外观与行为' },
  { id: 'updates', label: '更新', hint: '核心与面板' },
] as const

const themeOptions = [
  { value: 'auto', label: '跟随系统', hint: '随 Windows 外观切换' },
  { value: 'light', label: '浅色', hint: '明亮清晰' },
  { value: 'dark', label: '深色', hint: '低光环境' },
] as const

let settingsScrollRoot: HTMLElement | null = null
let scrollFrame = 0

function updateActiveSection() {
  scrollFrame = 0
  if (
    settingsScrollRoot
    && settingsScrollRoot.scrollTop + settingsScrollRoot.clientHeight >= settingsScrollRoot.scrollHeight - 8
  ) {
    activeSection.value = settingsSections[settingsSections.length - 1].id
    return
  }
  const rootTop = settingsScrollRoot?.getBoundingClientRect().top ?? 0
  const threshold = rootTop + 88
  let current: (typeof settingsSections)[number]['id'] = settingsSections[0].id

  for (const section of settingsSections) {
    const element = document.getElementById(`settings-${section.id}`)
    if (element && element.getBoundingClientRect().top <= threshold) {
      current = section.id
    }
  }
  activeSection.value = current
}

function scheduleActiveSectionUpdate() {
  if (!scrollFrame) scrollFrame = requestAnimationFrame(updateActiveSection)
}

function scrollToSection(id: string) {
  activeSection.value = id
  document.getElementById(`settings-${id}`)?.scrollIntoView({
    behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
    block: 'start',
  })
}

onMounted(() => {
  settingsScrollRoot = settingsRootRef.value?.closest('main') as HTMLElement | null
  settingsScrollRoot?.addEventListener('scroll', scheduleActiveSectionUpdate, { passive: true })
  updateActiveSection()
})

onBeforeUnmount(() => {
  settingsScrollRoot?.removeEventListener('scroll', scheduleActiveSectionUpdate)
  if (scrollFrame) cancelAnimationFrame(scrollFrame)
})

const { proxyGroups, loadProxies } = useProxiesStore()

const groupTestUrlsExpanded = ref(false)
const newGroupTestUrl = ref({ group: '', url: '' })
const editingGroupTestUrl = ref<string | null>(null)
const editGroupTestUrlGroup = ref('')
const editGroupTestUrlValue = ref('')

const groupTestUrlEntries = computed(() =>
  Object.entries(config.value.groupTestUrls)
)

const availableGroups = computed(() =>
  proxyGroups.value
    .filter((g) => g.name !== 'GLOBAL' && !config.value.groupTestUrls[g.name])
    .map((g) => g.name)
)

const editAvailableGroups = computed(() =>
  proxyGroups.value
    .filter((g) => g.name !== 'GLOBAL' && (g.name === editingGroupTestUrl.value || !config.value.groupTestUrls[g.name]))
    .map((g) => g.name)
)

function addGroupTestUrl() {
  const group = newGroupTestUrl.value.group.trim()
  const url = newGroupTestUrl.value.url.trim()
  if (!group || !url) return
  config.value.groupTestUrls = { ...config.value.groupTestUrls, [group]: url }
  newGroupTestUrl.value = { group: '', url: '' }
}

function startEditGroupTestUrl(group: string) {
  editingGroupTestUrl.value = group
  editGroupTestUrlGroup.value = group
  editGroupTestUrlValue.value = config.value.groupTestUrls[group] ?? ''
}

function saveEditGroupTestUrl() {
  const oldGroup = editingGroupTestUrl.value
  if (!oldGroup) return
  const newGroup = editGroupTestUrlGroup.value.trim()
  const url = editGroupTestUrlValue.value.trim()
  if (!newGroup || !url) return
  const { [oldGroup]: _, ...rest } = config.value.groupTestUrls
  config.value.groupTestUrls = { ...rest, [newGroup]: url }
  editingGroupTestUrl.value = null
}

function removeGroupTestUrl(group: string) {
  const { [group]: _, ...rest } = config.value.groupTestUrls
  config.value.groupTestUrls = rest
  if (editingGroupTestUrl.value === group) editingGroupTestUrl.value = null
}

const autoLaunchEnabled = ref(false)
getAutoLaunch().then((v) => { autoLaunchEnabled.value = v }).catch(() => {})

async function toggleAutoLaunch(e: Event) {
  const target = e.target as HTMLInputElement
  const enabled = target.checked
  try {
    await setAutoLaunch(enabled)
    autoLaunchEnabled.value = enabled
  } catch (err: any) {
    target.checked = autoLaunchEnabled.value
    pushToast({ message: '设置开机自启失败: ' + (err?.message || err), type: 'error' }, 6000)
  }
}

const clashMode = ref('Rule')
const clashModeOptions = ref<string[]>(['Rule'])
const { singboxVersion } = useSingboxVersionStore()
const actionLoading = ref('')
const showServiceConfigPanel = ref(false)
const startupTaskSyncing = ref(false)

function normalizeStartupDelayValue(value: unknown): number {
  const delay = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(delay)) return 30
  return Math.min(3600, Math.max(0, Math.round(delay)))
}
function parseApiUrl(url: string) {
  const match = url.match(/^(https?):\/\/([^:]+)(?::(\d+))?$/)
  if (match) return { protocol: match[1] as 'http' | 'https', host: match[2], port: match[3] ?? '' }
  return { protocol: 'http' as const, host: url, port: '' }
}

const activeApiForm = ref({
  name: '',
  protocol: 'http' as 'http' | 'https',
  host: '',
  port: '',
  secret: '',
})
const newApiForm = ref({
  name: '',
  protocol: 'http' as 'http' | 'https',
  host: '',
  port: '',
  secret: '',
})
const showEditApiForm = ref(false)
const showAddApiForm = ref(false)

function syncActiveApiForm() {
  const current = activeClashApi.value
  const { protocol, host, port } = parseApiUrl(current?.url ?? '')
  activeApiForm.value = {
    name: current?.name ?? '',
    protocol,
    host,
    port,
    secret: current?.secret ?? '',
  }
}

function toggleEditApiForm() {
  showEditApiForm.value = !showEditApiForm.value
  if (showEditApiForm.value) {
    syncActiveApiForm()
    showAddApiForm.value = false
  }
}

function toggleAddApiForm() {
  showAddApiForm.value = !showAddApiForm.value
  if (showAddApiForm.value) {
    newApiForm.value = { name: '', protocol: 'http', host: '', port: '', secret: '' }
    showEditApiForm.value = false
  }
}

function handleSwitchApi(id: string) {
  setActiveClashApi(id)
  syncActiveApiForm()
  refresh()
  loadClashConfig()
}

function handleSaveActiveApi() {
  const host = activeApiForm.value.host.trim()
  if (!host) {
    pushToast({ message: '请填写当前后端主机地址。', type: 'error' })
    return
  }
  const port = activeApiForm.value.port.trim()
  const name = activeApiForm.value.name.trim() || '后端'
  const url = `${activeApiForm.value.protocol}://${host}${port ? ':' + port : ''}`
  updateActiveClashApi({
    name,
    url,
    secret: activeApiForm.value.secret,
  })
  showEditApiForm.value = false
  refresh()
  loadClashConfig()
}

function handleAddApi() {
  const host = newApiForm.value.host.trim()
  if (!host) {
    pushToast({ message: '请填写新增后端主机地址。', type: 'error' })
    return
  }
  const port = newApiForm.value.port.trim()
  const name = newApiForm.value.name.trim() || `后端 ${clashApis.value.length + 1}`
  const url = `${newApiForm.value.protocol}://${host}${port ? ':' + port : ''}`
  const id = addClashApi(name, url, newApiForm.value.secret)
  setActiveClashApi(id)
  syncActiveApiForm()
  newApiForm.value = { name: '', protocol: 'http', host: '', port: '', secret: '' }
  showAddApiForm.value = false
  refresh()
  loadClashConfig()
}

async function handleRemoveActiveApi() {
  const current = activeClashApi.value
  if (!current) return
  if (clashApis.value.length <= 1) {
    pushToast({ message: '至少保留一个后端。', type: 'error' })
    return
  }
  const confirmed = await confirmDialogRef.value?.show({
    title: '删除后端',
    message: `确定删除后端：${current.name} ?`,
    confirmText: '删除',
    variant: 'danger',
  })
  if (!confirmed) return
  removeClashApi(current.id)
  syncActiveApiForm()
  showEditApiForm.value = false
  refresh()
  loadClashConfig()
}

function parseModeOptions(data: any): string[] {
  const modeList = Array.isArray(data?.['mode-list'])
    ? data['mode-list']
    : Array.isArray(data?.modes)
      ? data.modes
      : []
  const options = modeList.filter((mode: unknown): mode is string => typeof mode === 'string' && mode.length > 0)
  return options.length > 0 ? options : ['Rule']
}

async function loadClashConfig() {
  try {
    const { data } = await fetchConfig()
    const currentMode = typeof data.mode === 'string' && data.mode ? data.mode : 'Rule'
    const modeOptions = parseModeOptions(data)
    const matchedCurrent = modeOptions.find((mode) => mode.toLowerCase() === currentMode.toLowerCase())
    clashModeOptions.value = matchedCurrent ? modeOptions : [currentMode, ...modeOptions]
    clashMode.value = matchedCurrent ?? currentMode
  } catch {}
}

async function changeMode(mode: string) {
  try {
    await patchConfig({ mode } as any)
    await loadClashConfig()
  } catch {}
}

async function checkServiceAfterStart() {
  await new Promise((r) => setTimeout(r, 3000))
  await refresh()
  if (serviceStatus.value.state !== 'running') {
    // 服务未运行，尝试读取错误日志
    let detail = ''
    try {
      detail = await readServiceErrorLog(config.value.serviceName)
    } catch {}
    const msg = detail
      ? '服务启动失败:\n' + detail
      : '服务启动失败或异常退出，请检查配置文件'
    pushToast({ message: msg, type: 'error' }, 10000)
    return
  }
  // 服务显示 running，再验证后端是否可达
  try {
    await fetchConfig()
  } catch {
    pushToast({
      message: '服务进程已启动但无法连接后端，核心可能未正常运行，请检查配置文件',
      type: 'error',
    }, 8000)
  }
}

async function handleServiceAction(action: string) {
  actionLoading.value = action
  try {
    const name = config.value.serviceName
    switch (action) {
      case 'start':
        await startCore(name)
        checkServiceAfterStart()
        break
      case 'restart':
        await restartCore(name)
        checkServiceAfterStart()
        break
      case 'stop': await stopService(name); break
      case 'install': {
        const runningConfigPath = await getRunningConfigPath()
        const startupDelaySeconds = normalizeStartupDelayValue(config.value.startupDelaySeconds)
        config.value.startupDelaySeconds = startupDelaySeconds
        await installService(
          name,
          config.value.singboxPath,
          runningConfigPath,
          config.value.workingDir,
          startupDelaySeconds,
        )
        break
      }
      case 'uninstall': await uninstallService(name); break
    }
    setTimeout(refresh, 1000)
  } catch (e: any) {
    pushToast({ message: '操作失败: ' + (e?.message || e), type: 'error' }, 6000)
  } finally {
    actionLoading.value = ''
  }
}

async function browseSingboxPath() {
  const selected = await open({
    multiple: false,
    filters: [{ name: '可执行文件', extensions: ['exe'] }],
    defaultPath: config.value.workingDir.trim() || undefined,
  })
  if (selected) {
    config.value.singboxPath = selected as string
  }
}

async function browseWorkingDir() {
  const selected = await open({
    directory: true,
    defaultPath: config.value.workingDir.trim() || undefined,
  })
  if (selected) {
    config.value.workingDir = selected as string
  }
}


function updateStartupDelay() {
  config.value.startupDelaySeconds = normalizeStartupDelayValue(config.value.startupDelaySeconds)
}

async function syncStartupDelayToTask() {
  const serviceName = config.value.serviceName.trim()
  if (!serviceName || startupTaskSyncing.value) return

  startupTaskSyncing.value = true
  try {
    if (await startupTaskExists(serviceName)) {
      await createStartupTask(serviceName, config.value.startupDelaySeconds)
      pushToast({ message: `自启延迟已同步为 ${config.value.startupDelaySeconds} 秒`, type: 'info' })
    }
  } catch (e: any) {
    pushToast({ message: '同步自启延迟失败: ' + (e?.message || e), type: 'error' }, 6000)
  } finally {
    startupTaskSyncing.value = false
  }
}

const statusColor = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return 'badge-success'
    case 'stopped': return 'badge-error'
    default: return 'badge-warning'
  }
})

const serviceStateTone = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return 'is-running'
    case 'stopped': return 'is-stopped'
    case 'starting':
    case 'stopping': return 'is-pending'
    case 'not_installed': return 'is-unavailable'
    default: return 'is-unknown'
  }
})

const serviceStateDescription = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return '核心正在接管并处理网络流量。'
    case 'stopped': return '核心已停止，当前不会处理新流量。'
    case 'starting': return '正在加载配置并启动核心。'
    case 'stopping': return '正在安全停止核心服务。'
    case 'not_installed': return '先安装 Windows 服务，再启动核心。'
    default: return '暂时无法读取 Windows 服务状态。'
  }
})

loadClashConfig()
syncActiveApiForm()
if (serviceStatus.value.state === 'running') {
  loadProxies()
}

watch(
  () => serviceStatus.value.state,
  (newState, oldState) => {
    if (newState === 'running' && oldState !== 'running') {
      loadClashConfig()
      loadProxies()
    }
  }
)

watch(
  () => activeClashApiId.value,
  () => {
    syncActiveApiForm()
  },
)
</script>

<template>
  <div ref="settingsRootRef" class="settings-page">
    <ConfirmDialog ref="confirmDialogRef" />

    <header class="settings-header">
      <div>
        <div class="settings-eyebrow">ROUTE CONTROL</div>
        <h1 class="settings-title">设置</h1>
        <p class="settings-lead">控制 sing-box 核心、连接端点与面板行为。</p>
      </div>
      <div class="settings-instant-chip" title="配置更改会立即写入本机">
        <span class="settings-instant-dot"></span>
        更改即时生效
      </div>
    </header>

    <div class="settings-status-strip" :class="serviceStateTone">
      <div class="settings-status-primary">
        <span class="settings-status-beacon" aria-hidden="true"></span>
        <div>
          <span class="settings-status-kicker">WINDOWS SERVICE</span>
          <strong>{{ statusText }}</strong>
        </div>
      </div>
      <div class="settings-status-fact">
        <span>核心版本</span>
        <strong class="settings-mono settings-status-version">
          <OverflowingText :text="singboxVersion || '未检测'" />
        </strong>
      </div>
      <div class="settings-status-fact">
        <span>控制端点</span>
        <strong>{{ activeClashApi?.name || '未配置' }}</strong>
      </div>
      <div class="settings-status-fact">
        <span>路由模式</span>
        <strong class="settings-mono">{{ clashMode }}</strong>
      </div>
    </div>

    <div class="settings-layout">
      <aside class="settings-route-nav" aria-label="设置分类">
        <div class="settings-route-list">
          <button
            v-for="section in settingsSections"
            :key="section.id"
            type="button"
            class="settings-route-link"
            :class="{ 'is-active': activeSection === section.id }"
            :aria-current="activeSection === section.id ? 'location' : undefined"
            @click="scrollToSection(section.id)"
          >
            <span>
              <strong>{{ section.label }}</strong>
              <small>{{ section.hint }}</small>
            </span>
          </button>
        </div>
      </aside>

      <div class="settings-content">
        <section id="settings-core" class="settings-section">
          <div class="settings-section-heading">
            <span class="settings-section-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none">
                <path d="M12 3v3m0 12v3M3 12h3m12 0h3M5.6 5.6l2.1 2.1m8.6 8.6 2.1 2.1m0-12.8-2.1 2.1m-8.6 8.6-2.1 2.1" />
                <circle cx="12" cy="12" r="4" />
              </svg>
            </span>
            <div>
              <h2>核心与服务</h2>
              <p>运行核心、切换路由模式，并维护 Windows 服务。</p>
            </div>
          </div>

          <div class="settings-service-console" :class="serviceStateTone">
            <div class="settings-service-copy">
              <div class="settings-service-title">
                <span class="settings-service-pulse" aria-hidden="true"></span>
                <strong>{{ statusText }}</strong>
                <span class="badge badge-sm" :class="statusColor">{{ config.serviceName }}</span>
              </div>
              <p>{{ serviceStateDescription }}</p>
            </div>
            <div class="settings-service-actions">
              <button
                class="btn btn-sm btn-route"
                :class="{ loading: actionLoading === 'start' }"
                :disabled="serviceStatus.state === 'running'"
                @click="handleServiceAction('start')"
              >
                启动
              </button>
              <button
                class="btn btn-sm btn-outline"
                :class="{ loading: actionLoading === 'restart' }"
                @click="handleServiceAction('restart')"
              >
                重启
              </button>
              <button
                class="btn btn-sm btn-outline btn-error"
                :class="{ loading: actionLoading === 'stop' }"
                :disabled="serviceStatus.state === 'stopped'"
                @click="handleServiceAction('stop')"
              >
                停止
              </button>
            </div>
          </div>

          <div class="settings-card">
            <div class="settings-row">
              <div class="settings-row-copy">
                <strong>代理模式</strong>
                <span>切换核心当前采用的流量处理策略。</span>
              </div>
              <select
                class="select select-sm select-bordered settings-row-control"
                :value="clashMode"
                aria-label="代理模式"
                @change="changeMode(($event.target as HTMLSelectElement).value)"
              >
                <option v-for="mode in clashModeOptions" :key="mode" :value="mode">{{ mode }}</option>
              </select>
            </div>

            <div class="settings-row settings-row-stack">
              <button
                type="button"
                class="settings-disclosure"
                :aria-expanded="showServiceConfigPanel"
                @click="showServiceConfigPanel = !showServiceConfigPanel"
              >
                <span class="settings-row-copy">
                  <strong>服务参数</strong>
                  <span>服务名称、核心路径、工作目录与启动延迟。</span>
                </span>
                <svg viewBox="0 0 20 20" fill="none" :class="{ 'rotate-180': showServiceConfigPanel }" aria-hidden="true">
                  <path d="m5 8 5 5 5-5" />
                </svg>
              </button>

              <Transition name="settings-reveal">
                <div v-if="showServiceConfigPanel" class="settings-inline-panel">
                  <div class="settings-field-grid">
                    <label class="settings-field">
                      <span>服务名称</span>
                      <input v-model="config.serviceName" type="text" class="input input-sm input-bordered" placeholder="sing-box" />
                    </label>
                    <label class="settings-field settings-field-compact">
                      <span>延迟启动</span>
                      <div class="settings-input-unit">
                        <input
                          v-model.number="config.startupDelaySeconds"
                          type="number"
                          min="0"
                          max="3600"
                          step="1"
                          class="input input-sm input-bordered"
                          @change="updateStartupDelay(); syncStartupDelayToTask()"
                        />
                        <span>秒</span>
                      </div>
                    </label>
                    <label class="settings-field settings-field-wide">
                      <span>sing-box 可执行文件</span>
                      <div class="settings-path-control">
                        <input
                          v-model="config.singboxPath"
                          type="text"
                          class="input input-sm input-bordered settings-mono"
                          placeholder="C:\sing-box\sing-box.exe"
                        />
                        <button type="button" class="btn btn-sm btn-outline" @click="browseSingboxPath">浏览</button>
                      </div>
                    </label>
                    <label class="settings-field settings-field-wide">
                      <span>工作目录</span>
                      <div class="settings-path-control">
                        <input
                          v-model="config.workingDir"
                          type="text"
                          class="input input-sm input-bordered settings-mono"
                          placeholder="留空则使用配置文件所在目录"
                        />
                        <button type="button" class="btn btn-sm btn-outline" @click="browseWorkingDir">浏览</button>
                      </div>
                    </label>
                  </div>
                </div>
              </Transition>
            </div>

            <div class="settings-row settings-maintenance-row">
              <div class="settings-row-copy">
                <strong>Windows 服务</strong>
                <span>安装后，核心可独立于面板在后台运行。</span>
              </div>
              <div class="settings-row-actions">
                <button
                  class="btn btn-sm btn-outline"
                  :class="{ loading: actionLoading === 'install' }"
                  :disabled="serviceStatus.state !== 'not_installed'"
                  @click="handleServiceAction('install')"
                >
                  安装服务
                </button>
                <button
                  class="btn btn-sm btn-ghost settings-danger-action"
                  :class="{ loading: actionLoading === 'uninstall' }"
                  :disabled="serviceStatus.state === 'not_installed'"
                  @click="handleServiceAction('uninstall')"
                >
                  卸载
                </button>
              </div>
            </div>
          </div>
        </section>

        <section id="settings-backends" class="settings-section">
          <div class="settings-section-heading">
            <span class="settings-section-icon settings-section-icon-signal" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none">
                <circle cx="6" cy="12" r="2.5" />
                <circle cx="18" cy="6" r="2.5" />
                <circle cx="18" cy="18" r="2.5" />
                <path d="m8.3 10.9 7.4-3.8M8.3 13.1l7.4 3.8" />
              </svg>
            </span>
            <div>
              <h2>后端</h2>
              <p>选择面板连接的 Clash API 控制端点。</p>
            </div>
          </div>

          <div class="settings-card settings-card-padded">
            <div class="settings-card-label-row">
              <span class="settings-field-label">当前控制端点</span>
              <span class="settings-endpoint-count">{{ clashApis.length }} 个端点</span>
            </div>
            <div class="settings-endpoint-picker">
              <select
                class="select select-sm select-bordered settings-mono"
                :value="activeClashApiId"
                @change="handleSwitchApi(($event.target as HTMLSelectElement).value)"
              >
                <option v-for="api in clashApis" :key="api.id" :value="api.id">
                  {{ api.name }} · {{ api.url }}
                </option>
              </select>
              <button
                type="button"
                class="btn btn-sm btn-square btn-outline"
                :class="{ 'is-selected': showEditApiForm }"
                aria-label="编辑当前后端"
                title="编辑当前后端"
                @click="toggleEditApiForm"
              >
                <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                  <path d="m13.8 3.7 2.5 2.5M4 16l.7-3.3L13.9 3.5a1.8 1.8 0 0 1 2.6 0l.1.1a1.8 1.8 0 0 1 0 2.6l-9.2 9.2L4 16Z" />
                </svg>
              </button>
              <button
                type="button"
                class="btn btn-sm btn-square btn-outline"
                :class="{ 'is-selected': showAddApiForm }"
                aria-label="新增后端"
                title="新增后端"
                @click="toggleAddApiForm"
              >
                <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                  <path d="M10 4v12M4 10h12" />
                </svg>
              </button>
            </div>

            <Transition name="settings-reveal" mode="out-in">
              <div v-if="showEditApiForm" key="edit" class="settings-form-panel">
                <div class="settings-form-heading">
                  <div>
                    <strong>编辑当前端点</strong>
                    <span>保存后面板会立即重新连接。</span>
                  </div>
                  <span class="settings-mono settings-version-note">{{ singboxVersion || 'sing-box' }}</span>
                </div>
                <label class="settings-field">
                  <span>名称</span>
                  <input v-model="activeApiForm.name" type="text" class="input input-sm input-bordered" placeholder="默认后端" />
                </label>
                <div class="settings-endpoint-fields">
                  <label class="settings-field">
                    <span>协议</span>
                    <select v-model="activeApiForm.protocol" class="select select-sm select-bordered settings-mono">
                      <option value="http">http</option>
                      <option value="https">https</option>
                    </select>
                  </label>
                  <label class="settings-field">
                    <span>主机</span>
                    <input v-model="activeApiForm.host" type="text" class="input input-sm input-bordered settings-mono" placeholder="127.0.0.1" />
                  </label>
                  <label class="settings-field">
                    <span>端口</span>
                    <input v-model="activeApiForm.port" type="text" class="input input-sm input-bordered settings-mono" placeholder="9090" />
                  </label>
                </div>
                <label class="settings-field">
                  <span>访问密钥</span>
                  <input v-model="activeApiForm.secret" type="password" class="input input-sm input-bordered settings-mono" placeholder="留空表示无密钥" />
                </label>
                <div class="settings-form-actions settings-form-actions-between">
                  <button class="btn btn-sm btn-ghost settings-danger-action" :disabled="clashApis.length <= 1" @click="handleRemoveActiveApi">删除端点</button>
                  <button class="btn btn-sm btn-route" @click="handleSaveActiveApi">保存端点</button>
                </div>
              </div>

              <div v-else-if="showAddApiForm" key="add" class="settings-form-panel">
                <div class="settings-form-heading">
                  <div>
                    <strong>新增控制端点</strong>
                    <span>新增后会自动切换到这个端点。</span>
                  </div>
                </div>
                <label class="settings-field">
                  <span>名称</span>
                  <input v-model="newApiForm.name" type="text" class="input input-sm input-bordered" placeholder="后端 2" />
                </label>
                <div class="settings-endpoint-fields">
                  <label class="settings-field">
                    <span>协议</span>
                    <select v-model="newApiForm.protocol" class="select select-sm select-bordered settings-mono">
                      <option value="http">http</option>
                      <option value="https">https</option>
                    </select>
                  </label>
                  <label class="settings-field">
                    <span>主机</span>
                    <input v-model="newApiForm.host" type="text" class="input input-sm input-bordered settings-mono" placeholder="127.0.0.1" />
                  </label>
                  <label class="settings-field">
                    <span>端口</span>
                    <input v-model="newApiForm.port" type="text" class="input input-sm input-bordered settings-mono" placeholder="9090" />
                  </label>
                </div>
                <label class="settings-field">
                  <span>访问密钥</span>
                  <input v-model="newApiForm.secret" type="password" class="input input-sm input-bordered settings-mono" placeholder="留空表示无密钥" />
                </label>
                <div class="settings-form-actions">
                  <button class="btn btn-sm btn-route" @click="handleAddApi">新增并切换</button>
                </div>
              </div>
            </Transition>
          </div>
        </section>

        <section id="settings-network" class="settings-section">
          <div class="settings-section-heading">
            <span class="settings-section-icon settings-section-icon-signal" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none">
                <path d="M4 17h3v3H4zm6-6h3v9h-3zm6-7h3v16h-3z" />
                <path d="M3.5 8.5 9 4l4 3 7-5" />
              </svg>
            </span>
            <div>
              <h2>网络测试</h2>
              <p>设置代理延迟测试目标，并即时诊断 DNS。</p>
            </div>
          </div>

          <div class="settings-card settings-card-padded settings-stack">
            <label class="settings-field">
              <span>默认测速地址</span>
              <input
                v-model="config.latencyTestUrl"
                type="text"
                class="input input-sm input-bordered settings-mono"
                placeholder="https://www.gstatic.com/generate_204"
              />
              <small>没有单独指定测试地址的代理组会使用这里的 URL。</small>
            </label>

            <div class="settings-mapping-block">
              <button
                type="button"
                class="settings-mapping-toggle"
                :aria-expanded="groupTestUrlsExpanded"
                @click="groupTestUrlsExpanded = !groupTestUrlsExpanded"
              >
                <span>
                  <strong>代理组专用地址</strong>
                  <small>{{ groupTestUrlEntries.length ? `已配置 ${groupTestUrlEntries.length} 个代理组` : '暂未配置' }}</small>
                </span>
                <svg viewBox="0 0 20 20" fill="none" :class="{ 'rotate-180': groupTestUrlsExpanded }" aria-hidden="true">
                  <path d="m5 8 5 5 5-5" />
                </svg>
              </button>

              <Transition name="settings-reveal">
                <div v-show="groupTestUrlsExpanded" class="settings-mapping-list">
                  <div v-for="[group, url] in groupTestUrlEntries" :key="group" class="settings-mapping-row">
                    <template v-if="editingGroupTestUrl === group">
                      <select v-model="editGroupTestUrlGroup" class="select select-xs select-bordered">
                        <option v-for="name in editAvailableGroups" :key="name" :value="name">{{ name }}</option>
                      </select>
                      <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                      <input
                        v-model="editGroupTestUrlValue"
                        type="text"
                        class="input input-xs input-bordered settings-mono"
                        @keyup.enter="saveEditGroupTestUrl"
                        @keyup.escape="editingGroupTestUrl = null"
                      />
                      <button class="btn btn-ghost btn-xs btn-square" @click="saveEditGroupTestUrl" title="保存" aria-label="保存测速地址">
                        <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m4 10 4 4 8-9" /></svg>
                      </button>
                    </template>
                    <template v-else>
                      <span class="settings-group-badge">{{ group }}</span>
                      <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                      <span class="settings-mapping-url settings-mono" :title="url">{{ url }}</span>
                      <button class="btn btn-ghost btn-xs btn-square" @click="startEditGroupTestUrl(group)" title="编辑" aria-label="编辑测速地址">
                        <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m13.8 3.7 2.5 2.5M4 16l.7-3.3L13.9 3.5a1.8 1.8 0 0 1 2.6 0l.1.1a1.8 1.8 0 0 1 0 2.6l-9.2 9.2L4 16Z" /></svg>
                      </button>
                    </template>
                    <button class="btn btn-ghost btn-xs btn-square settings-danger-action" @click="removeGroupTestUrl(group)" title="删除" aria-label="删除测速地址">
                      <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M5 6h10m-7-3h4l1 3H7l1-3Zm-1 6 .5 7m5.5-7-.5 7M6 6l1 11h6l1-11" /></svg>
                    </button>
                  </div>

                  <div class="settings-mapping-row settings-mapping-new">
                    <select v-model="newGroupTestUrl.group" class="select select-xs select-bordered" aria-label="代理组">
                      <option value="" disabled hidden>选择代理组</option>
                      <option v-for="name in availableGroups" :key="name" :value="name">{{ name }}</option>
                    </select>
                    <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                    <input
                      v-model="newGroupTestUrl.url"
                      type="text"
                      class="input input-xs input-bordered settings-mono"
                      placeholder="测速地址"
                      @keyup.enter="addGroupTestUrl"
                    />
                    <button class="btn btn-xs btn-route" @click="addGroupTestUrl">添加</button>
                  </div>
                </div>
              </Transition>
            </div>

            <label class="settings-toggle-row">
              <span class="settings-row-copy">
                <strong>IPv6 连通性测试</strong>
                <span>测速时额外检查节点的 IPv6 可用性。</span>
              </span>
              <input v-model="config.ipv6TestEnabled" type="checkbox" class="toggle toggle-sm toggle-primary" />
            </label>
          </div>

          <DnsQueryTool />
        </section>

        <section id="settings-application" class="settings-section">
          <div class="settings-section-heading">
            <span class="settings-section-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none">
                <rect x="3" y="4" width="18" height="13" rx="2" />
                <path d="M8 21h8m-4-4v4" />
              </svg>
            </span>
            <div>
              <h2>应用</h2>
              <p>调整面板外观，以及它在 Windows 中的运行方式。</p>
            </div>
          </div>

          <div class="settings-card settings-card-padded settings-stack">
            <div>
              <div class="settings-card-label-row settings-theme-label-row">
                <div>
                  <strong>界面主题</strong>
                  <span>选择更适合当前环境的显示方式。</span>
                </div>
              </div>
              <div class="settings-theme-grid" role="radiogroup" aria-label="界面主题">
                <button
                  v-for="theme in themeOptions"
                  :key="theme.value"
                  type="button"
                  class="settings-theme-option"
                  :class="{ 'is-active': config.theme === theme.value }"
                  role="radio"
                  :aria-checked="config.theme === theme.value"
                  @click="updateConfig({ theme: theme.value })"
                >
                  <span class="settings-theme-preview" :class="`theme-preview-${theme.value}`" aria-hidden="true">
                    <span class="preview-sidebar"></span>
                    <span class="preview-card preview-card-top"></span>
                    <span class="preview-card preview-card-bottom"></span>
                  </span>
                  <span class="settings-theme-copy">
                    <strong>{{ theme.label }}</strong>
                    <small>{{ theme.hint }}</small>
                  </span>
                  <span class="settings-theme-check" aria-hidden="true">
                    <svg viewBox="0 0 16 16" fill="none"><path d="m3.5 8 3 3 6-7" /></svg>
                  </span>
                </button>
              </div>
            </div>

            <div class="settings-behavior-list">
              <label class="settings-toggle-row">
                <span class="settings-row-copy">
                  <strong>关闭到系统托盘</strong>
                  <span>关闭主窗口时让面板继续在后台运行。</span>
                </span>
                <input v-model="config.closeToTray" type="checkbox" class="toggle toggle-sm toggle-primary" />
              </label>
              <label class="settings-toggle-row">
                <span class="settings-row-copy">
                  <strong>开机启动面板</strong>
                  <span>登录 Windows 后自动启动 singboard。</span>
                </span>
                <input type="checkbox" class="toggle toggle-sm toggle-primary" :checked="autoLaunchEnabled" @change="toggleAutoLaunch" />
              </label>
            </div>
          </div>
        </section>

        <section id="settings-updates" class="settings-section">
          <div class="settings-section-heading">
            <span class="settings-section-icon settings-section-icon-update" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none">
                <path d="M20 7v5h-5M4 17v-5h5" />
                <path d="M18.2 9A7 7 0 0 0 6.5 6.5L4 9m2 6a7 7 0 0 0 11.5 2.5L20 15" />
              </svg>
            </span>
            <div>
              <h2>更新</h2>
              <p>管理 sing-box 核心与 singboard 面板版本。</p>
            </div>
          </div>

          <div class="settings-update-stack">
            <CoreUpdateCard />
            <PanelUpdateCard />
          </div>
        </section>
      </div>
    </div>
  </div>
</template>
