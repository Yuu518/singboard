<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'
import { usePanelUpdateStore } from '@/stores/panelUpdate'

const { config } = useConfigStore()
const { currentVersion, latest, checking, updating, check } = usePanelUpdateStore()

const latestDisplay = computed(() =>
  latest.value?.hasUpdate ? latest.value.latestVersion : '',
)
</script>

<template>
  <div class="bg-base-200 rounded-lg p-4 space-y-3">
    <h2 class="font-semibold text-sm">面板更新</h2>

    <div class="form-control">
      <div class="label justify-start gap-2">
        <input
          type="checkbox"
          class="toggle toggle-sm toggle-primary"
          v-model="config.panelAutoCheckUpdate"
        />
        <span class="label-text text-xs">启动时自动检查更新</span>
      </div>
    </div>

    <div class="flex items-center gap-2 text-xs text-base-content/70">
      <span>当前版本: {{ currentVersion || '未知' }}</span>
      <template v-if="latestDisplay">
        <span>→</span>
        <span>最新版本: {{ latestDisplay }}</span>
      </template>
      <span v-else-if="latest?.outOfSync" class="badge badge-warning badge-xs">与上游不一致</span>
    </div>

    <div class="flex items-center gap-2">
      <button
        class="btn btn-sm btn-primary"
        :class="{ loading: checking }"
        :disabled="checking || updating"
        @click="check(false)"
      >
        检查更新
      </button>
      <span class="text-xs text-base-content/60">更新下载复用「核心更新」的镜像设置</span>
    </div>
  </div>
</template>
