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
import { useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import { syncActiveConfigToRunning } from '@/utils/coreControl'
import { isLaunchedHidden, setWindowMaterial } from '@/bridge/app'
import { appVisible } from '@/stores/appVisible'
import TrayMenu from '@/components/tray/TrayMenu.vue'
import { useConfigAutoUpdate } from '@/composables/useConfigAutoUpdate'
import { useLiquidLens } from '@/composables/useLiquidLens'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { usePanelUpdateStore } from '@/stores/panelUpdate'
import { useLogsLifecycle } from '@/stores/logs'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getIPFromIpipnet, getIPFromIpsb } from '@/api/geoip'
import {
  getWechatLatency,
  getBilibiliLatency,
  getGithubLatency,
  getCloudflareLatency,
  getYoutubeLatency,
} from '@/api/latency'

const currentWindow = getCurrentWindow()
const isTrayWindow = currentWindow.label === 'tray'
document.documentElement.classList.toggle('tray-window', isTrayWindow)
const logsLifecycle = isTrayWindow ? null : useLogsLifecycle()

const { config, resolvedTheme } = useConfigStore()
const { serviceStatus, ready: serviceReady } = useServiceStore()
const configAutoUpdate = isTrayWindow ? null : useConfigAutoUpdate()
const { loadProxies, resumePendingTests, refreshDuringCoreWarmup } = useProxiesStore()
const { resetHistory: resetOverviewHistory } = useOverviewStore()
const { resetOnRestart: resetConnections } = useConnectionsStore()
const { detectVersion } = useSingboxVersionStore()
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

const NETWORK_CACHE_KEY = 'singboard-network'

function runNetworkAutoTest() {
  try {
    const saved = sessionStorage.getItem(NETWORK_CACHE_KEY)
    if (saved) {
      const cached = JSON.parse(saved)
      const hasIP = !!(cached?.chinaIP?.ip || cached?.globalIP?.ip)
      const hasLatency = !!(cached?.latency?.wechat || cached?.latency?.cloudflare)
      if (hasIP && hasLatency) return
    }
  } catch {}

  const result: any = {
    chinaIP: { ip: '', location: '', locationMasked: '' },
    globalIP: { ip: '', location: '', locationMasked: '' },
    latency: { wechat: '', bilibili: '', github: '', cloudflare: '', youtube: '' },
  }

  getIPFromIpipnet().then((res) => {
    const loc = res.location.filter(Boolean)
    result.chinaIP = {
      ip: res.ip,
      location: loc.join(' '),
      locationMasked: loc.length > 0
        ? loc[0] + ' ' + loc.slice(1).map(() => '**').join(' ')
        : '',
    }
    sessionStorage.setItem(NETWORK_CACHE_KEY, JSON.stringify(result))
  }).catch(() => {})

  getIPFromIpsb().then((res) => {
    const loc = [res.country, res.organization].filter(Boolean).join(' ')
    result.globalIP = { ip: res.ip, location: loc, locationMasked: loc }
    sessionStorage.setItem(NETWORK_CACHE_KEY, JSON.stringify(result))
  }).catch(() => {})

  const latencyTests = [
    { fn: getWechatLatency, key: 'wechat' },
    { fn: getBilibiliLatency, key: 'bilibili' },
    { fn: getGithubLatency, key: 'github' },
    { fn: getCloudflareLatency, key: 'cloudflare' },
    { fn: getYoutubeLatency, key: 'youtube' },
  ]
  for (const { fn, key } of latencyTests) {
    fn().then((ms) => {
      result.latency[key] = ms ? ms.toFixed(0) : '超时'
      sessionStorage.setItem(NETWORK_CACHE_KEY, JSON.stringify(result))
    }).catch(() => {})
  }
}

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
    if (!isTrayWindow) void detectVersion()
  },
)

let coreStartedOnce = false

if (!isTrayWindow) {
  watch(
    () => serviceStatus.value.state,
    (state) => {
      if (state === 'running') {
        if (coreStartedOnce) {
          resetOverviewHistory()
          resetConnections()
          sessionStorage.removeItem(NETWORK_CACHE_KEY)
        }
        coreStartedOnce = true
        // 核心可能在停止期间被手动替换，启动后重新检测版本
        void detectVersion()
        void refreshDuringCoreWarmup()
        setTimeout(runNetworkAutoTest, 3000)
      } else if (coreStartedOnce) {
        resetOverviewHistory()
        resetConnections()
      }
    },
    { immediate: true },
  )
}
</script>

<template>
  <TrayMenu v-if="isTrayWindow" />
  <div v-else class="relative flex h-screen text-base-content">
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
