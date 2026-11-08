import { effectScope, nextTick, ref } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({
  fetchConfig: vi.fn(),
  patchConfig: vi.fn(),
}))

vi.mock('@/api', () => api)

import { clashModeLabel, parseModeOptions, useClashMode } from './useClashMode'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

async function flush() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve()
    await nextTick()
  }
}

function setup(running = true) {
  const isRunning = ref(running)
  const endpointKey = ref('a')
  const onError = vi.fn()
  const scope = effectScope()
  const state = scope.run(() => useClashMode({ isRunning, endpointKey, onError }))!
  return { state, isRunning, endpointKey, onError, scope }
}

const config = (mode: string) => ({ data: { mode, 'mode-list': ['Rule', 'Global', 'Direct'] } })

beforeEach(() => {
  api.fetchConfig.mockReset()
  api.patchConfig.mockReset()
})

describe('clash mode helpers', () => {
  it('localizes the standard modes and keeps custom ones', () => {
    expect(clashModeLabel('Rule')).toBe('规则')
    expect(clashModeLabel('global')).toBe('全局')
    expect(clashModeLabel('Direct')).toBe('直连')
    expect(clashModeLabel('Custom')).toBe('Custom')
  })

  it('never invents a mode the core did not report', () => {
    expect(parseModeOptions({})).toEqual({ mode: '', options: [] })
    expect(parseModeOptions({ mode: 'rule', 'mode-list': ['Rule', 'Global'] })).toEqual({ mode: 'Rule', options: ['Rule', 'Global'] })
    expect(parseModeOptions({ mode: 'Custom', modes: ['Rule'] })).toEqual({ mode: 'Custom', options: ['Custom', 'Rule'] })
  })
})

describe('useClashMode', () => {
  it('does not query or show a mode while the core is stopped', async () => {
    const { state } = setup(false)
    await flush()
    expect(api.fetchConfig).not.toHaveBeenCalled()
    expect(state.status.value).toBe('stopped')
    expect(state.options.value).toEqual([])
    expect(state.disabled.value).toBe(true)
    expect(state.description.value).toContain('核心未运行')
  })

  it('reports a failed read instead of keeping a stale mode', async () => {
    api.fetchConfig.mockResolvedValueOnce(config('Global'))
    const { state, endpointKey } = setup()
    await flush()
    expect(state.mode.value).toBe('Global')

    api.fetchConfig.mockRejectedValueOnce(new Error('connect refused'))
    endpointKey.value = 'b'
    await flush()
    expect(state.status.value).toBe('error')
    expect(state.mode.value).toBe('')
    expect(state.description.value).toContain('connect refused')
  })

  it('rolls back and reports when switching fails', async () => {
    api.fetchConfig.mockResolvedValue(config('Rule'))
    api.patchConfig.mockRejectedValueOnce(new Error('denied'))
    const { state, onError } = setup()
    await flush()

    await state.change('Global')
    expect(state.mode.value).toBe('Rule')
    expect(onError).toHaveBeenCalledWith('切换到「全局」失败：denied')
    expect(state.switching.value).toBe(false)
  })

  it('confirms a successful switch by reading the mode back', async () => {
    api.fetchConfig.mockResolvedValueOnce(config('Rule')).mockResolvedValueOnce(config('Direct'))
    api.patchConfig.mockResolvedValueOnce({})
    const { state } = setup()
    await flush()

    await state.change('Direct')
    expect(api.patchConfig).toHaveBeenCalledWith({ mode: 'Direct' })
    expect(api.fetchConfig).toHaveBeenCalledTimes(2)
    expect(state.mode.value).toBe('Direct')
  })

  it('ignores a switch result that lands after the endpoint changed', async () => {
    api.fetchConfig.mockResolvedValueOnce(config('Rule'))
    const patch = deferred<unknown>()
    api.patchConfig.mockReturnValueOnce(patch.promise)
    const { state, endpointKey, onError } = setup()
    await flush()

    const pending = state.change('Global')
    api.fetchConfig.mockResolvedValueOnce(config('Direct'))
    endpointKey.value = 'b'
    await flush()
    patch.reject(new Error('old endpoint gone'))
    await pending

    expect(onError).not.toHaveBeenCalled()
    expect(state.mode.value).toBe('Direct')
  })

  it('drops a slow read that a newer read superseded', async () => {
    const slow = deferred<unknown>()
    api.fetchConfig.mockReturnValueOnce(slow.promise).mockResolvedValueOnce(config('Global'))
    const { state, endpointKey } = setup()
    endpointKey.value = 'b'
    await flush()
    slow.resolve(config('Rule'))
    await flush()
    expect(state.mode.value).toBe('Global')
  })
})
