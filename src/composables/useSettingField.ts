import { onBeforeUnmount, ref, watch, type Ref } from 'vue'

export interface UseSettingFieldOptions {
  source: Ref<string>
  validate?: (value: string) => string | null
  normalize?: (value: string) => string
  savedText?: (value: string) => string
  savedVisibleMs?: number
}

export function useSettingField(options: UseSettingFieldOptions) {
  const { source } = options
  const draft = ref(source.value)
  const error = ref('')
  const notice = ref('')
  let focused = false
  let noticeTimer: ReturnType<typeof setTimeout> | null = null

  function clearNotice() {
    if (noticeTimer) clearTimeout(noticeTimer)
    noticeTimer = null
    notice.value = ''
  }

  watch(source, (value) => {
    if (!focused) draft.value = value
  })

  function onFocus() {
    focused = true
  }

  function onInput() {
    error.value = ''
    clearNotice()
  }

  function commit(): boolean {
    focused = false
    const value = (options.normalize ?? ((text: string) => text.trim()))(draft.value)
    if (value === source.value) {
      draft.value = value
      error.value = ''
      return true
    }
    const problem = options.validate?.(value) ?? null
    if (problem) {
      error.value = problem
      clearNotice()
      return false
    }
    source.value = value
    draft.value = value
    error.value = ''
    clearNotice()
    notice.value = options.savedText?.(value) ?? '已保存'
    noticeTimer = setTimeout(clearNotice, options.savedVisibleMs ?? 4000)
    return true
  }

  function revert() {
    draft.value = source.value
    error.value = ''
    clearNotice()
  }

  onBeforeUnmount(clearNotice)

  return { draft, error, notice, onFocus, onInput, commit, revert }
}

export function validateHttpUrl(value: string, label: string): string | null {
  if (!value) return null
  let parsed: URL
  try {
    parsed = new URL(value)
  } catch {
    return `${label}要以 http:// 或 https:// 开头`
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return `${label}要以 http:// 或 https:// 开头`
  if (!parsed.hostname) return `${label}缺少主机名`
  return null
}

export function validateRepo(value: string): string | null {
  return /^[\w.-]+\/[\w.-]+$/.test(value) ? null : '格式应为 owner/repo，例如 SagerNet/sing-box'
}

export function validateMirror(value: string): string | null {
  const problem = validateHttpUrl(value, '镜像前缀')
  if (problem) return problem
  if (!value) return null
  const parsed = new URL(value)
  if (parsed.search || parsed.hash) return '镜像前缀里不能带 ? 或 #'
  return null
}
