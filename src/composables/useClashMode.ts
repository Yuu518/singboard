import { computed, ref, watch, type Ref } from 'vue'
import { fetchConfig, patchConfig } from '@/api'
import { getRequestErrorReason } from '@/utils/requestError'
import type { ClashConfig } from '@/types'

export type ClashModeStatus = 'stopped' | 'loading' | 'ready' | 'error'

const MODE_LABELS: Record<string, string> = {
  rule: '规则',
  global: '全局',
  direct: '直连',
}

export function clashModeLabel(mode: string): string {
  return MODE_LABELS[mode.toLowerCase()] ?? mode
}

export function parseModeOptions(data: Partial<ClashConfig> | null | undefined): { mode: string; options: string[] } {
  const rawList = Array.isArray(data?.['mode-list']) ? data['mode-list'] : Array.isArray(data?.modes) ? data.modes : []
  const list = rawList.filter((mode): mode is string => typeof mode === 'string' && mode.length > 0)
  const current = typeof data?.mode === 'string' ? data.mode : ''
  if (!current) return { mode: '', options: list }
  const matched = list.find((mode) => mode.toLowerCase() === current.toLowerCase())
  if (matched) return { mode: matched, options: list }
  return { mode: current, options: [current, ...list] }
}

export interface UseClashModeOptions {
  isRunning: Ref<boolean>
  endpointKey: Ref<string>
  onError: (message: string) => void
}

export function useClashMode({ isRunning, endpointKey, onError }: UseClashModeOptions) {
  const mode = ref('')
  const options = ref<string[]>([])
  const status = ref<ClashModeStatus>(isRunning.value ? 'loading' : 'stopped')
  const loadError = ref('')
  const switching = ref(false)
  let generation = 0

  function clear(next: ClashModeStatus) {
    mode.value = ''
    options.value = []
    status.value = next
  }

  async function load() {
    const token = ++generation
    loadError.value = ''
    if (!isRunning.value) {
      clear('stopped')
      return
    }
    if (status.value !== 'ready') status.value = 'loading'
    try {
      const { data } = await fetchConfig()
      if (token !== generation) return
      const parsed = parseModeOptions(data)
      if (!parsed.mode) throw new Error('核心没有返回当前模式')
      mode.value = parsed.mode
      options.value = parsed.options
      status.value = 'ready'
    } catch (error) {
      if (token !== generation) return
      loadError.value = getRequestErrorReason(error)
      clear('error')
    }
  }

  async function change(next: string): Promise<void> {
    if (status.value !== 'ready' || switching.value || !next || next === mode.value) return
    const token = ++generation
    const previous = mode.value
    mode.value = next
    switching.value = true
    try {
      await patchConfig({ mode: next })
      if (token !== generation) return
      await load()
    } catch (error) {
      if (token !== generation) return
      mode.value = previous
      onError(`切换到「${clashModeLabel(next)}」失败：${getRequestErrorReason(error)}`)
    } finally {
      switching.value = false
    }
  }

  const disabled = computed(() => status.value !== 'ready' || switching.value)

  const placeholder = computed(() => {
    switch (status.value) {
      case 'stopped': return '未运行'
      case 'loading': return '读取中…'
      case 'error': return '未知'
      default: return ''
    }
  })

  const description = computed(() => {
    switch (status.value) {
      case 'stopped': return '核心未运行，启动后才能查看和切换。'
      case 'loading': return '正在读取核心当前的模式…'
      case 'error': return `无法读取当前模式：${loadError.value}`
      default: return switching.value ? '正在切换…' : '核心当前采用的流量处理策略。'
    }
  })

  watch([isRunning, endpointKey], () => void load(), { immediate: true })

  return { mode, options, status, loadError, switching, disabled, placeholder, description, load, change }
}
