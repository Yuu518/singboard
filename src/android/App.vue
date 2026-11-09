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
import { getIPFromIpipnet, getIPFromIpsb } from '@/api/geoip'
import {
  getWechatLatency,
  getBilibiliLatency,
  getGithubLatency,
  getCloudflareLatency,
  getYoutubeLatency,
} from '@/api/latency'

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
    if (instance === handledInstance) return
    handledInstance = instance
    if (coreStartedOnce) {
      resetOverviewHistory()
      resetConnections()
      sessionStorage.removeItem(NETWORK_CACHE_KEY)
    }
    coreStartedOnce = true
    void detectVersion()
    void refreshDuringCoreWarmup()
    void reloadRules()
    setTimeout(runNetworkAutoTest, 3000)
  },
  { immediate: true },
)

watch(
  () => serviceStatus.value.state,
  (state) => {
    if (state === 'running') return
    handledInstance = null
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
