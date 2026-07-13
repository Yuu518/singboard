<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import Titlebar from '@/components/layout/Titlebar.vue'
import Sidebar from '@/components/layout/Sidebar.vue'
import ToastHost from '@/components/common/ToastHost.vue'
import SetupWizard from '@/components/common/SetupWizard.vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useProxiesStore } from '@/stores/proxies'
import { useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import { copyToRunningConfig, getRemoteConfigPath } from '@/bridge/config'
import { isLaunchedHidden } from '@/bridge/app'
import { appVisible } from '@/stores/appVisible'
import TrayMenu from '@/components/tray/TrayMenu.vue'
import { useConfigAutoUpdate } from '@/composables/useConfigAutoUpdate'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getIPFromIpipnet, getIPFromIpsb } from '@/api/geoip'
import {
  getWechatLatency,
  getBilibiliLatency,
  getGithubLatency,
  getCloudflareLatency,
  getYoutubeLatency,
} from '@/api/latency'

const isTrayWindow = getCurrentWindow().label === 'tray'

const { config, configProfiles } = useConfigStore()
const { serviceStatus, ready: serviceReady } = useServiceStore()
const { start: startAutoUpdate } = useConfigAutoUpdate()
const { loadProxies, resumePendingTests } = useProxiesStore()
const { resetHistory: resetOverviewHistory } = useOverviewStore()
const { resetOnRestart: resetConnections } = useConnectionsStore()
const { detectVersion } = useSingboxVersionStore()

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

async function syncActiveConfigToRunning() {
  const activeId = config.value.activeConfigProfileId
  if (!activeId) return
  const profile = configProfiles.value.find((p) => p.id === activeId)
  if (!profile) return
  try {
    let sourcePath: string
    if (profile.type === 'local') {
      sourcePath = profile.source
    } else {
      sourcePath = await getRemoteConfigPath(profile.id)
    }
    await copyToRunningConfig(sourcePath)
  } catch (e) {
    console.error('Failed to sync active config on startup:', e)
  }
}

onMounted(async () => {
  if (isTrayWindow) return
  await serviceReady
  const launchedHidden = await isLaunchedHidden().catch(() => false)
  if (launchedHidden) {
    appVisible.value = false
  } else {
    getCurrentWindow().show()
  }
  setupWizardRef.value?.checkAndOpen()
  await syncActiveConfigToRunning()
  await loadProxies()
  resumePendingTests()
  startAutoUpdate()
  void detectVersion()
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
  <div v-else class="flex flex-col h-screen bg-base-100 text-base-content">
    <Titlebar />
    <div class="flex flex-1 overflow-hidden">
      <Sidebar />
      <main class="flex-1 overflow-auto p-4">
        <router-view />
      </main>
    </div>
    <ToastHost />
    <SetupWizard ref="setupWizardRef" v-model:visible="setupWizardVisible" />
  </div>
</template>
