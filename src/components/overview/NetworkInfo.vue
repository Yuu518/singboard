<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useServiceStore } from '@/stores/service'
import { getIPFromIpipnet, getIPFromIpsb } from '@/api/geoip'
import {
  getWechatLatency,
  getBilibiliLatency,
  getGithubLatency,
  getCloudflareLatency,
  getYoutubeLatency,
} from '@/api/latency'

const CACHE_KEY = 'singboard-network'

interface IPData { ip: string; location: string; locationMasked: string }
interface LatencyData { wechat: string; bilibili: string; github: string; cloudflare: string; youtube: string }
interface NetworkCache { chinaIP: IPData; globalIP: IPData; latency: LatencyData }

function loadCache(): NetworkCache | null {
  try {
    const saved = sessionStorage.getItem(CACHE_KEY)
    if (saved) return JSON.parse(saved)
  } catch {}
  return null
}

function saveCache() {
  sessionStorage.setItem(CACHE_KEY, JSON.stringify({
    chinaIP: chinaIP.value,
    globalIP: globalIP.value,
    latency: latency.value,
  }))
}

const { serviceStatus } = useServiceStore()
const isRunning = computed(() => serviceStatus.value.state === 'running')

const cached = isRunning.value ? loadCache() : null

const chinaIP = ref<IPData>(cached?.chinaIP ?? { ip: '', location: '', locationMasked: '' })
const globalIP = ref<IPData>(cached?.globalIP ?? { ip: '', location: '', locationMasked: '' })
const ipLoading = ref(false)
const showIP = ref(false)

const latency = ref<LatencyData>(cached?.latency ?? {
  wechat: '',
  bilibili: '',
  github: '',
  cloudflare: '',
  youtube: '',
})
const latencyLoading = ref(false)

let ipGen = 0
let latencyGen = 0

function resetData() {
  sessionStorage.removeItem(CACHE_KEY)
  ipGen++
  latencyGen++
  ipLoading.value = false
  latencyLoading.value = false
  chinaIP.value = { ip: '', location: '', locationMasked: '' }
  globalIP.value = { ip: '', location: '', locationMasked: '' }
  latency.value = { wechat: '', bilibili: '', github: '', cloudflare: '', youtube: '' }
}

function maskIP(ip: string) {
  if (!ip) return ''
  return ip.replace(/\d/g, '*').replace(/[a-fA-F]/g, '*')
}

function latencyTextColor(ms: string) {
  const n = Number(ms)
  if (!n || n === 0) return 'text-base-content/40'
  if (n < 200) return 'text-success'
  if (n < 500) return 'text-warning'
  return 'text-error'
}

async function checkIP() {
  if (ipLoading.value) return
  ipLoading.value = true
  const gen = ++ipGen
  chinaIP.value = { ip: '', location: '检测中...', locationMasked: '检测中...' }
  globalIP.value = { ip: '', location: '检测中...', locationMasked: '检测中...' }

  try {
    const res = await getIPFromIpipnet()
    if (gen !== ipGen) return
    const loc = res.location.filter(Boolean)
    chinaIP.value = {
      ip: res.ip,
      location: loc.join(' '),
      locationMasked: loc.length > 0
        ? loc[0] + ' ' + loc.slice(1).map(() => '**').join(' ')
        : '',
    }
  } catch {
    if (gen !== ipGen) return
    chinaIP.value = { ip: '', location: '检测失败', locationMasked: '检测失败' }
  }

  try {
    const res = await getIPFromIpsb()
    if (gen !== ipGen) return
    const loc = [res.country, res.organization].filter(Boolean).join(' ')
    globalIP.value = {
      ip: res.ip,
      location: loc,
      locationMasked: loc,
    }
  } catch {
    if (gen !== ipGen) return
    globalIP.value = { ip: '', location: '检测失败', locationMasked: '检测失败' }
  }

  if (gen !== ipGen) return
  ipLoading.value = false
  saveCache()
}

async function checkLatency() {
  if (latencyLoading.value) return
  latencyLoading.value = true
  const gen = ++latencyGen
  latency.value = { wechat: '...', bilibili: '...', github: '...', cloudflare: '...', youtube: '...' }

  let done = 0
  const onDone = () => {
    if (gen !== latencyGen) return
    done++
    if (done >= 5) {
      latencyLoading.value = false
      saveCache()
    }
  }

  const write = (key: keyof LatencyData, ms: number) => {
    if (gen === latencyGen) latency.value[key] = ms ? ms.toFixed(0) : '超时'
  }

  getWechatLatency().then((ms) => write('wechat', ms)).finally(onDone)
  getBilibiliLatency().then((ms) => write('bilibili', ms)).finally(onDone)
  getGithubLatency().then((ms) => write('github', ms)).finally(onDone)
  getCloudflareLatency().then((ms) => write('cloudflare', ms)).finally(onDone)
  getYoutubeLatency().then((ms) => write('youtube', ms)).finally(onDone)
}

if (!cached && isRunning.value) {
  checkIP()
  checkLatency()
}

watch(() => serviceStatus.value.state, (state, oldState) => {
  if (state !== 'running' && oldState === 'running') {
    resetData()
  } else if (state === 'running' && oldState !== 'running') {
    checkIP()
    checkLatency()
  }
})
</script>

<template>
  <div class="surface-card p-4 space-y-3">
    <h2 class="text-sm font-semibold">网络信息</h2>
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
      <div class="surface-fill rounded-[var(--radius-tile)] p-3 space-y-2 relative">
        <div class="text-xs font-medium text-base-content/60 mb-2">IP 信息</div>
        <div class="grid grid-cols-[auto_auto_1fr] gap-x-2 gap-y-1 text-sm">
          <span>ipip.net</span>
          <span>:</span>
          <span>
            <template v-if="chinaIP.location">
              <template v-if="showIP">
                {{ chinaIP.location }} <span class="text-base-content/60">({{ chinaIP.ip }})</span>
              </template>
              <template v-else>
                {{ chinaIP.locationMasked }} <span class="text-base-content/60">({{ maskIP(chinaIP.ip) || '***.***.***.***' }})</span>
              </template>
            </template>
            <template v-else>
              <span class="text-base-content/30">未检测</span>
            </template>
          </span>
          <span>ip.sb</span>
          <span>:</span>
          <span>
            <template v-if="globalIP.location">
              <template v-if="showIP">
                {{ globalIP.location }} <span class="text-base-content/60">({{ globalIP.ip }})</span>
              </template>
              <template v-else>
                {{ globalIP.locationMasked }} <span class="text-base-content/60">({{ maskIP(globalIP.ip) || '***.***.***.***' }})</span>
              </template>
            </template>
            <template v-else>
              <span class="text-base-content/30">未检测</span>
            </template>
          </span>
        </div>
        <div class="absolute right-2 bottom-2 flex gap-1">
          <button class="btn btn-ghost btn-xs btn-circle" @click="showIP = !showIP">
            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
              <path v-if="showIP" stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
              <path v-if="showIP" stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              <path v-if="!showIP" stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12c1.292 4.338 5.31 7.5 10.066 7.5.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88" />
            </svg>
          </button>
          <button
            class="btn btn-ghost btn-xs btn-circle"
            :class="{ 'loading': ipLoading }"
            @click="checkIP"
          >
            <svg v-if="!ipLoading" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
            </svg>
          </button>
        </div>
      </div>

      <div class="surface-fill rounded-[var(--radius-tile)] p-3 space-y-2 relative">
        <div class="text-xs font-medium text-base-content/60 mb-2">连通性测试</div>
        <div class="grid grid-cols-[auto_auto_1fr] gap-x-2 gap-y-1 text-sm">
          <span>微信</span>
          <span>:</span>
          <span :class="latencyTextColor(latency.wechat)">{{ latency.wechat ? latency.wechat + 'ms' : '未检测' }}</span>
          <span>Bilibili</span>
          <span>:</span>
          <span :class="latencyTextColor(latency.bilibili)">{{ latency.bilibili ? latency.bilibili + 'ms' : '未检测' }}</span>
          <span>GitHub</span>
          <span>:</span>
          <span :class="latencyTextColor(latency.github)">{{ latency.github ? latency.github + 'ms' : '未检测' }}</span>
          <span>Cloudflare</span>
          <span>:</span>
          <span :class="latencyTextColor(latency.cloudflare)">{{ latency.cloudflare ? latency.cloudflare + 'ms' : '未检测' }}</span>
          <span>YouTube</span>
          <span>:</span>
          <span :class="latencyTextColor(latency.youtube)">{{ latency.youtube ? latency.youtube + 'ms' : '未检测' }}</span>
        </div>
        <div class="absolute right-2 bottom-2">
          <button
            class="btn btn-ghost btn-xs btn-circle"
            :class="{ 'loading': latencyLoading }"
            @click="checkLatency"
          >
            <svg v-if="!latencyLoading" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
