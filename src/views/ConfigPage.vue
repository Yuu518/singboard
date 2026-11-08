<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount, nextTick } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { registerCloseGuard } from '@/utils/closeGuard'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import { copyToRunningConfig, validateSingboxConfig, getRemoteConfigPath, deleteFile, restoreConfigBackup, readSingboxConfig } from '@/bridge/config'
import { assertConfigShape, ConfigRemovedError, MAX_UPDATE_INTERVAL_HOURS, currentVersionAt, describeRefreshError, firstLine, normalizeUpdateInterval, refreshRemoteConfig } from '@/utils/remoteConfig'
import { createConfigProfileId } from '@/stores/config'
import { findLocalDuplicate, resolveLocalPath, validateRemoteAddress } from '@/utils/configSources'
import { useSettledServiceState } from '@/composables/useSettledServiceState'
import { useConfigOps, type ConfigAction } from '@/stores/configOps'
import type { SaveCheck } from '@/types'
import { restartCore } from '@/utils/coreControl'
import { isElevationCancelled } from '@/bridge/service'
import { open } from '@tauri-apps/plugin-dialog'
import ConfigEditor from '@/components/settings/ConfigEditor.vue'
import ConfigProfileCard from '@/components/config/ConfigProfileCard.vue'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import GlassDialog from '@/components/common/GlassDialog.vue'
import type { ConfigProfile } from '@/types'

const {
  config,
  configProfiles,
  activeConfigProfileId,
  addConfigProfile,
  removeConfigProfile,
  setActiveConfigProfile,
  updateConfigProfile,
} = useConfigStore()
const { pushToast } = useToastStore()
const confirmDialogRef = ref<InstanceType<typeof ConfirmDialog> | null>(null)

const singboxPath = computed(() => config.value.singboxPath)
const workingDir = computed(() => config.value.workingDir)

// 编辑状态
const editingProfileId = ref('')
const editingProfileName = computed(() => {
  const p = configProfiles.value.find((p) => p.id === editingProfileId.value)
  return p?.name ?? ''
})
const editingConfigPath = ref('')
watch(editingProfileId, async (id) => {
  if (!id) { editingConfigPath.value = ''; return }
  const p = configProfiles.value.find((p) => p.id === id)
  if (!p) { editingConfigPath.value = ''; return }
  const path = await getProfileConfigPath(p)
  if (editingProfileId.value === id) editingConfigPath.value = path
}, { immediate: true })

// 对话框状态
const showAddLocalDialog = ref(false)
const showAddRemoteDialog = ref(false)
const showEditInfoDialog = ref(false)
const addLocalForm = ref({ name: '', path: '' })
const addRemoteForm = ref({ name: '', url: '', autoUpdateInterval: 0 })
const editInfoForm = ref({ id: '', name: '', source: '', type: '' as 'local' | 'remote', autoUpdateInterval: 0 })
const addingRemote = ref(false)
const addRemoteError = ref('')
const editInfoError = ref('')
const savingInfo = ref(false)
const addLocalError = ref('')
watch(() => [addLocalForm.value.name, addLocalForm.value.path], () => {
  addLocalError.value = ''
})
watch(() => [editInfoForm.value.name, editInfoForm.value.source], () => {
  editInfoError.value = ''
})
watch(() => [addRemoteForm.value.url, addRemoteForm.value.name], () => {
  addRemoteError.value = ''
})

const ops = useConfigOps()
const { running: runningOps, pendingRestart, backupIds } = ops

function noteApplied(message: string) {
  void ops.noteRunningChanged(message)
}

function versionStamp(profile: ConfigProfile): string {
  const at = currentVersionAt(profile)
  return at ? new Date(at).toISOString() : ''
}

function cardBusy(id: string): '' | 'select' | 'update' | 'delete' | 'restore' {
  const action: ConfigAction | undefined = runningOps.value.get(id)
  if (action === 'auto-update') return 'update'
  if (!action) return ''
  return action
}

async function runCardAction(id: string, action: ConfigAction, task: () => Promise<void>) {
  if (ops.isBusy(id)) return
  await ops.runExclusive(id, action, task)
}

const { isRunning } = useSettledServiceState()
const restarting = ref(false)

async function restartNow() {
  if (restarting.value) return
  restarting.value = true
  try {
    await restartCore()
    ops.clearPendingRestart()
    pushToast({ message: '核心已重启，新配置已生效', type: 'info' })
  } catch (error) {
    if (!isElevationCancelled(error)) {
      pushToast({ message: '重启核心失败：' + describeRefreshError(error), type: 'error' }, 8000)
    }
  } finally {
    restarting.value = false
  }
}

const hasNoActiveProfile = computed(() => configProfiles.value.length > 0 && !configProfiles.value.some((p) => p.id === activeConfigProfileId.value))

function focusFirst(selector: string) {
  void nextTick(() => document.querySelector<HTMLElement>(selector)?.focus())
}

function focusInCard(id: string, selector: string) {
  focusFirst(`[data-profile-id="${id}"] ${selector}`)
}

function refreshBackups() {
  return ops.refreshBackups(configProfiles.value)
}

void refreshBackups()


async function getProfileConfigPath(profile: ConfigProfile): Promise<string> {
  if (profile.type === 'local') return profile.source
  return await getRemoteConfigPath(profile.id)
}

async function applyToRunning(configFilePath: string, alreadyChecked = false): Promise<boolean | null> {
  const sp = singboxPath.value
  if (!sp) {
    pushToast({ message: '请先在设置中配置 sing-box 路径', type: 'error' })
    return null
  }
  if (!alreadyChecked) {
    try {
      await validateSingboxConfig(sp, configFilePath, workingDir.value)
    } catch (e: any) {
      pushToast({ message: '配置校验失败，运行配置没有更新：\n' + (e?.message || e), type: 'error' }, 8000)
      return null
    }
  }
  try {
    return (await copyToRunningConfig(configFilePath)).changed
  } catch (error) {
    pushToast({ message: '写入运行配置失败：' + describeRefreshError(error), type: 'error' }, 8000)
    return null
  }
}

function startEdit(id: string) {
  if (ops.isBusy(id)) return
  editingProfileId.value = id
  focusFirst('[aria-label="返回配置列表"]')
}

watch(editingProfileId, (id) => {
  ops.editingProfileId.value = id
}, { immediate: true })
onBeforeUnmount(() => {
  ops.editingProfileId.value = ''
})

const configEditorRef = ref<InstanceType<typeof ConfigEditor> | null>(null)
const editingRemote = computed(() => configProfiles.value.find((p) => p.id === editingProfileId.value)?.type === 'remote')

async function confirmLeaveEditor(): Promise<boolean> {
  if (!editingProfileId.value || !configEditorRef.value?.isDirty()) return true
  return !!(await confirmDialogRef.value?.show({
    title: '有未保存的改动',
    message: `「${editingProfileName.value}」还有没保存的改动，离开后这些改动会丢失。`,
    confirmText: '放弃改动',
    cancelText: '继续编辑',
    variant: 'danger',
  }))
}

async function stopEdit() {
  if (!(await confirmLeaveEditor())) return
  const id = editingProfileId.value
  editingProfileId.value = ''
  focusInCard(id, '[data-action="edit"]')
}

onBeforeRouteLeave(() => confirmLeaveEditor())
const unregisterCloseGuard = registerCloseGuard(() => confirmLeaveEditor())
onBeforeUnmount(unregisterCloseGuard)

function onPageKeydown(e: KeyboardEvent) {
  if (!editingProfileId.value || !(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 's') return
  if ((e.target as HTMLElement | null)?.closest('.cm-editor')) return
  e.preventDefault()
  void configEditorRef.value?.save()
}

window.addEventListener('keydown', onPageKeydown)
onBeforeUnmount(() => window.removeEventListener('keydown', onPageKeydown))

async function onConfigSaved(check: SaveCheck) {
  const profile = configProfiles.value.find((p) => p.id === editingProfileId.value)
  if (!profile) return

  const now = new Date().toISOString()
  if (profile.type === 'local') {
    updateConfigProfile(profile.id, { lastUpdated: now, backupUpdatedAt: profile.lastUpdated ?? '' })
  } else {
    updateConfigProfile(profile.id, { lastLocalChangeAt: now, backupUpdatedAt: versionStamp(profile) })
  }
  void refreshBackups()

  if (profile.id !== activeConfigProfileId.value) {
    pushToast({ message: '配置已保存', type: 'info' })
    return
  }
  if (check === 'failed') {
    pushToast({ message: '已保存，但没通过 sing-box 校验，运行配置没有更新', type: 'error' }, 8000)
    return
  }
  if (!singboxPath.value) {
    pushToast({ message: '已保存。还没配置 sing-box 路径，运行配置没有更新', type: 'info' })
    return
  }
  const changed = await applyToRunning(await getProfileConfigPath(profile), check === 'passed')
  if (changed === null) return
  if (changed) {
    pushToast({ message: '配置已保存并写入运行配置', type: 'info' })
    noteApplied(`「${profile.name}」已保存，重启核心后生效。`)
  } else {
    pushToast({ message: '配置已保存，运行配置本来就是这份内容', type: 'info' })
  }
}

async function selectProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return

  await runCardAction(id, 'select', async () => {
    try {
      const changed = await applyToRunning(await getProfileConfigPath(profile))
      if (changed === null) return
      setActiveConfigProfile(id)
      focusInCard(id, '[data-active-pill]')
      if (isRunning.value) {
        pushToast({ message: `已选用配置「${profile.name}」`, type: 'info' })
        if (changed) noteApplied(`已切换到「${profile.name}」，重启核心后生效。`)
      } else {
        pushToast({ message: `已选用配置「${profile.name}」，下次启动核心时使用`, type: 'info' })
      }
    } catch (error) {
      pushToast({ message: '选用配置失败：' + describeRefreshError(error), type: 'error' })
    }
  })
}

async function restoreProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return

  await runCardAction(id, 'restore', async () => {
    try {
      const path = await getProfileConfigPath(profile)
      await restoreConfigBackup(path)
      const swappedIn = profile.backupUpdatedAt ?? ''
      const swappedOut = versionStamp(profile)
      const now = new Date().toISOString()
      if (profile.type === 'remote') {
        updateConfigProfile(id, { lastFetchError: '', lastLocalChangeAt: now, backupUpdatedAt: swappedOut })
      } else {
        updateConfigProfile(id, { lastUpdated: swappedIn || now, backupUpdatedAt: swappedOut })
      }
      if (id === activeConfigProfileId.value) {
        const changed = await applyToRunning(path)
        if (changed === null) {
          pushToast({ message: `「${profile.name}」已换成上一版本，但它没通过校验，运行配置没有更新。再点一次可以换回来。`, type: 'error' }, 8000)
          return
        }
        if (changed) noteApplied(`「${profile.name}」换回了上一版本，重启核心后生效。`)
      }
      pushToast({ message: `已把「${profile.name}」换回上一版本，再点一次可以换回来`, type: 'info' })
    } catch (error) {
      pushToast({ message: `换回「${profile.name}」的上一版本失败：${describeRefreshError(error)}`, type: 'error' })
    } finally {
      void refreshBackups()
    }
  })
}

async function deleteProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return

  const isActive = id === activeConfigProfileId.value
  const msg = [
    `确定删除配置「${profile.name}」？`,
    profile.type === 'remote' ? '同时会删除已下载的远程配置文件和它的上一版本。' : '仅从面板移除，不会删除本地源文件。',
    isActive ? '这是当前使用的配置。删除后不会自动改用别的配置，核心继续使用上次写入的运行配置，直到你选用新的配置。' : '',
  ].filter(Boolean).join('\n')

  const confirmed = await confirmDialogRef.value?.show({
    title: '删除配置',
    message: msg,
    confirmText: '删除',
    variant: 'danger',
  })
  if (!confirmed) return

  await ops.runExclusive(id, 'delete', async () => {
    if (!configProfiles.value.some((p) => p.id === id)) return
    if (profile.type === 'remote') {
      const path = await getProfileConfigPath(profile).catch(() => '')
      if (path) {
        await deleteFile(path).catch(() => undefined)
        await deleteFile(`${path}.bak`).catch(() => undefined)
      }
    }
    const order = configProfiles.value.map((p) => p.id)
    const nextId = order[order.indexOf(id) + 1] ?? order[order.indexOf(id) - 1]
    removeConfigProfile(id)
    if (nextId) focusInCard(nextId, 'button')
    else focusFirst('[data-empty-add]')
    pushToast({
      message: isActive ? `已删除「${profile.name}」，现在没有选用的配置` : `已删除「${profile.name}」`,
      type: 'info',
    })
  })
}

async function updateRemoteProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile || profile.type !== 'remote') return

  await runCardAction(id, 'update', async () => {
    try {
      const versionBefore = versionStamp(profile)
      const hadLocalChanges = !!profile.lastLocalChangeAt && profile.lastLocalChangeAt > (profile.lastUpdated ?? '')
      const result = await refreshRemoteConfig(profile, {
        singboxPath: singboxPath.value,
        workingDir: workingDir.value,
        active: () => id === activeConfigProfileId.value,
        stillExists: () => configProfiles.value.some((p) => p.id === id),
      })
      updateConfigProfile(id, {
        lastUpdated: new Date().toISOString(),
        lastFetchError: '',
        ...(result.changed ? { lastLocalChangeAt: '', backupUpdatedAt: versionBefore } : {}),
      })
      pushToast({
        message: result.changed && hadLocalChanges
          ? `订阅内容覆盖了你在「${profile.name}」里的修改，可以点「换回上一版本」找回`
          : result.appliedToRunning
            ? `远程配置「${profile.name}」有新内容，已写入运行配置`
            : result.changed
              ? `远程配置「${profile.name}」有新内容，已更新`
              : `远程配置「${profile.name}」已是最新`,
        type: 'info',
      })
      if (result.appliedToRunning) noteApplied(`「${profile.name}」更新了新内容，重启核心后生效。`)
    } catch (error) {
      const message = describeRefreshError(error)
      updateConfigProfile(id, { lastFetchError: message, lastFetchFailedAt: new Date().toISOString() })
      pushToast({ message: `更新「${profile.name}」失败，保留现有配置：${firstLine(message)}`, type: 'error' }, 8000)
    } finally {
      void refreshBackups()
    }
  })
}

function openEditInfoDialog(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return
  editInfoForm.value = { id: profile.id, name: profile.name, source: profile.source, type: profile.type, autoUpdateInterval: profile.autoUpdateInterval }
  editInfoError.value = ''
  showEditInfoDialog.value = true
}

async function browseEditInfoFile() {
  const selected = await open({
    multiple: false,
    filters: [{ name: 'JSON', extensions: ['json'] }],
    defaultPath: workingDir.value.trim() || undefined,
  })
  if (selected) {
    editInfoForm.value.source = selected as string
  }
}

async function checkLocalConfig(path: string, mustPassCheck: boolean): Promise<string> {
  try {
    assertConfigShape(await readSingboxConfig(path))
  } catch (error) {
    return `无法使用这个文件：${describeRefreshError(error)}`
  }
  if (!mustPassCheck) return ''
  if (!singboxPath.value) return '请先在设置中配置 sing-box 路径，才能校验当前使用的配置'
  try {
    await validateSingboxConfig(singboxPath.value, path, workingDir.value)
  } catch (error) {
    return `sing-box 校验未通过，没有修改：${describeRefreshError(error)}`
  }
  return ''
}

async function saveEditInfo() {
  if (savingInfo.value) return
  const name = editInfoForm.value.name.trim()
  const rawSource = editInfoForm.value.source.trim()
  if (!name || !rawSource) {
    editInfoError.value = editInfoForm.value.type === 'local' ? '请填写名称和文件路径' : '请填写名称和远程地址'
    return
  }
  const id = editInfoForm.value.id
  const before = configProfiles.value.find((p) => p.id === id)
  if (!before) {
    showEditInfoDialog.value = false
    return
  }
  const interval = normalizeUpdateInterval(editInfoForm.value.autoUpdateInterval)
  const isActive = () => id === activeConfigProfileId.value

  if (before.type === 'remote') {
    const url = rawSource
    if (url === before.source) {
      updateConfigProfile(id, { name, autoUpdateInterval: interval })
      showEditInfoDialog.value = false
      return
    }
    const problem = validateRemoteAddress(url, configProfiles.value, id)
    if (problem) {
      editInfoError.value = problem
      return
    }
    savingInfo.value = true
    try {
      await ops.runExclusive(id, 'update', async () => {
        const result = await refreshRemoteConfig(
          { ...before, source: url },
          {
            singboxPath: singboxPath.value,
            workingDir: workingDir.value,
            active: isActive,
            stillExists: () => configProfiles.value.some((p) => p.id === id),
          },
        )
        updateConfigProfile(id, { name, source: url, autoUpdateInterval: interval, lastUpdated: new Date().toISOString(), lastFetchError: '' })
        if (result.appliedToRunning) noteApplied(`「${name}」改用了新的远程地址，重启核心后生效。`)
      })
      showEditInfoDialog.value = false
      pushToast({ message: `已更新「${name}」的远程地址并重新下载`, type: 'info' })
    } catch (error) {
      if (error instanceof ConfigRemovedError) {
        showEditInfoDialog.value = false
        return
      }
      editInfoError.value = `下载或校验失败，没有修改：${describeRefreshError(error)}`
    } finally {
      savingInfo.value = false
      void refreshBackups()
    }
    return
  }

  const resolved = resolveLocalPath(rawSource, workingDir.value)
  if ('error' in resolved) {
    editInfoError.value = resolved.error
    return
  }
  if (resolved.path.toLowerCase() === before.source.toLowerCase()) {
    updateConfigProfile(id, { name })
    showEditInfoDialog.value = false
    return
  }
  const duplicate = findLocalDuplicate(resolved.path, configProfiles.value.filter((p) => p.id !== id))
  if (duplicate) {
    editInfoError.value = `这个文件已经添加为「${duplicate.name}」`
    return
  }
  savingInfo.value = true
  try {
    const problem = await checkLocalConfig(resolved.path, isActive())
    if (problem) {
      editInfoError.value = problem
      return
    }
    updateConfigProfile(id, { name, source: resolved.path })
    showEditInfoDialog.value = false
    if (isActive() && await applyToRunning(resolved.path, true)) {
      noteApplied(`「${name}」改用了新的源文件，重启核心后生效。`)
    }
  } finally {
    savingInfo.value = false
  }
}

function openAddLocalDialog() {
  addLocalForm.value = { name: '', path: '' }
  addLocalError.value = ''
  showAddLocalDialog.value = true
}

function openAddRemoteDialog() {
  addRemoteForm.value = { name: '', url: '', autoUpdateInterval: 0 }
  addRemoteError.value = ''
  showAddRemoteDialog.value = true
}

async function browseLocalFile() {
  const selected = await open({
    multiple: false,
    filters: [{ name: 'JSON', extensions: ['json'] }],
    defaultPath: workingDir.value.trim() || undefined,
  })
  if (selected) {
    addLocalForm.value.path = selected as string
  }
}

const addingLocal = ref(false)

async function handleAddLocal() {
  if (addingLocal.value) return
  const name = addLocalForm.value.name.trim()
  const input = addLocalForm.value.path.trim()
  if (!name || !input) {
    addLocalError.value = '请填写名称和文件路径'
    return
  }
  const resolved = resolveLocalPath(input, workingDir.value)
  if ('error' in resolved) {
    addLocalError.value = resolved.error
    return
  }
  const existing = findLocalDuplicate(resolved.path, configProfiles.value)
  if (existing) {
    addLocalError.value = `这个文件已经添加为「${existing.name}」`
    return
  }
  addingLocal.value = true
  try {
    const problem = await checkLocalConfig(resolved.path, false)
    if (problem) {
      addLocalError.value = problem
      return
    }
    addConfigProfile(name, 'local', resolved.path)
    showAddLocalDialog.value = false
    pushToast({ message: `已添加本地配置「${name}」，点「使用」即可启用`, type: 'info' })
  } finally {
    addingLocal.value = false
  }
}

async function handleAddRemote() {
  if (addingRemote.value) return
  const name = addRemoteForm.value.name.trim()
  const url = addRemoteForm.value.url.trim()
  if (!name || !url) {
    addRemoteError.value = '请填写名称和远程地址'
    return
  }
  const problem = validateRemoteAddress(url, configProfiles.value)
  if (problem) {
    addRemoteError.value = problem
    return
  }

  addingRemote.value = true
  addRemoteError.value = ''
  const id = createConfigProfileId()
  const interval = normalizeUpdateInterval(addRemoteForm.value.autoUpdateInterval)
  try {
    await refreshRemoteConfig(
      { id, name, type: 'remote', source: url, autoUpdateInterval: interval },
      { singboxPath: singboxPath.value, workingDir: workingDir.value, active: false },
    )
    addConfigProfile(name, 'remote', url, interval, id)
    showAddRemoteDialog.value = false
    pushToast({ message: `已添加远程配置「${name}」，点「使用」即可启用`, type: 'info' })
  } catch (error) {
    addRemoteError.value = `下载或校验失败，没有添加：${describeRefreshError(error)}`
  } finally {
    addingRemote.value = false
  }
}
</script>

<template>
  <div class="config-page flex flex-col h-full min-h-0 gap-3">
    <ConfirmDialog ref="confirmDialogRef" />
    <div v-if="!editingProfileId" class="flex items-center justify-between gap-3 shrink-0">
      <h1 class="text-[26px] leading-tight tracking-tight font-bold">配置</h1>
      <div v-if="configProfiles.length > 0" class="flex gap-2">
        <button type="button" class="btn btn-sm btn-glass" @click="openAddLocalDialog">添加本地<span class="config-wide-only">配置</span></button>
        <button type="button" class="btn btn-sm btn-glass" @click="openAddRemoteDialog">添加远程<span class="config-wide-only">配置</span></button>
      </div>
    </div>
    <div v-else class="flex min-w-0 items-center gap-2 shrink-0">
      <button
        type="button"
        class="btn btn-sm btn-ghost btn-circle shrink-0"
        aria-label="返回配置列表"
        title="返回配置列表"
        @click="stopEdit"
      >
        <svg class="h-4 w-4" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M10 3.5 5.5 8l4.5 4.5" />
        </svg>
      </button>
      <h1 class="min-w-0 truncate text-[22px] leading-tight tracking-tight font-bold" :title="editingProfileName">{{ editingProfileName }}</h1>
      <span class="shrink-0 text-xs text-base-content/65">{{ editingRemote ? '远程副本' : '本地文件' }}</span>
    </div>

    <div
      v-if="pendingRestart"
      class="surface-card flex shrink-0 items-center gap-2 px-3 py-1.5 text-xs"
    >
      <span role="status">{{ pendingRestart }}</span>
      <button
        type="button"
        class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]"
        :disabled="restarting"
        @click="restartNow"
      >
        <span v-if="restarting" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        {{ restarting ? '正在重启…' : '立即重启核心' }}
      </button>
      <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2" :disabled="restarting" @click="ops.clearPendingRestart()">稍后</button>
    </div>
    <div
      v-if="!editingProfileId && hasNoActiveProfile"
      class="surface-card shrink-0 px-3 py-1.5 text-xs"
      role="status"
    >
      还没有选用配置，核心使用的是上次写入的运行配置。在下面选一份点「使用」。
    </div>

    <div v-if="!editingProfileId" class="config-list -mx-2 -mt-1 min-h-0 flex-1 overflow-auto px-2 pb-2 pt-1">
      <div class="config-grid grid content-start items-start gap-3">
      <ConfigProfileCard
        v-for="profile in configProfiles"
        :key="profile.id"
        :profile="profile"
        :is-active="profile.id === activeConfigProfileId"
        :has-backup="backupIds.has(profile.id)"
        :busy="cardBusy(profile.id)"
        :locked="runningOps.has(profile.id)"
        @edit="startEdit(profile.id)"
        @select="selectProfile(profile.id)"
        @delete="deleteProfile(profile.id)"
        @update="updateRemoteProfile(profile.id)"
        @edit-info="openEditInfoDialog(profile.id)"
        @restore="restoreProfile(profile.id)"
      />
      <div v-if="configProfiles.length === 0" class="col-span-full flex flex-col items-center gap-3 px-6 py-12 text-center" role="status">
        <p class="text-sm text-base-content/80">还没有配置</p>
        <p class="text-xs text-base-content/65">添加本地的 sing-box 配置文件，或者填入订阅地址下载远程配置。</p>
        <div class="flex gap-2">
          <button type="button" class="btn btn-sm btn-glass px-5" data-empty-add @click="openAddLocalDialog">添加本地配置</button>
          <button type="button" class="btn btn-sm btn-glass px-5" @click="openAddRemoteDialog">添加远程配置</button>
        </div>
      </div>
      </div>
    </div>

    <template v-if="editingProfileId">
      <div
        v-if="editingRemote"
        class="surface-card flex shrink-0 items-center gap-2 px-3 py-1.5 text-xs"
      >
        下次更新（手动或自动）会用订阅内容覆盖这里的改动。
      </div>
      <ConfigEditor
        v-if="editingConfigPath"
        ref="configEditorRef"
        class="flex-1 min-h-0"
        :config-path="editingConfigPath"
        :singbox-path="singboxPath"
        :working-dir="workingDir"
        :validate-before-save="editingProfileId === activeConfigProfileId"
        @saved="onConfigSaved"
      />
      <div
        v-else
        class="surface-card flex-1 min-h-0 flex items-center justify-center text-sm text-base-content/60"
      >
        正在加载配置路径…
      </div>
    </template>

    <GlassDialog :open="showAddLocalDialog" title="添加本地配置" :dismiss-on-backdrop="false" :busy="addingLocal" @close="showAddLocalDialog = false">
      <form class="space-y-4" novalidate @submit.prevent="handleAddLocal">
        <div class="space-y-1.5">
          <label for="add-local-name" class="block text-xs text-base-content/70">配置名称</label>
          <input id="add-local-name" v-model="addLocalForm.name" type="text" class="input input-sm input-bordered w-full" placeholder="例如：默认配置" :aria-invalid="!!addLocalError && !addLocalForm.name.trim()" />
        </div>
        <div class="space-y-1.5">
          <label for="add-local-path" class="block text-xs text-base-content/70">配置文件路径或文件名</label>
          <div class="flex gap-2">
            <input id="add-local-path" v-model="addLocalForm.path" type="text" class="input input-sm input-bordered min-w-0 flex-1" placeholder="config.json 或 C:\sing-box\config.json" :aria-invalid="!!addLocalError" :aria-describedby="addLocalError ? 'add-local-error' : undefined" />
            <button type="button" class="btn btn-sm btn-glass shrink-0" @click="browseLocalFile">浏览</button>
          </div>
        </div>
        <p v-if="addLocalError" id="add-local-error" class="max-h-32 overflow-auto whitespace-pre-wrap break-words text-xs text-[color:var(--danger-ink)]" role="alert">{{ addLocalError }}</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="btn btn-sm btn-ghost" :aria-disabled="addingLocal || undefined" :class="{ 'opacity-50': addingLocal }" @click="!addingLocal && (showAddLocalDialog = false)">取消</button>
          <button type="submit" class="btn btn-sm btn-primary" :aria-disabled="addingLocal || undefined">
            <span v-if="addingLocal" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            添加
          </button>
        </div>
      </form>
    </GlassDialog>

    <GlassDialog :open="showAddRemoteDialog" title="添加远程配置" :dismiss-on-backdrop="false" :busy="addingRemote" @close="showAddRemoteDialog = false">
      <form class="space-y-4" novalidate @submit.prevent="handleAddRemote">
        <div class="space-y-1.5">
          <label for="add-remote-name" class="block text-xs text-base-content/70">配置名称</label>
          <input id="add-remote-name" v-model="addRemoteForm.name" type="text" class="input input-sm input-bordered w-full" placeholder="例如：远程订阅" :aria-invalid="!!addRemoteError && !addRemoteForm.name.trim()" />
        </div>
        <div class="space-y-1.5">
          <label for="add-remote-url" class="block text-xs text-base-content/70">远程地址（URL）</label>
          <input
            id="add-remote-url"
            v-model="addRemoteForm.url"
            type="url"
            class="input input-sm input-bordered w-full"
            placeholder="https://example.com/config.json"
            :aria-invalid="!!addRemoteError"
            :aria-describedby="addRemoteError ? 'add-remote-error' : undefined"
          />
        </div>
        <div class="space-y-1.5">
          <label for="add-remote-interval" class="block text-xs text-base-content/70">自动更新间隔（小时，0 为不自动更新，最长 576）</label>
          <input id="add-remote-interval" v-model.number="addRemoteForm.autoUpdateInterval" type="number" min="0" :max="MAX_UPDATE_INTERVAL_HOURS" step="1" class="input input-sm input-bordered w-full" placeholder="0" />
        </div>
        <p
          v-if="addRemoteError"
          id="add-remote-error"
          class="max-h-32 overflow-auto whitespace-pre-wrap break-words text-xs text-[color:var(--danger-ink)]"
          role="alert"
        >{{ addRemoteError }}</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="btn btn-sm btn-ghost" :aria-disabled="addingRemote || undefined" :class="{ 'opacity-50': addingRemote }" @click="!addingRemote && (showAddRemoteDialog = false)">取消</button>
          <button type="submit" class="btn btn-sm btn-primary" :aria-disabled="addingRemote || undefined">
            <span v-if="addingRemote" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            {{ addingRemote ? (singboxPath ? '正在下载并校验…' : '正在下载…') : '添加' }}
          </button>
        </div>
      </form>
    </GlassDialog>

    <GlassDialog :open="showEditInfoDialog" title="修改信息" :dismiss-on-backdrop="false" :busy="savingInfo" @close="showEditInfoDialog = false">
      <form class="space-y-4" novalidate @submit.prevent="saveEditInfo">
        <div class="space-y-1.5">
          <label for="edit-info-name" class="block text-xs text-base-content/70">名称</label>
          <input id="edit-info-name" v-model="editInfoForm.name" type="text" class="input input-sm input-bordered w-full" :aria-invalid="!!editInfoError && !editInfoForm.name.trim()" />
        </div>
        <div class="space-y-1.5">
          <label for="edit-info-source" class="block text-xs text-base-content/70">{{ editInfoForm.type === 'local' ? '文件路径' : '远程地址' }}</label>
          <div class="flex gap-2">
            <input id="edit-info-source" v-model="editInfoForm.source" type="text" class="input input-sm input-bordered min-w-0 flex-1" :aria-invalid="!!editInfoError" :aria-describedby="editInfoError ? 'edit-info-error' : undefined" />
            <button v-if="editInfoForm.type === 'local'" type="button" class="btn btn-sm btn-glass shrink-0" @click="browseEditInfoFile">浏览</button>
          </div>
        </div>
        <div v-if="editInfoForm.type === 'remote'" class="space-y-1.5">
          <label for="edit-info-interval" class="block text-xs text-base-content/70">自动更新间隔（小时，0 为不自动更新，最长 576）</label>
          <input id="edit-info-interval" v-model.number="editInfoForm.autoUpdateInterval" type="number" min="0" :max="MAX_UPDATE_INTERVAL_HOURS" step="1" class="input input-sm input-bordered w-full" />
          <p v-if="Number(editInfoForm.autoUpdateInterval) > MAX_UPDATE_INTERVAL_HOURS" class="text-xs text-base-content/65">最长 {{ MAX_UPDATE_INTERVAL_HOURS }} 小时，会按 {{ MAX_UPDATE_INTERVAL_HOURS }} 小时保存。</p>
        </div>
        <p v-if="editInfoError" id="edit-info-error" class="max-h-32 overflow-auto whitespace-pre-wrap break-words text-xs text-[color:var(--danger-ink)]" role="alert">{{ editInfoError }}</p>
        <div class="flex justify-end gap-2">
          <button type="button" class="btn btn-sm btn-ghost" :aria-disabled="savingInfo || undefined" :class="{ 'opacity-50': savingInfo }" @click="!savingInfo && (showEditInfoDialog = false)">取消</button>
          <button type="submit" class="btn btn-sm btn-primary" :aria-disabled="savingInfo || undefined">
            <span v-if="savingInfo" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            {{ savingInfo ? '正在校验…' : '保存' }}
          </button>
        </div>
      </form>
    </GlassDialog>
  </div>
</template>

<style scoped>
.config-page {
  container-type: inline-size;
}

.config-grid {
  grid-template-columns: minmax(0, 1fr);
}

@container (min-width: 640px) {
  .config-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@container (max-width: 599px) {
  .config-wide-only {
    display: none;
  }
}
</style>
