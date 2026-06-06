<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { useProxiesStore } from '@/stores/proxies'
import {
  startService,
  stopService,
  installService,
  uninstallService,
  readServiceErrorLog,
  startupTaskExists,
  createStartupTask,
} from '@/bridge/service'
import { getSingboxVersion, validateSingboxConfig, getRunningConfigPath, getRemoteConfigPath, copyToRunningConfig } from '@/bridge/config'
import { normalizeVersionText } from '@/utils/format'
import { open } from '@tauri-apps/plugin-dialog'
import { patchConfig, fetchConfig } from '@/api'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import DnsQueryTool from '@/components/settings/DnsQueryTool.vue'

const {
  config,
  updateConfig,
  clashApis,
  activeClashApi,
  activeClashApiId,
  configProfiles,
  setActiveClashApi,
  addClashApi,
  updateActiveClashApi,
  removeClashApi,
} = useConfigStore()
const { serviceStatus, statusText, refresh } = useServiceStore()
const { pushToast } = useToastStore()
const confirmDialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)

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

const clashMode = ref('Rule')
const clashModeOptions = ref<string[]>(['Rule'])
const singboxVersion = ref('')
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

async function validateBeforeStart(): Promise<boolean> {
  const { singboxPath, workingDir } = config.value
  if (!singboxPath) {
    pushToast({ message: '请先配置 sing-box 路径', type: 'error' })
    return false
  }

  async function resolveActiveConfigPath(): Promise<string | null> {
    const activeId = config.value.activeConfigProfileId
    if (!activeId) return null
    const profile = configProfiles.value.find((p) => p.id === activeId)
    if (!profile) return null

    if (profile.type === 'local') return profile.source

    return await getRemoteConfigPath(profile.id)
  }

  try {
    const activeConfigPath = await resolveActiveConfigPath()

    if (activeConfigPath) {
      await validateSingboxConfig(singboxPath, activeConfigPath, workingDir)
      await copyToRunningConfig(activeConfigPath)
    } else {
      const runningConfigPath = await getRunningConfigPath()
      await validateSingboxConfig(singboxPath, runningConfigPath, workingDir)
    }
    return true
  } catch (e: any) {
    pushToast({ message: '配置文件校验或同步失败:\n' + (e?.message || e), type: 'error' }, 8000)
    return false
  }
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
        if (!(await validateBeforeStart())) return
        await startService(name)
        checkServiceAfterStart()
        break
      case 'restart':
        if (!(await validateBeforeStart())) return
        await stopService(name)
        await new Promise((r) => setTimeout(r, 500))
        await startService(name)
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

async function checkVersion() {
  if (!config.value.singboxPath) return
  try {
    const raw = await getSingboxVersion(config.value.singboxPath)
    singboxVersion.value = normalizeVersionText(raw)
  } catch {
    singboxVersion.value = '获取失败'
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
  <div class="space-y-6 max-w-2xl mx-auto w-full">
    <h1 class="text-xl font-bold">设置</h1>
    <ConfirmDialog ref="confirmDialogRef" />

    <div class="relative bg-base-200 rounded-lg p-4 space-y-3">
      <h2 class="font-semibold text-sm">服务控制</h2>
      <div class="flex items-center gap-2">
        <span class="text-sm">状态:</span>
        <span class="badge" :class="statusColor">{{ statusText }}</span>
      </div>
      <div class="flex gap-2">
        <button
          class="btn btn-sm btn-success"
          :class="{ loading: actionLoading === 'start' }"
          :disabled="serviceStatus.state === 'running'"
          @click="handleServiceAction('start')"
        >
          启动
        </button>
        <button
          class="btn btn-sm btn-warning"
          :class="{ loading: actionLoading === 'restart' }"
          @click="handleServiceAction('restart')"
        >
          重启
        </button>
        <button
          class="btn btn-sm btn-error"
          :class="{ loading: actionLoading === 'stop' }"
          :disabled="serviceStatus.state === 'stopped'"
          @click="handleServiceAction('stop')"
        >
          停止
        </button>
        <div class="divider divider-horizontal"></div>
        <button
          class="btn btn-sm btn-outline"
          :class="{ loading: actionLoading === 'install' }"
          :disabled="serviceStatus.state !== 'not_installed'"
          @click="handleServiceAction('install')"
        >
          安装服务
        </button>
        <button
          class="btn btn-sm btn-outline btn-error"
          :class="{ loading: actionLoading === 'uninstall' }"
          :disabled="serviceStatus.state === 'not_installed'"
          @click="handleServiceAction('uninstall')"
        >
          卸载服务
        </button>
        <button
          class="btn btn-sm btn-square btn-outline relative"
          :class="{ 'btn-primary z-30': showServiceConfigPanel }"
          title="服务配置"
          @click.stop="showServiceConfigPanel = !showServiceConfigPanel"
        >
          ⚙
        </button>
      </div>

      <div
        v-if="showServiceConfigPanel"
        class="fixed inset-0 z-20"
        @click="showServiceConfigPanel = false"
      ></div>
      <div
        v-if="showServiceConfigPanel"
        class="absolute left-4 right-4 top-[calc(100%_-_0.5rem)] z-30 rounded-lg border border-base-300 bg-base-200 p-4 shadow-xl space-y-3"
        @click.stop
      >
        <h3 class="text-sm font-semibold">服务配置</h3>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">服务名称</span></label>
          <input
            v-model="config.serviceName"
            type="text"
            class="input input-sm input-bordered"
            placeholder="sing-box"
          />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">sing-box 可执行文件路径</span></label>
          <div class="flex gap-2">
            <input
              v-model="config.singboxPath"
              type="text"
              class="input input-sm input-bordered flex-1"
              placeholder="C:\sing-box\sing-box.exe"
            />
            <button class="btn btn-sm btn-outline shrink-0" @click="browseSingboxPath">浏览</button>
          </div>
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">sing-box 工作目录</span></label>
          <div class="flex gap-2">
            <input
              v-model="config.workingDir"
              type="text"
              class="input input-sm input-bordered flex-1"
              placeholder="留空则使用配置文件所在目录"
            />
            <button class="btn btn-sm btn-outline shrink-0" @click="browseWorkingDir">浏览</button>
          </div>
        </div>
        <div class="form-control max-w-xs">
          <label class="label"><span class="label-text text-xs">延迟启动时间</span></label>
          <div class="flex items-center gap-2">
            <input
              v-model.number="config.startupDelaySeconds"
              type="number"
              min="0"
              max="3600"
              step="1"
              class="input input-sm input-bordered w-28"
              @change="updateStartupDelay(); syncStartupDelayToTask()"
            />
            <span class="text-xs text-base-content/60">秒</span>
          </div>
        </div>
      </div>
    </div>

    <div class="bg-base-200 rounded-lg p-4 space-y-3">
      <h2 class="font-semibold text-sm">代理模式</h2>
      <div class="form-control max-w-xs">
        <select
          class="select select-sm select-bordered"
          :value="clashMode"
          @change="changeMode(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="mode in clashModeOptions" :key="mode" :value="mode">{{ mode }}</option>
        </select>
      </div>
    </div>

    <div class="bg-base-200 rounded-lg p-4 space-y-3">
      <h2 class="font-semibold text-sm">后端</h2>
      <div class="space-y-2">
        <label class="label py-0"><span class="label-text text-xs">当前后端</span></label>
        <div class="flex items-center gap-2">
          <select
            class="select select-sm select-bordered flex-1"
            :value="activeClashApiId"
            @change="handleSwitchApi(($event.target as HTMLSelectElement).value)"
          >
            <option v-for="api in clashApis" :key="api.id" :value="api.id">
              {{ api.name }} ({{ api.url }})
            </option>
          </select>
          <button
            class="btn btn-sm btn-square btn-outline"
            :class="{ 'btn-primary': showEditApiForm }"
            title="编辑当前后端"
            @click="toggleEditApiForm"
          >
            ✎
          </button>
          <button
            class="btn btn-sm btn-square btn-outline"
            :class="{ 'btn-primary': showAddApiForm }"
            title="新增后端"
            @click="toggleAddApiForm"
          >
            +
          </button>
        </div>
      </div>

      <div v-if="showEditApiForm" class="rounded-md bg-base-100 p-3 space-y-2 border border-base-300">
        <div class="text-xs font-medium text-base-content/70">编辑当前后端</div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">名称</span></label>
          <input v-model="activeApiForm.name" type="text" class="input input-sm input-bordered" placeholder="默认后端" />
        </div>
        <div class="flex gap-2">
          <div class="form-control w-24 shrink-0">
            <label class="label"><span class="label-text text-xs">协议</span></label>
            <select v-model="activeApiForm.protocol" class="select select-sm select-bordered">
              <option value="http">http</option>
              <option value="https">https</option>
            </select>
          </div>
          <div class="form-control flex-1">
            <label class="label"><span class="label-text text-xs">主机</span></label>
            <input v-model="activeApiForm.host" type="text" class="input input-sm input-bordered" placeholder="127.0.0.1" />
          </div>
          <div class="form-control w-24 shrink-0">
            <label class="label"><span class="label-text text-xs">端口</span></label>
            <input v-model="activeApiForm.port" type="text" class="input input-sm input-bordered" placeholder="9090" />
          </div>
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">密钥 (Secret)</span></label>
          <input
            v-model="activeApiForm.secret"
            type="password"
            class="input input-sm input-bordered"
            placeholder="留空表示无密钥"
          />
        </div>
        <div class="flex justify-between gap-2">
          <button
            class="btn btn-sm btn-outline btn-error"
            :disabled="clashApis.length <= 1"
            @click="handleRemoveActiveApi"
          >
            删除当前
          </button>
          <button class="btn btn-sm btn-primary" @click="handleSaveActiveApi">保存当前</button>
        </div>
      </div>

      <div v-if="showAddApiForm" class="rounded-md bg-base-100 p-3 space-y-2 border border-base-300">
        <div class="text-xs font-medium text-base-content/70">新增后端</div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">名称</span></label>
          <input v-model="newApiForm.name" type="text" class="input input-sm input-bordered" placeholder="后端 2" />
        </div>
        <div class="flex gap-2">
          <div class="form-control w-24 shrink-0">
            <label class="label"><span class="label-text text-xs">协议</span></label>
            <select v-model="newApiForm.protocol" class="select select-sm select-bordered">
              <option value="http">http</option>
              <option value="https">https</option>
            </select>
          </div>
          <div class="form-control flex-1">
            <label class="label"><span class="label-text text-xs">主机</span></label>
            <input v-model="newApiForm.host" type="text" class="input input-sm input-bordered" placeholder="127.0.0.1" />
          </div>
          <div class="form-control w-24 shrink-0">
            <label class="label"><span class="label-text text-xs">端口</span></label>
            <input v-model="newApiForm.port" type="text" class="input input-sm input-bordered" placeholder="9090" />
          </div>
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">密钥 (Secret)</span></label>
          <input
            v-model="newApiForm.secret"
            type="password"
            class="input input-sm input-bordered"
            placeholder="留空表示无密钥"
          />
        </div>
        <div class="flex justify-end">
          <button class="btn btn-sm btn-outline" @click="handleAddApi">新增并切换</button>
        </div>
      </div>

      <div class="flex items-center gap-2">
        <button class="btn btn-sm btn-ghost" @click="checkVersion">检测版本</button>
        <span v-if="singboxVersion" class="text-xs text-base-content/60">{{ singboxVersion }}</span>
      </div>
    </div>

    <div class="bg-base-200 rounded-lg p-4 space-y-3">
      <h2 class="font-semibold text-sm">测速设置</h2>
      <div class="form-control">
        <label class="label"><span class="label-text text-xs">测速地址 (URL)</span></label>
        <input
          v-model="config.latencyTestUrl"
          type="text"
          class="input input-sm input-bordered"
          placeholder="https://www.gstatic.com/generate_204"
        />
      </div>
      <div class="space-y-2">
        <div
          class="flex items-center gap-1 cursor-pointer select-none"
          @click="groupTestUrlsExpanded = !groupTestUrlsExpanded"
        >
          <svg
            class="w-3 h-3 transition-transform"
            :class="{ 'rotate-90': groupTestUrlsExpanded }"
            viewBox="0 0 12 12"
          >
            <path fill="currentColor" d="M4 2l5 4-5 4z" />
          </svg>
          <span class="label-text text-xs">组测试链接 ({{ groupTestUrlEntries.length }})</span>
        </div>
        <div v-show="groupTestUrlsExpanded" class="space-y-2 pl-4">
          <div
            v-for="[group, url] in groupTestUrlEntries"
            :key="group"
            class="flex items-center gap-1.5"
          >
            <template v-if="editingGroupTestUrl === group">
              <select
                v-model="editGroupTestUrlGroup"
                class="select select-xs select-bordered w-28 shrink-0"
              >
                <option v-for="name in editAvailableGroups" :key="name" :value="name">{{ name }}</option>
              </select>
              <svg class="w-3 h-3 shrink-0 text-base-content/30" viewBox="0 0 12 12">
                <path d="M2 6h6M6 3l3 3-3 3" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
              <input
                v-model="editGroupTestUrlValue"
                type="text"
                class="input input-xs input-bordered flex-1"
                @keyup.enter="saveEditGroupTestUrl"
                @keyup.escape="editingGroupTestUrl = null"
              />
              <button class="btn btn-ghost btn-xs btn-square min-h-0 h-5 w-5" @click="saveEditGroupTestUrl" title="保存">
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                </svg>
              </button>
            </template>
            <template v-else>
              <span class="badge badge-sm badge-outline gap-1 shrink-0">
                {{ group }}
                <button class="text-base-content/40 hover:text-error" @click="removeGroupTestUrl(group)">×</button>
              </span>
              <svg class="w-3 h-3 shrink-0 text-base-content/30" viewBox="0 0 12 12">
                <path d="M2 6h6M6 3l3 3-3 3" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
              <span class="text-xs truncate flex-1" :title="url">{{ url }}</span>
              <button class="btn btn-ghost btn-xs btn-square min-h-0 h-5 w-5" @click="startEditGroupTestUrl(group)" title="编辑">
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L6.832 19.82a4.5 4.5 0 01-1.897 1.13l-2.685.8.8-2.685a4.5 4.5 0 011.13-1.897L16.863 4.487zm0 0L19.5 7.125" />
                </svg>
              </button>
            </template>
            <button class="btn btn-ghost btn-xs btn-square min-h-0 h-5 w-5 text-error/60 hover:text-error" @click="removeGroupTestUrl(group)" title="删除">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
              </svg>
            </button>
          </div>
          <div class="flex items-center gap-1.5">
            <select
              v-model="newGroupTestUrl.group"
              class="select select-xs select-bordered w-28 shrink-0"
            >
              <option value="" disabled hidden></option>
              <option v-for="name in availableGroups" :key="name" :value="name">{{ name }}</option>
            </select>
            <svg class="w-3 h-3 shrink-0 text-base-content/30" viewBox="0 0 12 12">
              <path d="M2 6h6M6 3l3 3-3 3" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            <input
              v-model="newGroupTestUrl.url"
              type="text"
              class="input input-xs input-bordered flex-1"
              placeholder="测速地址"
              @keyup.enter="addGroupTestUrl"
            />
            <button
              class="btn btn-ghost btn-xs btn-square min-h-0 h-5 w-5"
              @click="addGroupTestUrl"
              title="添加"
            >+</button>
          </div>
        </div>
      </div>
      <div class="form-control">
        <div class="label justify-start gap-2">
          <input
            type="checkbox"
            class="toggle toggle-sm toggle-primary"
            v-model="config.ipv6TestEnabled"
          />
          <span class="label-text text-xs">IPv6 连通性测试</span>
        </div>
      </div>
    </div>

    <DnsQueryTool />

    <div class="bg-base-200 rounded-lg p-4 space-y-3">
      <h2 class="font-semibold text-sm">外观</h2>
      <div class="flex gap-2">
        <button
          v-for="t in ['light', 'dark', 'dracula', 'nord']"
          :key="t"
          class="btn btn-sm"
          :class="config.theme === t ? 'btn-primary' : 'btn-ghost'"
          @click="updateConfig({ theme: t })"
        >
          {{ t }}
        </button>
      </div>
    </div>

    <div class="bg-base-200 rounded-lg p-4 space-y-3">
      <h2 class="font-semibold text-sm">窗口行为</h2>
      <div class="form-control">
        <div class="label justify-start gap-2">
          <input
            type="checkbox"
            class="toggle toggle-sm toggle-primary"
            v-model="config.closeToTray"
          />
          <span class="label-text text-xs">关闭时隐藏到系统托盘</span>

        </div>
      </div>
    </div>
  </div>
</template>
