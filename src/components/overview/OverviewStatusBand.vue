<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { fetchConfig } from '@/api'
import { useConfigStore } from '@/stores/config'
import { useProxiesStore } from '@/stores/proxies'
import { useServiceStore } from '@/stores/service'
import { useToastStore } from '@/stores/toast'
import { startCore } from '@/utils/coreControl'
import { isElevationCancelled } from '@/bridge/service'
import { formatBytes, formatDate, formatLatency, formatUptime, latencyColor } from '@/utils/format'
import { coreNotice, exitLabel, modeLabel, resolveCurrentExit } from '@/utils/overviewStatus'
import type { OverviewLinkState } from '@/stores/overview'
import type { MemoryData, ServiceStatus } from '@/types'

const props = defineProps<{
  link: OverviewLinkState
  serviceState: ServiceStatus['state']
  failures: number
  memory: MemoryData | null
}>()

const router = useRouter()
const { clashApiUrl, activeConfigProfile } = useConfigStore()
const { proxyMap, getLatency, loadProxies } = useProxiesStore()
const { serviceStatus, refresh } = useServiceStore()
const { pushToast } = useToastStore()

const live = computed(() => props.link === 'live')
const notice = computed(() => coreNotice(props.link, props.serviceState, props.failures))
const noticeDot = computed(() => {
  if (notice.value?.alert || props.serviceState === 'stopped') return 'bg-error'
  return 'status-dot-neutral'
})

const uptime = computed(() => {
  const seconds = serviceStatus.value.uptimeSeconds
  return serviceStatus.value.state === 'running' && typeof seconds === 'number' ? formatUptime(seconds) : ''
})
const memoryText = computed(() => (live.value && props.memory ? formatBytes(props.memory.inuse) : '—'))

const mode = ref<string | null>(null)
let modeRequest = 0

async function loadMode() {
  const request = ++modeRequest
  try {
    const { data } = await fetchConfig()
    if (request === modeRequest) mode.value = typeof data?.mode === 'string' ? data.mode : null
  } catch {
    if (request === modeRequest) mode.value = null
  }
}

watch(
  () => [live.value, clashApiUrl.value] as const,
  ([isLive]) => {
    if (isLive) {
      void loadMode()
      void loadProxies(false, true)
    } else {
      modeRequest++
      mode.value = null
    }
  },
  { immediate: true },
)

const exit = computed(() => (live.value && mode.value !== null ? resolveCurrentExit(proxyMap.value, mode.value) : null))
const exitHeading = computed(() => exitLabel(mode.value))
const exitLatency = computed(() => (exit.value && !exit.value.direct ? getLatency(exit.value.node) : null))

const profile = computed(() => activeConfigProfile.value)
const profileDetail = computed(() => {
  const p = profile.value
  if (!p) return '前往配置页选择'
  if (p.lastFetchError) return '上次更新失败'
  if (p.type === 'remote') return p.lastUpdated ? `更新于 ${formatDate(p.lastUpdated)}` : '尚未更新'
  return '本地配置'
})

const starting = ref(false)

async function handleStart() {
  if (starting.value) return
  starting.value = true
  try {
    await startCore()
  } catch (e: any) {
    if (!isElevationCancelled(e)) pushToast({ message: '启动服务失败: ' + (e?.message || e), type: 'error' }, 6000)
  }
  await refresh()
  starting.value = false
}
</script>

<template>
  <p class="sr-only" role="status" data-band-connecting>{{ link === 'connecting' ? '正在连接 sing-box…' : '' }}</p>
  <section class="surface-card overview-band" :class="{ 'has-notice': notice }" aria-label="运行概况">
    <div
      v-if="notice"
      class="overview-band-notice flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3"
      data-band-notice
    >
      <span v-if="notice.busy" class="loading loading-spinner loading-sm text-base-content/55" aria-hidden="true"></span>
      <span v-else class="status-dot" :class="noticeDot" aria-hidden="true" data-band-notice-dot></span>
      <div class="min-w-0 flex-1">
        <p class="text-[13px] font-medium" :role="notice.alert ? 'alert' : 'status'" data-band-notice-title>{{ notice.title }}</p>
        <p class="text-xs text-base-content/65" data-band-notice-detail>
          {{ notice.detail }}
          <template v-if="notice.action === 'api'">
            地址 <span class="settings-mono break-all">{{ clashApiUrl || '未配置' }}</span>
          </template>
        </p>
      </div>
      <button v-if="notice.action === 'start'" type="button" class="btn btn-sm btn-primary px-5" :disabled="starting" @click="handleStart">
        <span v-if="starting" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        启动服务
      </button>
      <button v-else-if="notice.action === 'settings'" type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">前往设置</button>
      <button v-else-if="notice.action === 'api'" type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">检查 API 配置</button>
    </div>

    <button v-else type="button" class="overview-band-cell" data-band-cell="core" title="打开日志页" @click="router.push('/logs')">
      <span class="overview-band-label">sing-box</span>
      <span class="overview-band-value">
        <span class="status-dot bg-success" aria-hidden="true"></span>
        运行中 <span class="tabular-nums">{{ uptime }}</span>
      </span>
      <span class="sr-only">，打开日志页</span>
      <span v-if="live" class="overview-band-detail">内存 <span class="tabular-nums">{{ memoryText }}</span></span>
      <span v-else class="overview-band-detail">正在连接 sing-box…</span>
    </button>

    <button type="button" class="overview-band-cell" data-band-cell="mode" title="打开设置页" @click="router.push('/settings')">
      <span class="overview-band-label">模式</span>
      <span class="overview-band-value" :class="{ 'text-base-content/40': !live || !mode }">{{ live ? modeLabel(mode) : '—' }}</span>
      <span class="sr-only">，打开设置页</span>
    </button>

    <button type="button" class="overview-band-cell overview-band-exit" data-band-cell="exit" :title="`${exitHeading.hint}。打开代理页`" @click="router.push('/proxies')">
      <span class="overview-band-label">{{ exitHeading.label }}</span>
      <span class="overview-band-value" :class="{ 'text-base-content/40': !exit }">
        <span class="truncate" :title="exit && !exit.direct ? exit.node : undefined">{{ exit ? (exit.direct ? '直连' : exit.node) : '—' }}</span>
        <span
          v-if="exitLatency !== null"
          class="shrink-0 rounded-full px-2 py-0.5 text-xs font-normal tabular-nums"
          :class="latencyColor(exitLatency)"
        >{{ formatLatency(exitLatency) }}</span>
      </span>
      <span v-if="exit && exit.via.length" class="overview-band-detail">经 {{ exit.via.join(' › ') }}</span>
      <span v-else-if="exit?.direct" class="overview-band-detail">不经过代理</span>
      <span class="sr-only">，{{ exitHeading.hint }}，打开代理页</span>
    </button>

    <button type="button" class="overview-band-cell" data-band-cell="config" title="打开配置页" @click="router.push('/config')">
      <span class="overview-band-label">配置</span>
      <span class="overview-band-value" :class="{ 'text-base-content/40': !profile }">
        <span class="truncate" :title="profile?.name" data-band-config-name>{{ profile?.name || '未选择' }}</span>
      </span>
      <span
        class="overview-band-detail"
        :class="{ 'text-[color:var(--danger-ink)]': profile?.lastFetchError }"
      >{{ profileDetail }}</span>
      <span class="sr-only">，打开配置页</span>
    </button>
  </section>
</template>

<style scoped>
.overview-band {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  padding: 4px;
}

.overview-band.has-notice {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.overview-band-notice {
  grid-column: 1 / -1;
}

.overview-band-cell {
  display: flex;
  min-width: 0;
  flex-direction: column;
  align-items: flex-start;
  gap: 1px;
  border-radius: var(--radius-tile);
  padding: 8px 12px;
  text-align: left;
  transition: background-color 0.15s ease;
}

.overview-band-cell:hover {
  background-color: var(--fill);
}

.overview-band-cell:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: -2px;
}

.overview-band-label {
  font-size: 12px;
  color: oklch(var(--bc) / 0.55);
}

.overview-band-value {
  display: flex;
  overflow: hidden;
  max-width: 100%;
  min-width: 0;
  align-items: center;
  gap: 6px;
  font-size: 16px;
  font-weight: 600;
  line-height: 1.4;
  white-space: nowrap;
}

.overview-band-detail {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: oklch(var(--bc) / 0.65);
}

.status-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  flex-shrink: 0;
  border-radius: 9999px;
}

.status-dot.bg-success {
  box-shadow: 0 0 0 3px oklch(var(--su) / 0.22);
}

.status-dot.bg-error {
  box-shadow: 0 0 0 3px oklch(var(--er) / 0.2);
}

.status-dot-neutral {
  box-shadow: inset 0 0 0 1px oklch(var(--bc) / 0.5);
}

@container overview (min-width: 640px) {
  .overview-band:not(.has-notice) {
    grid-template-columns: minmax(0, 1.25fr) minmax(0, 0.7fr) minmax(0, 1.6fr) minmax(0, 1fr);
  }
}

@media (prefers-reduced-motion: reduce) {
  .overview-band-cell {
    transition: none;
  }
}
</style>
