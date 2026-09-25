<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useConfigStore } from '@/stores/config'
import { detectRuntimeFiles, copyToRunningConfig } from '@/bridge/config'
import { open } from '@tauri-apps/plugin-dialog'

const router = useRouter()
const {
  config,
  updateConfig,
  clashApiUrl,
  clashApiSecret,
  setSingleClashApi,
  addConfigProfile,
  setActiveConfigProfile,
  configProfiles,
} = useConfigStore()

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'update:visible', value: boolean): void
}>()

const setupError = ref('')
const setupSaving = ref(false)
const setupForm = ref({
  workingDir: '',
  clashApiUrl: '',
  clashApiSecret: '',
})

function openWizard() {
  setupForm.value = {
    workingDir: config.value.workingDir ?? '',
    clashApiUrl: clashApiUrl.value || 'http://127.0.0.1:9090',
    clashApiSecret: clashApiSecret.value || '',
  }
  setupError.value = ''
  emit('update:visible', true)
}

async function browseWorkingDir() {
  const selected = await open({
    directory: true,
    defaultPath: setupForm.value.workingDir.trim() || undefined,
  })
  if (selected) {
    setupForm.value.workingDir = selected as string
  }
}

async function saveSetup() {
  const workingDir = setupForm.value.workingDir.trim()
  const apiUrl = setupForm.value.clashApiUrl.trim()
  const apiSecret = setupForm.value.clashApiSecret

  if (!workingDir) {
    setupError.value = '请填写工作目录。'
    return
  }
  if (!apiUrl) {
    setupError.value = '请填写 Clash API 地址。'
    return
  }

  setupSaving.value = true
  setupError.value = ''
  try {
    const detected = await detectRuntimeFiles(workingDir)
    if (!detected.singboxPath) {
      setupError.value = '未在该目录及子目录中检测到 sing-box 核心，请检查目录后重试。'
      return
    }

    updateConfig({
      workingDir: detected.baseDir,
      singboxPath: detected.singboxPath,
    })
    setSingleClashApi(apiUrl, apiSecret)

    if (detected.configPath && configProfiles.value.length === 0) {
      const id = addConfigProfile('默认配置', 'local', detected.configPath)
      setActiveConfigProfile(id)
      try {
        await copyToRunningConfig(detected.configPath)
      } catch { }
    }

    emit('update:visible', false)
  } catch (e: any) {
    setupError.value = e?.message || '自动检测失败，请检查工作目录是否有效。'
  } finally {
    setupSaving.value = false
  }
}

function goToSettings() {
  emit('update:visible', false)
  router.push('/settings')
}

function checkAndOpen() {
  if (!(config.value.singboxPath.trim() && config.value.workingDir.trim())) {
    openWizard()
  }
}

defineExpose({ checkAndOpen })
</script>

<template>
  <Transition name="glass-pop">
  <div
    v-if="props.visible"
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
  >
    <div class="glass-popover w-full max-w-xl rounded-[var(--radius-panel)] p-5 space-y-4">
      <h2 class="text-lg font-semibold">初始化向导</h2>
      <p class="text-sm text-base-content/70">
        只需填写工作目录，系统会自动扫描该目录及其子目录，识别 sing-box 核心与配置文件。
      </p>

      <div class="text-sm font-medium text-base-content/70">路径配置</div>
      <div class="form-control">
        <label class="label"><span class="label-text text-xs">工作目录</span></label>
        <div class="flex gap-2">
          <input
            v-model="setupForm.workingDir"
            type="text"
            class="input input-sm input-bordered flex-1"
            placeholder="C:\sing-box"
          />
          <button class="btn btn-sm btn-glass shrink-0" @click="browseWorkingDir">浏览</button>
        </div>
      </div>

      <div class="divider my-1"></div>
      <div class="text-sm font-medium text-base-content/70">Clash API</div>
      <div class="form-control">
        <label class="label"><span class="label-text text-xs">API 地址</span></label>
        <input
          v-model="setupForm.clashApiUrl"
          type="text"
          class="input input-sm input-bordered"
          placeholder="http://127.0.0.1:9090"
        />
      </div>
      <div class="form-control">
        <label class="label"><span class="label-text text-xs">密钥 (Secret)</span></label>
        <input
          v-model="setupForm.clashApiSecret"
          type="password"
          class="input input-sm input-bordered"
          placeholder="留空表示无密钥"
        />
      </div>

      <p v-if="setupError" class="text-sm text-error">{{ setupError }}</p>

      <div class="flex justify-end gap-2">
        <button class="btn btn-sm btn-ghost" @click="goToSettings">前往设置页</button>
        <button class="btn btn-sm btn-primary" :class="{ loading: setupSaving }" @click="saveSetup">
          自动检测并继续
        </button>
      </div>
    </div>
  </div>
  </Transition>
</template>
