import { effectScope, nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { parseStartupDelay, useStartupDelay } from './useStartupDelay'

function setup(options: { exists?: boolean; write?: () => Promise<void> } = {}) {
  const saved = ref(30)
  const taskExists = vi.fn(async () => options.exists ?? true)
  const writeTask = vi.fn(options.write ?? (async () => {}))
  const cancelled = new Error('cancelled')
  const scope = effectScope()
  const state = scope.run(() => useStartupDelay({
    saved,
    taskExists,
    writeTask,
    isCancelled: (error) => error === cancelled,
    describeError: (error) => (error instanceof Error ? error.message : String(error)),
  }))!
  return { state, saved, taskExists, writeTask, cancelled }
}

describe('parseStartupDelay', () => {
  it('accepts whole seconds from 0 to 3600 only', () => {
    expect(parseStartupDelay(' 45 ')).toBe(45)
    expect(parseStartupDelay('0')).toBe(0)
    expect(parseStartupDelay('3600')).toBe(3600)
    expect(parseStartupDelay('3601')).toBeNull()
    expect(parseStartupDelay('1.5')).toBeNull()
    expect(parseStartupDelay('-1')).toBeNull()
    expect(parseStartupDelay('')).toBeNull()
  })
})

describe('useStartupDelay', () => {
  it('does nothing until the user applies', async () => {
    const { state, writeTask, taskExists } = setup()
    state.draft.value = '60'
    await nextTick()
    expect(state.dirty.value).toBe(true)
    expect(taskExists).not.toHaveBeenCalled()
    expect(writeTask).not.toHaveBeenCalled()
  })

  it('writes the task and the setting together when the task exists', async () => {
    const { state, saved, writeTask } = setup()
    state.draft.value = '60'
    const result = await state.apply()
    expect(writeTask).toHaveBeenCalledWith(60)
    expect(saved.value).toBe(60)
    expect(result).toEqual({ kind: 'applied', seconds: 60 })
    expect(state.resultText.value).toBe('已应用：下次登录 60 秒后启动核心')
    expect(state.dirty.value).toBe(false)
  })

  it('only saves the setting when there is no task yet', async () => {
    const { state, saved, writeTask } = setup({ exists: false })
    state.draft.value = '90'
    expect(await state.apply()).toEqual({ kind: 'saved', seconds: 90 })
    expect(writeTask).not.toHaveBeenCalled()
    expect(saved.value).toBe(90)
  })

  it('keeps the old value and the draft when elevation is cancelled', async () => {
    const holder: { cancelled?: Error } = {}
    const { state, saved, cancelled } = setup({ write: async () => { throw holder.cancelled } })
    holder.cancelled = cancelled
    state.draft.value = '60'
    expect(await state.apply()).toEqual({ kind: 'cancelled', seconds: 30 })
    expect(saved.value).toBe(30)
    expect(state.draft.value).toBe('60')
    expect(state.resultText.value).toBe('已取消授权，自启任务仍是 30 秒')
  })

  it('reports a failure in words and keeps the setting unchanged', async () => {
    const { state, saved } = setup({ write: async () => { throw new Error('创建任务计划失败: access denied') } })
    state.draft.value = '60'
    expect(await state.apply()).toEqual({ kind: 'failed', reason: '创建任务计划失败: access denied' })
    expect(saved.value).toBe(30)
  })

  it('refuses invalid input and ignores a second apply while one runs', async () => {
    let release!: () => void
    const { state, writeTask } = setup({ write: () => new Promise<void>((resolve) => { release = resolve }) })
    state.draft.value = '4000'
    expect(state.error.value).toBe('填 0–3600 之间的整数')
    await state.apply()
    expect(writeTask).not.toHaveBeenCalled()

    state.draft.value = '60'
    const first = state.apply()
    await Promise.resolve()
    await state.apply()
    release()
    await first
    expect(writeTask).toHaveBeenCalledTimes(1)
  })

  it('follows outside changes only while the draft is untouched', async () => {
    const { state, saved } = setup()
    saved.value = 45
    await nextTick()
    expect(state.draft.value).toBe('45')
    state.draft.value = '50'
    saved.value = 55
    await nextTick()
    expect(state.draft.value).toBe('50')
    state.revert()
    expect(state.draft.value).toBe('55')
  })
})
