<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useNetworkStore, type IPKey, type LatencyKey } from '@/stores/network'
import { ipAnnouncement, ipView, latencyAnnouncement, latencyBadge } from '@/utils/networkDisplay'

const props = withDefaults(defineProps<{ running?: boolean }>(), { running: true })

const { ip, latency, ipBusy, latencyBusy, checkIP, checkLatency } = useNetworkStore()
const showIP = ref(false)

const ipRows: { key: IPKey; label: string; source: string; hint: string }[] = [
  { key: 'domestic', label: '直连出口', source: 'ipip.net', hint: '国内站点 myip.ipip.net 看到的地址' },
  { key: 'abroad', label: '代理出口', source: 'ip.sb', hint: '国外站点 ip.sb 看到的地址' },
]

const latencyRows: { key: LatencyKey; label: string }[] = [
  { key: 'wechat', label: '微信' },
  { key: 'bilibili', label: 'Bilibili' },
  { key: 'github', label: 'GitHub' },
  { key: 'cloudflare', label: 'Cloudflare' },
  { key: 'youtube', label: 'YouTube' },
]

const ipViews = computed(() => ipRows.map((row) => ({ ...row, view: ipView(ip.value[row.key], showIP.value) })))
const latencyBadges = computed(() => latencyRows.map((row) => ({ ...row, badge: latencyBadge(latency.value[row.key]) })))

const announcement = ref('')

watch(ipBusy, (busy, wasBusy) => {
  if (busy || !wasBusy) return
  const rows = ipRows.map((row) => ({ label: row.label, result: ip.value[row.key] }))
  if (rows.every((row) => row.result.status === 'done' || row.result.status === 'error')) {
    announcement.value = ipAnnouncement(rows, showIP.value)
  }
})

watch(latencyBusy, (busy, wasBusy) => {
  if (busy || !wasBusy) return
  const rows = latencyRows.map((row) => ({ label: row.label, result: latency.value[row.key] }))
  if (rows.every((row) => row.result.status === 'done' || row.result.status === 'error')) {
    announcement.value = latencyAnnouncement(rows)
  }
})
</script>

<template>
  <div class="surface-card p-4 space-y-3">
    <h2 class="text-sm font-semibold">网络信息</h2>
    <p class="sr-only" aria-live="polite" data-network-announcement>{{ announcement }}</p>
    <div class="network-info-grid grid grid-cols-1 gap-3">
      <section class="surface-fill rounded-[var(--radius-tile)] p-3 space-y-2 min-w-0">
        <div class="flex items-start gap-2">
          <div class="min-w-0 flex-1">
            <h3 class="text-xs font-medium text-base-content/70">出口 IP</h3>
            <p v-if="!props.running" class="text-xs text-base-content/55" data-network-ip-hint>sing-box 未运行，启动后自动检测。</p>
          </div>
          <div v-if="props.running" class="flex shrink-0 gap-1">
            <button
              type="button"
              class="btn btn-ghost btn-xs btn-circle"
              aria-label="显示完整出口 IP"
              :title="showIP ? '隐藏出口 IP' : '显示完整出口 IP'"
              :aria-pressed="showIP"
              data-action="toggle-ip"
              @click="showIP = !showIP"
            >
              <svg aria-hidden="true" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                <path v-if="showIP" stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
                <path v-if="showIP" stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                <path v-if="!showIP" stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12c1.292 4.338 5.31 7.5 10.066 7.5.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88" />
              </svg>
            </button>
            <button
              type="button"
              class="btn btn-ghost btn-xs px-2 font-medium"
              aria-label="重新检测出口 IP"
              :aria-disabled="ipBusy"
              :aria-busy="ipBusy"
              data-action="check-ip"
              @click="checkIP()"
            >
              <span v-if="ipBusy" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
              重新检测
            </button>
          </div>
        </div>
        <dl class="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
          <template v-for="row in ipViews" :key="row.key">
            <dt class="text-base-content/70" :title="row.hint">
              {{ row.label }}
              <span class="block text-xs text-base-content/55">{{ row.source }}</span>
              <span class="sr-only">，{{ row.hint }}</span>
            </dt>
            <dd class="min-w-0" :data-ip-row="row.key">
              <template v-if="row.view.state === 'found'">
                <div v-if="row.view.location" class="truncate">{{ row.view.location }}</div>
                <div class="settings-mono break-all text-base-content/70" :aria-hidden="!showIP">{{ row.view.ip }}</div>
                <span v-if="!showIP" class="sr-only">IP 已隐藏</span>
              </template>
              <span v-else-if="row.view.state === 'failed'" class="text-xs text-[color:var(--danger-ink)] break-words">{{ row.view.message }}</span>
              <span v-else class="text-base-content/55">{{ row.view.message }}</span>
            </dd>
          </template>
        </dl>
      </section>

      <section class="surface-fill rounded-[var(--radius-tile)] p-3 space-y-2 min-w-0">
        <div class="flex items-start gap-2">
          <div class="min-w-0 flex-1">
            <h3 class="text-xs font-medium text-base-content/70">访问延迟</h3>
            <p v-if="!props.running" class="text-xs text-base-content/55" data-network-latency-hint>sing-box 未运行，启动后自动检测。</p>
          </div>
          <button
            v-if="props.running"
            type="button"
            class="btn btn-ghost btn-xs shrink-0 px-2 font-medium"
            aria-label="重新检测访问延迟"
            :aria-disabled="latencyBusy"
            :aria-busy="latencyBusy"
            data-action="check-latency"
            @click="checkLatency()"
          >
            <span v-if="latencyBusy" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            重新检测
          </button>
        </div>
        <dl class="grid grid-cols-[4.5rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5 text-sm">
          <template v-for="row in latencyBadges" :key="row.key">
            <dt class="text-base-content/70">{{ row.label }}</dt>
            <dd class="flex min-w-0 items-center gap-2" :data-latency-row="row.key">
              <span class="relative shrink-0 rounded-full px-2 py-0.5 text-xs tabular-nums" :class="row.badge.tone">
                <span :class="{ invisible: row.badge.busy }">{{ row.badge.text }}</span>
                <span v-if="row.badge.busy" class="loading loading-spinner absolute inset-0 m-auto h-2.5 w-2.5" aria-hidden="true"></span>
              </span>
              <span v-if="row.badge.reason" class="truncate text-xs text-base-content/65" :title="row.badge.reason">{{ row.badge.reason }}</span>
            </dd>
          </template>
        </dl>
      </section>
    </div>
  </div>
</template>

<style scoped>
@container overview (min-width: 640px) {
  .network-info-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
