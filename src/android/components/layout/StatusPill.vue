<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useCoreActions } from '@/android/composables/useCoreActions'
import { formatUptime } from '@/utils/format'

const { config } = useConfigStore()
const { serviceStatus, statusText } = useServiceStore()
const { busy, run } = useCoreActions()

const uptimeText = computed(() => {
  if (serviceStatus.value.state !== 'running') return ''
  const s = serviceStatus.value.uptimeSeconds
  return typeof s === 'number' ? formatUptime(s) : ''
})

const pillBusy = computed(() =>
  !!busy.value
  || serviceStatus.value.state === 'starting'
  || serviceStatus.value.state === 'stopping',
)

const interactive = computed(() => config.value.rootMode)

const pillTitle = computed(() => {
  if (!interactive.value) return statusText.value
  return serviceStatus.value.state === 'running' ? '点击停止核心' : '点击启动核心'
})

function toggleService() {
  if (!interactive.value) return
  const state = serviceStatus.value.state
  if (state === 'running') void run('stop')
  else if (state === 'stopped' || state === 'not_installed') void run('start')
}

const statusPillClass = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return 'text-success'
    case 'stopped':
    case 'not_installed': return 'text-error'
    case 'starting':
    case 'stopping': return 'text-warning'
    default: return 'text-base-content/60'
  }
})
</script>

<template>
  <button
    class="glass-press surface-fill surface-fill-hover inline-flex max-w-full items-center gap-2 rounded-full px-3 py-1.5 text-xs font-medium"
    :class="[statusPillClass, { 'pointer-events-none': !interactive }]"
    :title="pillTitle"
    :aria-label="pillTitle"
    :disabled="pillBusy"
    @click="toggleService"
  >
    <span v-if="pillBusy" class="loading loading-spinner h-3 w-3 shrink-0"></span>
    <span v-else class="h-2 w-2 shrink-0 rounded-full bg-current shadow-[0_0_0_3px_color-mix(in_oklab,currentColor_22%,transparent)]"></span>
    <span v-if="uptimeText" class="tabular-nums text-base-content/80">{{ uptimeText }}</span>
    <span v-else>{{ statusText }}</span>
  </button>
</template>
