import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { isElevationCancelled } from '@/bridge/service'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import {
  checkPanelUpdate,
  isInstallPermissionRequired,
  performPanelUpdate,
  type PanelUpdateInfo,
  type PanelUpdateProgress,
} from '@/bridge/selfUpdate'
import { useConfigStore } from './config'
import { useToastStore } from './toast'
import { failureMessage } from '@/utils/humanError'
import { IS_ANDROID } from '@/platform'

// Module-level singleton: the startup check can fire on any page, while the
// settings card only exists on the settings page.
const currentVersion = ref('')
const latest = ref<PanelUpdateInfo | null>(null)
const checking = ref(false)
const updating = ref(false)
const progress = ref<PanelUpdateProgress | null>(null)
const pendingConfirm = ref<PanelUpdateInfo | null>(null)

let checkedThisSession = false

void getVersion().then((v) => { currentVersion.value = v }).catch(() => { })

void listen<PanelUpdateProgress>('panel-update-progress', (event) => {
  progress.value = event.payload
})

/** Silent mode stays quiet unless a newer version exists. */
async function check(silent: boolean) {
  if (checking.value || updating.value) return
  const { pushToast } = useToastStore()
  checking.value = true
  try {
    const info = await checkPanelUpdate(useConfigStore().config.value.githubToken)
    latest.value = info
    currentVersion.value = info.currentVersion
    // Hash mismatch is manual-check only: self-built and dev binaries never
    // match the release asset, so prompting on every startup would be endless.
    if (info.hasUpdate || (!silent && info.outOfSync)) {
      pendingConfirm.value = info
    } else if (!silent) {
      pushToast({ message: `当前已是最新版本（${info.currentVersion}）`, type: 'info' })
    }
  } catch (e) {
    if (!silent) pushToast({ message: failureMessage('检查面板更新失败', e), type: 'error' }, 6000)
  } finally {
    checking.value = false
  }
}

async function checkOnStartup() {
  if (checkedThisSession) return
  checkedThisSession = true
  const previousError = IS_ANDROID
    ? null
    : await invoke<string | null>('take_panel_update_error').catch(() => null)
  if (previousError) {
    const { pushToast } = useToastStore()
    pushToast({ message: isElevationCancelled(previousError) ? '已取消面板更新' : failureMessage('上次面板更新失败', previousError), type: 'info' }, 6000)
  }
  await check(true)
}

async function runUpdate() {
  const info = latest.value
  if (!info || updating.value) return
  const { config } = useConfigStore()
  const { pushToast } = useToastStore()
  updating.value = true
  progress.value = null
  try {
    await performPanelUpdate({
      assetUrl: info.assetUrl,
      assetApiUrl: info.assetApiUrl,
      assetSize: info.assetSize,
      assetDigest: info.assetDigest,
      mirror: config.value.coreUpdateMirror,
      githubToken: config.value.githubToken,
    })
    // Deliberately leaves `updating` set: the overlay must stay up until the
    // process actually dies.
    if (IS_ANDROID) {
      pushToast({ message: '安装包已校验，请在系统安装界面中确认更新', type: 'info' }, 6000)
      updating.value = false
      progress.value = null
    }
  } catch (e) {
    if (IS_ANDROID && isInstallPermissionRequired(e)) {
      pushToast({ message: '请在系统设置中允许 singboard 安装应用，返回后再次点击更新', type: 'info' }, 8000)
    } else if (!isElevationCancelled(e)) {
      pushToast({ message: failureMessage('面板更新失败', e), type: 'error' }, 8000)
    }
    updating.value = false
    progress.value = null
  }
}

export function usePanelUpdateStore() {
  return {
    currentVersion,
    latest,
    checking,
    updating,
    progress,
    pendingConfirm,
    check,
    checkOnStartup,
    runUpdate,
  }
}
