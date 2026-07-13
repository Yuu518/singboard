<script setup lang="ts">
import { onMounted, ref, computed, watch } from 'vue'
import { useConnectionsStore } from '@/stores/connections'
import { formatBytes, formatSpeed, formatDuration } from '@/utils/format'
import { getGeoIPForIP, type IPGeoInfo } from '@/api/geoip'
import type { Connection } from '@/types'

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

// Sort state: cycles none → asc → desc → none
type SortDir = 'none' | 'asc' | 'desc'
type SortKey = 'host' | 'rule' | 'chains' | 'dlSpeed' | 'ulSpeed' | 'dl' | 'ul' | 'duration'
const sortKey = ref<SortKey>('host')
const sortDir = ref<SortDir>('none')

function toggleSort(key: SortKey) {
  if (sortKey.value !== key) {
    sortKey.value = key
    sortDir.value = 'asc'
  } else if (sortDir.value === 'none') {
    sortDir.value = 'asc'
  } else if (sortDir.value === 'asc') {
    sortDir.value = 'desc'
  } else {
    sortDir.value = 'none'
  }
}

function sortIcon(key: SortKey): string {
  if (sortKey.value !== key || sortDir.value === 'none') return ''
  return sortDir.value === 'asc' ? ' ↑' : ' ↓'
}

function connSortValue(conn: Connection, key: SortKey): string | number {
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
  if (sortDir.value === 'none') return list
  const dir = sortDir.value === 'asc' ? 1 : -1
  return [...list].sort((a, b) => {
    const va = connSortValue(a, sortKey.value)
    const vb = connSortValue(b, sortKey.value)
    if (typeof va === 'number' && typeof vb === 'number') return (va - vb) * dir
    return String(va).localeCompare(String(vb)) * dir
  })
}

const sortedActiveConnections = computed(() => sortConnections(filteredConnections.value))
const sortedClosedConnections = computed(() => sortConnections(filteredClosedConnections.value))

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
  const sep = m.processPath.includes('/') ? '/' : '\\'
  return m.processPath.split(sep).pop() || m.processPath
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
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-3">
        <h1 class="text-xl font-bold shrink-0">连接</h1>
        <div class="tabs tabs-boxed tabs-sm">
          <a class="tab" :class="{ 'tab-active': activeTab === 'active' }" @click="activeTab = 'active'">
            活跃 ({{ filteredConnections.length }})
          </a>
          <a class="tab" :class="{ 'tab-active': activeTab === 'closed' }" @click="activeTab = 'closed'">
            已关闭 ({{ closedConnections.length }})
          </a>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <span class="text-xs text-base-content/50">
          ↑ {{ formatBytes(uploadTotal) }} / ↓ {{ formatBytes(downloadTotal) }}
        </span>
        <button
          class="btn btn-xs"
          :class="paused ? 'btn-warning' : 'btn-ghost'"
          @click="paused = !paused"
        >
          {{ paused ? '继续' : '暂停' }}
        </button>
        <button v-if="activeTab === 'active'" class="btn btn-xs btn-error btn-outline" @click="closeAllConnections">
          断开全部
        </button>
      </div>
    </div>

    <input
      v-model="filterText"
      type="text"
      placeholder="搜索: 主机名, IP, 进程, 规则..."
      class="input input-sm input-bordered w-full"
    />

    <div v-show="activeTab === 'active'" class="flex-1 overflow-auto">
      <table class="table table-xs table-pin-rows">
        <thead>
          <tr class="bg-base-200 border-b border-base-content/20">
            <th class="z-20 bg-base-200 cursor-pointer select-none hover:text-primary" @click="toggleSort('host')">主机{{ sortIcon('host') }}</th>
            <th class="z-20 bg-base-200 cursor-pointer select-none hover:text-primary" @click="toggleSort('rule')">规则{{ sortIcon('rule') }}</th>
            <th class="z-20 bg-base-200 cursor-pointer select-none hover:text-primary" @click="toggleSort('chains')">链路{{ sortIcon('chains') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('dlSpeed')">下载速度{{ sortIcon('dlSpeed') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('ulSpeed')">上传速度{{ sortIcon('ulSpeed') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('dl')">下载{{ sortIcon('dl') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('ul')">上传{{ sortIcon('ul') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('duration')">时长{{ sortIcon('duration') }}</th>
            <th class="z-20 bg-base-200 w-8"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="conn in sortedActiveConnections"
            :key="conn.id"
            class="hover:bg-base-200/50 cursor-pointer"
            @click="openDetail(conn)"
          >
            <td class="whitespace-nowrap" :title="getHost(conn)">
              <span class="text-xs leading-none px-1.5 py-0.5 rounded mr-1.5 inline-block" :class="conn.metadata.network === 'tcp' ? 'bg-info/15 text-info' : 'bg-accent/15 text-accent'">
                {{ conn.metadata.network }}
              </span>
              <span class="truncate inline">{{ getHost(conn) }}</span>
            </td>
            <td class="text-xs text-base-content/100 align-top min-w-64 max-w-xl" :title="conn.rule">
              <span class="block whitespace-normal break-words leading-relaxed [overflow-wrap:anywhere]">{{ conn.rule }}</span>
            </td>
            <td class="text-xs text-base-content/100 max-w-xl truncate" :title="formatChains(conn.chains)">
              {{ formatChains(conn.chains) }}
            </td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatSpeed(conn.downloadSpeed || 0) }}</td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatSpeed(conn.uploadSpeed || 0) }}</td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatBytes(conn.download) }}</td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatBytes(conn.upload) }}</td>
            <td class="text-right text-xs text-base-content/50 whitespace-nowrap">{{ formatDuration(conn.start) }}</td>
            <td>
              <button
                class="btn btn-ghost btn-xs text-error"
                @click.stop="closeConnection(conn.id)"
                title="断开"
              >
                ✕
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <div
        v-if="filteredConnections.length === 0"
        class="flex items-center justify-center py-10 text-base-content/40"
      >
        暂无活跃连接
      </div>
    </div>

    <div v-show="activeTab === 'closed'" class="flex-1 overflow-auto">
      <table class="table table-xs table-pin-rows">
        <thead>
          <tr class="bg-base-200 border-b border-base-content/20">
            <th class="z-20 bg-base-200 cursor-pointer select-none hover:text-primary" @click="toggleSort('host')">主机{{ sortIcon('host') }}</th>
            <th class="z-20 bg-base-200 cursor-pointer select-none hover:text-primary" @click="toggleSort('rule')">规则{{ sortIcon('rule') }}</th>
            <th class="z-20 bg-base-200 cursor-pointer select-none hover:text-primary" @click="toggleSort('chains')">链路{{ sortIcon('chains') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('dlSpeed')">下载速度{{ sortIcon('dlSpeed') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('ulSpeed')">上传速度{{ sortIcon('ulSpeed') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('dl')">下载{{ sortIcon('dl') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('ul')">上传{{ sortIcon('ul') }}</th>
            <th class="z-20 bg-base-200 text-right cursor-pointer select-none hover:text-primary" @click="toggleSort('duration')">时长{{ sortIcon('duration') }}</th>
            <th class="z-20 bg-base-200 w-8"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="conn in sortedClosedConnections"
            :key="conn.id"
            class="hover:bg-base-200/50 opacity-60 cursor-pointer"
            @click="openDetail(conn)"
          >
            <td class="whitespace-nowrap" :title="getHost(conn)">
              <span class="text-xs leading-none px-1.5 py-0.5 rounded mr-1.5 inline-block" :class="conn.metadata.network === 'tcp' ? 'bg-info/15 text-info' : 'bg-accent/15 text-accent'">
                {{ conn.metadata.network }}
              </span>
              <span class="truncate inline">{{ getHost(conn) }}</span>
            </td>
            <td class="text-xs text-base-content/100 align-top min-w-64 max-w-xl" :title="conn.rule">
              <span class="block whitespace-normal break-words leading-relaxed [overflow-wrap:anywhere]">{{ conn.rule }}</span>
            </td>
            <td class="text-xs text-base-content/100 max-w-xl truncate" :title="formatChains(conn.chains)">
              {{ formatChains(conn.chains) }}
            </td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatSpeed(conn.downloadSpeed || 0) }}</td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatSpeed(conn.uploadSpeed || 0) }}</td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatBytes(conn.download) }}</td>
            <td class="text-right text-xs whitespace-nowrap min-w-20">{{ formatBytes(conn.upload) }}</td>
            <td class="text-right text-xs text-base-content/50 whitespace-nowrap">{{ formatDuration(conn.start) }}</td>
            <td class="w-8"><span class="block h-6"></span></td>
          </tr>
        </tbody>
      </table>

      <div
        v-if="filteredClosedConnections.length === 0"
        class="flex items-center justify-center py-10 text-base-content/40"
      >
        暂无已关闭连接
      </div>
    </div>

    <!-- 连接详情弹窗 -->
    <div class="modal" :class="{ 'modal-visible': selectedConnection }" @click.self="closeDetail">
      <div v-if="selectedConnection" class="modal-box max-w-2xl">
        <div class="flex items-center justify-between mb-4">
          <h3 class="font-bold text-lg">连接详情</h3>
          <button class="btn btn-sm btn-circle btn-ghost" @click="closeDetail">✕</button>
        </div>

        <div class="space-y-4">
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

        <div class="modal-action">
          <button
            v-if="activeTab === 'active'"
            class="btn btn-sm btn-error btn-outline"
            @click="closeConnection(selectedConnection.id); closeDetail()"
          >
            断开连接
          </button>
          <button class="btn btn-sm" @click="closeDetail">关闭</button>
        </div>
      </div>
      <div class="modal-backdrop" @click="closeDetail"></div>
    </div>
  </div>
</template>

<style scoped>
.modal.modal-visible {
  pointer-events: auto;
  visibility: visible;
  opacity: 1;
}
</style>
