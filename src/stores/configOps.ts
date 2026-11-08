import { ref, shallowRef, watch } from 'vue'
import { getFileHash, getRemoteConfigPath, getRunningConfigPath, hasConfigBackup } from '@/bridge/config'
import { serviceStatus } from './service'
import type { ConfigProfile } from '@/types'

export type ConfigAction = 'select' | 'update' | 'auto-update' | 'delete' | 'restore'

const running = shallowRef(new Map<string, ConfigAction>())
const queues = new Map<string, Promise<unknown>>()
const editingProfileId = ref('')
const pendingRestart = ref('')
const backupIds = shallowRef(new Set<string>())
let loadedHash: string | null = null

async function runningConfigHash(): Promise<string | null> {
  try {
    return await getFileHash(await getRunningConfigPath())
  } catch {
    return null
  }
}

export async function recordLoadedConfig() {
  loadedHash = await runningConfigHash()
  pendingRestart.value = ''
}

export async function noteRunningChanged(message: string) {
  if (serviceStatus.value.state !== 'running') return
  const current = await runningConfigHash()
  if (loadedHash && current && current === loadedHash) {
    pendingRestart.value = ''
    return
  }
  pendingRestart.value = message
}

const CORE_STOPPED = new Set(['stopped', 'stopping', 'not_installed'])

watch(
  () => serviceStatus.value.state,
  (state, previous) => {
    if (CORE_STOPPED.has(state)) {
      pendingRestart.value = ''
      loadedHash = null
    } else if (state === 'running' && previous !== 'running' && !loadedHash) {
      void recordLoadedConfig()
    }
  },
  { immediate: true },
)

watch(
  () => serviceStatus.value.pid,
  (pid, previous) => {
    if (pid && previous && pid !== previous) void recordLoadedConfig()
  },
)

function setRunning(id: string, action: ConfigAction | null) {
  const next = new Map(running.value)
  if (action) next.set(id, action)
  else next.delete(id)
  running.value = next
}

export function runExclusive<T>(id: string, action: ConfigAction, task: () => Promise<T>): Promise<T> {
  const previous = queues.get(id) ?? Promise.resolve()
  const result = previous.catch(() => undefined).then(async () => {
    setRunning(id, action)
    try {
      return await task()
    } finally {
      setRunning(id, null)
    }
  })
  const tail = result.catch(() => undefined)
  queues.set(id, tail)
  void tail.then(() => {
    if (queues.get(id) === tail) queues.delete(id)
  })
  return result
}

export function isBusy(id: string): boolean {
  return running.value.has(id) || queues.has(id)
}

export function noteApplied(message: string) {
  if (serviceStatus.value.state === 'running') pendingRestart.value = message
}

export function clearPendingRestart() {
  pendingRestart.value = ''
}

export async function refreshBackups(profiles: ConfigProfile[]) {
  const next = new Set<string>()
  await Promise.all(
    profiles.map(async (p) => {
      try {
        const path = p.type === 'remote' ? await getRemoteConfigPath(p.id) : p.source
        if (path && await hasConfigBackup(path)) next.add(p.id)
      } catch { /* 读不到就当没有备份 */ }
    }),
  )
  backupIds.value = next
}

export function useConfigOps() {
  return {
    running,
    editingProfileId,
    pendingRestart,
    backupIds,
    runExclusive,
    isBusy,
    noteApplied,
    noteRunningChanged,
    recordLoadedConfig,
    clearPendingRestart,
    refreshBackups,
  }
}
