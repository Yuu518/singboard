import { ref, computed, onUnmounted } from 'vue'
import { queryServiceStatus, syncServiceComponent } from '@/bridge/service'
import { useConfigStore } from './config'
import type { ServiceStatus } from '@/types'

const serviceStatus = ref<ServiceStatus>({ state: 'unknown' })
let pollTimer: ReturnType<typeof setInterval> | null = null
let refCount = 0
let componentSynced = false

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

// 估算核心本轮启动时间(毫秒时间戳);未在运行或无 uptime 信息时返回 null
export function getCoreStartTimestamp(): number | null {
  const status = serviceStatus.value
  if (status.state !== 'running' || typeof status.uptimeSeconds !== 'number') return null
  return Date.now() - status.uptimeSeconds * 1000
}

export function useServiceStore() {
  if (refCount === 0) {
    if (!componentSynced) {
      componentSynced = true
      const { serviceName } = useConfigStore()
      // 静默同步服务宿主组件,必要时迁移/热换(会短暂重启服务)
      syncServiceComponent(serviceName.value)
        .then((result) => {
          if (result === 'migrated' || result === 'updated') {
            console.info(`[service] 服务组件已同步: ${result}`)
          }
        })
        .catch((e) => console.warn('[service] 服务组件同步失败:', e))
    }
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
