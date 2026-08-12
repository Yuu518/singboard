<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'
import { usePanelUpdateStore } from '@/stores/panelUpdate'

const { config } = useConfigStore()
const { currentVersion, latest, checking, updating, check } = usePanelUpdateStore()

const latestDisplay = computed(() =>
  latest.value?.latestVersion ?? '',
)
</script>

<template>
  <div class="settings-card settings-update-card settings-panel-update-card">
    <header class="settings-update-header">
      <span class="settings-update-mark settings-update-mark-panel" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <rect x="4" y="4" width="16" height="13" rx="2" />
          <path d="M9 21h6m-3-4v4M8 9h8m-8 4h5" />
        </svg>
      </span>
      <div>
        <h3>singboard 面板</h3>
        <p>获取界面与桌面集成的最新版本。</p>
      </div>
      <span class="settings-update-version settings-mono">{{ currentVersion || '未知' }}</span>
    </header>

    <div class="settings-update-body settings-panel-update-body">
      <label class="settings-toggle-row settings-update-toggle">
        <span class="settings-row-copy">
          <strong>启动时自动检查</strong>
          <span>打开面板后静默检查是否有新版本。</span>
        </span>
        <input v-model="config.panelAutoCheckUpdate" type="checkbox" class="toggle toggle-sm toggle-primary" />
      </label>

      <div class="settings-update-footer">
        <div class="settings-update-status" role="status" aria-live="polite" aria-atomic="true">
          <span v-if="checking">正在检查上游版本…</span>
          <span v-else-if="latest?.hasUpdate">可用版本 <strong class="settings-mono">{{ latestDisplay }}</strong></span>
          <span v-else-if="latest?.outOfSync" class="badge badge-warning badge-sm">与上游不一致</span>
          <span v-else-if="latest">已是最新版本 <strong class="settings-mono">{{ latestDisplay }}</strong></span>
          <span v-else>尚未检查上游版本</span>
        </div>
        <button
          type="button"
          class="btn btn-sm btn-route"
          :disabled="checking || updating"
          :aria-busy="checking"
          @click="check(false)"
        >
          <span v-if="checking" class="loading loading-spinner loading-xs settings-update-spinner" aria-hidden="true"></span>
          <span>{{ checking ? '检查中' : '检查更新' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
