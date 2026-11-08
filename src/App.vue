<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import Titlebar from '@/components/layout/Titlebar.vue'
import Sidebar from '@/components/layout/Sidebar.vue'
import { navItems } from '@/components/layout/navItems'
import ToastHost from '@/components/common/ToastHost.vue'
import SetupWizard from '@/components/common/SetupWizard.vue'
import PanelUpdateDialog from '@/components/common/PanelUpdateDialog.vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useProxiesStore } from '@/stores/proxies'
import { resetOverview, resumeOverview, suspendOverview } from '@/stores/overview'
import { resetConnectionsOnRestart, resumeConnections, suspendConnections } from '@/stores/connections'
import { syncActiveConfigToRunning } from '@/utils/coreControl'
import { isLaunchedHidden, setWindowMaterial } from '@/bridge/app'
import { appVisible } from '@/stores/appVisible'
import TrayMenu from '@/components/tray/TrayMenu.vue'
import { useConfigAutoUpdate } from '@/composables/useConfigAutoUpdate'
import { useLiquidLens } from '@/composables/useLiquidLens'
import { useGlassSheen } from '@/composables/useGlassSheen'
import { useBackgroundStore } from '@/stores/background'
import { useBackdropTone } from '@/composables/useBackdropTone'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { usePanelUpdateStore } from '@/stores/panelUpdate'
import { useLogsLifecycle } from '@/stores/logs'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useNetworkStore } from '@/stores/network'

const currentWindow = getCurrentWindow()
const isTrayWindow = currentWindow.label === 'tray'
const { backgroundUrl, init: initBackground } = useBackgroundStore()
document.documentElement.classList.toggle('tray-window', isTrayWindow)
const logsLifecycle = isTrayWindow ? null : useLogsLifecycle()

const { config, resolvedTheme } = useConfigStore()
const { serviceStatus, ready: serviceReady } = useServiceStore()
const configAutoUpdate = isTrayWindow ? null : useConfigAutoUpdate()
const { loadProxies, resumePendingTests, refreshDuringCoreWarmup } = useProxiesStore()
const network = useNetworkStore()
const { detectVersion, scheduleDetectVersion } = useSingboxVersionStore()
const { checkOnStartup: checkPanelUpdateOnStartup } = usePanelUpdateStore()

const route = useRoute()
const pageTitle = computed(() => navItems.find((item) => item.path === route.path)?.label ?? '')
const mainRef = ref<HTMLElement | null>(null)
const scrolled = ref(false)

function onMainScroll() {
  scrolled.value = (mainRef.value?.scrollTop ?? 0) > 0
}

watch(
  () => route.path,
  () => {
    void nextTick(() => {
      if (mainRef.value) mainRef.value.scrollTop = 0
      scrolled.value = false
    })
  },
)

if (!isTrayWindow) {
  useLiquidLens()
  useGlassSheen()
  void initBackground()
  useBackdropTone()
  watch(
    resolvedTheme,
    (theme) => {
      setWindowMaterial(theme === 'dark')
        .then((applied) => document.documentElement.classList.toggle('has-mica', applied))
        .catch(() => document.documentElement.classList.remove('has-mica'))
    },
    { immediate: true },
  )
}

const setupWizardVisible = ref(false)
const setupWizardRef = ref<InstanceType<typeof SetupWizard> | null>(null)

onMounted(async () => {
  if (isTrayWindow) return
  await serviceReady
  const launchedHidden = await isLaunchedHidden().catch(() => false)
  if (launchedHidden) {
    appVisible.value = false
    // The tray may have restored the window while startup work was still pending.
    appVisible.value = await currentWindow.isVisible().catch(() => false)
  } else {
    appVisible.value = true
    await currentWindow.show().catch(() => {})
  }
  logsLifecycle?.start()
  setupWizardRef.value?.checkAndOpen()
  await syncActiveConfigToRunning().catch((e) => {
    console.error('Failed to sync active config on startup:', e)
  })
  await loadProxies()
  resumePendingTests()
  configAutoUpdate?.start()
  void detectVersion()

  if (config.value.panelAutoCheckUpdate) {
    if (launchedHidden) {
      // A confirm dialog on a hidden window is invisible, so defer it.
      const stopVisibleWatch = watch(appVisible, (visible) => {
        if (!visible) return
        stopVisibleWatch()
        void checkPanelUpdateOnStartup()
      })
    } else {
      void checkPanelUpdateOnStartup()
    }
  }
})

watch(
  () => config.value.singboxPath,
  () => {
    if (!isTrayWindow) scheduleDetectVersion()
  },
)

let coreStartedOnce = false
let lastKnownState: string | null = null

if (!isTrayWindow) {
  watch(
    () => serviceStatus.value.uptimeSeconds,
    (uptime, previous) => {
      if (serviceStatus.value.state !== 'running' || typeof uptime !== 'number' || typeof previous !== 'number') return
      if (uptime + 2 >= previous) return
      suspendOverview()
      suspendConnections()
      resetOverview()
      resetConnectionsOnRestart()
      resumeOverview()
      resumeConnections()
      network.scheduleAutoCheck()
    },
  )
  watch(
    () => serviceStatus.value.state,
    (state) => {
      if (state === 'unknown') return
      const wasRunning = lastKnownState === 'running'
      lastKnownState = state
      if (state === 'running') {
        if (wasRunning) return
        if (coreStartedOnce) {
          resetOverview()
          resetConnectionsOnRestart()
        }
        resumeOverview()
        resumeConnections()
        coreStartedOnce = true
        // 核心可能在停止期间被手动替换，启动后重新检测版本
        void detectVersion()
        void refreshDuringCoreWarmup()
        network.scheduleAutoCheck()
      } else {
        network.reset()
        suspendOverview()
        suspendConnections()
        if (coreStartedOnce) {
          resetOverview()
          resetConnectionsOnRestart()
        }
      }
    },
    { immediate: true },
  )
}
</script>

<template>
  <TrayMenu v-if="isTrayWindow" />
  <div v-else class="relative flex h-screen text-base-content">
    <div
      v-if="backgroundUrl"
      class="app-backdrop"
      :style="{ backgroundImage: `url(${backgroundUrl})` }"
      aria-hidden="true"
    />
    <Sidebar />
    <div class="relative flex min-w-0 flex-1 flex-col">
      <Titlebar class="absolute inset-x-0 top-0 z-30" :title="pageTitle" :scrolled="scrolled" />
      <main ref="mainRef" class="app-main min-h-0 flex-1 overflow-auto px-5 pb-5 pt-12" @scroll.passive="onMainScroll">
        <router-view />
      </main>
    </div>
    <ToastHost />
    <SetupWizard ref="setupWizardRef" v-model:visible="setupWizardVisible" />
    <PanelUpdateDialog />
  </div>
</template>
