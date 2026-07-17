<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useConfigStore } from '@/stores/config'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { useToastStore } from '@/stores/toast'
import { checkCoreUpdate, performCoreUpdate, type CoreUpdateInfo, type CoreUpdateProgress } from '@/bridge/coreUpdate'
import { getFileHash } from '@/bridge/config'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'

// 面板每次成功更新核心后记录 { 资产 digest, 安装后 exe 哈希 }，
// 用于识别"版本号相同但上游重新构建"的情况
const INSTALL_RECORD_KEY = 'singboard-core-install-record'

interface InstallRecord {
  assetDigest: string
  exeHash: string
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
  if (!p) return '准备中…'
  switch (p.phase) {
    case 'download': {
      const mb = (n: number) => (n / 1048576).toFixed(1)
      return p.total > 0
        ? `下载中… ${mb(p.downloaded)} MB / ${mb(p.total)} MB`
        : `下载中… ${mb(p.downloaded)} MB`
    }
    case 'extract': return '正在解压…'
    case 'replace': return '正在替换核心…'
    case 'restart': return '正在重启服务…'
    default: return '更新中…'
  }
})

// 版本号相同时判断上游是否重新构建：本地核心必须仍是面板安装的那一份
// （exe 哈希与记录一致），且上游资产 digest 与安装时不同
async function isUpstreamRebuilt(info: CoreUpdateInfo): Promise<boolean> {
  const record = loadInstallRecord()
  const path = config.value.singboxPath.trim()
  if (!record || !info.assetDigest || !path) return false
  if (info.assetDigest === record.assetDigest) return false
  try {
    return await getFileHash(path) === record.exeHash
  } catch {
    return false
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
    const rebuilt = hasUpdate.value ? false : await isUpstreamRebuilt(info)
    if (!hasUpdate.value && !rebuilt) {
      pushToast({ message: `当前已是最新版本（${latestDisplay.value}）`, type: 'info' })
      return
    }
    const channelLabel = config.value.coreUpdateChannel === 'testing' ? '测试版' : '稳定版'
    const publishedAt = latest.value.publishedAt
      ? new Date(latest.value.publishedAt).toLocaleString()
      : '未知'
    const message = rebuilt
      ? `当前版本：${singboxVersion.value || '未检测到'}\n最新版本：${latest.value.version}（${channelLabel}）\n发布时间：${publishedAt}\n\n版本号相同，但上游已重新构建该版本（资产指纹变化）。\n重新安装将自动停止并重启核心服务，是否继续？`
      : `当前版本：${singboxVersion.value || '未检测到'}\n最新版本：${latest.value.version}（${channelLabel}）\n发布时间：${publishedAt}\n\n更新将自动停止并重启核心服务，是否立即更新？`
    const confirmed = await dialogRef.value?.show({
      title: rebuilt ? '上游已重新构建当前版本' : '发现新核心版本',
      message,
      confirmText: rebuilt ? '重新安装' : '立即更新',
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
    // 记录安装来源指纹，供后续识别"同版本上游重新构建"
    try {
      if (assetDigest) {
        const exeHash = await getFileHash(config.value.singboxPath)
        localStorage.setItem(INSTALL_RECORD_KEY, JSON.stringify({ assetDigest, exeHash } satisfies InstallRecord))
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
  <div class="bg-base-200 rounded-lg p-4 space-y-3">
    <h2 class="font-semibold text-sm">核心更新</h2>

    <div class="flex gap-2">
      <div class="form-control flex-1">
        <label class="label"><span class="label-text text-xs">更新源</span></label>
        <select v-model="config.coreUpdateSource" class="select select-sm select-bordered">
          <option value="official">官方核心 (SagerNet/sing-box)</option>
          <option value="ref1nd">reF1nd 核心</option>
          <option value="custom">自定义仓库</option>
        </select>
      </div>
      <div class="form-control w-28 shrink-0">
        <label class="label"><span class="label-text text-xs">版本通道</span></label>
        <select v-model="config.coreUpdateChannel" class="select select-sm select-bordered">
          <option value="stable">稳定版</option>
          <option value="testing">测试版</option>
        </select>
      </div>
    </div>

    <div v-if="config.coreUpdateSource === 'custom'" class="form-control">
      <label class="label"><span class="label-text text-xs">GitHub 仓库</span></label>
      <input
        v-model="config.coreUpdateCustomRepo"
        type="text"
        class="input input-sm input-bordered"
        placeholder="owner/repo"
      />
    </div>

    <div class="form-control">
      <label class="label"><span class="label-text text-xs">下载镜像前缀（可选）</span></label>
      <input
        v-model="config.coreUpdateMirror"
        type="text"
        class="input input-sm input-bordered"
        placeholder="https://ghproxy.com/（留空直连，仅用于下载）"
      />
    </div>

    <div class="flex items-center gap-2 text-xs text-base-content/70">
      <span>当前版本: {{ singboxVersion || '未检测到' }}</span>
      <template v-if="latest">
        <span>→</span>
        <span>最新版本: {{ latestDisplay }}</span>
        <span v-if="latest.prerelease" class="badge badge-warning badge-xs">预发布</span>
      </template>
    </div>

    <div class="flex items-center gap-2">
      <button
        class="btn btn-sm btn-primary"
        :class="{ loading: checking }"
        :disabled="checking || updating"
        @click="handleCheck"
      >
        检查更新
      </button>
    </div>

    <div v-if="updating" class="space-y-1">
      <div class="text-xs text-base-content/70">{{ phaseText }}</div>
      <progress
        v-if="progress?.phase === 'download' && progress.total > 0"
        class="progress progress-primary w-full"
        :value="progress.downloaded"
        :max="progress.total"
      />
      <progress v-else class="progress progress-primary w-full" />
    </div>

    <ConfirmDialog ref="dialogRef" />
  </div>
</template>
