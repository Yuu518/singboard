<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import { detectRuntimeFiles } from '@/bridge/config'
import { checkRootAccess, setRootEnabled } from '@/bridge/app'

const router = useRouter()
const {
  config,
  updateConfig,
  clashApiUrl,
  clashApiSecret,
  setSingleClashApi,
} = useConfigStore()
const { pushToast } = useToastStore()

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'update:visible', value: boolean): void
}>()

const setupError = ref('')
const setupSaving = ref(false)
const setupForm = ref({
  clashApiUrl: '',
  clashApiSecret: '',
  rootMode: false,
  workingDir: '',
})

function openWizard() {
  setupForm.value = {
    clashApiUrl: clashApiUrl.value || 'http://127.0.0.1:9090',
    clashApiSecret: clashApiSecret.value || '',
    rootMode: config.value.rootMode,
    workingDir: config.value.workingDir,
  }
  setupError.value = ''
  emit('update:visible', true)
}

async function enableRootMode(): Promise<boolean> {
  const workingDir = setupForm.value.workingDir.trim().replace(/\/+$/, '')
  if (!workingDir.startsWith('/')) {
    setupError.value = '请填写 sing-box 工作目录的绝对路径，例如 /data/adb/sing-box。'
    return false
  }
  if (!(await checkRootAccess())) {
    setupError.value = '未获得 root 权限，请在 Magisk / KernelSU / APatch 中授权 singboard 后重试，或关闭 root 管理。'
    return false
  }
  await setRootEnabled(true)
  const detected = await detectRuntimeFiles(workingDir)
  updateConfig({
    rootMode: true,
    workingDir: detected.baseDir,
    singboxPath: detected.singboxPath ?? '',
    coreInstallPath: detected.coreInstallPath,
    configPath: detected.configPath ?? '',
    rulesDir: detected.rulesDir ?? '',
  })
  if (!detected.singboxPath) {
    pushToast({ message: '工作目录中未找到 sing-box 核心，可在「设置 → 更新」中下载', type: 'info' }, 8000)
  } else if (!detected.configPath) {
    pushToast({ message: '工作目录中未找到配置文件', type: 'error' }, 8000)
  }
  return true
}

async function saveSetup() {
  const apiUrl = setupForm.value.clashApiUrl.trim()
  const apiSecret = setupForm.value.clashApiSecret

  if (!/^https?:\/\/.+/.test(apiUrl)) {
    setupError.value = '请填写以 http:// 或 https:// 开头的 Clash API 地址。'
    return
  }

  setupSaving.value = true
  setupError.value = ''
  try {
    if (setupForm.value.rootMode && !(await enableRootMode())) return
    if (!setupForm.value.rootMode) updateConfig({ rootMode: false })
    setSingleClashApi(apiUrl, apiSecret)
    updateConfig({ setupCompleted: true })
    emit('update:visible', false)
  } catch (e: any) {
    setupError.value = e?.message || String(e)
  } finally {
    setupSaving.value = false
  }
}

function goToSettings() {
  updateConfig({ setupCompleted: true })
  emit('update:visible', false)
  router.push('/settings')
}

function checkAndOpen() {
  if (!config.value.setupCompleted) {
    openWizard()
  }
}

defineExpose({ checkAndOpen })
</script>

<template>
  <Transition name="glass-pop">
  <div
    v-if="props.visible"
    class="glass-scrim safe-overlay fixed inset-0 z-50 flex items-center justify-center"
  >
    <div class="glass-popover max-h-full w-full max-w-xl overflow-y-auto rounded-[var(--radius-panel)] p-5 space-y-4">
      <h2 class="text-lg font-semibold">初始化向导</h2>
      <p class="text-sm text-base-content/70">
        填写要连接的 Clash API。sing-box 可以运行在路由器、电脑或本机上。
      </p>

      <div class="form-control">
        <label class="label"><span class="label-text text-xs">API 地址</span></label>
        <input
          v-model="setupForm.clashApiUrl"
          type="url"
          inputmode="url"
          autocapitalize="off"
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

      <label class="flex items-start justify-between gap-4 rounded-xl surface-fill px-3 py-2.5">
        <span class="min-w-0">
          <span class="block text-sm font-medium">使用 root 管理本机 sing-box</span>
          <span class="block text-xs text-base-content/60">
            需要 Magisk / KernelSU / APatch。支持启动、停止、重启、开机自启和核心更新。
          </span>
        </span>
        <input v-model="setupForm.rootMode" type="checkbox" class="toggle toggle-sm toggle-primary mt-1 shrink-0" />
      </label>

      <div v-if="setupForm.rootMode" class="form-control">
        <label class="label"><span class="label-text text-xs">sing-box 工作目录</span></label>
        <input
          v-model="setupForm.workingDir"
          type="text"
          autocapitalize="off"
          spellcheck="false"
          class="input input-sm input-bordered font-mono"
          placeholder="/data/adb/sing-box"
        />
        <span class="mt-1 text-xs text-base-content/55">
          会在目录及子目录中自动查找核心（优先 bin 文件夹）、配置文件和规则，日志写入 logs 或 log 文件夹。
        </span>
      </div>

      <p v-if="setupError" class="text-sm text-error">{{ setupError }}</p>

      <div class="flex flex-wrap justify-end gap-2">
        <button class="btn btn-sm btn-ghost" @click="goToSettings">稍后在设置中配置</button>
        <button class="btn btn-sm btn-primary" :disabled="setupSaving" @click="saveSetup">
          <span v-if="setupSaving" class="loading loading-spinner loading-xs"></span>
          完成
        </button>
      </div>
    </div>
  </div>
  </Transition>
</template>
