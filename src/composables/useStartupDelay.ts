import { computed, ref, watch, type Ref } from 'vue'

export const STARTUP_DELAY_MAX = 3600

export type StartupDelayResult =
  | { kind: 'idle' }
  | { kind: 'applied'; seconds: number }
  | { kind: 'saved'; seconds: number }
  | { kind: 'cancelled'; seconds: number }
  | { kind: 'failed'; reason: string }

export interface UseStartupDelayOptions {
  saved: Ref<number>
  taskExists: () => Promise<boolean>
  writeTask: (seconds: number) => Promise<void>
  isCancelled: (error: unknown) => boolean
  describeError: (error: unknown) => string
}

export function parseStartupDelay(text: string): number | null {
  const trimmed = text.trim()
  if (!/^\d+$/.test(trimmed)) return null
  const value = Number(trimmed)
  return value <= STARTUP_DELAY_MAX ? value : null
}

export function useStartupDelay(options: UseStartupDelayOptions) {
  const { saved } = options
  const draft = ref(String(saved.value))
  const applying = ref(false)
  const result = ref<StartupDelayResult>({ kind: 'idle' })

  const parsed = computed(() => parseStartupDelay(draft.value))
  const error = computed(() => (parsed.value === null ? `填 0–${STARTUP_DELAY_MAX} 之间的整数` : ''))
  const dirty = computed(() => draft.value.trim() !== String(saved.value))

  watch(saved, (value, previous) => {
    if (applying.value) return
    if (draft.value.trim() === String(previous)) draft.value = String(value)
  })

  watch(draft, () => {
    if (!applying.value && result.value.kind !== 'idle') result.value = { kind: 'idle' }
  })

  function revert() {
    if (applying.value) return
    draft.value = String(saved.value)
    result.value = { kind: 'idle' }
  }

  async function apply(): Promise<StartupDelayResult> {
    const seconds = parsed.value
    if (applying.value || seconds === null || !dirty.value) return result.value
    applying.value = true
    let outcome: StartupDelayResult
    try {
      if (await options.taskExists()) {
        await options.writeTask(seconds)
        saved.value = seconds
        outcome = { kind: 'applied', seconds }
      } else {
        saved.value = seconds
        outcome = { kind: 'saved', seconds }
      }
      draft.value = String(seconds)
    } catch (caught) {
      outcome = options.isCancelled(caught)
        ? { kind: 'cancelled', seconds: saved.value }
        : { kind: 'failed', reason: options.describeError(caught) }
    } finally {
      applying.value = false
    }
    result.value = outcome
    return outcome
  }

  const resultText = computed(() => {
    const current = result.value
    switch (current.kind) {
      case 'applied': return `已应用：下次登录 ${current.seconds} 秒后启动核心`
      case 'saved': return `已保存 ${current.seconds} 秒，安装服务时写入`
      case 'cancelled': return `已取消授权，自启任务仍是 ${current.seconds} 秒`
      case 'failed': return `没能更新自启任务：${current.reason}`
      default: return ''
    }
  })

  return { draft, parsed, error, dirty, applying, result, resultText, apply, revert }
}
