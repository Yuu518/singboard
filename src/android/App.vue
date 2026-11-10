<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import BottomNav from '@/android/components/layout/BottomNav.vue'
import MainPager from '@/android/components/layout/MainPager.vue'
import { pagerIndex } from '@/android/stores/pager'
import AppHeader from '@/android/components/layout/AppHeader.vue'
import { navItems } from '@/android/components/layout/navItems'
import ToastHost from '@/components/common/ToastHost.vue'
import SetupWizard from '@/android/components/SetupWizard.vue'
import PanelUpdateDialog from '@/components/common/PanelUpdateDialog.vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore, whenClashApiSettled } from '@/stores/service'
import { useProxiesStore } from '@/stores/proxies'
import { useRulesStore } from '@/stores/rules'
import { useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import { getSystemInsets, setSystemBars, type SystemInsets } from '@/bridge/app'
import { cleanupPanelUpdate } from '@/bridge/selfUpdate'
import { useLiquidLens } from '@/composables/useLiquidLens'
import { useGlassSheen } from '@/composables/useGlassSheen'
import { useBackgroundStore } from '@/stores/background'
import { useBackdropTone } from '@/composables/useBackdropTone'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { usePanelUpdateStore } from '@/stores/panelUpdate'
import { useLogsLifecycle } from '@/stores/logs'
import { keyboardVisible } from '@/android/stores/keyboard'
import { useNetworkStore } from '@/stores/network'

const { backgroundUrl, init: initBackground } = useBackgroundStore()
const logsLifecycle = useLogsLifecycle()

const { config, resolvedTheme, clashApiEndpoint } = useConfigStore()
const { serviceStatus, clashApiReady, ready: serviceReady } = useServiceStore()
const { loadProxies, resumePendingTests, refreshDuringCoreWarmup } = useProxiesStore()
const { reloadAfterCoreReady: reloadRules } = useRulesStore()
const { reset: resetOverviewHistory } = useOverviewStore()
const { resetOnRestart: resetConnections } = useConnectionsStore()
const { detectVersion } = useSingboxVersionStore()
const { checkOnStartup: checkPanelUpdateOnStartup } = usePanelUpdateStore()
const network = useNetworkStore()

const pageTitle = computed(() => navItems[pagerIndex.value]?.label ?? '')
const scrolled = ref(false)

useLiquidLens()
useGlassSheen()
void initBackground()
useBackdropTone()

function applyInsets(insets: SystemInsets) {
  const style = document.documentElement.style
  style.setProperty('--safe-top', `${insets.top}px`)
  style.setProperty('--safe-right', `${insets.right}px`)
  style.setProperty('--safe-bottom', `${insets.bottom}px`)
  style.setProperty('--safe-left', `${insets.left}px`)
}

void getSystemInsets()
  .then((insets) => {
    if (insets && (insets.top || insets.right || insets.bottom || insets.left)) applyInsets(insets)
  })
  .catch(() => {})

watch(
  resolvedTheme,
  (theme) => {
    void nextTick(() => {
      const color = getComputedStyle(document.documentElement).getPropertyValue('--page-bg').trim()
      setSystemBars(theme === 'dark', color || undefined).catch(() => {})
    })
  },
  { immediate: true },
)

const setupWizardVisible = ref(false)
const setupWizardRef = ref<InstanceType<typeof SetupWizard> | null>(null)

onMounted(async () => {
  void cleanupPanelUpdate().catch(() => {})
  await serviceReady
  logsLifecycle.start()
  setupWizardRef.value?.checkAndOpen()
  void detectVersion()

  if (config.value.panelAutoCheckUpdate) {
    void checkPanelUpdateOnStartup()
  }

  if (await whenClashApiSettled() === 'ready') {
    await loadProxies()
    resumePendingTests()
  }
})

watch(
  () => [config.value.singboxPath, config.value.rootMode, clashApiEndpoint.value],
  () => {
    void detectVersion()
  },
)

let coreStartedOnce = false
let handledInstance: string | null = null
let endpointChanged = false

watch(clashApiEndpoint, () => {
  resetConnections()
  endpointChanged = true
})

watch(
  clashApiReady,
  (ready) => {
    if (!ready) return
    const reloadForEndpoint = endpointChanged
    endpointChanged = false
    if (reloadForEndpoint) void loadProxies(true)
    const instance = String(serviceStatus.value.pid ?? '')
    if (instance === handledInstance) {
      if (reloadForEndpoint) network.scheduleAutoCheck()
      return
    }
    handledInstance = instance
    if (coreStartedOnce) {
      resetOverviewHistory()
      resetConnections()
    }
    coreStartedOnce = true
    void detectVersion()
    void refreshDuringCoreWarmup()
    void reloadRules()
    network.scheduleAutoCheck()
  },
  { immediate: true },
)

watch(
  () => serviceStatus.value.state,
  (state) => {
    if (state === 'running') return
    handledInstance = null
    network.reset()
    if (coreStartedOnce) {
      resetOverviewHistory()
      resetConnections()
    }
  },
)
</script>

<template>
  <div class="relative h-full text-base-content" :class="{ 'keyboard-open': keyboardVisible }">
    <div
      v-if="backgroundUrl"
      class="app-backdrop"
      :style="{ backgroundImage: `url(${backgroundUrl})` }"
      aria-hidden="true"
    />
    <AppHeader :title="pageTitle" :scrolled="scrolled" />
    <MainPager @scrolled="scrolled = $event" />
    <div class="edge-fade is-top" aria-hidden="true" />
    <BottomNav />
    <ToastHost />
    <SetupWizard ref="setupWizardRef" v-model:visible="setupWizardVisible" />
    <PanelUpdateDialog />
  </div>
</template>
