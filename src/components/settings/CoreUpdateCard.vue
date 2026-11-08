<script setup lang="ts">
import { computed, ref } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { useCoreUpdateStore } from '@/stores/coreUpdate'
import { useToastStore } from '@/stores/toast'
import { openReleasePage } from '@/bridge/coreUpdate'
import { failureMessage } from '@/utils/humanError'
import { useSettingField, validateMirror, validateRepo } from '@/composables/useSettingField'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'

const { config } = useConfigStore()
const { singboxVersion } = useSingboxVersionStore()
const { pushToast } = useToastStore()
const { checking, verifying, updating, latest, outOfSync, progress, releaseUrl, currentRepo, check, update } = useCoreUpdateStore()

const dialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)

const latestDisplay = computed(() => latest.value?.version.replace(/^v/, '') ?? '')
const currentVersionNumber = computed(() => singboxVersion.value.match(/\d+\.\d+\S*/)?.[0] ?? '')
const hasUpdate = computed(() => !!latest.value && latestDisplay.value !== currentVersionNumber.value)
const busy = computed(() => checking.value || updating.value || verifying.value)

const repoField = useSettingField({
  source: computed({
    get: () => config.value.coreUpdateCustomRepo,
    set: (value: string) => { config.value.coreUpdateCustomRepo = value },
  }),
  validate: validateRepo,
})
const mirrorField = useSettingField({
  source: computed({
    get: () => config.value.coreUpdateMirror,
    set: (value: string) => { config.value.coreUpdateMirror = value },
  }),
  validate: validateMirror,
  savedText: (value) => (value ? '已保存，核心和面板更新都会经过这个镜像' : '已清空，改为直连 GitHub'),
})

const phaseText = computed(() => {
  const p = progress.value
  const prefix = verifying.value ? '正在核对本地核心和官方发布的文件，' : ''
  if (!p) return verifying.value ? '正在核对本地核心和官方发布的文件…' : '准备中…'
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

function openChangelog() {
  const url = releaseUrl.value
  if (!url) return
  openReleasePage(url).catch((error) => {
    pushToast({ message: failureMessage('打开更新日志失败', error), type: 'error' })
  })
}

async function handleCheck() {
  if (busy.value) return
  const outcome = await check()
  const info = latest.value
  if (!info || (outcome !== 'update' && outcome !== 'out-of-sync')) return
  const channelLabel = config.value.coreUpdateChannel === 'testing' ? '测试版通道' : '稳定版通道'
  const mirror = config.value.coreUpdateMirror.trim()
  const confirmed = await dialogRef.value?.show({
    title: outcome === 'out-of-sync' ? '本地核心和官方发布的不同' : '发现新核心版本',
    details: [
      { label: '当前', value: currentVersionNumber.value || '未检测到', mono: true },
      { label: '最新', value: `${latestDisplay.value}${info.prerelease ? ' · 测试版' : ''}`, mono: true },
      { label: '发布时间', value: info.publishedAt ? new Date(info.publishedAt).toLocaleString() : '未知' },
      { label: '来源', value: `${currentRepo()} · ${channelLabel}`, mono: true },
      ...(mirror ? [{ label: '下载经由', value: mirror, mono: true }] : []),
    ],
    message: outcome === 'out-of-sync'
      ? '版本号相同，但本地的 sing-box.exe 和官方发布的文件不一样，可能是上游重新构建过，或本地文件被替换过。重新安装会先停止核心，替换后自动重启，期间代理短暂中断。'
      : '更新会先停止核心，替换后自动重启，期间代理短暂中断。',
    warning: !info.assetDigest && mirror
      ? '这个版本没有提供文件校验值，经镜像下载时无法确认文件没有被改动。'
      : '',
    link: releaseUrl.value ? { label: '查看更新日志', open: openChangelog } : undefined,
    confirmText: outcome === 'out-of-sync' ? '重新安装' : '立即更新',
    cancelText: '稍后',
  })
  if (confirmed) await update()
}
</script>

<template>
  <div class="settings-card">
    <div class="settings-row">
      <div class="settings-row-copy">
        <strong>sing-box 核心</strong>
        <div class="settings-update-status" role="status" aria-live="polite" aria-atomic="true">
          <span v-if="updating">正在更新核心…</span>
          <span v-else-if="verifying">正在核对本地核心和官方发布的文件…</span>
          <span v-else-if="checking">正在检查上游版本…</span>
          <span v-else-if="hasUpdate">可用版本 <strong class="settings-mono">{{ latestDisplay }}</strong></span>
          <template v-else-if="outOfSync">
            <span>版本号相同</span>
            <span class="settings-neutral-pill">本地文件和官方发布的不同</span>
          </template>
          <span v-else-if="latest">已是最新版本 <strong class="settings-mono">{{ latestDisplay }}</strong></span>
          <span v-else>尚未检查上游版本</span>
          <span v-if="latest?.prerelease && !busy" class="settings-neutral-pill">测试版</span>
        </div>
      </div>
      <button
        type="button"
        class="btn btn-sm settings-btn"
        :disabled="busy"
        :aria-busy="checking"
        @click="handleCheck"
      >
        <span v-if="checking" class="loading loading-spinner loading-xs settings-update-spinner" aria-hidden="true"></span>
        <span>{{ checking ? '检查中' : '检查核心更新' }}</span>
      </button>
    </div>

    <div v-if="updating || verifying" class="settings-row settings-row-stack settings-update-progress">
      <div>{{ phaseText }}</div>
      <progress
        v-if="progress?.phase === 'download' && progress.total > 0"
        class="progress progress-primary w-full"
        :value="progress.downloaded"
        :max="progress.total"
        aria-label="核心下载进度"
      />
      <progress v-else class="progress progress-primary w-full" aria-label="核心更新进度" />
    </div>

    <label class="settings-row">
      <span class="settings-row-copy">
        <strong>更新源</strong>
      </span>
      <select v-model="config.coreUpdateSource" class="select select-sm select-bordered settings-row-control" :disabled="busy">
        <option value="official">官方核心 (SagerNet/sing-box)</option>
        <option value="ref1nd">reF1nd 核心</option>
        <option value="custom">自定义仓库</option>
      </select>
    </label>

    <div v-if="config.coreUpdateSource === 'custom'" class="settings-row settings-row-stack">
      <label for="core-update-repo" class="settings-row-copy">
        <strong>GitHub 仓库</strong>
        <span>写成 owner/repo，从它的 Releases 下载 Windows 版核心。</span>
      </label>
      <input
        id="core-update-repo"
        v-model="repoField.draft.value"
        type="text"
        class="input input-sm input-bordered settings-mono"
        placeholder="SagerNet/sing-box"
        spellcheck="false"
        :aria-invalid="!!repoField.error.value"
        aria-describedby="core-update-repo-feedback"
        @focus="repoField.onFocus"
        @input="repoField.onInput"
        @blur="repoField.commit"
        @keydown.enter.prevent="repoField.commit"
        @keydown.escape.prevent="repoField.revert"
      />
      <p id="core-update-repo-feedback" class="settings-field-feedback" :class="{ 'is-error': repoField.error.value }" role="status">
        {{ repoField.error.value || repoField.notice.value }}
      </p>
    </div>

    <label class="settings-row">
      <span class="settings-row-copy">
        <strong>版本通道</strong>
      </span>
      <select v-model="config.coreUpdateChannel" class="select select-sm select-bordered settings-row-control" :disabled="busy">
        <option value="stable">稳定版</option>
        <option value="testing">测试版</option>
      </select>
    </label>

    <div class="settings-row settings-row-stack">
      <label for="core-update-mirror" class="settings-row-copy">
        <strong>下载镜像前缀</strong>
        <span id="core-update-mirror-desc">可选。下载核心和面板更新时都会把它加在 GitHub 地址前面；留空直连 GitHub。镜像能看到并改写下载内容，只填你信任的地址。</span>
      </label>
      <input
        id="core-update-mirror"
        v-model="mirrorField.draft.value"
        type="text"
        class="input input-sm input-bordered settings-mono"
        placeholder="https://mirror.example.com/"
        spellcheck="false"
        :aria-invalid="!!mirrorField.error.value"
        aria-describedby="core-update-mirror-desc core-update-mirror-feedback"
        @focus="mirrorField.onFocus"
        @input="mirrorField.onInput"
        @blur="mirrorField.commit"
        @keydown.enter.prevent="mirrorField.commit"
        @keydown.escape.prevent="mirrorField.revert"
      />
      <p id="core-update-mirror-feedback" class="settings-field-feedback" :class="{ 'is-error': mirrorField.error.value }" role="status">
        {{ mirrorField.error.value || mirrorField.notice.value }}
      </p>
    </div>

    <ConfirmDialog ref="dialogRef" />
  </div>
</template>
