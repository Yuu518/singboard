<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import { copyToRunningConfig, writeSingboxConfig, fetchUrl, validateSingboxConfig, getRemoteConfigPath, deleteFile } from '@/bridge/config'
import { open } from '@tauri-apps/plugin-dialog'
import ConfigEditor from '@/components/settings/ConfigEditor.vue'
import ConfigProfileCard from '@/components/config/ConfigProfileCard.vue'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
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
  editingConfigPath.value = await getProfileConfigPath(p)
}, { immediate: true })

// 对话框状态
const showAddLocalDialog = ref(false)
const showAddRemoteDialog = ref(false)
const showEditInfoDialog = ref(false)
const addLocalForm = ref({ name: '', path: '' })
const addRemoteForm = ref({ name: '', url: '', autoUpdateInterval: 0 })
const editInfoForm = ref({ id: '', name: '', source: '', type: '' as 'local' | 'remote', autoUpdateInterval: 0 })
const addingRemote = ref(false)

// 操作加载状态
const selectingId = ref('')
const updatingId = ref('')
const deletingId = ref('')


async function getProfileConfigPath(profile: ConfigProfile): Promise<string> {
  if (profile.type === 'local') return profile.source
  return await getRemoteConfigPath(profile.id)
}

async function validateAndCopyToRunning(configFilePath: string): Promise<boolean> {
  const sp = singboxPath.value
  if (!sp) {
    pushToast({ message: '请先在设置中配置 sing-box 路径', type: 'error' })
    return false
  }
  try {
    await validateSingboxConfig(sp, configFilePath, workingDir.value)
  } catch (e: any) {
    pushToast({ message: '配置校验失败，未更新运行配置:\n' + (e?.message || e), type: 'error' }, 8000)
    return false
  }
  await copyToRunningConfig(configFilePath)
  return true
}

function startEdit(id: string) {
  editingProfileId.value = id
}

function stopEdit() {
  editingProfileId.value = ''
}

async function onConfigSaved() {
  const profile = configProfiles.value.find((p) => p.id === editingProfileId.value)
  if (!profile) return

  updateConfigProfile(profile.id, { lastUpdated: new Date().toISOString() })

  // 如果是激活配置，校验后复制到 running-config
  if (profile.id === activeConfigProfileId.value) {
    const ok = await validateAndCopyToRunning(await getProfileConfigPath(profile))
    if (ok) {
      pushToast({ message: '配置已保存并更新到运行配置', type: 'info' })
    }
  } else {
    pushToast({ message: '配置已保存', type: 'info' })
  }
}

async function selectProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return

  selectingId.value = id
  try {
    const ok = await validateAndCopyToRunning(await getProfileConfigPath(profile))
    if (ok) {
      setActiveConfigProfile(id)
      pushToast({ message: `已选用配置「${profile.name}」，重启核心后生效`, type: 'info' })
    }
  } catch (e: any) {
    pushToast({ message: '选用配置失败: ' + (e?.message || e), type: 'error' })
  } finally {
    selectingId.value = ''
  }
}

async function deleteProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return

  const msg = profile.type === 'remote'
    ? `确定删除配置「${profile.name}」？\n同时会删除已下载的远程配置文件。`
    : `确定删除配置「${profile.name}」？\n仅从面板移除，不会删除本地源文件。`

  const confirmed = await confirmDialogRef.value?.show({
    title: '删除配置',
    message: msg,
    confirmText: '删除',
    variant: 'danger',
  })
  if (!confirmed) return

  deletingId.value = id
  if (profile.type === 'remote') {
    try {
      await deleteFile(await getProfileConfigPath(profile))
    } catch { /* 文件不存在也无所谓 */ }
  }
  removeConfigProfile(id)
  deletingId.value = ''
  pushToast({ message: '配置已删除', type: 'info' })
}

async function updateRemoteProfile(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile || profile.type !== 'remote') return

  updatingId.value = id
  try {
    const content = await fetchUrl(profile.source)
    const destPath = await getProfileConfigPath(profile)
    await writeSingboxConfig(destPath, content)
    updateConfigProfile(id, { lastUpdated: new Date().toISOString() })

    // 如果是激活配置，校验后重新复制到 running-config
    if (id === activeConfigProfileId.value) {
      const ok = await validateAndCopyToRunning(destPath)
      if (ok) {
        pushToast({ message: `远程配置「${profile.name}」已更新`, type: 'info' })
      }
    } else {
      pushToast({ message: `远程配置「${profile.name}」已更新`, type: 'info' })
    }
  } catch (e: any) {
    pushToast({ message: '更新远程配置失败: ' + (e?.message || e), type: 'error' })
  } finally {
    updatingId.value = ''
  }
}

function openEditInfoDialog(id: string) {
  const profile = configProfiles.value.find((p) => p.id === id)
  if (!profile) return
  editInfoForm.value = { id: profile.id, name: profile.name, source: profile.source, type: profile.type, autoUpdateInterval: profile.autoUpdateInterval }
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

function saveEditInfo() {
  const name = editInfoForm.value.name.trim()
  const source = editInfoForm.value.source.trim()
  if (!name || !source) return
  updateConfigProfile(editInfoForm.value.id, { name, source, autoUpdateInterval: editInfoForm.value.autoUpdateInterval })
  showEditInfoDialog.value = false
}

function openAddLocalDialog() {
  addLocalForm.value = { name: '', path: '' }
  showAddLocalDialog.value = true
}

function openAddRemoteDialog() {
  addRemoteForm.value = { name: '', url: '', autoUpdateInterval: 0 }
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

function isAbsolutePath(p: string): boolean {
  return /^[a-zA-Z]:[/\\]/.test(p) || p.startsWith('\\\\') || p.startsWith('/')
}

function handleAddLocal() {
  const name = addLocalForm.value.name.trim()
  let path = addLocalForm.value.path.trim()
  if (!name || !path) {
    pushToast({ message: '请填写名称和文件路径', type: 'error' })
    return
  }
  // 非绝对路径视为文件名，在工作目录下查找
  if (!isAbsolutePath(path)) {
    const dir = workingDir.value.trim()
    if (!dir) {
      pushToast({ message: '工作目录未配置，请输入完整路径', type: 'error' })
      return
    }
    path = dir.endsWith('\\') || dir.endsWith('/') ? `${dir}${path}` : `${dir}\\${path}`
  }
  addConfigProfile(name, 'local', path)
  showAddLocalDialog.value = false
  pushToast({ message: '已添加本地配置', type: 'info' })
}

async function handleAddRemote() {
  const name = addRemoteForm.value.name.trim()
  const url = addRemoteForm.value.url.trim()
  if (!name || !url) {
    pushToast({ message: '请填写名称和远程地址', type: 'error' })
    return
  }

  addingRemote.value = true
  try {
    // 先创建 profile 获取 ID
    const interval = addRemoteForm.value.autoUpdateInterval
    const id = addConfigProfile(name, 'remote', url, interval)
    const profile = configProfiles.value.find((p) => p.id === id)!

    // 下载远程配置并保存
    const content = await fetchUrl(url)
    const destPath = await getProfileConfigPath(profile)
    await writeSingboxConfig(destPath, content)

    showAddRemoteDialog.value = false
    pushToast({ message: '已添加远程配置', type: 'info' })
  } catch (e: any) {
    pushToast({ message: '添加远程配置失败: ' + (e?.message || e), type: 'error' })
  } finally {
    addingRemote.value = false
  }
}
</script>

<template>
  <div class="flex flex-col h-full min-h-0 gap-3">
    <ConfirmDialog ref="confirmDialogRef" />
    <!-- 标题栏 -->
    <div class="flex items-center justify-between shrink-0">
      <h1 class="text-[26px] leading-tight tracking-tight font-bold" v-if="!editingProfileId">配置</h1>
      <template v-if="!editingProfileId">
        <div class="flex gap-2">
          <button class="btn btn-sm btn-glass" @click="openAddLocalDialog">添加本地配置</button>
          <button class="btn btn-sm btn-glass" @click="openAddRemoteDialog">添加远程配置</button>
        </div>
      </template>
    </div>

    <!-- 卡片列表 -->
    <div v-if="!editingProfileId" class="-mx-2 -mt-1 grid grid-cols-1 md:grid-cols-2 gap-3 overflow-auto flex-1 content-start items-start px-2 pb-2 pt-1">
      <ConfigProfileCard
        v-for="profile in configProfiles"
        :key="profile.id"
        :profile="profile"
        :is-active="profile.id === activeConfigProfileId"
        @edit="startEdit(profile.id)"
        @select="selectProfile(profile.id)"
        @delete="deleteProfile(profile.id)"
        @update="updateRemoteProfile(profile.id)"
        @edit-info="openEditInfoDialog(profile.id)"
      />
      <div v-if="configProfiles.length === 0" class="col-span-full text-center text-base-content/50 py-12">
        暂无配置，请添加本地或远程配置
      </div>
    </div>

    <!-- 编辑状态 -->
    <template v-if="editingProfileId">
      <div class="flex items-center gap-2 shrink-0">
        <button class="btn btn-sm btn-ghost" @click="stopEdit">← 返回</button>
        <span class="text-sm text-base-content/60">正在编辑: {{ editingProfileName }}</span>
      </div>
      <ConfigEditor
        v-if="editingConfigPath"
        class="flex-1 min-h-0"
        :config-path="editingConfigPath"
        :singbox-path="singboxPath"
        :working-dir="workingDir"
        @saved="onConfigSaved"
      />
      <div
        v-else
        class="surface-card flex-1 min-h-0 flex items-center justify-center text-sm text-base-content/60"
      >
        正在加载配置路径...
      </div>
    </template>

    <!-- 添加本地配置对话框 -->
    <Transition name="glass-pop">
    <div
      v-if="showAddLocalDialog"
      class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      @click.self="showAddLocalDialog = false"
    >
      <div class="glass-popover w-full max-w-md rounded-[var(--radius-panel)] p-5 space-y-4">
        <h2 class="text-lg font-semibold">添加本地配置</h2>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">配置名称</span></label>
          <input
            v-model="addLocalForm.name"
            type="text"
            class="input input-sm input-bordered"
            placeholder="例如：默认配置"
          />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">配置文件路径或文件名</span></label>
          <div class="flex gap-2">
            <input
              v-model="addLocalForm.path"
              type="text"
              class="input input-sm input-bordered flex-1"
              placeholder="config.json 或 C:\sing-box\config.json"
            />
            <button class="btn btn-sm btn-glass shrink-0" @click="browseLocalFile">浏览</button>
          </div>
        </div>
        <div class="flex justify-end gap-2">
          <button class="btn btn-sm btn-ghost" @click="showAddLocalDialog = false">取消</button>
          <button class="btn btn-sm btn-primary" @click="handleAddLocal">添加</button>
        </div>
      </div>
    </div>
    </Transition>

    <!-- 添加远程配置对话框 -->
    <Transition name="glass-pop">
    <div
      v-if="showAddRemoteDialog"
      class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      @click.self="showAddRemoteDialog = false"
    >
      <div class="glass-popover w-full max-w-md rounded-[var(--radius-panel)] p-5 space-y-4">
        <h2 class="text-lg font-semibold">添加远程配置</h2>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">配置名称</span></label>
          <input
            v-model="addRemoteForm.name"
            type="text"
            class="input input-sm input-bordered"
            placeholder="例如：远程订阅"
          />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">远程地址 (URL)</span></label>
          <input
            v-model="addRemoteForm.url"
            type="text"
            class="input input-sm input-bordered"
            placeholder="https://example.com/config.json"
          />
        </div>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">自动更新间隔（小时，0 为不自动更新）</span></label>
          <input
            v-model.number="addRemoteForm.autoUpdateInterval"
            type="number"
            min="0"
            step="1"
            class="input input-sm input-bordered"
            placeholder="0"
          />
        </div>
        <div class="flex justify-end gap-2">
          <button class="btn btn-sm btn-ghost" @click="showAddRemoteDialog = false">取消</button>
          <button
            class="btn btn-sm btn-primary"
            :class="{ loading: addingRemote }"
            :disabled="addingRemote"
            @click="handleAddRemote"
          >
            添加
          </button>
        </div>
      </div>
    </div>
    </Transition>
    <!-- 修改信息对话框 -->
    <Transition name="glass-pop">
    <div
      v-if="showEditInfoDialog"
      class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      @click.self="showEditInfoDialog = false"
    >
      <div class="glass-popover w-full max-w-md rounded-[var(--radius-panel)] p-5 space-y-4">
        <h2 class="text-lg font-semibold">修改信息</h2>
        <div class="form-control">
          <label class="label"><span class="label-text text-xs">名称</span></label>
          <input
            v-model="editInfoForm.name"
            type="text"
            class="input input-sm input-bordered"
          />
        </div>
        <div class="form-control">
          <label class="label">
            <span class="label-text text-xs">{{ editInfoForm.type === 'local' ? '文件路径' : '远程地址' }}</span>
          </label>
          <div class="flex gap-2">
            <input
              v-model="editInfoForm.source"
              type="text"
              class="input input-sm input-bordered flex-1"
            />
            <button
              v-if="editInfoForm.type === 'local'"
              class="btn btn-sm btn-glass shrink-0"
              @click="browseEditInfoFile"
            >
              浏览
            </button>
          </div>
        </div>
        <div v-if="editInfoForm.type === 'remote'" class="form-control">
          <label class="label"><span class="label-text text-xs">自动更新间隔（小时，0 为关闭）</span></label>
          <input
            v-model.number="editInfoForm.autoUpdateInterval"
            type="number"
            min="0"
            step="1"
            class="input input-sm input-bordered"
          />
        </div>
        <div class="flex justify-end gap-2">
          <button class="btn btn-sm btn-ghost" @click="showEditInfoDialog = false">取消</button>
          <button class="btn btn-sm btn-primary" @click="saveEditInfo">保存</button>
        </div>
      </div>
    </div>
    </Transition>
  </div>
</template>
