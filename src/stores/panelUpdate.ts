import { ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { checkPanelUpdate, performPanelUpdate, type PanelUpdateInfo, type PanelUpdateProgress } from '@/bridge/selfUpdate'
import { useConfigStore } from './config'
import { useToastStore } from './toast'

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
    const info = await checkPanelUpdate()
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
    if (!silent) pushToast({ message: `检查更新失败: ${e}`, type: 'error' })
  } finally {
    checking.value = false
  }
}

async function checkOnStartup() {
  if (checkedThisSession) return
  checkedThisSession = true
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
      assetSize: info.assetSize,
      assetDigest: info.assetDigest,
      mirror: config.value.coreUpdateMirror,
    })
    // Deliberately leaves `updating` set: the overlay must stay up until the
    // process actually dies.
  } catch (e) {
    pushToast({ message: `更新失败: ${e}`, type: 'error' })
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
