import { ref, computed, onUnmounted } from 'vue'
import { queryServiceStatus } from '@/bridge/service'
import { useConfigStore } from './config'
import type { ServiceStatus } from '@/types'

const serviceStatus = ref<ServiceStatus>({ state: 'unknown' })
let pollTimer: ReturnType<typeof setInterval> | null = null
let refCount = 0

let firstPollResolve: (() => void) | null = null
const firstPollReady = new Promise<void>((resolve) => { firstPollResolve = resolve })

async function poll() {
  const { serviceName } = useConfigStore()
  try {
    serviceStatus.value = await queryServiceStatus(serviceName.value)
  } catch {
    serviceStatus.value = { state: 'unknown' }
  }
  if (firstPollResolve) {
    firstPollResolve()
    firstPollResolve = null
  }
}

export function useServiceStore() {
  if (refCount === 0) {
    poll()
    pollTimer = setInterval(poll, 1000)
  }
  refCount++

  onUnmounted(() => {
    refCount--
    if (refCount === 0 && pollTimer) {
      clearInterval(pollTimer)
      pollTimer = null
    }
  })

  const statusText = computed(() => {
    const map: Record<string, string> = {
      running: '运行中',
      stopped: '已停止',
      starting: '启动中',
      stopping: '停止中',
      not_installed: '未安装',
      unknown: '未知',
    }
    return map[serviceStatus.value.state] || '未知'
  })

  return {
    serviceStatus,
    statusText,
    ready: firstPollReady,
    refresh: poll,
  }
}
