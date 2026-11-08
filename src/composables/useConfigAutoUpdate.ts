import { watch, onUnmounted } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import {
  ConfigRemovedError,
  MAX_UPDATE_INTERVAL_HOURS,
  currentVersionAt,
  describeRefreshError,
  firstLine,
  refreshRemoteConfig,
} from '@/utils/remoteConfig'
import { useConfigOps } from '@/stores/configOps'
import type { ConfigProfile } from '@/types'

const RETRY_DELAYS_MS = [5 * 60_000, 15 * 60_000, 60 * 60_000]
const STARTUP_DELAY_MS = 15_000
const STARTUP_STAGGER_MS = 10_000
const BUSY_RECHECK_MS = 60_000
const FAILURE_BATCH_MS = 12_000

interface ProfileTimers {
  timeout?: ReturnType<typeof setTimeout>
  interval?: ReturnType<typeof setInterval>
  retry?: ReturnType<typeof setTimeout>
}

export function useConfigAutoUpdate() {
  const { config, configProfiles, activeConfigProfileId, updateConfigProfile } = useConfigStore()
  const { pushToast } = useToastStore()
  const ops = useConfigOps()

  const timers = new Map<string, ProfileTimers>()
  const failureCounts = new Map<string, number>()
  let pendingFailures: Array<{ name: string; reason: string }> = []
  let failureFlush: ReturnType<typeof setTimeout> | null = null

  function currentProfile(id: string): ConfigProfile | undefined {
    return configProfiles.value.find((p) => p.id === id)
  }

  function intervalMsOf(profile: ConfigProfile): number {
    return Math.min(profile.autoUpdateInterval, MAX_UPDATE_INTERVAL_HOURS) * 3_600_000
  }

  function reportFailure(name: string, reason: string) {
    pendingFailures.push({ name, reason })
    if (failureFlush) clearTimeout(failureFlush)
    failureFlush = setTimeout(() => {
      const batch = pendingFailures
      pendingFailures = []
      failureFlush = null
      if (batch.length === 1) {
        pushToast({ message: `自动更新「${batch[0].name}」失败，保留现有配置：${batch[0].reason}`, type: 'error' }, 8000)
      } else if (batch.length > 1) {
        pushToast({ message: `${batch.length} 个远程配置自动更新失败，各自保留现有配置：${batch.map((f) => `「${f.name}」`).join('、')}。详情见各自的卡片。`, type: 'error' }, 8000)
      }
    }, FAILURE_BATCH_MS)
  }

  function clearRetry(id: string) {
    const t = timers.get(id)
    if (t?.retry != null) clearTimeout(t.retry)
    if (t) t.retry = undefined
  }

  function scheduleRetry(id: string) {
    const profile = currentProfile(id)
    if (!profile || profile.autoUpdateInterval <= 0) return
    const count = failureCounts.get(id) ?? 0
    failureCounts.set(id, count + 1)
    const delay = Math.min(RETRY_DELAYS_MS[Math.min(count, RETRY_DELAYS_MS.length - 1)], intervalMsOf(profile))
    clearRetry(id)
    const entry = timers.get(id) ?? {}
    entry.retry = setTimeout(() => {
      entry.retry = undefined
      const latest = currentProfile(id)
      if (latest) void updateRemoteProfile(latest)
    }, delay)
    timers.set(id, entry)
  }

  async function updateRemoteProfile(scheduled: ConfigProfile) {
    const id = scheduled.id
    if (ops.editingProfileId.value === id) return
    if (ops.isBusy(id)) {
      const entry = timers.get(id) ?? {}
      clearRetry(id)
      entry.retry = setTimeout(() => {
        entry.retry = undefined
        const latest = currentProfile(id)
        if (latest && Date.now() - currentVersionAt(latest) >= intervalMsOf(latest)) void updateRemoteProfile(latest)
      }, BUSY_RECHECK_MS)
      timers.set(id, entry)
      return
    }
    await ops.runExclusive(id, 'auto-update', async () => {
      const profile = currentProfile(id)
      if (!profile || profile.type !== 'remote' || ops.editingProfileId.value === id) return
      const versionBefore = new Date(currentVersionAt(profile)).toISOString()
      const hadLocalChanges = !!profile.lastLocalChangeAt && profile.lastLocalChangeAt > (profile.lastUpdated ?? '')
      try {
        const result = await refreshRemoteConfig(profile, {
          singboxPath: config.value.singboxPath,
          workingDir: config.value.workingDir,
          active: () => id === activeConfigProfileId.value,
          stillExists: () => !!currentProfile(id),
        })
        failureCounts.delete(id)
        clearRetry(id)
        updateConfigProfile(id, {
          lastUpdated: new Date().toISOString(),
          lastFetchError: '',
          ...(result.changed ? { lastLocalChangeAt: '', backupUpdatedAt: versionBefore } : {}),
        })
        if (result.changed && hadLocalChanges) {
          pushToast({ message: `「${profile.name}」的订阅内容覆盖了你的本地修改，可以点「换回上一版本」找回`, type: 'info' }, 8000)
        } else if (result.appliedToRunning) {
          pushToast({ message: `远程配置「${profile.name}」有新内容，已写入运行配置`, type: 'info' })
        } else if (result.changed) {
          pushToast({ message: `远程配置「${profile.name}」有新内容，已自动更新`, type: 'info' })
        }
        if (result.appliedToRunning) await ops.noteRunningChanged(`「${profile.name}」自动更新了新内容，重启核心后生效。`)
      } catch (error) {
        if (error instanceof ConfigRemovedError || !currentProfile(id)) return
        const message = describeRefreshError(error)
        updateConfigProfile(id, { lastFetchError: message, lastFetchFailedAt: new Date().toISOString() })
        reportFailure(profile.name, firstLine(message))
        scheduleRetry(id)
      } finally {
        void ops.refreshBackups(configProfiles.value)
      }
    })
  }

  function clearTimer(id: string) {
    const t = timers.get(id)
    if (!t) return
    if (t.timeout != null) clearTimeout(t.timeout)
    if (t.interval != null) clearInterval(t.interval)
    if (t.retry != null) clearTimeout(t.retry)
    timers.delete(id)
  }

  function scheduleProfile(profile: ConfigProfile, overdueDelay = 0) {
    clearTimer(profile.id)
    if (profile.type !== 'remote' || profile.autoUpdateInterval <= 0) return

    const intervalMs = intervalMsOf(profile)
    const elapsed = Date.now() - currentVersionAt(profile)
    const initialDelay = Math.max(overdueDelay, intervalMs - elapsed)

    const startInterval = () => {
      const latest = currentProfile(profile.id)
      if (latest) void updateRemoteProfile(latest)
      const iv = setInterval(() => {
        const current = currentProfile(profile.id)
        if (current) void updateRemoteProfile(current)
      }, intervalMs)
      const entry = timers.get(profile.id) ?? {}
      entry.timeout = undefined
      entry.interval = iv
      timers.set(profile.id, entry)
    }

    if (initialDelay <= 0) {
      startInterval()
    } else {
      const to = setTimeout(startInterval, initialDelay)
      timers.set(profile.id, { timeout: to })
    }
  }

  function syncTimers(profiles: ConfigProfile[], startup = false) {
    const wanted = profiles.filter((p) => p.type === 'remote' && p.autoUpdateInterval > 0)
    const wantedIds = new Set(wanted.map((p) => p.id))
    for (const id of timers.keys()) {
      if (!wantedIds.has(id)) clearTimer(id)
    }
    let overdueIndex = 0
    for (const profile of wanted) {
      if (timers.has(profile.id)) continue
      const overdue = Date.now() - currentVersionAt(profile) >= intervalMsOf(profile)
      const delay = startup && overdue ? STARTUP_DELAY_MS + STARTUP_STAGGER_MS * overdueIndex++ : 0
      scheduleProfile(profile, delay)
    }
  }

  const stopWatch = watch(
    () =>
      configProfiles.value
        .filter((p) => p.type === 'remote')
        .map((p) => ({ id: p.id, interval: p.autoUpdateInterval, source: p.source, lastUpdated: p.lastUpdated, lastLocalChangeAt: p.lastLocalChangeAt })),
    (curr, prev) => {
      const prevMap = new Map((prev ?? []).map((p) => [p.id, p]))
      for (const p of curr) {
        const old = prevMap.get(p.id)
        if (!old || old.interval !== p.interval || old.source !== p.source || old.lastUpdated !== p.lastUpdated || old.lastLocalChangeAt !== p.lastLocalChangeAt) {
          const full = currentProfile(p.id)
          if (full) scheduleProfile(full)
        }
      }
      const currIds = new Set(curr.map((p) => p.id))
      for (const id of timers.keys()) {
        if (!currIds.has(id)) clearTimer(id)
      }
    },
    { deep: true },
  )

  const stopEditingWatch = watch(ops.editingProfileId, (current, previous) => {
    if (!previous || current === previous) return
    const full = currentProfile(previous)
    if (full && full.type === 'remote' && full.autoUpdateInterval > 0) scheduleProfile(full)
  })

  function start() {
    syncTimers(configProfiles.value, true)
  }

  function stop() {
    stopWatch()
    stopEditingWatch()
    for (const id of [...timers.keys()]) clearTimer(id)
    if (failureFlush) clearTimeout(failureFlush)
  }

  onUnmounted(stop)

  return { start, stop }
}
