<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useServiceStore } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { startCore } from '@/utils/coreControl'
import { isElevationCancelled } from '@/bridge/service'
import type { ServiceStatus } from '@/types'

const props = defineProps<{
  state: ServiceStatus['state']
  pageTitle: string
  stoppedDetail: string
  startingDetail: string
}>()

const router = useRouter()
const { refresh } = useServiceStore()
const { pushToast } = useToastStore()

const notice = computed(() => {
  switch (props.state) {
    case 'stopped':
      return { title: 'sing-box 服务已停止', detail: props.stoppedDetail, action: 'start' as const }
    case 'starting':
      return { title: '正在启动 sing-box…', detail: props.startingDetail, action: null }
    case 'stopping':
      return { title: '正在停止 sing-box…', detail: '服务停止后可以在这里重新启动。', action: null }
    case 'not_installed':
      return { title: '尚未安装 singboard 服务', detail: '在设置中安装服务后，即可启动 sing-box。', action: 'settings' as const }
    default:
      return { title: '无法获取服务状态', detail: '可以在设置中查看服务状态和错误日志。', action: 'settings' as const }
  }
})

const starting = ref(false)

async function handleStart() {
  if (starting.value) return
  starting.value = true
  try {
    await startCore()
  } catch (e: any) {
    if (!isElevationCancelled(e)) pushToast({ message: '启动服务失败: ' + (e?.message || e), type: 'error' }, 6000)
  }
  await refresh()
  starting.value = false
}
</script>

<template>
  <h1 class="sr-only">{{ pageTitle }}</h1>
  <div class="flex flex-col items-center justify-center flex-1 gap-4 px-6" role="status" aria-live="polite">
    <span v-if="!notice.action" class="loading loading-spinner loading-lg text-base-content/40" aria-hidden="true"></span>
    <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1" stroke="currentColor" class="w-16 h-16 text-base-content/40" aria-hidden="true">
      <path stroke-linecap="round" stroke-linejoin="round" d="M5.636 5.636a9 9 0 1012.728 0M12 3v9" />
    </svg>
    <div class="text-center space-y-1">
      <p class="text-lg font-medium text-base-content/80">{{ notice.title }}</p>
      <p class="text-sm text-base-content/65">{{ notice.detail }}</p>
    </div>
    <button
      v-if="notice.action === 'start'"
      type="button"
      class="btn btn-sm btn-primary px-5"
      :disabled="starting"
      @click="handleStart"
    >
      <span v-if="starting" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
      启动服务
    </button>
    <button
      v-else-if="notice.action === 'settings'"
      type="button"
      class="btn btn-sm btn-glass px-5"
      @click="router.push('/settings')"
    >
      前往设置
    </button>
  </div>
</template>
