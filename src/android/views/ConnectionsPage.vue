<script setup lang="ts">
import { onMounted, ref, computed, watch } from 'vue'
import { useConnectionsStore } from '@/stores/connections'
import { formatBytes, formatSpeed, formatDuration, formatDate } from '@/utils/format'
import { getGeoIPForIP, type IPGeoInfo } from '@/api/geoip'
import type { Connection } from '@/types'
import { usePageActive } from '@/android/composables/usePageActive'

const {
  filteredConnections,
  filteredClosedConnections,
  closedConnections,
  downloadTotal,
  uploadTotal,
  paused,
  filterText,
  start,
  closeConnection,
  closeAllConnections,
} = useConnectionsStore()

const activeTab = ref<'active' | 'closed'>('active')
const selectedConnection = ref<Connection | null>(null)

type SortKey = 'none' | 'host' | 'rule' | 'chains' | 'dlSpeed' | 'ulSpeed' | 'dl' | 'ul' | 'duration'
const SORT_OPTIONS: Array<{ key: SortKey; label: string }> = [
  { key: 'none', label: '默认顺序' },
  { key: 'host', label: '主机' },
  { key: 'rule', label: '规则' },
  { key: 'chains', label: '链路' },
  { key: 'dlSpeed', label: '下载速度' },
  { key: 'ulSpeed', label: '上传速度' },
  { key: 'dl', label: '下载量' },
  { key: 'ul', label: '上传量' },
  { key: 'duration', label: '开始时间' },
]
const sortKey = ref<SortKey>('none')
const sortDir = ref<'asc' | 'desc'>('desc')

function toggleSortDir() {
  sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
}

function connSortValue(conn: Connection, key: Exclude<SortKey, 'none'>): string | number {
  switch (key) {
    case 'host': return getHost(conn).toLowerCase()
    case 'rule': return (conn.rule ?? '').toLowerCase()
    case 'chains': return formatChains(conn.chains).toLowerCase()
    case 'dlSpeed': return conn.downloadSpeed ?? 0
    case 'ulSpeed': return conn.uploadSpeed ?? 0
    case 'dl': return conn.download ?? 0
    case 'ul': return conn.upload ?? 0
    case 'duration': return new Date(conn.start).getTime()
  }
}

function sortConnections(list: Connection[]): Connection[] {
  const key = sortKey.value
  if (key === 'none') return list
  const dir = sortDir.value === 'asc' ? 1 : -1
  return [...list].sort((a, b) => {
    const va = connSortValue(a, key)
    const vb = connSortValue(b, key)
    if (typeof va === 'number' && typeof vb === 'number') return (va - vb) * dir
    return String(va).localeCompare(String(vb)) * dir
  })
}

const pageActive = usePageActive()
let lastDisplayed: Connection[] = []
const displayedConnections = computed(() => {
  if (!pageActive.value) return lastDisplayed
  lastDisplayed = sortConnections(activeTab.value === 'active' ? filteredConnections.value : filteredClosedConnections.value)
  return lastDisplayed
})

function hostLabel(conn: Connection): string {
  const host = getHost(conn)
  const port = conn.metadata.destinationPort
  if (!port || host === '-') return host
  return host.includes(':') ? `[${host}]:${port}` : `${host}:${port}`
}

function connSummary(conn: Connection): string {
  return [conn.metadata.type, conn.metadata.network, getProcess(conn)].filter(Boolean).join(' | ')
}

function exitOutbound(conn: Connection): string {
  return conn.chains?.[0] || '-'
}

function getHost(conn: Connection): string {
  const m = conn.metadata
  if (m.host && !isIPAddress(m.host)) {
    return m.host
  }
  return m.sniffHost || m.host || m.destinationIP || '-'
}

function getProcess(conn: Connection): string {
  const m = conn.metadata
  if (m.process) return m.process
  if (!m.processPath) return ''
  return m.processPath.split('/').pop() || m.processPath
}

function openDetail(conn: Connection) {
  selectedConnection.value = conn
}

function formatChains(chains?: string[]): string {
  if (!chains || chains.length === 0) return '-'
  return [...chains].reverse().join(' → ')
}

function closeDetail() {
  selectedConnection.value = null
}

function isIPAddress(str: string): boolean {
  return /^[\d.]+$/.test(str) || str.includes(':')
}

// Get the real destination IP (skip fakeip range 198.18.0.0/15)
function getDestIP(conn: Connection): string | null {
  const ip = conn.metadata.destinationIP
  if (!ip) return null
  const parts = ip.split('.')
  if (parts.length === 4) {
    const a = parseInt(parts[0])
    const b = parseInt(parts[1])
    if (a === 198 && (b === 18 || b === 19)) return null
  }
  return ip
}

const geoInfo = ref<IPGeoInfo | null>(null)
const geoLoading = ref(false)
const geoError = ref(false)

watch(selectedConnection, async (conn) => {
  geoInfo.value = null
  geoError.value = false
  geoLoading.value = false

  if (!conn) return

  const ip = getDestIP(conn)
  if (!ip) return

  geoLoading.value = true
  try {
    geoInfo.value = await getGeoIPForIP(ip)
  } catch {
    geoError.value = true
  } finally {
    geoLoading.value = false
  }
})

onMounted(() => {
  start()
})
</script>

<template>
  <div class="flex flex-col h-full gap-3">
    <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
      <div class="flex items-center gap-3">
        <h1 class="text-[26px] leading-tight tracking-tight font-bold shrink-0">连接</h1>
        <div class="tabs tabs-boxed tabs-sm flex-nowrap whitespace-nowrap">
          <a class="tab" :class="{ 'tab-active': activeTab === 'active' }" @click="activeTab = 'active'">
            活跃 ({{ filteredConnections.length }})
          </a>
          <a class="tab" :class="{ 'tab-active': activeTab === 'closed' }" @click="activeTab = 'closed'">
            已关闭 ({{ closedConnections.length }})
          </a>
        </div>
      </div>
      <span class="whitespace-nowrap text-xs tabular-nums text-base-content/50">
        ↓ {{ formatBytes(downloadTotal) }} · ↑ {{ formatBytes(uploadTotal) }}
      </span>
    </div>

    <div class="conn-toolbar">
      <select v-model="sortKey" class="select select-sm select-bordered conn-sort" aria-label="排序方式">
        <option v-for="option in SORT_OPTIONS" :key="option.key" :value="option.key">{{ option.label }}</option>
      </select>
      <button
        type="button"
        class="btn btn-sm btn-square btn-glass"
        :disabled="sortKey === 'none'"
        :title="sortDir === 'asc' ? '升序' : '降序'"
        :aria-label="sortDir === 'asc' ? '当前升序，切换为降序' : '当前降序，切换为升序'"
        @click="toggleSortDir"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" class="h-4 w-4" :class="{ 'rotate-180': sortDir === 'asc' }" aria-hidden="true">
          <path d="M4 6h10M4 12h7M4 18h4" />
          <path d="M18 5v14m0 0-3-3m3 3 3-3" />
        </svg>
      </button>
      <button
        type="button"
        class="btn btn-sm btn-square btn-glass"
        :class="{ 'conn-paused': paused }"
        :title="paused ? '继续刷新' : '暂停刷新'"
        :aria-label="paused ? '继续刷新' : '暂停刷新'"
        :aria-pressed="paused"
        @click="paused = !paused"
      >
        <svg v-if="paused" viewBox="0 0 24 24" fill="currentColor" class="h-4 w-4" aria-hidden="true">
          <path d="M7 5.5v13a1 1 0 0 0 1.5.86l11-6.5a1 1 0 0 0 0-1.72l-11-6.5A1 1 0 0 0 7 5.5Z" />
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="h-4 w-4" aria-hidden="true">
          <path d="M9 5v14M15 5v14" />
        </svg>
      </button>
      <button
        v-if="activeTab === 'active'"
        type="button"
        class="btn btn-sm btn-square btn-glass btn-glass-danger"
        title="断开全部"
        aria-label="断开全部连接"
        :disabled="filteredConnections.length === 0"
        @click="closeAllConnections"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="h-4 w-4" aria-hidden="true">
          <path d="M6 6l12 12M18 6 6 18" />
        </svg>
      </button>
    </div>

    <input
      v-model="filterText"
      type="text"
      placeholder="搜索: 主机名, IP, 进程, 规则..."
      class="input input-sm input-bordered w-full"
      aria-label="搜索连接"
    />

    <div class="conn-list under-nav">
      <div
        v-for="conn in displayedConnections"
        :key="conn.id"
        class="surface-card conn-card"
        :class="{ 'is-closed': activeTab === 'closed' }"
        role="button"
        tabindex="0"
        @click="openDetail(conn)"
        @keydown.enter.prevent="openDetail(conn)"
      >
        <div class="conn-line">
          <span class="conn-host" :title="hostLabel(conn)">{{ hostLabel(conn) }}</span>
          <span class="conn-time">{{ formatDate(conn.start) }}</span>
        </div>
        <div class="conn-line">
          <span class="conn-muted conn-ellipsis" :title="connSummary(conn)">{{ connSummary(conn) }}</span>
          <span class="conn-muted conn-nums">↓ {{ formatBytes(conn.download) }}&ensp;↑ {{ formatBytes(conn.upload) }}</span>
        </div>
        <div class="conn-line">
          <span class="conn-chain" :title="formatChains(conn.chains)">
            <span class="conn-ellipsis">{{ exitOutbound(conn) }}</span>
          </span>
          <span class="conn-speed conn-nums">
            <span :class="conn.downloadSpeed ? 'text-success' : 'conn-muted'">↓ {{ formatSpeed(conn.downloadSpeed || 0) }}</span>
            <span :class="conn.uploadSpeed ? 'text-info' : 'conn-muted'">↑ {{ formatSpeed(conn.uploadSpeed || 0) }}</span>
          </span>
          <button
            v-if="activeTab === 'active'"
            type="button"
            class="conn-close"
            title="断开"
            :aria-label="`断开 ${hostLabel(conn)}`"
            @click.stop="closeConnection(conn.id)"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
              <path d="M7 7l10 10M17 7 7 17" />
            </svg>
          </button>
        </div>
      </div>

      <div v-if="displayedConnections.length === 0" class="flex items-center justify-center py-10 text-sm text-base-content/40">
        {{ filterText ? '没有匹配的连接' : activeTab === 'active' ? '暂无活跃连接' : '暂无已关闭连接' }}
      </div>
    </div>

    <!-- 连接详情弹窗 -->
    <Teleport to="body">
    <Transition name="glass-pop">
    <div
      v-if="selectedConnection"
      class="glass-scrim safe-overlay fixed inset-0 z-50 flex items-center justify-center"
      @click.self="closeDetail"
    >
      <div class="glass-popover w-full max-w-2xl max-h-[80vh] flex flex-col rounded-[var(--radius-panel)]">
        <div class="flex items-center justify-between px-5 pt-4 pb-3 shrink-0">
          <h3 class="font-bold text-lg">连接详情</h3>
          <button class="btn btn-sm btn-circle btn-ghost" @click="closeDetail">✕</button>
        </div>

        <div class="flex-1 min-h-0 overflow-y-auto px-5 space-y-4">
          <!-- 基本信息 -->
          <div>
            <h4 class="text-sm font-semibold text-base-content/70 mb-2">基本信息</h4>
            <div class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/50">ID</span>
              <span class="break-all">{{ selectedConnection.id }}</span>
              <span class="text-base-content/50">类型</span>
              <span>{{ selectedConnection.metadata.network }} / {{ selectedConnection.metadata.type }}</span>
              <span class="text-base-content/50">主机</span>
              <span class="break-all">{{ getHost(selectedConnection) }}</span>
              <span class="text-base-content/50">规则</span>
              <span>{{ selectedConnection.rule }}</span>
              <span v-if="selectedConnection.rulePayload" class="text-base-content/50">规则载荷</span>
              <span v-if="selectedConnection.rulePayload">{{ selectedConnection.rulePayload }}</span>
              <span class="text-base-content/50">链路</span>
              <span>{{ formatChains(selectedConnection.chains) }}</span>
              <span class="text-base-content/50">开始时间</span>
              <span>{{ new Date(selectedConnection.start).toLocaleString() }}</span>
              <span class="text-base-content/50">持续时长</span>
              <span>{{ formatDuration(selectedConnection.start) }}</span>
            </div>
          </div>

          <!-- 网络元数据 -->
          <div>
            <h4 class="text-sm font-semibold text-base-content/70 mb-2">网络信息</h4>
            <div class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/50">源地址</span>
              <span>{{ selectedConnection.metadata.sourceIP }}:{{ selectedConnection.metadata.sourcePort }}</span>
              <span class="text-base-content/50">目标地址</span>
              <span>{{ selectedConnection.metadata.destinationIP || '-' }}:{{ selectedConnection.metadata.destinationPort }}</span>
              <span class="text-base-content/50">DNS 模式</span>
              <span>{{ selectedConnection.metadata.dnsMode || '-' }}</span>
              <span v-if="selectedConnection.metadata.sniffHost" class="text-base-content/50">嗅探主机</span>
              <span v-if="selectedConnection.metadata.sniffHost">{{ selectedConnection.metadata.sniffHost }}</span>
              <span class="text-base-content/50">进程</span>
              <span class="break-all">{{ getProcess(selectedConnection) || '-' }}</span>
              <span v-if="selectedConnection.metadata.processPath" class="text-base-content/50">进程路径</span>
              <span v-if="selectedConnection.metadata.processPath" class="break-all">{{ selectedConnection.metadata.processPath }}</span>
            </div>
          </div>

          <!-- IP 地理信息 -->
          <div v-if="getDestIP(selectedConnection)">
            <h4 class="text-sm font-semibold text-base-content/70 mb-2">IP 地理信息</h4>
            <div v-if="geoLoading" class="text-sm text-base-content/40">
              <span class="loading loading-spinner loading-xs mr-1"></span>加载中...
            </div>
            <div v-else-if="geoError" class="text-sm text-base-content/40">
              查询失败
            </div>
            <div v-else-if="geoInfo" class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/50">IP</span>
              <span>{{ geoInfo.ip }}</span>
              <span class="text-base-content/50">ASN</span>
              <span>{{ geoInfo.asn ? `AS${geoInfo.asn}` : '-' }}</span>
              <span class="text-base-content/50">运营商</span>
              <span>{{ geoInfo.asnOrganization || geoInfo.organization || '-' }}</span>
              <span class="text-base-content/50">位置</span>
              <span>{{ [geoInfo.city, geoInfo.region, geoInfo.country].filter(Boolean).join(', ') || '-' }}</span>
            </div>
          </div>

          <!-- 流量数据 -->
          <div>
            <h4 class="text-sm font-semibold text-base-content/70 mb-2">流量信息</h4>
            <div class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/50">下载总量</span>
              <span>{{ formatBytes(selectedConnection.download) }}</span>
              <span class="text-base-content/50">上传总量</span>
              <span>{{ formatBytes(selectedConnection.upload) }}</span>
              <span class="text-base-content/50">下载速度</span>
              <span>{{ formatSpeed(selectedConnection.downloadSpeed || 0) }}</span>
              <span class="text-base-content/50">上传速度</span>
              <span>{{ formatSpeed(selectedConnection.uploadSpeed || 0) }}</span>
            </div>
          </div>
        </div>

        <div class="flex justify-end gap-2 px-5 pt-3 pb-4 shrink-0">
          <button
            v-if="activeTab === 'active'"
            class="btn btn-sm btn-glass btn-glass-danger"
            @click="closeConnection(selectedConnection.id); closeDetail()"
          >
            断开连接
          </button>
          <button class="btn btn-sm" @click="closeDetail">关闭</button>
        </div>
      </div>
    </div>
    </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.conn-toolbar {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.conn-sort {
  min-width: 0;
  flex: 1 1 auto;
  max-width: 16rem;
  font-size: 0.8125rem;
}

.conn-paused {
  color: oklch(var(--wa));
}

.conn-list {
  display: flex;
  min-height: 0;
  flex: 1 1 0;
  flex-direction: column;
  gap: 0.5rem;
  overflow-x: hidden;
  overflow-y: auto;
  margin-inline: -0.5rem;
  padding: 0.25rem 0.5rem 0;
}

.under-nav {
  margin-bottom: calc(-1 * var(--nav-clearance, 0px));
  padding-bottom: calc(var(--nav-clearance, 0px) + 0.5rem);
}

.conn-card {
  display: flex;
  flex-shrink: 0;
  flex-direction: column;
  gap: 5px;
  padding: 11px 14px 10px;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.conn-card:focus-visible {
  outline: 2px solid oklch(var(--p) / 0.6);
  outline-offset: 2px;
}

.conn-card.is-closed {
  opacity: 0.65;
}

.conn-line {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  font-size: 12px;
  line-height: 18px;
}

.conn-host {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  font-size: 14px;
  font-weight: 600;
  line-height: 20px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conn-time {
  flex-shrink: 0;
  color: oklch(var(--bc) / 0.5);
}

.conn-muted {
  color: oklch(var(--bc) / 0.55);
}

.conn-ellipsis {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conn-line > .conn-ellipsis {
  flex: 1 1 auto;
}

.conn-nums {
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.conn-chain {
  display: flex;
  min-width: 0;
  flex: 1 1 auto;
  align-items: center;
  gap: 3px;
  font-size: 13px;
  color: oklch(var(--bc) / 0.8);
}

.conn-speed {
  display: flex;
  gap: 0.5rem;
}

.conn-close {
  display: inline-flex;
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  margin-block: -5px;
  border-radius: 9999px;
  background: var(--fill);
  color: oklch(var(--bc) / 0.6);
  transition: background-color 150ms ease, color 150ms ease;
}

.conn-close:hover,
.conn-close:focus-visible {
  background: oklch(var(--er) / 0.15);
  color: oklch(var(--er));
}

.conn-close svg {
  width: 15px;
  height: 15px;
}
</style>
