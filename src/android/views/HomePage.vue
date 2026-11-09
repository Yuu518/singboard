<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useConfigStore } from '@/stores/config'
import { cpuHistory, useServiceStore } from '@/stores/service'
import { useOverviewStore } from '@/stores/overview'
import { useConnectionsStore } from '@/stores/connections'
import { useSingboxVersionStore } from '@/stores/singboxVersion'
import { useCoreActions } from '@/android/composables/useCoreActions'
import NetworkInfo from '@/android/components/home/NetworkInfo.vue'
import SparkLine from '@/android/components/home/SparkLine.vue'
import { formatBytes, formatSpeed, formatUptime } from '@/utils/format'

const { config, activeClashApi } = useConfigStore()
const { serviceStatus, statusText } = useServiceStore()
const { singboxVersion } = useSingboxVersionStore()
const { busy, run } = useCoreActions()
const {
  currentTraffic, memory, uploadSpeedHistory, downloadSpeedHistory, memoryHistory,
  start: startOverview,
} = useOverviewStore()
const { downloadTotal, uploadTotal, start: startConnections } = useConnectionsStore()

const isRunning = computed(() => serviceStatus.value.state === 'running')
const transitional = computed(() => ['starting', 'stopping'].includes(serviceStatus.value.state))
const restarting = computed(() => busy.value === 'restart')

const statusTone = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return 'is-running'
    case 'stopped':
    case 'not_installed': return 'is-stopped'
    case 'starting':
    case 'stopping': return 'is-pending'
    default: return 'is-unknown'
  }
})

const uptimeText = computed(() => {
  const uptime = serviceStatus.value.uptimeSeconds
  return isRunning.value && typeof uptime === 'number' ? formatUptime(uptime) : ''
})

const cpuText = computed(() => {
  const cpu = serviceStatus.value.cpuPercent
  if (!config.value.rootMode) return '—'
  return isRunning.value && typeof cpu === 'number' ? `${cpu.toFixed(1)}%` : '0%'
})

const processMemory = computed(() => {
  const bytes = serviceStatus.value.memoryBytes
  return config.value.rootMode && isRunning.value && typeof bytes === 'number' ? formatBytes(bytes) : ''
})

const speedLabel = (v: number) => (v === 0 ? '' : formatSpeed(v))
const memoryLabel = (v: number) => (v === 0 ? '' : formatBytes(v))
const cpuLabel = (v: number) => (v === 0 ? '' : `${v.toFixed(0)}%`)

onMounted(() => {
  startOverview()
  startConnections()
})
</script>

<template>
  <div class="space-y-3 pb-1">
    <h1 class="text-[26px] leading-tight tracking-tight font-bold">首页</h1>

    <section v-if="config.rootMode" class="surface-card home-status p-4" :class="statusTone" aria-label="运行状态">
      <div class="flex items-center gap-3">
        <span class="home-status-dot" aria-hidden="true"></span>
        <div class="min-w-0 flex-1">
          <div class="flex min-w-0 items-baseline gap-2">
            <span class="shrink-0 text-lg font-semibold leading-tight">{{ statusText }}</span>
            <span v-if="uptimeText" class="truncate text-xs text-base-content/60 tabular-nums">{{ uptimeText }}</span>
          </div>
          <div class="truncate text-xs text-base-content/60 tabular-nums">{{ singboxVersion || '核心版本未知' }}</div>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <button
            v-if="isRunning || restarting"
            type="button"
            class="home-action glass-press"
            :class="{ 'is-restarting': restarting }"
            aria-label="重启核心"
            title="重启核心"
            :aria-busy="restarting"
            :disabled="!!busy"
            @click="run('restart')"
          >
            <span v-if="restarting" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M20 11a8 8 0 1 0-2.34 5.66" />
              <path d="M20 4v7h-7" />
            </svg>
          </button>
          <button
            type="button"
            class="home-action home-power glass-press"
            :class="{ 'is-on': isRunning && !restarting, 'is-paused': restarting }"
            :aria-label="isRunning ? '停止核心' : '启动核心'"
            :title="isRunning ? '停止核心' : '启动核心'"
            :aria-pressed="isRunning"
            :disabled="!!busy || transitional"
            @click="run(isRunning ? 'stop' : 'start')"
          >
            <span v-if="(busy && busy !== 'restart') || transitional" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M12 3.5v8" />
              <path d="M17.66 6.84a8 8 0 1 1-11.32 0" />
            </svg>
          </button>
        </div>
      </div>

      <dl class="home-facts mt-3">
        <div>
          <dt>运行方式</dt>
          <dd>Root</dd>
        </div>
        <div>
          <dt>控制端点</dt>
          <dd class="truncate" :title="activeClashApi?.url">{{ activeClashApi?.name }}</dd>
        </div>
        <div class="col-span-2">
          <dt>工作目录</dt>
          <dd class="truncate font-mono" :title="config.workingDir">{{ config.workingDir || '未设置' }}</dd>
        </div>
      </dl>

      <router-link v-if="!config.workingDir" to="/settings" class="mt-3 inline-block text-xs text-primary">
        前往设置指定工作目录 →
      </router-link>
    </section>

    <div class="grid grid-cols-2 gap-3">
      <section class="surface-card flex flex-col gap-1.5 p-4" aria-label="上传速度">
        <div class="text-xs font-semibold tracking-wider text-base-content/60">上传</div>
        <div class="whitespace-nowrap text-2xl font-extralight tabular-nums sm:text-3xl">
          {{ formatSpeed(currentTraffic?.up ?? 0) }}
        </div>
        <div class="mt-1 h-14">
          <SparkLine :data="uploadSpeedHistory" color="#0A84FF" :min="60000" :label-formatter="speedLabel" />
        </div>
        <div class="text-xs text-base-content/50">总计 {{ formatBytes(uploadTotal) }}</div>
      </section>
      <section class="surface-card flex flex-col gap-1.5 p-4" aria-label="下载速度">
        <div class="text-xs font-semibold tracking-wider text-base-content/60">下载</div>
        <div class="whitespace-nowrap text-2xl font-extralight tabular-nums sm:text-3xl">
          {{ formatSpeed(currentTraffic?.down ?? 0) }}
        </div>
        <div class="mt-1 h-14">
          <SparkLine :data="downloadSpeedHistory" color="#BF5AF2" :min="60000" :label-formatter="speedLabel" />
        </div>
        <div class="text-xs text-base-content/50">总计 {{ formatBytes(downloadTotal) }}</div>
      </section>

      <section class="surface-card flex flex-col gap-1.5 p-4" aria-label="内存占用">
        <div class="text-xs font-semibold tracking-wider text-base-content/60">内存</div>
        <div class="whitespace-nowrap text-2xl font-extralight tabular-nums sm:text-3xl">
          {{ formatBytes(memory?.inuse ?? 0) }}
        </div>
        <div class="mt-1 h-14">
          <SparkLine :data="memoryHistory" color="#FF9F0A" :min="16 * 1024 * 1024" :label-formatter="memoryLabel" />
        </div>
        <div class="truncate text-xs text-base-content/50">
          {{ processMemory ? `进程常驻 ${processMemory}` : 'sing-box 运行时内存' }}
        </div>
      </section>
      <section class="surface-card flex flex-col gap-1.5 p-4" aria-label="CPU 占用">
        <div class="text-xs font-semibold tracking-wider text-base-content/60">CPU</div>
        <div class="whitespace-nowrap text-2xl font-extralight tabular-nums sm:text-3xl">{{ cpuText }}</div>
        <div class="mt-1 h-14">
          <SparkLine v-if="config.rootMode" :data="cpuHistory" color="#30D158" :min="10" :label-formatter="cpuLabel" />
        </div>
        <div class="truncate text-xs text-base-content/50">
          {{ config.rootMode ? '占全部核心的比例' : '需要 root 模式' }}
        </div>
      </section>
    </div>

    <NetworkInfo />
  </div>
</template>

<style scoped>
.home-status {
  --status-color: oklch(var(--bc) / 0.35);
}

.home-status.is-running {
  --status-color: oklch(var(--su));
}

.home-status.is-stopped {
  --status-color: oklch(var(--er));
}

.home-status.is-pending {
  --status-color: oklch(var(--wa));
}

.home-status-dot {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
  border-radius: 9999px;
  background: var(--status-color);
  box-shadow: 0 0 0 4px color-mix(in oklab, var(--status-color) 22%, transparent);
}

.home-action {
  display: inline-flex;
  width: 40px;
  height: 40px;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  background: var(--fill);
  color: oklch(var(--bc) / 0.75);
  transition: background-color 180ms ease, color 180ms ease, box-shadow 180ms ease;
}

.home-action svg {
  width: 20px;
  height: 20px;
}

.home-action:disabled {
  opacity: 0.5;
}

.home-power {
  width: 48px;
  height: 48px;
  background: oklch(var(--p));
  color: oklch(var(--pc));
  box-shadow: 0 6px 16px -6px oklch(var(--p) / 0.6);
}

.home-power svg {
  width: 22px;
  height: 22px;
}

.home-power.is-on {
  background: color-mix(in oklab, oklch(var(--su)) 16%, transparent);
  color: oklch(var(--su));
  box-shadow: inset 0 0 0 1px color-mix(in oklab, oklch(var(--su)) 35%, transparent);
}

.home-action.is-restarting {
  background: color-mix(in oklab, oklch(var(--wa)) 22%, transparent);
  color: oklch(var(--wa));
  box-shadow: inset 0 0 0 1px color-mix(in oklab, oklch(var(--wa)) 40%, transparent);
}

.home-power.is-paused {
  background: var(--fill);
  color: oklch(var(--bc) / 0.45);
  box-shadow: none;
}

.home-action.is-restarting:disabled,
.home-power.is-paused:disabled {
  opacity: 1;
}

.home-facts {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--fill);
  font-size: 12px;
}

.home-facts > div {
  min-width: 0;
}

.home-facts dt {
  color: oklch(var(--bc) / 0.55);
}

.home-facts dd {
  margin: 0;
  font-weight: 500;
}
</style>
