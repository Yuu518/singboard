import { effectScope, nextTick, ref } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useSettingField, validateHttpUrl, validateMirror, validateRepo } from './useSettingField'

function setup(initial: string, options: Partial<Parameters<typeof useSettingField>[0]> = {}) {
  const source = ref(initial)
  const scope = effectScope()
  const field = scope.run(() => useSettingField({ source, ...options }))!
  return { source, field }
}

afterEach(() => {
  vi.useRealTimers()
})

describe('useSettingField', () => {
  it('saves only on commit and says so briefly', () => {
    vi.useFakeTimers()
    const { source, field } = setup('a')
    field.onFocus()
    field.draft.value = ' b '
    expect(source.value).toBe('a')
    expect(field.commit()).toBe(true)
    expect(source.value).toBe('b')
    expect(field.notice.value).toBe('已保存')
    vi.advanceTimersByTime(4000)
    expect(field.notice.value).toBe('')
  })

  it('keeps an invalid value out of the setting', () => {
    const { source, field } = setup('SagerNet/sing-box', { validate: validateRepo })
    field.draft.value = 'sagernet'
    expect(field.commit()).toBe(false)
    expect(source.value).toBe('SagerNet/sing-box')
    expect(field.error.value).toContain('owner/repo')
    field.onInput()
    expect(field.error.value).toBe('')
  })

  it('stays quiet when nothing changed', () => {
    const { field } = setup('x')
    expect(field.commit()).toBe(true)
    expect(field.notice.value).toBe('')
  })

  it('uses a custom message for the saved value', () => {
    const { field } = setup('https://a.test', { savedText: (value) => (value ? '已保存' : '已清空，将使用默认地址') })
    field.draft.value = ''
    field.commit()
    expect(field.notice.value).toBe('已清空，将使用默认地址')
  })

  it('follows outside changes unless the field is being edited', async () => {
    const { source, field } = setup('a')
    source.value = 'b'
    await nextTick()
    expect(field.draft.value).toBe('b')
    field.onFocus()
    field.draft.value = 'typing'
    source.value = 'c'
    await nextTick()
    expect(field.draft.value).toBe('typing')
    field.revert()
    expect(field.draft.value).toBe('c')
  })
})

describe('setting validators', () => {
  it('accepts http and https URLs or nothing', () => {
    expect(validateHttpUrl('', '测速地址')).toBeNull()
    expect(validateHttpUrl('https://www.gstatic.com/generate_204', '测速地址')).toBeNull()
    expect(validateHttpUrl('www.gstatic.com', '测速地址')).toBe('测速地址要以 http:// 或 https:// 开头')
    expect(validateHttpUrl('ftp://x.test', '测速地址')).toBe('测速地址要以 http:// 或 https:// 开头')
  })

  it('checks mirror prefixes and repositories', () => {
    expect(validateMirror('https://mirror.example.com/')).toBeNull()
    expect(validateMirror('https://mirror.example.com/?x=1')).toBe('镜像前缀里不能带 ? 或 #')
    expect(validateRepo('reF1nd/sing-box-releases')).toBeNull()
    expect(validateRepo('a/b/c')).not.toBeNull()
  })
})
