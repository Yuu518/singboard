<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useServiceStore } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { stopService, isElevationCancelled } from '@/bridge/service'
import { startCore } from '@/utils/coreControl'
import { formatUptime } from '@/utils/format'
import { navItems } from './navItems'

const route = useRoute()
const router = useRouter()
const { serviceStatus, statusText, refresh } = useServiceStore()
const { pushToast } = useToastStore()

const currentPath = computed(() => route.path)

function navigate(path: string) {
  router.push(path)
}

const uptimeText = computed(() => {
  if (serviceStatus.value.state !== 'running') return ''
  const s = serviceStatus.value.uptimeSeconds
  return typeof s === 'number' ? formatUptime(s) : ''
})

const togglingService = ref(false)
const pillBusy = computed(() =>
  togglingService.value
  || serviceStatus.value.state === 'starting'
  || serviceStatus.value.state === 'stopping',
)

async function toggleService() {
  if (togglingService.value) return
  const state = serviceStatus.value.state
  if (state !== 'running' && state !== 'stopped') return
  togglingService.value = true
  try {
    if (serviceStatus.value.state === 'running') {
      await stopService()
    } else {
      await startCore()
    }
  } catch (e: any) {
    if (!isElevationCancelled(e)) pushToast({ message: '服务操作失败: ' + (e?.message || e), type: 'error' }, 6000)
  }
  await refresh()
  togglingService.value = false
}

const statusPillClass = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return 'text-success'
    case 'stopped': return 'text-error'
    case 'starting':
    case 'stopping': return 'text-warning'
    default: return 'text-base-content/60'
  }
})
</script>

<template>
  <aside class="glass-float relative m-2 mr-0 flex w-52 shrink-0 flex-col rounded-[var(--radius-panel)]">
    <div class="flex h-12 items-center gap-2.5 px-4 select-none" data-tauri-drag-region>
      <img src="/favicon.png" alt="" class="pointer-events-none h-6 w-6 rounded-md" />
      <span class="pointer-events-none text-[15px] font-semibold tracking-tight">singboard</span>
    </div>

    <nav class="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-1">
      <button
        v-for="item in navItems"
        :key="item.path"
        class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-[13px] transition-colors duration-150"
        :class="
          currentPath === item.path
            ? 'bg-primary/15 font-semibold text-primary'
            : 'text-base-content/75 hover:bg-base-content/[0.06] hover:text-base-content'
        "
        :aria-current="currentPath === item.path ? 'page' : undefined"
        @click="navigate(item.path)"
      >
        <svg
          class="h-[18px] w-[18px] shrink-0"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.75"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path v-for="(d, i) in item.icon" :key="i" :d="d" />
        </svg>
        <span>{{ item.label }}</span>
      </button>
    </nav>

    <div class="p-3">
      <button
        class="surface-fill surface-fill-hover inline-flex max-w-full items-center gap-2 rounded-full px-3 py-1.5 text-xs font-medium"
        :class="statusPillClass"
        :title="serviceStatus.state === 'running' ? '点击停止服务' : '点击启动服务'"
        :disabled="pillBusy"
        @click="toggleService"
      >
        <span v-if="pillBusy" class="loading loading-spinner h-3 w-3 shrink-0"></span>
        <span v-else class="h-2 w-2 shrink-0 rounded-full bg-current shadow-[0_0_0_3px_color-mix(in_oklab,currentColor_22%,transparent)]"></span>
        <span v-if="uptimeText" class="tabular-nums text-base-content/80">{{ uptimeText }}</span>
        <span v-else>{{ statusText }}</span>
      </button>
    </div>
  </aside>
</template>
