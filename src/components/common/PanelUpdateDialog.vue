<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { usePanelUpdateStore } from '@/stores/panelUpdate'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import { openReleasePage } from '@/bridge/coreUpdate'
import { failureMessage } from '@/utils/humanError'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'

const PANEL_RELEASES_URL = 'https://github.com/Yuu518/singboard/releases'

// Mounted at the App root, not in the settings card: manual and startup checks
// share this confirm dialog and progress overlay.
const { updating, progress, pendingConfirm, runUpdate } = usePanelUpdateStore()

const dialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)

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
    case 'verify': return '正在校验…'
    case 'replace': return '正在重启面板…'
    default: return '更新中…'
  }
})

function openChangelog() {
  openReleasePage(PANEL_RELEASES_URL).catch((error) => {
    useToastStore().pushToast({ message: failureMessage('打开更新日志失败', error), type: 'error' })
  })
}

watch(pendingConfirm, async (info) => {
  if (!info) return
  pendingConfirm.value = null
  const mirror = useConfigStore().config.value.coreUpdateMirror.trim()
  const restartNote = '更新会关闭面板并自动重新打开，代理服务不受影响。'
  const confirmed = await dialogRef.value?.show({
    title: info.hasUpdate ? '发现新面板版本' : '本地面板和官方发布的不同',
    details: [
      { label: '当前', value: info.currentVersion, mono: true },
      { label: '最新', value: info.latestVersion, mono: true },
      { label: '发布时间', value: info.publishedAt ? new Date(info.publishedAt).toLocaleString() : '未知' },
      ...(mirror ? [{ label: '下载经由', value: mirror, mono: true }] : []),
    ],
    message: info.hasUpdate
      ? restartNote
      : `版本号相同，但本地面板和官方发布的文件不一样，可能是上游重新构建过，或本地文件被替换过。重新安装会覆盖当前面板。${restartNote}`,
    warning: !info.assetDigest && mirror
      ? '这个版本没有提供文件校验值，经镜像下载时无法确认文件没有被改动。'
      : '',
    link: { label: '查看更新日志', open: openChangelog },
    confirmText: info.hasUpdate ? '立即更新' : '重新安装',
    cancelText: '稍后',
  })
  if (confirmed) {
    await runUpdate()
  }
})
</script>

<template>
  <ConfirmDialog ref="dialogRef" />

  <!-- Covers the titlebar too, so the window cannot be closed mid-update. -->
  <Transition name="glass-pop">
  <div v-if="updating" class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4">
    <div class="glass-popover w-full max-w-sm rounded-[var(--radius-panel)] p-5 space-y-3">
      <h2 class="text-lg font-semibold">正在更新面板</h2>
      <div class="text-xs text-base-content/70">{{ phaseText }}</div>
      <progress
        v-if="progress?.phase === 'download' && progress.total > 0"
        class="progress progress-primary w-full"
        :value="progress.downloaded"
        :max="progress.total"
      />
      <progress v-else class="progress progress-primary w-full" />
      <p class="text-xs text-base-content/60">请勿关闭面板，更新完成后会自动重新启动。</p>
    </div>
  </div>
  </Transition>
</template>
