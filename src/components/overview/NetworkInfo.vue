<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useNetworkStore, type IPKey, type LatencyKey } from '@/stores/network'
import { dotColor } from '@/utils/format'
import { ipAnnouncement, ipView, latencyAnnouncement, latencyBadge } from '@/utils/networkDisplay'

const props = withDefaults(defineProps<{ running?: boolean }>(), { running: true })

const { ip, latency, ipBusy, latencyBusy, checkAll } = useNetworkStore()
const showIP = ref(false)
const busy = computed(() => ipBusy.value || latencyBusy.value)

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
const latencyViews = computed(() => latencyRows.map((row) => {
  const result = latency.value[row.key]
  const badge = latencyBadge(result)
  const ms = result.status === 'done' ? result.ms : result.status === 'error' ? 0 : -1
  return { ...row, badge, dot: dotColor(ms) }
}))

const announcement = ref('')

watch(ipBusy, (isBusy, wasBusy) => {
  if (isBusy || !wasBusy) return
  const rows = ipRows.map((row) => ({ label: row.label, result: ip.value[row.key] }))
  if (rows.every((row) => row.result.status === 'done' || row.result.status === 'error')) {
    announcement.value = ipAnnouncement(rows, showIP.value)
  }
})

watch(latencyBusy, (isBusy, wasBusy) => {
  if (isBusy || !wasBusy) return
  const rows = latencyRows.map((row) => ({ label: row.label, result: latency.value[row.key] }))
  if (rows.every((row) => row.result.status === 'done' || row.result.status === 'error')) {
    announcement.value = latencyAnnouncement(rows)
  }
})

function recheck() {
  if (busy.value) return
  void checkAll()
}
</script>

<template>
  <section class="overview-section" aria-labelledby="network-title">
    <div class="overview-section-head">
      <h2 id="network-title">网络</h2>
      <span v-if="!props.running" class="overview-section-sub" data-network-hint>sing-box 未运行，启动后自动检测</span>
      <div v-if="props.running" class="overview-section-actions">
        <button
          type="button"
          class="overview-text-action"
          aria-label="显示完整出口 IP"
          :title="showIP ? '隐藏出口 IP' : '显示完整出口 IP'"
          :aria-pressed="showIP"
          data-action="toggle-ip"
          @click="showIP = !showIP"
        >显示 IP</button>
        <button
          type="button"
          class="overview-text-action"
          aria-label="重新检测出口 IP 和访问延迟"
          :aria-disabled="busy"
          :aria-busy="busy"
          data-action="check-all"
          @click="recheck"
        >
          <span v-if="busy" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          重新检测
        </button>
      </div>
    </div>
    <p class="sr-only" aria-live="polite" data-network-announcement>{{ announcement }}</p>
    <div class="surface-card network-card">
      <dl class="network-ips">
        <div v-for="row in ipViews" :key="row.key" class="network-ip">
          <dt class="network-key" :title="row.hint">
            {{ row.label }}
            <span class="network-source">{{ row.source }}</span>
            <span class="sr-only">，{{ row.hint }}</span>
          </dt>
          <dd class="network-ip-value" :data-ip-row="row.key">
            <template v-if="row.view.state === 'found'">
              <span class="network-location">{{ row.view.location || '位置未知' }}</span>
              <span class="network-address settings-mono break-all" :aria-hidden="!showIP">{{ row.view.ip }}</span>
              <span v-if="!showIP" class="sr-only">IP 已隐藏</span>
            </template>
            <span v-else-if="row.view.state === 'failed'" class="network-failed">{{ row.view.message }}</span>
            <span v-else class="network-muted">{{ row.view.message }}</span>
          </dd>
        </div>
      </dl>
      <dl class="network-latency">
        <div v-for="row in latencyViews" :key="row.key" class="network-lat">
          <dt>{{ row.label }}</dt>
          <dd :data-latency-row="row.key">
            <span v-if="row.badge.reason" class="network-reason" :title="row.badge.reason">{{ row.badge.reason }}</span>
            <span v-if="row.badge.busy" class="loading loading-spinner network-spinner" aria-hidden="true"></span>
            <i v-else class="overview-dot" :class="row.dot" aria-hidden="true"></i>
            <span class="network-ms">{{ row.badge.text }}</span>
          </dd>
        </div>
      </dl>
    </div>
  </section>
</template>

<style scoped>
.network-card {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
}

.network-ips {
  margin: 0;
  padding: 4px 0;
}

.network-ip {
  position: relative;
  display: grid;
  grid-template-columns: 4.75rem minmax(0, 1fr);
  align-items: baseline;
  gap: 12px;
  padding: 10px 16px;
}

.network-ip + .network-ip::before,
.network-latency::before {
  content: '';
  position: absolute;
  top: 0;
  right: 0;
  left: 16px;
  border-top: 0.5px solid var(--hairline);
}

.network-key {
  font-size: 13px;
  color: oklch(var(--bc) / 0.65);
}

.network-source {
  display: block;
  font-size: 11px;
  color: oklch(var(--bc) / 0.45);
}

.network-ip-value {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: baseline;
  justify-content: space-between;
  gap: 2px 12px;
  margin: 0;
}

.network-location {
  font-size: 14px;
  font-weight: 500;
}

.network-address {
  min-width: 0;
  color: oklch(var(--bc) / 0.55);
  text-align: right;
}

.network-failed {
  font-size: 12px;
  color: var(--danger-ink);
  overflow-wrap: anywhere;
}

.network-muted {
  font-size: 13px;
  color: oklch(var(--bc) / 0.55);
}

.network-latency {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  margin: 0;
  padding: 4px 0;
}

.network-lat {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 16px;
  font-size: 13px;
}

.network-lat dt {
  flex: none;
  color: oklch(var(--bc) / 0.75);
}

.network-lat dd {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 7px;
  margin: 0;
}

.network-reason {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: oklch(var(--bc) / 0.55);
}

.network-spinner {
  width: 10px;
  height: 10px;
  color: oklch(var(--bc) / 0.5);
}

.network-ms {
  flex: none;
  font-variant-numeric: tabular-nums;
  color: oklch(var(--bc) / 0.85);
}

@container overview (min-width: 640px) {
  .network-card {
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
  }

  .network-ip {
    padding: 11px 18px;
  }

  .network-latency {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    align-content: center;
    padding: 8px 4px;
  }

  .network-latency::before {
    top: 16px;
    bottom: 16px;
    left: 0;
    right: auto;
    border-top: 0;
    border-left: 0.5px solid var(--hairline);
  }

  .network-lat {
    padding: 7px 18px;
  }
}
</style>
