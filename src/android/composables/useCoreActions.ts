import { ref } from 'vue'
import { readServiceErrorLog, stopService } from '@/bridge/service'
import {
  resumeClashApi,
  suspendClashApi,
  useServiceStore,
  whenClashApiSettled,
} from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { restartCore, startCore } from '@/android/utils/coreControl'

export type CoreAction = 'start' | 'stop' | 'restart'

const busy = ref<CoreAction | ''>('')
let actionGeneration = 0

export function useCoreActions() {
  const { serviceStatus, refresh } = useServiceStore()
  const { pushToast } = useToastStore()

  async function reportStartFailure(generation: number) {
    const detail = await readServiceErrorLog().catch(() => '')
    if (generation !== actionGeneration) return
    pushToast({
      message: detail ? '核心启动失败:\n' + detail : '核心启动失败或异常退出，请检查配置文件',
      type: 'error',
    }, 10000)
  }

  async function verifyStarted(generation: number) {
    while (serviceStatus.value.state === 'running') {
      const outcome = await whenClashApiSettled()
      if (generation !== actionGeneration) return
      if (outcome === 'ready') return
      if (outcome === 'auth_failed') {
        pushToast({ message: 'Clash API 密钥不匹配，请在设置中检查 Clash API 密钥', type: 'error' }, 8000)
        return
      }
      await refresh()
      if (generation !== actionGeneration) return
    }
    const state = serviceStatus.value.state
    if (state === 'stopped' || state === 'not_installed') await reportStartFailure(generation)
  }

  async function run(action: CoreAction) {
    if (busy.value) return
    busy.value = action
    const generation = ++actionGeneration
    const starting = action !== 'stop'
    let succeeded = false
    if (starting) suspendClashApi()
    try {
      if (action === 'stop') {
        await stopService()
      } else {
        await (action === 'start' ? startCore() : restartCore())
      }
      succeeded = true
    } catch (e: any) {
      pushToast({ message: '操作失败: ' + (e?.message || e), type: 'error' }, 6000)
    } finally {
      await refresh()
      if (starting) resumeClashApi()
      busy.value = ''
    }
    if (starting && succeeded) void verifyStarted(generation)
  }

  return { busy, run }
}
