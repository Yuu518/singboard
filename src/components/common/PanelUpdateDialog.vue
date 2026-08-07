<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { usePanelUpdateStore } from '@/stores/panelUpdate'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'

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

watch(pendingConfirm, async (info) => {
  if (!info) return
  pendingConfirm.value = null
  const publishedAt = info.publishedAt
    ? new Date(info.publishedAt).toLocaleString()
    : '未知'
  const versions = `当前版本：${info.currentVersion}\n最新版本：${info.latestVersion}\n发布时间：${publishedAt}`
  const restartNote = '更新将关闭并自动重启面板，代理服务不受影响。'
  const confirmed = await dialogRef.value?.show({
    title: info.hasUpdate ? '发现新面板版本' : '本地面板与上游不一致',
    message: info.hasUpdate
      ? `${versions}\n\n${restartNote}是否立即更新？`
      : `${versions}\n\n版本号相同，但本地面板与 Release 资产哈希不一致（可能上游重新构建，或本体被手动替换）。\n重新安装将覆盖当前面板，${restartNote}是否继续？`,
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
  <div v-if="updating" class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
    <div class="w-full max-w-sm rounded-lg bg-base-100 p-5 shadow-xl space-y-3">
      <h3 class="text-base font-semibold">正在更新面板</h3>
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
</template>
