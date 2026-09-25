<script setup lang="ts">
import { ref, computed, watch } from 'vue'
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
  isElevationCancelled,
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

const themeOptions = [
  { value: 'auto', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
] as const

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
      detail = await readServiceErrorLog()
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
    switch (action) {
      case 'start':
        await startCore()
        checkServiceAfterStart()
        break
      case 'restart':
        await restartCore()
        checkServiceAfterStart()
        break
      case 'stop': await stopService(); break
      case 'install': {
        const runningConfigPath = await getRunningConfigPath()
        const startupDelaySeconds = normalizeStartupDelayValue(config.value.startupDelaySeconds)
        config.value.startupDelaySeconds = startupDelaySeconds
        await installService(
          config.value.singboxPath,
          runningConfigPath,
          config.value.workingDir,
          startupDelaySeconds,
        )
        break
      }
      case 'uninstall': await uninstallService(); break
    }
    setTimeout(refresh, 1000)
  } catch (e: any) {
    if (!isElevationCancelled(e)) pushToast({ message: '操作失败: ' + (e?.message || e), type: 'error' }, 6000)
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
  if (startupTaskSyncing.value) return

  startupTaskSyncing.value = true
  try {
    if (await startupTaskExists()) {
      await createStartupTask(config.value.startupDelaySeconds)
      pushToast({ message: `自启延迟已同步为 ${config.value.startupDelaySeconds} 秒`, type: 'info' })
    }
  } catch (e: any) {
    if (!isElevationCancelled(e)) pushToast({ message: '同步自启延迟失败: ' + (e?.message || e), type: 'error' }, 6000)
  } finally {
    startupTaskSyncing.value = false
  }
}

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
  <div class="settings-page">
    <ConfirmDialog ref="confirmDialogRef" />

    <h1 class="text-xl font-bold mb-5">设置</h1>

    <section class="settings-group">
      <h2 class="settings-group-title">核心与服务</h2>
      <div class="settings-card">
        <div class="settings-row settings-service-row" :class="serviceStateTone">
          <div class="settings-status">
            <span class="settings-status-dot" aria-hidden="true"></span>
            <div class="settings-status-copy">
              <strong>{{ statusText }}</strong>
              <span class="settings-status-version settings-mono">
                <OverflowingText :text="singboxVersion || '未检测'" />
              </span>
            </div>
          </div>
          <div class="settings-row-actions">
            <button
              class="btn btn-sm btn-route relative"
              :aria-busy="actionLoading === 'start'"
              :disabled="!!actionLoading || serviceStatus.state === 'running'"
              @click="handleServiceAction('start')"
            >
              <span :class="{ 'opacity-0': actionLoading === 'start' }">启动</span>
              <span v-if="actionLoading === 'start'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
            <button
              class="btn btn-sm settings-btn relative"
              :aria-busy="actionLoading === 'restart'"
              :disabled="!!actionLoading"
              @click="handleServiceAction('restart')"
            >
              <span :class="{ 'opacity-0': actionLoading === 'restart' }">重启</span>
              <span v-if="actionLoading === 'restart'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
            <button
              class="btn btn-sm settings-btn settings-danger-action relative"
              :aria-busy="actionLoading === 'stop'"
              :disabled="!!actionLoading || serviceStatus.state === 'stopped'"
              @click="handleServiceAction('stop')"
            >
              <span :class="{ 'opacity-0': actionLoading === 'stop' }">停止</span>
              <span v-if="actionLoading === 'stop'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
          </div>
        </div>

        <div class="settings-row">
          <div class="settings-row-copy">
            <strong>代理模式</strong>
            <span>核心当前采用的流量处理策略。</span>
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
              <span>核心路径、工作目录与启动延迟。</span>
            </span>
            <svg viewBox="0 0 20 20" fill="none" :class="{ 'rotate-180': showServiceConfigPanel }" aria-hidden="true">
              <path d="m5 8 5 5 5-5" />
            </svg>
          </button>

          <Transition name="settings-reveal">
            <div v-if="showServiceConfigPanel" class="settings-service-fields">
              <label for="service-startup-delay">延迟启动</label>
              <div class="settings-input-unit">
                <input
                  id="service-startup-delay"
                  v-model.number="config.startupDelaySeconds"
                  type="number"
                  min="0"
                  max="3600"
                  step="1"
                  class="input input-sm input-bordered"
                  aria-describedby="service-startup-delay-unit"
                  @change="updateStartupDelay(); syncStartupDelayToTask()"
                />
                <span id="service-startup-delay-unit">秒</span>
              </div>
              <label for="service-singbox-path">sing-box 可执行文件</label>
              <div class="settings-path-control">
                <input
                  id="service-singbox-path"
                  v-model="config.singboxPath"
                  type="text"
                  class="input input-sm input-bordered settings-mono"
                  placeholder="C:\sing-box\sing-box.exe"
                />
                <button type="button" class="btn btn-sm settings-btn" aria-label="浏览 sing-box 可执行文件" @click="browseSingboxPath">浏览</button>
              </div>
              <label for="service-working-dir">工作目录</label>
              <div class="settings-path-control">
                <input
                  id="service-working-dir"
                  v-model="config.workingDir"
                  type="text"
                  class="input input-sm input-bordered settings-mono"
                  placeholder="留空则使用配置文件所在目录"
                />
                <button type="button" class="btn btn-sm settings-btn" aria-label="浏览工作目录" @click="browseWorkingDir">浏览</button>
              </div>
            </div>
          </Transition>
        </div>

        <div class="settings-row">
          <div class="settings-row-copy">
            <strong>Windows 服务</strong>
            <span>安装后，核心可独立于面板在后台运行。</span>
          </div>
          <div class="settings-row-actions">
            <button
              class="btn btn-sm settings-btn relative"
              :aria-busy="actionLoading === 'install'"
              :disabled="!!actionLoading || serviceStatus.state !== 'not_installed'"
              @click="handleServiceAction('install')"
            >
              <span :class="{ 'opacity-0': actionLoading === 'install' }">安装服务</span>
              <span v-if="actionLoading === 'install'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
            <button
              class="btn btn-sm btn-ghost settings-danger-action relative"
              :aria-busy="actionLoading === 'uninstall'"
              :disabled="!!actionLoading || serviceStatus.state === 'not_installed'"
              @click="handleServiceAction('uninstall')"
            >
              <span :class="{ 'opacity-0': actionLoading === 'uninstall' }">卸载</span>
              <span v-if="actionLoading === 'uninstall'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
          </div>
        </div>
      </div>
    </section>

    <section class="settings-group">
      <h2 class="settings-group-title">后端</h2>
      <div class="settings-card">
        <div class="settings-row">
          <div class="settings-row-copy">
            <strong>控制端点</strong>
            <span>面板连接的 Clash API，共 {{ clashApis.length }} 个。</span>
          </div>
          <div class="settings-endpoint-picker">
            <select
              class="select select-sm select-bordered settings-mono"
              :value="activeClashApiId"
              aria-label="当前控制端点"
              @change="handleSwitchApi(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="api in clashApis" :key="api.id" :value="api.id">
                {{ api.name }} · {{ api.url }}
              </option>
            </select>
            <button
              type="button"
              class="btn btn-sm btn-square btn-ghost settings-icon-btn"
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
              class="btn btn-sm btn-square btn-ghost settings-icon-btn"
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
        </div>

        <Transition name="settings-reveal" mode="out-in">
          <div v-if="showEditApiForm" key="edit" class="settings-row settings-row-stack">
            <div class="settings-form-heading">
              <strong>编辑当前端点</strong>
              <span>保存后面板会立即重新连接。</span>
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
              <button class="btn btn-sm btn-route" @click="handleSaveActiveApi">保存</button>
            </div>
          </div>

          <div v-else-if="showAddApiForm" key="add" class="settings-row settings-row-stack">
            <div class="settings-form-heading">
              <strong>新增控制端点</strong>
              <span>新增后会自动切换到这个端点。</span>
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

    <section class="settings-group">
      <h2 class="settings-group-title">网络测试</h2>
      <div class="settings-card">
        <div class="settings-row settings-row-stack">
          <label for="settings-latency-url" class="settings-row-copy">
            <strong>默认测速地址</strong>
            <span>没有单独指定测试地址的代理组会使用这里的 URL。</span>
          </label>
          <input
            id="settings-latency-url"
            v-model="config.latencyTestUrl"
            type="text"
            class="input input-sm input-bordered settings-mono"
            placeholder="https://www.gstatic.com/generate_204"
          />
        </div>

        <div class="settings-row settings-row-stack">
          <button
            type="button"
            class="settings-disclosure"
            :aria-expanded="groupTestUrlsExpanded"
            @click="groupTestUrlsExpanded = !groupTestUrlsExpanded"
          >
            <span class="settings-row-copy">
              <strong>代理组专用地址</strong>
              <span>{{ groupTestUrlEntries.length ? `已配置 ${groupTestUrlEntries.length} 个代理组` : '暂未配置' }}</span>
            </span>
            <svg viewBox="0 0 20 20" fill="none" :class="{ 'rotate-180': groupTestUrlsExpanded }" aria-hidden="true">
              <path d="m5 8 5 5 5-5" />
            </svg>
          </button>

          <Transition name="settings-reveal">
            <div v-show="groupTestUrlsExpanded" class="settings-mapping-list">
              <div v-for="[group, url] in groupTestUrlEntries" :key="group" class="settings-mapping-row">
                <template v-if="editingGroupTestUrl === group">
                  <select v-model="editGroupTestUrlGroup" class="select select-sm select-bordered" aria-label="代理组">
                    <option v-for="name in editAvailableGroups" :key="name" :value="name">{{ name }}</option>
                  </select>
                  <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                  <input
                    v-model="editGroupTestUrlValue"
                    type="text"
                    class="input input-sm input-bordered settings-mono"
                    aria-label="测速地址"
                    @keyup.enter="saveEditGroupTestUrl"
                    @keyup.escape="editingGroupTestUrl = null"
                  />
                  <button class="btn btn-ghost btn-sm btn-square settings-icon-btn" @click="saveEditGroupTestUrl" title="保存" aria-label="保存测速地址">
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m4 10 4 4 8-9" /></svg>
                  </button>
                </template>
                <template v-else>
                  <span class="settings-group-badge" :title="group">{{ group }}</span>
                  <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                  <span class="settings-mapping-url settings-mono" :title="url">{{ url }}</span>
                  <button class="btn btn-ghost btn-sm btn-square settings-icon-btn" @click="startEditGroupTestUrl(group)" title="编辑" aria-label="编辑测速地址">
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m13.8 3.7 2.5 2.5M4 16l.7-3.3L13.9 3.5a1.8 1.8 0 0 1 2.6 0l.1.1a1.8 1.8 0 0 1 0 2.6l-9.2 9.2L4 16Z" /></svg>
                  </button>
                </template>
                <button class="btn btn-ghost btn-sm btn-square settings-icon-btn settings-danger-action" @click="removeGroupTestUrl(group)" title="删除" aria-label="删除测速地址">
                  <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M5 6h10m-7-3h4l1 3H7l1-3Zm-1 6 .5 7m5.5-7-.5 7M6 6l1 11h6l1-11" /></svg>
                </button>
              </div>

              <div class="settings-mapping-row settings-mapping-new">
                <select v-model="newGroupTestUrl.group" class="select select-sm select-bordered" aria-label="代理组">
                  <option value="" disabled hidden>选择代理组</option>
                  <option v-for="name in availableGroups" :key="name" :value="name">{{ name }}</option>
                </select>
                <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                <input
                  v-model="newGroupTestUrl.url"
                  type="text"
                  class="input input-sm input-bordered settings-mono"
                  placeholder="测速地址"
                  aria-label="测速地址"
                  @keyup.enter="addGroupTestUrl"
                />
                <button class="btn btn-sm settings-btn" @click="addGroupTestUrl">添加</button>
              </div>
            </div>
          </Transition>
        </div>

        <label class="settings-row settings-toggle-row">
          <span class="settings-row-copy">
            <strong>IPv6 连通性测试</strong>
            <span>测速时额外检查节点的 IPv6 可用性。</span>
          </span>
          <input v-model="config.ipv6TestEnabled" type="checkbox" class="toggle toggle-sm toggle-primary" />
        </label>
      </div>
    </section>

    <section class="settings-group">
      <h2 class="settings-group-title">DNS 查询</h2>
      <DnsQueryTool />
    </section>

    <section class="settings-group">
      <h2 class="settings-group-title">应用</h2>
      <div class="settings-card">
        <div class="settings-row">
          <div class="settings-row-copy">
            <strong id="settings-theme-label">界面主题</strong>
          </div>
          <div class="settings-segmented" role="radiogroup" aria-labelledby="settings-theme-label">
            <button
              v-for="theme in themeOptions"
              :key="theme.value"
              type="button"
              role="radio"
              :class="{ 'is-active': config.theme === theme.value }"
              :aria-checked="config.theme === theme.value"
              @click="updateConfig({ theme: theme.value })"
            >
              {{ theme.label }}
            </button>
          </div>
        </div>
        <label class="settings-row settings-toggle-row">
          <span class="settings-row-copy">
            <strong>关闭到系统托盘</strong>
            <span>关闭主窗口时让面板继续在后台运行。</span>
          </span>
          <input v-model="config.closeToTray" type="checkbox" class="toggle toggle-sm toggle-primary" />
        </label>
        <label class="settings-row settings-toggle-row">
          <span class="settings-row-copy">
            <strong>开机启动面板</strong>
            <span>登录 Windows 后自动启动 singboard。</span>
          </span>
          <input type="checkbox" class="toggle toggle-sm toggle-primary" :checked="autoLaunchEnabled" @change="toggleAutoLaunch" />
        </label>
      </div>
    </section>

    <section class="settings-group">
      <h2 class="settings-group-title">更新</h2>
      <CoreUpdateCard />
      <PanelUpdateCard />
    </section>
  </div>
</template>
