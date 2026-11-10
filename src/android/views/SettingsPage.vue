<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useConfigStore } from '@/stores/config'
import { retryClashApi, useServiceStore, whenClashApiSettled } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { useProxiesStore } from '@/stores/proxies'
import { assertBackgroundFile, useBackgroundStore } from '@/stores/background'
import {
  bootHookExists,
  createBootHook,
  deleteBootHook,
} from '@/bridge/service'
import { ensureServiceInstalled } from '@/android/utils/coreControl'
import { useCoreActions } from '@/android/composables/useCoreActions'
import { detectRuntimeFiles } from '@/bridge/config'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { checkRootAccess, setRootEnabled } from '@/bridge/app'
import { patchConfig, fetchConfig } from '@/api'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import OverflowingText from '@/components/common/OverflowingText.vue'
import DnsQueryTool from '@/components/settings/DnsQueryTool.vue'
import CoreUpdateCard from '@/android/components/settings/CoreUpdateCard.vue'
import PanelUpdateCard from '@/android/components/settings/PanelUpdateCard.vue'
import BackgroundEditorDialog from '@/components/settings/BackgroundEditorDialog.vue'
import GlassTintSlider from '@/components/common/GlassTintSlider.vue'
import SettingsSelect from '@/android/components/settings/SettingsSelect.vue'
import { navItems, resolveStartPage } from '@/android/components/layout/navItems'
import { glassTintLabel } from '@/utils/glassTint'

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
const { serviceStatus, statusText, clashApiState, clashApiReady, refresh } = useServiceStore()
const { pushToast } = useToastStore()
const confirmDialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)

const themeOptions = [
  { value: 'auto', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
] as const

const startPage = computed(() => resolveStartPage(config.value.startPage))
const startPageOptions = navItems.map((item) => ({ value: item.path, label: item.label }))

const { backgroundUrl, hasBackground, setBackground, removeBackground } = useBackgroundStore()
const personalizationExpanded = ref(false)
const backgroundInput = ref<HTMLInputElement | null>(null)
const backgroundEditorRef = ref<InstanceType<typeof BackgroundEditorDialog> | null>(null)
const backgroundBusy = ref(false)

const personalizationSummary = computed(() => {
  const glass = glassTintLabel(config.value.glassTint)
  const backdrop = hasBackground.value ? '已设置背景' : '默认背景'
  return `${glass} · ${backdrop}`
})

async function onBackgroundPicked(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  backgroundBusy.value = true
  try {
    assertBackgroundFile(file)
    const edited = await backgroundEditorRef.value?.edit(file)
    if (edited) await setBackground(edited)
  } catch (error) {
    pushToast({ message: error instanceof Error ? error.message : '设置背景失败', type: 'error' })
  } finally {
    backgroundBusy.value = false
  }
}

async function clearBackground() {
  backgroundBusy.value = true
  try {
    await removeBackground()
  } catch (error) {
    pushToast({ message: error instanceof Error ? error.message : '删除背景失败', type: 'error' })
  } finally {
    backgroundBusy.value = false
  }
}

const { proxyGroups, loadProxies } = useProxiesStore()

const groupTestUrlsExpanded = ref(false)
const newGroupTestUrl = ref({ group: '', url: '' })
const editingGroupTestUrl = ref<string | null>(null)
const editGroupTestUrlGroup = ref('')
const editGroupTestUrlValue = ref('')

function toSelectOptions(values: string[]) {
  return values.map((value) => ({ value, label: value }))
}

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

const rootSwitching = ref(false)

async function toggleRootMode(e: Event) {
  const target = e.target as HTMLInputElement
  const enabled = target.checked
  if (!enabled) {
    updateConfig({ rootMode: false })
    return
  }
  rootSwitching.value = true
  try {
    if (!(await checkRootAccess())) {
      target.checked = false
      pushToast({ message: '未获得 root 权限，请在 Magisk / KernelSU / APatch 中授权 singboard', type: 'error' }, 6000)
      return
    }
    await setRootEnabled(true)
    updateConfig({ rootMode: true })
    void loadBootHookState()
  } finally {
    rootSwitching.value = false
  }
}

const bootHookEnabled = ref(false)

async function loadBootHookState() {
  if (!config.value.rootMode) {
    bootHookEnabled.value = false
    return
  }
  bootHookEnabled.value = await bootHookExists().catch(() => false)
}

async function toggleBootHook(e: Event) {
  const target = e.target as HTMLInputElement
  const enabled = target.checked
  bootHookSyncing.value = true
  try {
    if (enabled) {
      await ensureServiceInstalled()
      await createBootHook()
    } else {
      await deleteBootHook()
    }
    bootHookEnabled.value = enabled
  } catch (err: any) {
    target.checked = bootHookEnabled.value
    pushToast({ message: '设置开机自启失败: ' + (err?.message || err), type: 'error' }, 6000)
  } finally {
    bootHookSyncing.value = false
  }
}

const clashMode = ref('Rule')
const clashModeOptions = ref<string[]>(['Rule'])
const clashModeSelectOptions = computed(() => toSelectOptions(clashModeOptions.value))
const clashApiSelectOptions = computed(() =>
  clashApis.value.map((api) => ({ value: api.id, label: api.name, description: api.url }))
)
const protocolOptions: { value: 'http' | 'https'; label: string }[] = [
  { value: 'http', label: 'http' },
  { value: 'https', label: 'https' },
]
const { singboxVersion } = useSingboxVersionStore()
const bootHookSyncing = ref(false)

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

const RECONNECT_WAIT_MS = 3000

async function reconnect() {
  await refresh()
  retryClashApi()
  await Promise.race([
    whenClashApiSettled(),
    new Promise((resolve) => setTimeout(resolve, RECONNECT_WAIT_MS)),
  ])
  switch (clashApiState.value) {
    case 'ready':
      break
    case 'waiting':
      pushToast({ message: '核心启动中，Clash API 就绪后会自动连接', type: 'info' })
      break
    case 'auth_failed':
      pushToast({ message: 'Clash API 密钥不匹配，请检查密钥', type: 'error' })
      break
    default:
      pushToast({ message: '无法连接 Clash API，请检查地址、端口和密钥', type: 'error' })
  }
}

async function changeMode(mode: string) {
  try {
    await patchConfig({ mode } as any)
    await loadClashConfig()
  } catch {}
}

const { busy: coreBusy, run: runCoreAction } = useCoreActions()

const workingDirInput = ref(config.value.workingDir)
const detecting = ref(false)

watch(() => config.value.workingDir, (dir) => { workingDirInput.value = dir })

async function applyWorkingDir() {
  const dir = workingDirInput.value.trim().replace(/\/+$/, '')
  if (!dir.startsWith('/')) {
    pushToast({ message: '请填写绝对路径，例如 /data/adb/sing-box', type: 'error' })
    return
  }
  detecting.value = true
  try {
    const detected = await detectRuntimeFiles(dir)
    updateConfig({
      workingDir: detected.baseDir,
      singboxPath: detected.singboxPath ?? '',
      coreInstallPath: detected.coreInstallPath,
      configPath: detected.configPath ?? '',
      rulesDir: detected.rulesDir ?? '',
    })
    if (detected.found && serviceStatus.value.state !== 'not_installed') {
      await ensureServiceInstalled()
    }
    if (detected.found) {
      pushToast({
        message: serviceStatus.value.state === 'running' ? '已识别核心和配置，重启核心后生效' : '已识别核心和配置文件',
        type: 'info',
      })
    } else {
      const missing = [detected.singboxPath ? '' : 'sing-box 核心', detected.configPath ? '' : '配置文件'].filter(Boolean).join('和')
      pushToast({ message: `工作目录中未找到${missing}`, type: 'error' }, 6000)
    }
  } catch (e: any) {
    pushToast({ message: '识别工作目录失败: ' + (e?.message || e), type: 'error' }, 6000)
  } finally {
    detecting.value = false
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

syncActiveApiForm()
void loadBootHookState()

watch(
  clashApiReady,
  (ready) => {
    if (!ready) return
    loadClashConfig()
    loadProxies()
  },
  { immediate: true },
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
    <BackgroundEditorDialog ref="backgroundEditorRef" />

    <h1 class="text-[26px] leading-tight tracking-tight font-bold mb-5">设置</h1>

    <section class="settings-group">
      <h2 class="settings-group-title">核心与服务</h2>
      <div class="settings-card">
        <label class="settings-row settings-toggle-row">
          <span class="settings-row-copy">
            <strong>Root 管理本机核心</strong>
            <span>需要 Magisk / KernelSU / APatch。关闭时仅作为远程面板连接 Clash API。</span>
          </span>
          <span v-if="rootSwitching" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          <input
            v-else
            type="checkbox"
            class="toggle toggle-sm toggle-primary"
            :checked="config.rootMode"
            @change="toggleRootMode"
          />
        </label>

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
          <div v-if="config.rootMode" class="settings-row-actions">
            <button
              class="btn btn-sm btn-route relative"
              :aria-busy="coreBusy === 'start'"
              :disabled="!!coreBusy || serviceStatus.state === 'running'"
              @click="runCoreAction('start')"
            >
              <span :class="{ 'opacity-0': coreBusy === 'start' }">启动</span>
              <span v-if="coreBusy === 'start'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
            <button
              class="btn btn-sm settings-btn relative"
              :aria-busy="coreBusy === 'restart'"
              :disabled="!!coreBusy || serviceStatus.state !== 'running'"
              @click="runCoreAction('restart')"
            >
              <span :class="{ 'opacity-0': coreBusy === 'restart' }">重启</span>
              <span v-if="coreBusy === 'restart'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
            <button
              class="btn btn-sm settings-btn settings-danger-action relative"
              :aria-busy="coreBusy === 'stop'"
              :disabled="!!coreBusy || serviceStatus.state !== 'running'"
              @click="runCoreAction('stop')"
            >
              <span :class="{ 'opacity-0': coreBusy === 'stop' }">停止</span>
              <span v-if="coreBusy === 'stop'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
          </div>
          <div v-else class="settings-row-actions">
            <button class="btn btn-sm settings-btn" @click="reconnect">重新连接</button>
          </div>
        </div>

        <div class="settings-row">
          <div class="settings-row-copy">
            <strong>代理模式</strong>
            <span>核心当前采用的流量处理策略。</span>
          </div>
          <SettingsSelect
            class="settings-row-control"
            :model-value="clashMode"
            :options="clashModeSelectOptions"
            aria-label="代理模式"
            @update:model-value="changeMode"
          />
        </div>

        <template v-if="config.rootMode">
          <div class="settings-row settings-row-stack">
            <label for="service-working-dir" class="settings-row-copy">
              <strong>工作目录</strong>
              <span>自动在目录及其子目录中查找核心（优先 bin 文件夹）、配置文件和规则。</span>
            </label>
            <div class="settings-path-control">
              <input
                id="service-working-dir"
                v-model="workingDirInput"
                type="text"
                autocapitalize="off"
                spellcheck="false"
                class="input input-sm input-bordered settings-mono"
                placeholder="/data/adb/sing-box"
                @keyup.enter="applyWorkingDir"
              />
              <button type="button" class="btn btn-sm btn-route relative" :disabled="detecting" @click="applyWorkingDir">
                <span :class="{ 'opacity-0': detecting }">识别</span>
                <span v-if="detecting" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
              </button>
            </div>
          </div>

          <label class="settings-row settings-toggle-row">
            <span class="settings-row-copy">
              <strong>开机自启动</strong>
              <span>通过 /data/adb/service.d 在开机完成后启动核心。</span>
            </span>
            <input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              :checked="bootHookEnabled"
              :disabled="bootHookSyncing || !config.singboxPath || !config.configPath"
              @change="toggleBootHook"
            />
          </label>

        </template>
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
            <SettingsSelect
              class="settings-mono"
              :model-value="activeClashApiId"
              :options="clashApiSelectOptions"
              :title="activeClashApi?.url"
              aria-label="当前控制端点"
              @update:model-value="handleSwitchApi"
            />
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
              <div class="settings-field">
                <span>协议</span>
                <SettingsSelect v-model="activeApiForm.protocol" :options="protocolOptions" class="settings-mono" aria-label="协议" />
              </div>
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
              <div class="settings-field">
                <span>协议</span>
                <SettingsSelect v-model="newApiForm.protocol" :options="protocolOptions" class="settings-mono" aria-label="协议" />
              </div>
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
                  <SettingsSelect v-model="editGroupTestUrlGroup" :options="toSelectOptions(editAvailableGroups)" aria-label="代理组" />
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
                <SettingsSelect
                  v-model="newGroupTestUrl.group"
                  :options="toSelectOptions(availableGroups)"
                  placeholder="选择代理组"
                  aria-label="代理组"
                />
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
        <div class="settings-row">
          <div class="settings-row-copy">
            <strong>默认启动页</strong>
            <span>启动面板时首先打开的页面，下次启动生效。</span>
          </div>
          <SettingsSelect
            class="settings-row-control"
            :model-value="startPage"
            :options="startPageOptions"
            aria-label="默认启动页"
            @update:model-value="updateConfig({ startPage: $event })"
          />
        </div>
        <label class="settings-row settings-toggle-row">
          <span class="settings-row-copy">
            <strong>左右滑动切换页面</strong>
            <span>关闭时只能点底栏切换页面，避免和页面里的横向滚动冲突。</span>
          </span>
          <input v-model="config.pageSwipe" type="checkbox" class="toggle toggle-sm toggle-primary" />
        </label>
        <div class="settings-row settings-row-stack">
          <button
            type="button"
            class="settings-disclosure"
            :aria-expanded="personalizationExpanded"
            @click="personalizationExpanded = !personalizationExpanded"
          >
            <span class="settings-row-copy">
              <strong>个性化</strong>
              <span>{{ personalizationSummary }}</span>
            </span>
            <svg viewBox="0 0 20 20" fill="none" :class="{ 'rotate-180': personalizationExpanded }" aria-hidden="true">
              <path d="m5 8 5 5 5-5" />
            </svg>
          </button>

          <Transition name="settings-reveal">
            <div v-show="personalizationExpanded" class="settings-subpanel">
              <div class="settings-row settings-row-stack">
                <div class="settings-row-copy">
                  <strong id="settings-glass-label">玻璃效果</strong>
                  <span>作用于顶部栏、底栏、卡片和弹窗。往左更通透，往右底色更重、文字更易读。</span>
                </div>
                <GlassTintSlider
                  :model-value="config.glassTint"
                  labelledby="settings-glass-label"
                  @update:model-value="updateConfig({ glassTint: $event })"
                />
              </div>
              <div class="settings-row">
                <div class="settings-row-copy">
                  <strong>背景图片</strong>
                  <span>{{ hasBackground ? '显示在应用背后，玻璃越通透越能看清它。' : '未设置，使用系统默认材质。' }}</span>
                </div>
                <div class="settings-background-actions">
                  <img v-if="backgroundUrl" :src="backgroundUrl" alt="" class="settings-background-thumb" />
                  <button type="button" class="btn btn-sm settings-btn" :disabled="backgroundBusy" @click="backgroundInput?.click()">
                    {{ hasBackground ? '更换' : '选择图片' }}
                  </button>
                  <button
                    v-if="hasBackground"
                    type="button"
                    class="btn btn-ghost btn-sm btn-square settings-icon-btn settings-danger-action"
                    :disabled="backgroundBusy"
                    title="删除背景"
                    aria-label="删除背景"
                    @click="clearBackground"
                  >
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M5 6h10m-7-3h4l1 3H7l1-3Zm-1 6 .5 7m5.5-7-.5 7M6 6l1 11h6l1-11" /></svg>
                  </button>
                  <input ref="backgroundInput" type="file" accept="image/*" class="hidden" @change="onBackgroundPicked" />
                </div>
              </div>
            </div>
          </Transition>
        </div>
      </div>
    </section>

    <section class="settings-group">
      <h2 class="settings-group-title">更新</h2>
      <CoreUpdateCard v-if="config.rootMode" />
      <PanelUpdateCard />
    </section>
  </div>
</template>
