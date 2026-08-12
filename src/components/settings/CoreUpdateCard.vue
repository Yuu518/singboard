<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useConfigStore } from '@/stores/config'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { useToastStore } from '@/stores/toast'
import { checkCoreUpdate, performCoreUpdate, probeAssetExeHash, type CoreUpdateInfo, type CoreUpdateProgress } from '@/bridge/coreUpdate'
import { getFileHash } from '@/bridge/config'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'

// "资产 digest → 资产内 sing-box.exe 哈希"的缓存（只留最近一条），
// 版本号相同时用本地 exe 哈希与之比对，识别上游重建或本地被手动替换
const INSTALL_RECORD_KEY = 'singboard-core-install-record'

interface InstallRecord {
  assetDigest: string
  exeHash: string
}

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

const { config } = useConfigStore()
const { singboxVersion, detectVersion } = useSingboxVersionStore()
const { pushToast } = useToastStore()

const REPOS: Record<string, string> = {
  official: 'SagerNet/sing-box',
  ref1nd: 'reF1nd/sing-box-releases',
}

const dialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const checking = ref(false)
const verifying = ref(false)
const updating = ref(false)
const latest = ref<CoreUpdateInfo | null>(null)
const progress = ref<CoreUpdateProgress | null>(null)

const repo = computed(() =>
  config.value.coreUpdateSource === 'custom'
    ? config.value.coreUpdateCustomRepo.trim()
    : REPOS[config.value.coreUpdateSource],
)

const latestDisplay = computed(() => latest.value?.version.replace(/^v/, '') ?? '')

// 从版本检测输出（如 "sing-box 1.13.14 ..."）里提取版本号
const currentVersionNumber = computed(() => {
  const match = singboxVersion.value.match(/\d+\.\d+\S*/)
  return match ? match[0] : ''
})

// 不做 semver 比较：不一致即视为可更新（允许换源/降级）
const hasUpdate = computed(() =>
  !!latest.value && latestDisplay.value !== currentVersionNumber.value,
)

// 换源/换通道后旧的检查结果失效（只清除，不自动重新检查）
watch(
  () => [config.value.coreUpdateSource, config.value.coreUpdateCustomRepo, config.value.coreUpdateChannel],
  () => { latest.value = null },
)

const phaseText = computed(() => {
  const p = progress.value
  const prefix = verifying.value ? '正在校验与上游一致性，' : ''
  if (!p) return verifying.value ? '正在校验与上游一致性…' : '准备中…'
  switch (p.phase) {
    case 'download': {
      const mb = (n: number) => (n / 1048576).toFixed(1)
      return p.total > 0
        ? `${prefix}下载中… ${mb(p.downloaded)} MB / ${mb(p.total)} MB`
        : `${prefix}下载中… ${mb(p.downloaded)} MB`
    }
    case 'extract': return `${prefix}正在解压…`
    case 'replace': return '正在替换核心…'
    case 'restart': return '正在重启服务…'
    default: return '更新中…'
  }
})

// 版本号相同时判断本地核心与上游最新资产是否不一致（上游重建或本地被手动替换）。
// 缓存命中直接比对；未命中则下载资产解压计算 exe 哈希（每个 digest 只下载一次）
async function isLocalOutOfSync(info: CoreUpdateInfo): Promise<boolean> {
  const path = config.value.singboxPath.trim()
  // 源未提供资产 digest 时无法缓存校验结果，跳过检测避免每次检查都下载
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
    const exeHash = await probeAssetExeHash({
      assetUrl: info.assetUrl,
      assetSize: info.assetSize,
      mirror: config.value.coreUpdateMirror,
    })
    saveInstallRecord({ assetDigest: info.assetDigest, exeHash })
    return localHash !== exeHash
  } finally {
    verifying.value = false
    progress.value = null
  }
}

async function handleCheck() {
  if (checking.value || updating.value) return
  if (!repo.value || !/^[\w.-]+\/[\w.-]+$/.test(repo.value)) {
    pushToast({ message: '请填写正确的仓库地址（owner/repo）', type: 'error' })
    return
  }
  checking.value = true
  try {
    // 核心文件可能在面板运行期间被替换过，先重新检测当前版本再比较
    const [info] = await Promise.all([
      checkCoreUpdate(repo.value, config.value.coreUpdateChannel),
      detectVersion(),
    ])
    latest.value = info
    const outOfSync = hasUpdate.value ? false : await isLocalOutOfSync(info)
    if (!hasUpdate.value && !outOfSync) {
      pushToast({ message: `当前已是最新版本（${latestDisplay.value}）`, type: 'info' })
      return
    }
    const channelLabel = config.value.coreUpdateChannel === 'testing' ? '测试版' : '稳定版'
    const publishedAt = latest.value.publishedAt
      ? new Date(latest.value.publishedAt).toLocaleString()
      : '未知'
    const message = outOfSync
      ? `当前版本：${singboxVersion.value || '未检测到'}\n最新版本：${latest.value.version}（${channelLabel}）\n发布时间：${publishedAt}\n\n版本号相同，但本地核心与上游最新资产不一致（可能上游重新构建或本地被手动替换）。\n重新安装将覆盖本地核心，自动停止并重启核心服务，是否继续？`
      : `当前版本：${singboxVersion.value || '未检测到'}\n最新版本：${latest.value.version}（${channelLabel}）\n发布时间：${publishedAt}\n\n更新将自动停止并重启核心服务，是否立即更新？`
    const confirmed = await dialogRef.value?.show({
      title: outOfSync ? '本地核心与上游不一致' : '发现新核心版本',
      message,
      confirmText: outOfSync ? '重新安装' : '立即更新',
      cancelText: '取消',
    })
    if (confirmed) {
      await handleUpdate()
    }
  } catch (e) {
    pushToast({ message: `检查更新失败: ${e}`, type: 'error' })
  } finally {
    checking.value = false
  }
}

async function handleUpdate() {
  if (!latest.value || updating.value) return
  if (!config.value.singboxPath.trim()) {
    pushToast({ message: '请先在服务配置中设置 sing-box 路径', type: 'error' })
    return
  }
  updating.value = true
  progress.value = null
  const assetDigest = latest.value.assetDigest
  try {
    const result = await performCoreUpdate({
      assetUrl: latest.value.assetUrl,
      assetSize: latest.value.assetSize,
      mirror: config.value.coreUpdateMirror,
      singboxPath: config.value.singboxPath,
      serviceName: config.value.serviceName,
    })
    pushToast({
      message: `核心已更新至 ${result.version}${result.restarted ? '，服务已重启' : ''}`,
      type: 'info',
    })
    latest.value = null
    // 安装后本地 exe 即该资产内的 exe，直接记录哈希作为校验缓存
    try {
      if (assetDigest) {
        const exeHash = await getFileHash(config.value.singboxPath)
        saveInstallRecord({ assetDigest, exeHash })
      } else {
        localStorage.removeItem(INSTALL_RECORD_KEY)
      }
    } catch { }
    await detectVersion()
  } catch (e) {
    pushToast({ message: `更新失败: ${e}`, type: 'error' })
  } finally {
    updating.value = false
    progress.value = null
  }
}

let unlisten: UnlistenFn | null = null
onMounted(async () => {
  unlisten = await listen<CoreUpdateProgress>('core-update-progress', (event) => {
    progress.value = event.payload
  })
})
onUnmounted(() => {
  unlisten?.()
  unlisten = null
})
</script>

<template>
  <div class="settings-card settings-update-card">
    <header class="settings-update-header">
      <span class="settings-update-mark" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <path d="M8 3h8l4 4v10l-4 4H8l-4-4V7l4-4Z" />
          <circle cx="12" cy="12" r="3" />
        </svg>
      </span>
      <div>
        <h3>sing-box 核心</h3>
        <p>选择上游来源和版本通道，检查或替换核心。</p>
      </div>
      <span class="settings-update-version settings-mono">{{ singboxVersion || '未检测' }}</span>
    </header>

    <div class="settings-update-body">
      <div class="settings-update-source-grid">
        <label class="settings-field">
          <span>更新源</span>
        <select v-model="config.coreUpdateSource" class="select select-sm select-bordered">
          <option value="official">官方核心 (SagerNet/sing-box)</option>
          <option value="ref1nd">reF1nd 核心</option>
          <option value="custom">自定义仓库</option>
        </select>
        </label>
        <label class="settings-field">
          <span>版本通道</span>
        <select v-model="config.coreUpdateChannel" class="select select-sm select-bordered">
          <option value="stable">稳定版</option>
          <option value="testing">测试版</option>
        </select>
        </label>
      </div>

      <label v-if="config.coreUpdateSource === 'custom'" class="settings-field">
        <span>GitHub 仓库</span>
        <input
          v-model="config.coreUpdateCustomRepo"
          type="text"
          class="input input-sm input-bordered settings-mono"
          placeholder="owner/repo"
        />
      </label>

      <label class="settings-field">
        <span>下载镜像前缀 <small>可选</small></span>
        <input
          v-model="config.coreUpdateMirror"
          type="text"
          class="input input-sm input-bordered settings-mono"
          placeholder="https://ghproxy.com/（留空直连，仅用于下载）"
        />
      </label>

      <div class="settings-update-footer">
        <div class="settings-update-status">
          <span v-if="latest">可用版本 <strong class="settings-mono">{{ latestDisplay }}</strong></span>
          <span v-else>尚未检查上游版本</span>
          <span v-if="latest?.prerelease" class="badge badge-warning badge-xs">预发布</span>
        </div>
        <button
          class="btn btn-sm btn-route"
          :class="{ loading: checking }"
          :disabled="checking || updating"
          @click="handleCheck"
        >
          检查更新
        </button>
      </div>

      <div v-if="updating || verifying" class="settings-update-progress">
        <div>{{ phaseText }}</div>
        <progress
          v-if="progress?.phase === 'download' && progress.total > 0"
          class="progress progress-primary w-full"
          :value="progress.downloaded"
          :max="progress.total"
        />
        <progress v-else class="progress progress-primary w-full" />
      </div>
    </div>

    <ConfirmDialog ref="dialogRef" />
  </div>
</template>
