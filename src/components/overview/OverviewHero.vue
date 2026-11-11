<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { fetchConfig } from '@/api'
import { useConfigStore } from '@/stores/config'
import { useProxiesStore } from '@/stores/proxies'
import { useServiceStore } from '@/stores/service'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { useToastStore } from '@/stores/toast'
import { startCore } from '@/utils/coreControl'
import { isElevationCancelled } from '@/bridge/service'
import { useCoreActions } from '@/android/composables/useCoreActions'
import { IS_ANDROID } from '@/platform'
import TrafficWave from './TrafficWave.vue'
import { dotColor, formatBytes, formatDate, formatLatency, formatSpeed, formatUptime } from '@/utils/format'
import { coreNotice, exitLabel, historySummary, modePhrase, resolveCurrentExit, splitRate } from '@/utils/overviewStatus'
import type { HistoryPoint, OverviewLinkState } from '@/stores/overview'
import type { ServiceStatus, TrafficData } from '@/types'

const props = defineProps<{
  link: OverviewLinkState
  serviceState: ServiceStatus['state']
  failures: number
  traffic: TrafficData | null
  totals: { up: number; down: number } | null
  downHistory: HistoryPoint[]
  upHistory: HistoryPoint[]
}>()

const router = useRouter()
const { config, clashApiUrl, activeClashApi, activeConfigProfile } = useConfigStore()
const { proxyMap, getLatency, loadProxies } = useProxiesStore()
const { serviceStatus, statusText, clashApiState, refresh } = useServiceStore()
const { singboxVersion } = useSingboxVersionStore()
const { pushToast } = useToastStore()
const core = IS_ANDROID ? useCoreActions() : null

const rootMode = computed(() => IS_ANDROID && config.value.rootMode)
const live = computed(() => props.link === 'live')
const notice = computed(() => coreNotice(props.link, props.serviceState, props.failures, {
  android: IS_ANDROID,
  rootMode: rootMode.value,
  authFailed: IS_ANDROID && clashApiState.value === 'auth_failed',
  missingWorkingDir: rootMode.value && !config.value.workingDir,
}))
const noticeDot = computed(() => {
  if (notice.value?.alert || props.serviceState === 'stopped') return 'bg-error'
  return 'hero-dot-neutral'
})

const uptime = computed(() => {
  const seconds = serviceStatus.value.uptimeSeconds
  return serviceStatus.value.state === 'running' && typeof seconds === 'number' ? formatUptime(seconds) : ''
})
const stateText = computed(() => (props.link === 'connecting' ? '正在连接 sing-box…' : statusText.value))
const metaDetail = computed(() => {
  if (!IS_ANDROID || rootMode.value) return singboxVersion.value
  return activeClashApi.value?.name ?? ''
})

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
const nodeText = computed(() => (exit.value ? (exit.value.direct ? '直连' : exit.value.node) : '—'))
const viaText = computed(() => {
  if (!exit.value) return ''
  if (exit.value.via.length) return exit.value.via.join(' › ')
  return exit.value.direct ? '不经过代理' : ''
})
const exitSpoken = computed(() => {
  const parts = [exitHeading.value.label, nodeText.value]
  if (exitLatency.value !== null) parts.push(`延迟 ${formatLatency(exitLatency.value)}`)
  if (viaText.value) parts.push(exit.value?.via.length ? `经 ${viaText.value}` : viaText.value)
  return `${parts.join(' ')}，${exitHeading.value.hint}，打开代理页`
})

const profile = computed(() => activeConfigProfile.value)
const profileDetail = computed(() => {
  const p = profile.value
  if (!p) return '前往配置页选择'
  if (p.lastFetchError) return '上次更新失败'
  if (p.type === 'remote') return p.lastUpdated ? `更新于 ${formatDate(p.lastUpdated)}` : '尚未更新'
  return '本地配置'
})

const speedText = (v: number) => formatSpeed(Math.round(v))
const rates = computed(() => {
  const traffic = props.traffic
  const totals = props.totals
  return [
    { key: 'down', label: '下载', value: traffic ? splitRate(formatSpeed(traffic.down)) : null, total: totals ? formatBytes(totals.down) : '—' },
    { key: 'up', label: '上传', value: traffic ? splitRate(formatSpeed(traffic.up)) : null, total: totals ? formatBytes(totals.up) : '—' },
  ]
})
const ratesLabel = computed(() => {
  const traffic = props.traffic
  if (!traffic) return '网速暂无数据。查看连接'
  return `下载 ${formatSpeed(traffic.down)}，${historySummary(props.downHistory, speedText)}；上传 ${formatSpeed(traffic.up)}，${historySummary(props.upHistory, speedText)}。查看连接`
})

const showControls = computed(() => rootMode.value && !!core && (props.serviceState === 'running' || core.busy.value === 'restart'))
const restarting = computed(() => core?.busy.value === 'restart')
const stopping = computed(() => core?.busy.value === 'stop')

const starting = ref(false)
const startBusy = computed(() => starting.value || core?.busy.value === 'start')

async function handleStart() {
  if (core) {
    void core.run('start')
    return
  }
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
  <p class="sr-only" role="status" data-hero-connecting>{{ link === 'connecting' && !notice ? '正在连接 sing-box…' : '' }}</p>
  <section class="surface-card overview-hero" aria-label="运行概况">
    <div class="hero-body">
      <div class="hero-top">
        <template v-if="notice">
          <span v-if="notice.busy" class="loading loading-spinner loading-xs text-base-content/55" aria-hidden="true"></span>
          <span v-else class="hero-dot" :class="noticeDot" aria-hidden="true" data-hero-notice-dot></span>
          <span class="hero-state" :role="notice.alert ? 'alert' : 'status'" data-hero-notice-title>{{ notice.title }}</span>
        </template>
        <div v-else class="hero-status">
          <div class="hero-status-line">
            <span class="hero-dot" :class="live ? 'bg-success' : 'hero-dot-neutral'" aria-hidden="true"></span>
            <span class="hero-state" data-hero-state>
              {{ stateText }}<span v-if="uptime && link !== 'connecting'" class="tabular-nums">{{ ` ${uptime}` }}</span>
            </span>
            <span v-if="live && mode" class="hero-meta-sep" aria-hidden="true">·</span>
            <button
              v-if="live && mode"
              type="button"
              class="hero-meta-link"
              title="打开设置页切换模式"
              data-hero-mode
              @click="router.push('/settings')"
            >{{ modePhrase(mode) }}<span class="sr-only">，打开设置页</span></button>
          </div>
          <span v-if="metaDetail" class="hero-meta" :title="metaDetail" data-hero-meta>{{ metaDetail }}</span>
        </div>
        <div class="hero-aside">
          <button
            v-if="!IS_ANDROID"
            type="button"
            class="hero-chip"
            :title="`${profile?.name || '未选择配置'} · ${profileDetail}。打开配置页`"
            data-hero-config
            @click="router.push('/config')"
          >
            <span class="truncate" data-hero-config-name>{{ profile?.name || '未选择配置' }}</span>
            <span v-if="profile?.lastFetchError" class="shrink-0 text-[color:var(--danger-ink)]">更新失败</span>
            <svg class="hero-chip-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
            <span class="sr-only">，{{ profileDetail }}，打开配置页</span>
          </button>
          <template v-if="showControls && core">
            <button
              type="button"
              class="hero-control glass-press"
              :class="{ 'is-restarting': restarting }"
              aria-label="重启核心"
              title="重启核心"
              :aria-busy="restarting"
              :disabled="!!core.busy.value"
              data-hero-control="restart"
              @click="core.run('restart')"
            >
              <span v-if="restarting" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
              <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M20 11a8 8 0 1 0-2.34 5.66" />
                <path d="M20 4v7h-7" />
              </svg>
            </button>
            <button
              type="button"
              class="hero-control hero-power glass-press"
              :class="{ 'is-paused': restarting }"
              aria-label="停止核心"
              title="停止核心"
              :aria-busy="stopping"
              :disabled="!!core.busy.value"
              data-hero-control="stop"
              @click="core.run('stop')"
            >
              <span v-if="stopping" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
              <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M12 3.5v8" />
                <path d="M17.66 6.84a8 8 0 1 1-11.32 0" />
              </svg>
            </button>
          </template>
        </div>
      </div>

      <div class="hero-main">
        <div v-if="notice" class="hero-notice" data-hero-notice>
          <p class="hero-notice-detail" data-hero-notice-detail>
            {{ notice.detail }}
            <template v-if="notice.action === 'api'">
              地址 <span class="settings-mono break-all">{{ clashApiUrl || '未配置' }}</span>
            </template>
          </p>
          <button v-if="notice.action === 'start'" type="button" class="btn btn-sm btn-primary px-5" :disabled="startBusy" @click="handleStart">
            <span v-if="startBusy" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            {{ IS_ANDROID ? '启动 sing-box' : '启动服务' }}
          </button>
          <button v-else-if="notice.action === 'settings'" type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">前往设置</button>
          <button v-else-if="notice.action === 'api'" type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">检查 API 配置</button>
        </div>
        <div v-else class="hero-exit" data-hero-exit>
          <button type="button" class="overview-cover" :title="`${exitHeading.hint}。打开代理页`" @click="router.push('/proxies')">
            <span class="sr-only">{{ exitSpoken }}</span>
          </button>
          <div class="overview-cover-body" aria-hidden="true">
            <div class="hero-label" data-hero-exit-label>{{ exitHeading.label }}</div>
            <div class="hero-node">
              <span class="hero-node-name" :class="{ 'overview-dim': !exit }" :title="exit && !exit.direct ? exit.node : undefined">{{ nodeText }}</span>
              <span v-if="exitLatency !== null" class="hero-latency">
                <i class="overview-dot" :class="dotColor(exitLatency)"></i>{{ formatLatency(exitLatency) }}
              </span>
            </div>
            <div v-if="viaText" class="hero-via">{{ viaText }}</div>
          </div>
        </div>

        <div class="hero-rates" data-hero-rates>
          <button type="button" class="overview-cover" :aria-label="ratesLabel" @click="router.push('/connections')"></button>
          <div class="hero-rates-grid overview-cover-body" aria-hidden="true">
            <div v-for="rate in rates" :key="rate.key" class="hero-rate" :data-rate="rate.key">
              <div class="hero-label">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path v-if="rate.key === 'down'" d="M12 4v16M5 13l7 7 7-7" />
                  <path v-else d="M12 20V4M5 11l7-7 7 7" />
                </svg>
                {{ rate.label }}
              </div>
              <div class="hero-rate-value" :class="{ 'overview-dim': !rate.value }">
                <template v-if="rate.value">{{ rate.value.value }}<small>{{ rate.value.unit }}</small></template>
                <template v-else>—</template>
              </div>
              <div class="hero-rate-total">总计 <span class="tabular-nums">{{ rate.total }}</span></div>
            </div>
          </div>
        </div>
      </div>
    </div>
    <TrafficWave :down="downHistory" :up="upHistory" :live="!!traffic" />
  </section>
</template>

<style scoped>
.overview-hero {
  overflow: hidden;
}

.hero-body {
  padding: 18px 20px 0;
}

.hero-top {
  display: flex;
  min-width: 0;
  min-height: 34px;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.hero-status {
  display: flex;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 2px;
}

.hero-status-line {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
}

.hero-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  flex: none;
  border-radius: 9999px;
}

.hero-dot.bg-success {
  box-shadow: 0 0 0 3px oklch(var(--su) / 0.22);
}

.hero-dot.bg-error {
  box-shadow: 0 0 0 3px oklch(var(--er) / 0.2);
}

.hero-dot-neutral {
  box-shadow: inset 0 0 0 1px oklch(var(--bc) / 0.5);
}

.hero-state {
  flex: none;
  margin-left: 2px;
  font-weight: 600;
  white-space: nowrap;
}

.hero-meta-sep,
.hero-meta {
  color: oklch(var(--bc) / 0.55);
  white-space: nowrap;
}

.hero-meta {
  min-width: 0;
  overflow: hidden;
  padding-left: 18px;
  font-size: 12px;
  text-overflow: ellipsis;
}

.hero-meta-link {
  flex: none;
  border-radius: 6px;
  color: oklch(var(--bc) / 0.6);
  white-space: nowrap;
  text-decoration: underline;
  text-decoration-color: transparent;
  text-underline-offset: 3px;
  transition: text-decoration-color 0.15s ease, color 0.15s ease;
}

.hero-meta-link:hover {
  color: oklch(var(--bc) / 0.85);
  text-decoration-color: oklch(var(--bc) / 0.3);
}

.hero-meta-link:focus-visible,
.hero-chip:focus-visible,
.hero-control:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}

.hero-aside {
  display: flex;
  min-width: 0;
  flex: 0 1 auto;
  align-items: center;
  gap: 8px;
  margin-left: auto;
}

.hero-chip {
  display: inline-flex;
  min-width: 0;
  max-width: 220px;
  align-items: center;
  gap: 6px;
  border-radius: 9999px;
  padding: 4px 6px 4px 10px;
  color: oklch(var(--bc) / 0.62);
  transition: background-color 0.15s ease;
}

.hero-chip:hover {
  background: var(--fill);
}

.hero-chip-chevron {
  width: 14px;
  height: 14px;
  flex: none;
  color: oklch(var(--bc) / 0.35);
}

.hero-control {
  display: inline-grid;
  width: 36px;
  height: 36px;
  flex: none;
  place-items: center;
  border-radius: 9999px;
  background: var(--fill);
  color: oklch(var(--bc) / 0.72);
  transition: background-color 180ms ease, color 180ms ease, box-shadow 180ms ease;
}

.hero-control svg {
  width: 18px;
  height: 18px;
}

.hero-control:disabled {
  opacity: 0.5;
}

.hero-power {
  background: color-mix(in oklab, oklch(var(--su)) 16%, transparent);
  color: oklch(var(--su));
  box-shadow: inset 0 0 0 1px color-mix(in oklab, oklch(var(--su)) 35%, transparent);
}

.hero-control.is-restarting {
  background: color-mix(in oklab, oklch(var(--wa)) 22%, transparent);
  color: oklch(var(--wa));
  box-shadow: inset 0 0 0 1px color-mix(in oklab, oklch(var(--wa)) 40%, transparent);
}

.hero-power.is-paused {
  background: var(--fill);
  color: oklch(var(--bc) / 0.45);
  box-shadow: none;
}

.hero-control.is-restarting:disabled,
.hero-power.is-paused:disabled {
  opacity: 1;
}

.hero-main {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 20px;
  margin-top: 20px;
}

.hero-exit,
.hero-rates {
  position: relative;
  min-width: 0;
}

.hero-label {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: oklch(var(--bc) / 0.55);
}

.hero-label svg {
  width: 12px;
  height: 12px;
}

.hero-node {
  display: flex;
  min-width: 0;
  align-items: baseline;
  gap: 12px;
  margin-top: 3px;
}

.hero-node-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 26px;
  font-weight: 600;
  letter-spacing: -0.022em;
  line-height: 1.2;
}

.hero-latency {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  color: oklch(var(--bc) / 0.72);
}

.hero-via {
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  color: oklch(var(--bc) / 0.55);
}

.hero-notice {
  min-width: 0;
}

.hero-notice-detail {
  max-width: 34em;
  margin: 0 0 14px;
  font-size: 13px;
  line-height: 1.6;
  color: oklch(var(--bc) / 0.65);
}

.hero-rates {
  border-top: 0.5px solid var(--hairline);
  padding-top: 16px;
}

.hero-rates .overview-cover {
  top: 10px;
}

.hero-rates-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0 28px;
}

.hero-rate {
  min-width: 0;
}

.hero-rate-value {
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 28px;
  font-weight: 300;
  letter-spacing: -0.02em;
  line-height: 1.2;
  font-variant-numeric: tabular-nums;
}

.hero-rate-value small {
  margin-left: 4px;
  font-size: 14px;
  font-weight: 400;
  letter-spacing: 0;
  color: oklch(var(--bc) / 0.5);
}

.hero-rate-total {
  margin-top: 2px;
  font-size: 12px;
  color: oklch(var(--bc) / 0.5);
}

@container overview (min-width: 640px) {
  .hero-body {
    padding: 20px 24px 0;
  }

  .hero-main {
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 32px;
    margin-top: 24px;
  }

  .hero-node-name {
    font-size: 28px;
  }

  .hero-rates {
    border-top: 0;
    padding-top: 0;
  }

  .hero-rates .overview-cover {
    top: -6px;
  }

  .hero-rates-grid {
    grid-template-columns: repeat(2, minmax(7.5rem, auto));
    gap: 0 40px;
  }

  .hero-rate-value {
    font-size: 32px;
  }
}

@media (prefers-reduced-motion: reduce) {
  .hero-meta-link,
  .hero-chip,
  .hero-control {
    transition: none;
  }
}
</style>
