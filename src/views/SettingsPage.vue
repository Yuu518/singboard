<script setup lang="ts">
import { ref, computed, watch, shallowRef, nextTick, onBeforeUnmount, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { useProxiesStore } from '@/stores/proxies'
import { assertBackgroundFile, useBackgroundStore } from '@/stores/background'
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
import { fetchConfig } from '@/api'
import { useSettledServiceState } from '@/composables/useSettledServiceState'
import { useClashMode, clashModeLabel } from '@/composables/useClashMode'
import { useStartupDelay } from '@/composables/useStartupDelay'
import { useSettingField, validateHttpUrl } from '@/composables/useSettingField'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import OverflowingText from '@/components/common/OverflowingText.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import GlassTintSlider from '@/components/common/GlassTintSlider.vue'
import { glassTintLabel } from '@/utils/glassTint'
import DnsQueryTool from '@/components/settings/DnsQueryTool.vue'
import CoreUpdateCard from '@/components/settings/CoreUpdateCard.vue'
import PanelUpdateCard from '@/components/settings/PanelUpdateCard.vue'
import BackgroundEditorDialog from '@/components/settings/BackgroundEditorDialog.vue'
import { removeEntry, restoreEntry, type RemovedEntry } from '@/utils/orderedRecord'
import { buildApiUrl, hostForDisplay, parseApiUrl, type EndpointField, type EndpointProtocol } from '@/utils/clashEndpoint'
import { describeEndpointStatus, type EndpointProbe } from '@/utils/endpointStatus'
import { getRequestErrorReason } from '@/utils/requestError'
import { failureMessage, humanizeError } from '@/utils/humanError'
import { useLogsStore } from '@/stores/logs'

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
const UNDO_WINDOW_MS = 8000
const announcement = ref('')

function announce(message: string) {
  announcement.value = ''
  void nextTick(() => { announcement.value = message })
}

const themeOptions = [
  { value: 'auto', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
] as const

const { backgroundUrl, hasBackground, setBackground, removeBackground } = useBackgroundStore()
const personalizationExpanded = ref(false)
const backgroundInput = ref<HTMLInputElement | null>(null)
const backgroundEditorRef = ref<InstanceType<typeof BackgroundEditorDialog> | null>(null)
const backgroundBusy = ref(false)

const personalizationSummary = computed(() => {
  const glass = glassTintLabel(config.value.glassTint)
  return `${glass} · ${hasBackground.value ? '已设置背景' : '默认背景'}`
})

const removedBackground = shallowRef<Blob | null>(null)
const backgroundUndoButton = ref<HTMLButtonElement | null>(null)
const backgroundPickButton = ref<HTMLButtonElement | null>(null)
let backgroundUndoTimer: ReturnType<typeof setTimeout> | null = null

function forgetRemovedBackground() {
  if (backgroundUndoTimer) clearTimeout(backgroundUndoTimer)
  backgroundUndoTimer = null
  removedBackground.value = null
}

async function onBackgroundPicked(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  forgetRemovedBackground()
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
    const removed = await removeBackground()
    forgetRemovedBackground()
    if (removed) {
      removedBackground.value = removed
      backgroundUndoTimer = setTimeout(forgetRemovedBackground, UNDO_WINDOW_MS)
      announce('背景已删除，8 秒内可撤销')
    }
  } catch (error) {
    pushToast({ message: error instanceof Error ? error.message : '删除背景失败', type: 'error' })
  } finally {
    backgroundBusy.value = false
  }
  await nextTick()
  ;(backgroundUndoButton.value ?? backgroundPickButton.value)?.focus()
}

async function undoBackgroundRemoval() {
  const blob = removedBackground.value
  if (!blob || backgroundBusy.value) return
  forgetRemovedBackground()
  backgroundBusy.value = true
  try {
    await setBackground(blob)
    announce('已恢复背景')
  } catch (error) {
    pushToast({ message: `恢复背景失败：${error instanceof Error ? error.message : '未知错误'}`, type: 'error' })
  } finally {
    backgroundBusy.value = false
  }
  await nextTick()
  backgroundPickButton.value?.focus()
}

const { proxyGroups, loadProxies } = useProxiesStore()

const DEFAULT_LATENCY_URL = 'https://www.gstatic.com/generate_204'
const latencyUrlField = useSettingField({
  source: computed({
    get: () => config.value.latencyTestUrl,
    set: (value: string) => { config.value.latencyTestUrl = value },
  }),
  normalize: (value) => value.trim() || DEFAULT_LATENCY_URL,
  validate: (value) => validateHttpUrl(value, '测速地址'),
  savedText: (value) => (value === DEFAULT_LATENCY_URL ? `已恢复默认地址 ${DEFAULT_LATENCY_URL}` : '已保存'),
})

const groupTestUrlsExpanded = ref(false)
const newGroupTestUrl = ref({ group: '', url: '' })
const editingGroupTestUrl = ref<string | null>(null)
const editGroupTestUrlGroup = ref('')
const editGroupTestUrlValue = ref('')

const groupTestUrlEntries = computed(() =>
  Object.entries(config.value.groupTestUrls)
)

const removedGroupTestUrl = shallowRef<RemovedEntry<string> | null>(null)
const mappingUndoButton = ref<HTMLButtonElement[]>([])
const mappingListRef = ref<HTMLElement | null>(null)
let mappingUndoTimer: ReturnType<typeof setTimeout> | null = null

type MappingRow =
  | { kind: 'entry'; group: string; url: string }
  | { kind: 'removed'; group: string; url: string }

const groupTestUrlRows = computed<MappingRow[]>(() => {
  const rows: MappingRow[] = groupTestUrlEntries.value.map(([group, url]) => ({ kind: 'entry', group, url }))
  const removed = removedGroupTestUrl.value
  if (removed && !(removed.key in config.value.groupTestUrls)) {
    rows.splice(Math.min(removed.index, rows.length), 0, { kind: 'removed', group: removed.key, url: removed.value })
  }
  return rows
})

function forgetRemovedGroupTestUrl() {
  if (mappingUndoTimer) clearTimeout(mappingUndoTimer)
  mappingUndoTimer = null
  removedGroupTestUrl.value = null
}

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

function rejectMappingUrl(url: string): boolean {
  const problem = validateHttpUrl(url, '测速地址')
  if (problem) pushToast({ message: problem, type: 'error' })
  return !!problem
}

function addGroupTestUrl() {
  const group = newGroupTestUrl.value.group.trim()
  const url = newGroupTestUrl.value.url.trim()
  if (!group || !url || rejectMappingUrl(url)) return
  config.value.groupTestUrls = { ...config.value.groupTestUrls, [group]: url }
  newGroupTestUrl.value = { group: '', url: '' }
}

function startEditGroupTestUrl(group: string) {
  editingGroupTestUrl.value = group
  editGroupTestUrlGroup.value = group
  editGroupTestUrlValue.value = config.value.groupTestUrls[group] ?? ''
}

async function cancelEditGroupTestUrl() {
  const group = editingGroupTestUrl.value
  editingGroupTestUrl.value = null
  if (!group) return
  await nextTick()
  const row = mappingListRef.value?.querySelector<HTMLElement>(`[data-mapping-group="${CSS.escape(group)}"]`)
  row?.querySelector<HTMLButtonElement>('button')?.focus()
}

function saveEditGroupTestUrl() {
  const oldGroup = editingGroupTestUrl.value
  if (!oldGroup) return
  const newGroup = editGroupTestUrlGroup.value.trim()
  const url = editGroupTestUrlValue.value.trim()
  if (!newGroup || !url || rejectMappingUrl(url)) return
  const { [oldGroup]: _, ...rest } = config.value.groupTestUrls
  config.value.groupTestUrls = { ...rest, [newGroup]: url }
  editingGroupTestUrl.value = null
}

async function removeGroupTestUrl(group: string) {
  const { next, removed } = removeEntry(config.value.groupTestUrls, group)
  if (!removed) return
  forgetRemovedGroupTestUrl()
  config.value.groupTestUrls = next
  if (editingGroupTestUrl.value === group) editingGroupTestUrl.value = null
  removedGroupTestUrl.value = removed
  mappingUndoTimer = setTimeout(forgetRemovedGroupTestUrl, UNDO_WINDOW_MS)
  announce(`已删除「${group}」的测速地址，8 秒内可撤销`)
  await nextTick()
  mappingUndoButton.value[0]?.focus()
}

async function undoGroupTestUrlRemoval() {
  const removed = removedGroupTestUrl.value
  if (!removed) return
  forgetRemovedGroupTestUrl()
  config.value.groupTestUrls = restoreEntry(config.value.groupTestUrls, removed)
  announce(`已恢复「${removed.key}」的测速地址`)
  await nextTick()
  const rows = mappingListRef.value?.querySelectorAll<HTMLElement>('[data-mapping-group]') ?? []
  const row = Array.from(rows).find((element) => element.dataset.mappingGroup === removed.key)
  row?.querySelector<HTMLButtonElement>('button')?.focus()
}

onBeforeUnmount(() => {
  forgetRemovedGroupTestUrl()
  forgetRemovedBackground()
})

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
    pushToast({ message: failureMessage(enabled ? '开启开机启动失败' : '关闭开机启动失败', err), type: 'error' }, 6000)
  }
}

const { singboxVersion: detectedVersion, versionStatus, versionError } = useSingboxVersionStore()
const singboxVersion = computed(() => {
  if (versionStatus.value === 'failed') return `检测失败：${versionError.value}`
  return detectedVersion.value
})
const { isRunning: coreRunning } = useSettledServiceState()
const clashEndpointKey = computed(() => {
  const api = activeClashApi.value
  return api ? `${api.id}
${api.url}
${api.secret}` : ''
})
const clashMode = useClashMode({
  isRunning: coreRunning,
  endpointKey: clashEndpointKey,
  onError: (message) => pushToast({ message, type: 'error' }, 6000),
})
const clashModeValue = clashMode.mode
const clashModeOptions = clashMode.options

async function onModeSelected(event: Event) {
  const select = event.target as HTMLSelectElement
  await clashMode.change(select.value)
  select.value = clashModeValue.value
}
const actionLoading = ref('')
const showServiceConfigPanel = ref(false)

function normalizeStartupDelayValue(value: unknown): number {
  const delay = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(delay)) return 30
  return Math.min(3600, Math.max(0, Math.round(delay)))
}
type ApiFormMode = 'edit' | 'add'

function emptyApiForm() {
  return { name: '', protocol: 'http' as EndpointProtocol, host: '', port: '', secret: '' }
}

const apiForm = ref(emptyApiForm())
const apiFormMode = ref<ApiFormMode | null>(null)
const apiFormErrors = ref<Partial<Record<EndpointField, string>>>({})
const apiNameInput = ref<HTMLInputElement | null>(null)
const apiHostInput = ref<HTMLInputElement | null>(null)
const apiPortInput = ref<HTMLInputElement | null>(null)
const endpointSelect = ref<HTMLSelectElement | null>(null)
const endpointGroup = ref<HTMLElement | null>(null)

function fillApiFormFromActive() {
  const current = activeClashApi.value
  const { protocol, host, port } = parseApiUrl(current?.url ?? '')
  apiForm.value = { name: current?.name ?? '', protocol, host, port, secret: current?.secret ?? '' }
}

function toggleApiForm(mode: ApiFormMode) {
  apiFormErrors.value = {}
  if (apiFormMode.value === mode) {
    apiFormMode.value = null
    return
  }
  apiFormMode.value = mode
  if (mode === 'edit') fillApiFormFromActive()
  else apiForm.value = emptyApiForm()
  void nextTick(() => apiNameInput.value?.focus())
}

function closeApiForm() {
  apiFormMode.value = null
  apiFormErrors.value = {}
  void nextTick(() => endpointSelect.value?.focus())
}

function clearApiFieldError(field: EndpointField) {
  if (!apiFormErrors.value[field]) return
  const { [field]: _, ...rest } = apiFormErrors.value
  apiFormErrors.value = rest
}

const endpointProbe = ref<EndpointProbe>({ state: 'idle' })
let probeGeneration = 0
let probeResetTimer: ReturnType<typeof setTimeout> | null = null
const { socketState: apiSocketState, socketFailures: apiSocketFailures } = useLogsStore()

const activeEndpointAddress = computed(() => hostForDisplay(activeClashApi.value?.url ?? ''))
const endpointStatus = computed(() => describeEndpointStatus({
  probe: endpointProbe.value,
  socketState: apiSocketState.value,
  socketFailures: apiSocketFailures.value,
  coreRunning: coreRunning.value,
  address: activeEndpointAddress.value,
}))

async function probeEndpoint() {
  const token = ++probeGeneration
  if (probeResetTimer) clearTimeout(probeResetTimer)
  probeResetTimer = null
  if (!coreRunning.value) {
    endpointProbe.value = { state: 'idle' }
    return
  }
  endpointProbe.value = { state: 'probing' }
  try {
    await fetchConfig()
    if (token !== probeGeneration) return
    endpointProbe.value = { state: 'ok' }
    probeResetTimer = setTimeout(() => {
      if (token === probeGeneration) endpointProbe.value = { state: 'idle' }
    }, 4000)
  } catch (error) {
    if (token !== probeGeneration) return
    endpointProbe.value = { state: 'error', reason: getRequestErrorReason(error) }
  }
  announce(endpointStatus.value.text)
}

watch(apiSocketState, (state) => {
  if (state === 'open' && endpointProbe.value.state === 'error') endpointProbe.value = { state: 'idle' }
})

function afterEndpointChange() {
  refresh()
  void probeEndpoint()
}

function handleSwitchApi(id: string) {
  setActiveClashApi(id)
  if (apiFormMode.value === 'edit') fillApiFormFromActive()
  afterEndpointChange()
}

function handleSaveApi() {
  const result = buildApiUrl(apiForm.value)
  if (!result.ok) {
    apiFormErrors.value = { [result.field]: result.error }
    void nextTick(() => (result.field === 'host' ? apiHostInput : apiPortInput).value?.focus())
    return
  }
  apiFormErrors.value = {}
  const name = apiForm.value.name.trim()
  if (apiFormMode.value === 'edit') {
    updateActiveClashApi({
      name: name || activeClashApi.value?.name || '端点',
      url: result.url,
      secret: apiForm.value.secret,
    })
  } else {
    const id = addClashApi(name || `端点 ${clashApis.value.length + 1}`, result.url, apiForm.value.secret)
    setActiveClashApi(id)
  }
  closeApiForm()
  afterEndpointChange()
}

async function handleRemoveActiveApi() {
  const current = activeClashApi.value
  if (!current || clashApis.value.length <= 1) return
  const next = clashApis.value.find((api) => api.id !== current.id)
  const confirmed = await confirmDialogRef.value?.show({
    title: `删除端点「${current.name}」？`,
    message: `面板会改为连接「${next?.name ?? ''}」。`,
    confirmText: '删除端点',
    variant: 'danger',
  })
  if (!confirmed) return
  removeClashApi(current.id)
  closeApiForm()
  afterEndpointChange()
}

const route = useRoute()

function focusEndpointGroup() {
  const group = endpointGroup.value
  if (!group) return
  const reduceMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  group.scrollIntoView?.({ block: 'start', behavior: reduceMotion ? 'auto' : 'smooth' })
  endpointSelect.value?.focus({ preventScroll: true })
}

onMounted(() => {
  if (route.hash !== '#clash-api') return
  requestAnimationFrame(() => requestAnimationFrame(focusEndpointGroup))
})

onBeforeUnmount(() => {
  if (probeResetTimer) clearTimeout(probeResetTimer)
  probeGeneration++
})

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
  // 服务显示 running，再验证 Clash API 是否可达
  try {
    await fetchConfig()
  } catch {
    pushToast({
      message: '服务进程已启动，但连不上 Clash API 端点。核心可能没有正常运行，请检查配置文件。',
      type: 'error',
    }, 8000)
  }
}

const SERVICE_ACTION_FAILURES: Record<string, string> = {
  start: '启动核心失败',
  restart: '重启核心失败',
  stop: '停止核心失败',
  install: '安装服务失败',
  uninstall: '卸载服务失败',
}

async function confirmServiceAction(action: string): Promise<boolean> {
  const running = serviceStatus.value.state === 'running'
  if (action === 'stop') {
    return (await confirmDialogRef.value?.show({
      title: '停止核心？',
      message: '代理会中断，直到再次启动核心。',
      confirmText: '停止核心',
      variant: 'danger',
    })) ?? false
  }
  if (action === 'uninstall') {
    return (await confirmDialogRef.value?.show({
      title: '卸载 Windows 服务？',
      message: running
        ? '核心将停止，代理会中断。之后要在后台运行核心，需要重新安装服务。'
        : '之后要在后台运行核心，需要重新安装服务。',
      confirmText: '卸载服务',
      variant: 'danger',
    })) ?? false
  }
  return true
}

async function handleServiceAction(action: string) {
  if (actionLoading.value) return
  if (!(await confirmServiceAction(action))) return
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
    if (!isElevationCancelled(e)) pushToast({ message: failureMessage(SERVICE_ACTION_FAILURES[action] ?? '操作失败', e), type: 'error' }, 6000)
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


const startupDelaySaved = computed({
  get: () => config.value.startupDelaySeconds,
  set: (value: number) => { config.value.startupDelaySeconds = value },
})
const startupDelay = useStartupDelay({
  saved: startupDelaySaved,
  taskExists: startupTaskExists,
  writeTask: createStartupTask,
  isCancelled: isElevationCancelled,
  describeError: humanizeError,
})
const startupDelayDraft = startupDelay.draft
const startupDelayNeedsElevation = computed(() => serviceStatus.value.state !== 'not_installed')
const startupDelayDescription = computed(() => startupDelayNeedsElevation.value
  ? '登录 Windows 后等这么久再启动核心服务，给网络留出准备时间。下次登录生效。'
  : '登录 Windows 后等这么久再启动核心服务。安装服务时写入。')
const startupDelayHint = computed(() => {
  if (startupDelay.error.value) return { tone: 'error', text: startupDelay.error.value }
  if (startupDelay.applying.value) return { tone: 'neutral', text: startupDelayNeedsElevation.value ? '等待管理员授权…' : '正在保存…' }
  const result = startupDelay.result.value
  if (result.kind === 'applied' || result.kind === 'saved') return { tone: 'ok', text: startupDelay.resultText.value }
  if (result.kind === 'failed') return { tone: 'error', text: startupDelay.resultText.value }
  if (result.kind === 'cancelled') return { tone: 'neutral', text: startupDelay.resultText.value }
  if (startupDelay.dirty.value && startupDelayNeedsElevation.value) {
    return { tone: 'neutral', text: '应用时 Windows 会请求管理员授权，用来更新开机自启任务。' }
  }
  return { tone: 'neutral', text: '' }
})

function applyStartupDelay() {
  if (startupDelay.error.value) return
  void startupDelay.apply()
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

if (serviceStatus.value.state === 'running') {
  loadProxies()
}

watch(
  () => serviceStatus.value.state,
  (newState, oldState) => {
    if (newState === 'running' && oldState !== 'running') {
      loadProxies()
    }
  }
)

watch(
  () => activeClashApiId.value,
  () => {
    if (apiFormMode.value === 'edit') fillApiFormFromActive()
  },
)
</script>

<template>
  <div class="settings-page">
    <ConfirmDialog ref="confirmDialogRef" />
    <p class="sr-only" role="status" aria-live="polite">{{ announcement }}</p>
    <BackgroundEditorDialog ref="backgroundEditorRef" />

    <h1 class="text-[26px] leading-tight tracking-tight font-bold mb-5">设置</h1>

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
            <span id="settings-mode-desc" :role="clashMode.status.value === 'error' ? 'alert' : undefined">{{ clashMode.description.value }}</span>
          </div>
          <select
            class="select select-sm select-bordered settings-row-control"
            :value="clashModeValue"
            :disabled="clashMode.disabled.value"
            :aria-busy="clashMode.switching.value || clashMode.status.value === 'loading'"
            aria-label="代理模式"
            aria-describedby="settings-mode-desc"
            @change="onModeSelected"
          >
            <option v-if="!clashModeOptions.length" value="">{{ clashMode.placeholder.value }}</option>
            <option v-for="mode in clashModeOptions" :key="mode" :value="mode">{{ clashModeLabel(mode) }}</option>
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
              <span>核心路径、工作目录与启动延迟。路径和目录在安装服务时写入，修改后需重新安装服务才生效。</span>
            </span>
            <svg viewBox="0 0 20 20" fill="none" :class="{ 'rotate-180': showServiceConfigPanel }" aria-hidden="true">
              <path d="m5 8 5 5 5-5" />
            </svg>
          </button>

          <Transition name="settings-reveal">
            <div v-if="showServiceConfigPanel" class="settings-service-fields">
              <label for="service-startup-delay" class="settings-service-label">
                登录后延迟启动
                <small id="service-startup-delay-desc">{{ startupDelayDescription }}</small>
              </label>
              <div class="settings-delay-control">
                <div class="settings-input-unit">
                  <input
                    id="service-startup-delay"
                    v-model="startupDelayDraft"
                    type="text"
                    inputmode="numeric"
                    class="input input-sm input-bordered"
                    :disabled="startupDelay.applying.value"
                    :aria-invalid="!!startupDelay.error.value"
                    aria-describedby="service-startup-delay-unit service-startup-delay-desc service-startup-delay-hint"
                    @keydown.enter.prevent="applyStartupDelay"
                    @keydown.escape.prevent="startupDelay.revert()"
                  />
                  <span id="service-startup-delay-unit">秒</span>
                </div>
                <template v-if="startupDelay.dirty.value || startupDelay.applying.value">
                  <button
                    type="button"
                    class="btn btn-sm settings-btn"
                    :disabled="!!startupDelay.error.value || startupDelay.applying.value"
                    :aria-busy="startupDelay.applying.value"
                    @click="applyStartupDelay"
                  >
                    <span v-if="startupDelay.applying.value" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
                    <svg v-else-if="startupDelayNeedsElevation" class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M10 2.5 4 5v4.5c0 3.7 2.5 6.7 6 8 3.5-1.3 6-4.3 6-8V5l-6-2.5Z" /></svg>
                    {{ startupDelay.applying.value ? '正在应用…' : startupDelayNeedsElevation ? '应用延迟' : '保存延迟' }}
                  </button>
                  <button
                    v-if="!startupDelay.applying.value"
                    type="button"
                    class="btn btn-sm btn-ghost settings-quiet-btn"
                    @click="startupDelay.revert()"
                  >还原</button>
                </template>
                <p
                  id="service-startup-delay-hint"
                  class="settings-delay-hint"
                  :class="`is-${startupDelayHint.tone}`"
                  role="status"
                >{{ startupDelayHint.text }}</p>
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
              <span :class="{ 'opacity-0': actionLoading === 'uninstall' }">卸载服务</span>
              <span v-if="actionLoading === 'uninstall'" class="loading loading-spinner loading-xs absolute inset-0 m-auto h-4" aria-hidden="true"></span>
            </button>
          </div>
        </div>
      </div>
    </section>

    <section id="clash-api" ref="endpointGroup" class="settings-group" aria-labelledby="settings-clash-api-title">
      <h2 id="settings-clash-api-title" class="settings-group-title">Clash API</h2>
      <div class="settings-card">
        <div class="settings-row">
          <div class="settings-row-copy">
            <strong id="settings-endpoint-label">当前端点</strong>
            <span id="settings-endpoint-status" :class="{ 'settings-text-danger': endpointStatus.tone === 'error' }">
              {{ endpointStatus.text }}<template v-if="endpointStatus.tone !== 'error'"> · 共 {{ clashApis.length }} 个端点</template>
            </span>
          </div>
          <div class="settings-endpoint-picker">
            <span class="settings-endpoint-dot" :class="`is-${endpointStatus.tone}`" aria-hidden="true"></span>
            <select
              ref="endpointSelect"
              class="select select-sm select-bordered"
              :value="activeClashApiId"
              :title="activeClashApi?.url"
              aria-labelledby="settings-endpoint-label"
              aria-describedby="settings-endpoint-status"
              @change="handleSwitchApi(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="api in clashApis" :key="api.id" :value="api.id">
                {{ api.name }}
              </option>
            </select>
            <button
              type="button"
              class="btn btn-sm btn-square btn-ghost settings-icon-btn"
              :class="{ 'is-selected': apiFormMode === 'edit' }"
              :aria-expanded="apiFormMode === 'edit'"
              :aria-label="`编辑端点「${activeClashApi?.name ?? ''}」`"
              title="编辑端点"
              @click="toggleApiForm('edit')"
            >
              <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                <path d="m13.8 3.7 2.5 2.5M4 16l.7-3.3L13.9 3.5a1.8 1.8 0 0 1 2.6 0l.1.1a1.8 1.8 0 0 1 0 2.6l-9.2 9.2L4 16Z" />
              </svg>
            </button>
            <button
              type="button"
              class="btn btn-sm btn-square btn-ghost settings-icon-btn"
              :class="{ 'is-selected': apiFormMode === 'add' }"
              :aria-expanded="apiFormMode === 'add'"
              aria-label="新增端点"
              title="新增端点"
              @click="toggleApiForm('add')"
            >
              <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                <path d="M10 4v12M4 10h12" />
              </svg>
            </button>
          </div>
        </div>

        <Transition name="settings-reveal" mode="out-in">
          <form
            v-if="apiFormMode"
            :key="apiFormMode"
            class="settings-row settings-row-stack"
            novalidate
            :aria-labelledby="`settings-api-form-${apiFormMode}`"
            @submit.prevent="handleSaveApi"
            @keydown.escape.prevent="closeApiForm"
          >
            <div class="settings-form-heading">
              <strong :id="`settings-api-form-${apiFormMode}`">{{ apiFormMode === 'edit' ? '编辑端点' : '新增端点' }}</strong>
              <span>{{ apiFormMode === 'edit' ? '保存后面板会立即重新连接。' : '新增后会自动切换到这个端点。' }}主机里可以直接粘贴完整地址，如 http://127.0.0.1:9090。</span>
            </div>
            <label class="settings-field">
              <span>名称</span>
              <input ref="apiNameInput" v-model="apiForm.name" type="text" class="input input-sm input-bordered" :placeholder="apiFormMode === 'edit' ? activeClashApi?.name : `端点 ${clashApis.length + 1}`" />
            </label>
            <div class="settings-endpoint-fields">
              <label class="settings-field">
                <span>协议</span>
                <select v-model="apiForm.protocol" class="select select-sm select-bordered settings-mono">
                  <option value="http">http</option>
                  <option value="https">https</option>
                </select>
              </label>
              <label class="settings-field">
                <span>主机</span>
                <input
                  ref="apiHostInput"
                  v-model="apiForm.host"
                  type="text"
                  class="input input-sm input-bordered settings-mono"
                  placeholder="127.0.0.1"
                  spellcheck="false"
                  :aria-invalid="!!apiFormErrors.host"
                  :aria-describedby="apiFormErrors.host ? 'settings-api-host-error' : undefined"
                  @input="clearApiFieldError('host')"
                />
                <small v-if="apiFormErrors.host" id="settings-api-host-error" class="settings-field-error">{{ apiFormErrors.host }}</small>
              </label>
              <label class="settings-field">
                <span>端口</span>
                <input
                  ref="apiPortInput"
                  v-model="apiForm.port"
                  type="text"
                  inputmode="numeric"
                  class="input input-sm input-bordered settings-mono"
                  placeholder="9090"
                  :aria-invalid="!!apiFormErrors.port"
                  :aria-describedby="apiFormErrors.port ? 'settings-api-port-error' : undefined"
                  @input="clearApiFieldError('port')"
                />
                <small v-if="apiFormErrors.port" id="settings-api-port-error" class="settings-field-error">{{ apiFormErrors.port }}</small>
              </label>
            </div>
            <label class="settings-field">
              <span>访问密钥</span>
              <input v-model="apiForm.secret" type="password" class="input input-sm input-bordered settings-mono" placeholder="留空表示无密钥" />
            </label>
            <div class="settings-form-actions" :class="{ 'settings-form-actions-between': apiFormMode === 'edit' }">
              <button
                v-if="apiFormMode === 'edit'"
                type="button"
                class="btn btn-sm btn-ghost settings-danger-action"
                :disabled="clashApis.length <= 1"
                :title="clashApis.length <= 1 ? '至少要保留一个端点' : undefined"
                @click="handleRemoveActiveApi"
              >删除端点</button>
              <button type="submit" class="btn btn-sm settings-btn">{{ apiFormMode === 'edit' ? '保存端点' : '新增并切换' }}</button>
            </div>
          </form>
        </Transition>
      </div>
    </section>

    <section class="settings-group">
      <h2 class="settings-group-title">网络测试</h2>
      <div class="settings-card">
        <div class="settings-row settings-row-stack">
          <label for="settings-latency-url" class="settings-row-copy">
            <strong>默认测速地址</strong>
            <span id="settings-latency-url-desc">没有单独指定测试地址的代理组会使用这里的 URL。留空则恢复默认地址。</span>
          </label>
          <input
            id="settings-latency-url"
            v-model="latencyUrlField.draft.value"
            type="text"
            class="input input-sm input-bordered settings-mono"
            :placeholder="DEFAULT_LATENCY_URL"
            spellcheck="false"
            :aria-invalid="!!latencyUrlField.error.value"
            aria-describedby="settings-latency-url-desc settings-latency-url-feedback"
            @focus="latencyUrlField.onFocus"
            @input="latencyUrlField.onInput"
            @blur="latencyUrlField.commit"
            @keydown.enter.prevent="latencyUrlField.commit"
            @keydown.escape.prevent="latencyUrlField.revert"
          />
          <p id="settings-latency-url-feedback" class="settings-field-feedback" :class="{ 'is-error': latencyUrlField.error.value }" role="status">
            {{ latencyUrlField.error.value || latencyUrlField.notice.value }}
          </p>
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
            <div v-show="groupTestUrlsExpanded" ref="mappingListRef" class="settings-mapping-list">
              <template v-for="row in groupTestUrlRows" :key="`${row.kind}:${row.group}`">
              <div v-if="row.kind === 'removed'" class="settings-mapping-undo">
                <span>已删除「{{ row.group }}」的测速地址</span>
                <button ref="mappingUndoButton" type="button" class="settings-undo" :aria-label="`撤销删除「${row.group}」的测速地址`" @click="undoGroupTestUrlRemoval">撤销</button>
              </div>
              <div v-else class="settings-mapping-row" :data-mapping-group="row.group">
                <template v-if="editingGroupTestUrl === row.group">
                  <select v-model="editGroupTestUrlGroup" class="select select-sm select-bordered" :aria-label="`「${row.group}」改用的代理组`">
                    <option v-for="name in editAvailableGroups" :key="name" :value="name">{{ name }}</option>
                  </select>
                  <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                  <input
                    v-model="editGroupTestUrlValue"
                    type="text"
                    class="input input-sm input-bordered settings-mono"
                    :aria-label="`「${row.group}」的测速地址`"
                    @keyup.enter="saveEditGroupTestUrl"
                    @keyup.escape="cancelEditGroupTestUrl"
                  />
                  <button class="btn btn-ghost btn-sm btn-square settings-icon-btn" title="保存" :aria-label="`保存「${row.group}」的测速地址`" @click="saveEditGroupTestUrl">
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m4 10 4 4 8-9" /></svg>
                  </button>
                  <button class="btn btn-ghost btn-sm btn-square settings-icon-btn" title="取消编辑" :aria-label="`取消编辑「${row.group}」的测速地址`" @click="cancelEditGroupTestUrl">
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
                  </button>
                </template>
                <template v-else>
                  <span class="settings-group-badge" :title="row.group">{{ row.group }}</span>
                  <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                  <span class="settings-mapping-url settings-mono" :title="row.url">{{ row.url }}</span>
                  <button class="btn btn-ghost btn-sm btn-square settings-icon-btn" title="编辑" :aria-label="`编辑「${row.group}」的测速地址`" @click="startEditGroupTestUrl(row.group)">
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m13.8 3.7 2.5 2.5M4 16l.7-3.3L13.9 3.5a1.8 1.8 0 0 1 2.6 0l.1.1a1.8 1.8 0 0 1 0 2.6l-9.2 9.2L4 16Z" /></svg>
                  </button>
                  <button class="btn btn-ghost btn-sm btn-square settings-icon-btn settings-danger-action" title="删除" :aria-label="`删除「${row.group}」的测速地址`" @click="removeGroupTestUrl(row.group)">
                    <svg class="settings-button-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M5 6h10m-7-3h4l1 3H7l1-3Zm-1 6 .5 7m5.5-7-.5 7M6 6l1 11h6l1-11" /></svg>
                  </button>
                </template>
              </div>
              </template>

              <div class="settings-mapping-row settings-mapping-new">
                <select v-model="newGroupTestUrl.group" class="select select-sm select-bordered" aria-label="新映射的代理组">
                  <option value="" disabled hidden>选择代理组</option>
                  <option v-for="name in availableGroups" :key="name" :value="name">{{ name }}</option>
                </select>
                <span class="settings-mapping-arrow" aria-hidden="true">→</span>
                <input
                  v-model="newGroupTestUrl.url"
                  type="text"
                  class="input input-sm input-bordered settings-mono"
                  placeholder="测速地址"
                  aria-label="新映射的测速地址"
                  @keyup.enter="addGroupTestUrl"
                />
                <button class="btn btn-sm settings-btn" @click="addGroupTestUrl">添加映射</button>
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
          <SegmentedControl
            :model-value="config.theme"
            :options="themeOptions"
            labelledby="settings-theme-label"
            @update:model-value="updateConfig({ theme: $event })"
          />
        </div>
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
                  <span>作用于顶部栏、侧栏、卡片和弹窗。往左更通透，偏通透时带折射；往右底色更重、文字更易读。</span>
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
                  <span v-if="hasBackground">显示在窗口背后，玻璃偏通透时会折射它。</span>
                  <span v-else-if="removedBackground">
                    背景已删除，改用系统默认材质。<button ref="backgroundUndoButton" type="button" class="settings-undo" aria-label="撤销删除背景" :disabled="backgroundBusy" @click="undoBackgroundRemoval">撤销</button>
                  </span>
                  <span v-else>未设置，使用系统默认材质。</span>
                </div>
                <div class="settings-background-actions">
                  <img v-if="backgroundUrl" :src="backgroundUrl" alt="" class="settings-background-thumb" />
                  <button ref="backgroundPickButton" type="button" class="btn btn-sm settings-btn" :disabled="backgroundBusy" @click="backgroundInput?.click()">
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
