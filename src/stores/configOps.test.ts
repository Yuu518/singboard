import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'

const runningHash = vi.hoisted(() => ({ value: 'loaded' }))

vi.mock('@/bridge/config', () => ({
  getRemoteConfigPath: vi.fn(async (id: string) => `C:/data/${id}.json`),
  hasConfigBackup: vi.fn(async (path: string) => path.includes('with-backup')),
  getRunningConfigPath: vi.fn(async () => 'C:/data/running-config.json'),
  getFileHash: vi.fn(async () => runningHash.value),
}))

vi.mock('./service', async () => {
  const { ref } = await import('vue')
  return { serviceStatus: ref({ state: 'running' }) }
})

async function load() {
  const ops = await import('./configOps')
  const { serviceStatus } = await import('./service')
  return { ops: ops.useConfigOps(), serviceStatus }
}

function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((r) => { resolve = r })
  return { promise, resolve }
}

describe('config operations', () => {
  beforeEach(() => {
    vi.resetModules()
  })

  it('runs work for one profile one at a time and reports what is running', async () => {
    const { ops } = await load()
    const order: string[] = []
    const gate = deferred()

    const first = ops.runExclusive('a', 'auto-update', async () => {
      order.push('auto start')
      await gate.promise
      order.push('auto end')
    })
    const second = ops.runExclusive('a', 'delete', async () => {
      order.push('delete')
    })
    await Promise.resolve()
    await Promise.resolve()

    expect(ops.running.value.get('a')).toBe('auto-update')
    expect(ops.isBusy('a')).toBe(true)
    gate.resolve()
    await Promise.all([first, second])

    expect(order).toEqual(['auto start', 'auto end', 'delete'])
    expect(ops.running.value.size).toBe(0)
    expect(ops.isBusy('a')).toBe(false)
  })

  it('lets different profiles work in parallel and keeps going after a failure', async () => {
    const { ops } = await load()
    const gate = deferred()
    const slow = ops.runExclusive('a', 'update', () => gate.promise)
    const failing = ops.runExclusive('b', 'update', async () => { throw new Error('boom') })

    await expect(failing).rejects.toThrow('boom')
    expect(ops.running.value.get('a')).toBe('update')
    gate.resolve()
    await slow

    await expect(ops.runExclusive('b', 'restore', async () => 'ok')).resolves.toBe('ok')
  })

  it('keeps a pending restart across pages until the core stops', async () => {
    const { ops, serviceStatus } = await load()
    ops.noteApplied('已切换到「A」，重启核心后生效。')
    expect(ops.pendingRestart.value).toContain('A')

    serviceStatus.value = { state: 'stopped' }
    await nextTick()
    expect(ops.pendingRestart.value).toBe('')

    ops.noteApplied('不会出现')
    expect(ops.pendingRestart.value).toBe('')
  })

  it('drops a pending restart once the core comes back with a new process', async () => {
    const { ops, serviceStatus } = await load()
    serviceStatus.value = { state: 'running', pid: 100 }
    await nextTick()
    ops.noteApplied('重启后生效')
    serviceStatus.value = { state: 'running', pid: 200 }
    await nextTick()
    expect(ops.pendingRestart.value).toBe('')
  })

  it('only asks for a restart when the running config differs from what the core loaded', async () => {
    const { ops } = await load()
    runningHash.value = 'loaded'
    await ops.recordLoadedConfig()

    await ops.noteRunningChanged('切到 B')
    expect(ops.pendingRestart.value).toBe('')

    runningHash.value = 'changed'
    await ops.noteRunningChanged('切到 B')
    expect(ops.pendingRestart.value).toBe('切到 B')

    runningHash.value = 'loaded'
    await ops.noteRunningChanged('又切回 A')
    expect(ops.pendingRestart.value).toBe('')
  })

  it('tracks which profiles have a backup', async () => {
    const { ops } = await load()
    await ops.refreshBackups([
      { id: 'with-backup', name: 'A', type: 'remote', source: 'https://a', autoUpdateInterval: 0 },
      { id: 'plain', name: 'B', type: 'remote', source: 'https://b', autoUpdateInterval: 0 },
      { id: 'local', name: 'C', type: 'local', source: 'C:/with-backup.json', autoUpdateInterval: 0 },
    ])
    expect([...ops.backupIds.value].sort()).toEqual(['local', 'with-backup'])
  })
})
