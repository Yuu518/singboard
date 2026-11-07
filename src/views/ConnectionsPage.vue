<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { matchesFilter, useConnectionsStore } from '@/stores/connections'
import { useToastStore } from '@/stores/toast'
import { formatBytes, formatSpeed, formatDuration } from '@/utils/format'
import { getRequestErrorReason } from '@/utils/requestError'
import { getGeoIPForIP, type IPGeoInfo } from '@/api/geoip'
import type { Connection } from '@/types'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import ServiceNotice from '@/components/common/ServiceNotice.vue'
import { useSettledServiceState } from '@/composables/useSettledServiceState'

const {
  connections,
  closedConnections,
  downloadTotal,
  uploadTotal,
  filterText,
  socketState,
  start,
  closeConnection,
  closeConnections,
  closeAllConnections,
} = useConnectionsStore()
const { pushToast } = useToastStore()
const { serviceState, isRunning } = useSettledServiceState()

const confirmDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const activeTab = ref<'active' | 'closed'>('active')

function onTabKeydown(e: KeyboardEvent) {
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
  e.preventDefault()
  activeTab.value = activeTab.value === 'active' ? 'closed' : 'active'
  void nextTick(() => document.getElementById(`connections-tab-${activeTab.value}`)?.focus())
}

const announcement = ref('')

function announce(message: string) {
  announcement.value = ''
  void nextTick(() => {
    announcement.value = message
  })
}

const paused = ref(false)
const pausedSnapshot = ref<{ active: Connection[]; closed: Connection[]; down: number; up: number } | null>(null)

watch(paused, (on) => {
  pausedSnapshot.value = on
    ? { active: connections.value, closed: closedConnections.value, down: downloadTotal.value, up: uploadTotal.value }
    : null
  announce(on ? '已暂停刷新' : '已恢复刷新')
})

const shownDownTotal = computed(() => pausedSnapshot.value?.down ?? downloadTotal.value)
const shownUpTotal = computed(() => pausedSnapshot.value?.up ?? uploadTotal.value)

const closingIds = ref<Set<string>>(new Set())

watch(connections, (list) => {
  if (closingIds.value.size === 0 || paused.value) return
  const present = new Set(list.map((c) => c.id))
  for (const id of closingIds.value) {
    if (!present.has(id)) closingIds.value.delete(id)
  }
})

const query = computed(() => filterText.value.trim().toLowerCase())

type SortKey = 'recent' | 'duration' | 'download' | 'upload' | 'downloadTotal' | 'uploadTotal' | 'host'

const SORT_OPTIONS: { value: SortKey; label: string }[] = [
  { value: 'recent', label: '最新' },
  { value: 'duration', label: '时长最长' },
  { value: 'download', label: '下载速度' },
  { value: 'upload', label: '上传速度' },
  { value: 'downloadTotal', label: '下载总量' },
  { value: 'uploadTotal', label: '上传总量' },
  { value: 'host', label: '域名' },
]

const SORT_STORAGE_KEY = 'singboard-connections-sort'
const storedSort = localStorage.getItem(SORT_STORAGE_KEY)
const sortKey = ref<SortKey>(SORT_OPTIONS.some((o) => o.value === storedSort) ? (storedSort as SortKey) : 'recent')

watch(sortKey, (key) => {
  localStorage.setItem(SORT_STORAGE_KEY, key)
})

function startTime(conn: Connection): number {
  return new Date(conn.start).getTime() || 0
}

function compareConnections(a: Connection, b: Connection, closed: boolean): number {
  switch (sortKey.value) {
    case 'recent': {
      const diff = closed ? (b.closedAt ?? 0) - (a.closedAt ?? 0) : startTime(b) - startTime(a)
      if (diff) return diff
      break
    }
    case 'duration': {
      const diff = startTime(a) - startTime(b)
      if (diff) return diff
      break
    }
    case 'download': {
      const diff = (b.downloadSpeed ?? 0) - (a.downloadSpeed ?? 0)
      if (diff) return diff
      break
    }
    case 'upload': {
      const diff = (b.uploadSpeed ?? 0) - (a.uploadSpeed ?? 0)
      if (diff) return diff
      break
    }
    case 'downloadTotal': {
      const diff = (b.download ?? 0) - (a.download ?? 0)
      if (diff) return diff
      break
    }
    case 'uploadTotal': {
      const diff = (b.upload ?? 0) - (a.upload ?? 0)
      if (diff) return diff
      break
    }
    case 'host': {
      const diff = getHost(a).localeCompare(getHost(b))
      if (diff) return diff
      break
    }
  }
  return startTime(b) - startTime(a) || a.id.localeCompare(b.id)
}

const listHovered = ref(false)
const listFocused = ref(false)
const frozenIds = ref<string[] | null>(null)

function prepareList(source: Connection[], closed: boolean): Connection[] {
  const q = query.value
  const filtered = source.filter((c) => !closingIds.value.has(c.id) && (!q || matchesFilter(c, q)))
  const sorted = filtered.sort((a, b) => compareConnections(a, b, closed))
  if (!frozenIds.value) return sorted
  const order = new Map(frozenIds.value.map((id, index) => [id, index]))
  return sorted
    .map((conn, index) => ({ conn, rank: order.get(conn.id) ?? frozenIds.value!.length + index }))
    .sort((a, b) => a.rank - b.rank)
    .map((item) => item.conn)
}

const activeList = computed(() => prepareList(pausedSnapshot.value?.active ?? connections.value, false))
const closedList = computed(() => prepareList(pausedSnapshot.value?.closed ?? closedConnections.value, true))
const visibleList = computed(() => (activeTab.value === 'active' ? activeList.value : closedList.value))

watch([listHovered, listFocused], ([hovered, focused]) => {
  if (!hovered && !focused) {
    frozenIds.value = null
  } else if (!frozenIds.value) {
    frozenIds.value = visibleList.value.map((c) => c.id)
  }
})

watch([activeTab, sortKey, query], () => {
  if (!frozenIds.value) return
  frozenIds.value = null
  frozenIds.value = visibleList.value.map((c) => c.id)
})

function onListFocusOut(e: FocusEvent) {
  if ((e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) return
  listFocused.value = false
}

const focusedRowId = ref<string | null>(null)

const rowTabTarget = computed(() => {
  const list = visibleList.value
  if (focusedRowId.value && list.some((c) => c.id === focusedRowId.value)) return focusedRowId.value
  return list[0]?.id ?? null
})

function onListKeydown(e: KeyboardEvent) {
  const list = e.currentTarget as HTMLElement
  const rows = Array.from(list.querySelectorAll<HTMLElement>('[data-conn-row]'))
  const index = rows.indexOf(e.target as HTMLElement)
  if (index < 0) return
  if (e.key === 'Delete' && activeTab.value === 'active') {
    e.preventDefault()
    const conn = visibleList.value[index]
    if (conn) {
      rows[Math.min(index + 1, rows.length - 1)]?.focus()
      void disconnect(conn)
    }
    return
  }
  let next: number
  switch (e.key) {
    case 'ArrowDown': next = index + 1; break
    case 'ArrowUp': next = index - 1; break
    case 'Home': next = 0; break
    case 'End': next = rows.length - 1; break
    default: return
  }
  e.preventDefault()
  rows[Math.min(Math.max(next, 0), rows.length - 1)].focus()
}

function getHost(conn: Connection): string {
  const m = conn.metadata
  if (m.host && !isIPAddress(m.host)) {
    return m.host
  }
  return m.sniffHost || m.host || m.destinationIP || '-'
}

function withPort(host: string, port?: string | number): string {
  if (!port) return host
  const bracketed = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host
  return `${bracketed}:${port}`
}

function getProcess(conn: Connection): string {
  const m = conn.metadata
  if (m.process) return m.process
  if (!m.processPath) return ''
  const sep = m.processPath.includes('/') ? '/' : '\\'
  return m.processPath.split(sep).pop() || m.processPath
}

function formatChains(chains?: string[]): string {
  if (!chains || chains.length === 0) return '-'
  return [...chains].reverse().join(' → ')
}

function routeLine(conn: Connection): string {
  return `${conn.rule || '默认出站'} → ${formatChains(conn.chains)}`
}

function connectionDuration(conn: Connection): string {
  return formatDuration(conn.start, conn.closedAt)
}

function rowLabel(conn: Connection): string {
  const process = getProcess(conn)
  const parts = [
    `${conn.metadata.network.toUpperCase()} ${withPort(getHost(conn), conn.metadata.destinationPort)}`,
    process,
    routeLine(conn),
  ]
  if (!conn.closedAt) parts.push(`下载 ${formatSpeed(conn.downloadSpeed)}，上传 ${formatSpeed(conn.uploadSpeed)}`)
  parts.push(`持续 ${connectionDuration(conn)}`)
  return parts.filter(Boolean).join('，')
}

function isIPAddress(str: string): boolean {
  return /^[\d.]+$/.test(str) || str.includes(':')
}

function isPublicIP(ip: string): boolean {
  if (ip.includes(':')) {
    const lower = ip.toLowerCase()
    return !(lower === '::1' || lower.startsWith('fe8') || lower.startsWith('fe9') || lower.startsWith('fea')
      || lower.startsWith('feb') || lower.startsWith('fc') || lower.startsWith('fd'))
  }
  const [a, b] = ip.split('.').map((part) => parseInt(part, 10))
  if (Number.isNaN(a) || Number.isNaN(b)) return false
  return !(a === 0 || a === 10 || a === 127 || a >= 224
    || (a === 100 && b >= 64 && b <= 127)
    || (a === 169 && b === 254)
    || (a === 172 && b >= 16 && b <= 31)
    || (a === 192 && b === 168)
    || (a === 198 && (b === 18 || b === 19)))
}

async function disconnect(conn: Connection) {
  if (closingIds.value.has(conn.id)) return
  closingIds.value.add(conn.id)
  const host = getHost(conn)
  try {
    await closeConnection(conn.id)
    announce(`已断开 ${host}`)
  } catch (error) {
    closingIds.value.delete(conn.id)
    pushToast({ type: 'error', message: `断开连接失败\n${host}\n原因: ${getRequestErrorReason(error)}` })
  }
}

const disconnectLabel = computed(() =>
  query.value ? `断开筛选结果 (${activeList.value.length})` : '断开全部'
)

async function handleDisconnectMany() {
  const targets = activeList.value
  if (targets.length === 0) return
  const filtered = !!query.value
  const confirmed = await confirmDialog.value?.show({
    title: filtered ? '断开筛选结果' : '断开全部连接',
    message: filtered
      ? `将断开匹配“${filterText.value.trim()}”的 ${targets.length} 个连接。`
      : `将断开当前全部 ${targets.length} 个连接，正在进行的下载、通话和视频都会中断。`,
    confirmText: '断开',
    variant: 'danger',
  })
  if (!confirmed) return
  const ids = targets.map((c) => c.id)
  for (const id of ids) closingIds.value.add(id)
  try {
    if (filtered) {
      const failed = await closeConnections(ids)
      if (failed > 0) {
        pushToast({ type: 'error', message: `有 ${failed} 个连接没能断开，可能已经结束或核心拒绝了请求。` })
      }
    } else {
      await closeAllConnections()
    }
    announce(`已断开 ${ids.length} 个连接`)
  } catch (error) {
    for (const id of ids) closingIds.value.delete(id)
    pushToast({ type: 'error', message: `断开连接失败\n原因: ${getRequestErrorReason(error)}` })
  }
}

const searchInput = ref<HTMLInputElement | null>(null)

function clearSearch() {
  filterText.value = ''
  searchInput.value?.focus()
}

function onSearchEscape(e: KeyboardEvent) {
  if (filterText.value) filterText.value = ''
  else (e.target as HTMLInputElement).blur()
}

const selectedId = ref<string | null>(null)
const lastSelected = ref<Connection | null>(null)
let detailOpener: HTMLElement | null = null
const detailCloseButton = ref<HTMLButtonElement | null>(null)

const selectedLive = computed(() => {
  const id = selectedId.value
  if (!id) return null
  return (pausedSnapshot.value?.active ?? connections.value).find((c) => c.id === id)
    ?? (pausedSnapshot.value?.closed ?? closedConnections.value).find((c) => c.id === id)
    ?? null
})

watch(selectedLive, (conn) => {
  if (conn) lastSelected.value = conn
})

const selectedConnection = computed(() => (selectedId.value ? selectedLive.value ?? lastSelected.value : null))
const selectedIsOpen = computed(() => !!selectedConnection.value && !selectedConnection.value.closedAt
  && (pausedSnapshot.value?.active ?? connections.value).some((c) => c.id === selectedId.value))

function openDetail(conn: Connection) {
  detailOpener = document.activeElement as HTMLElement | null
  lastSelected.value = conn
  selectedId.value = conn.id
  void nextTick(() => detailCloseButton.value?.focus())
}

function closeDetail() {
  selectedId.value = null
  lastSelected.value = null
  const opener = detailOpener
  detailOpener = null
  void nextTick(() => opener?.focus())
}

async function disconnectSelected() {
  const conn = selectedConnection.value
  if (!conn) return
  await disconnect(conn)
  if (closingIds.value.has(conn.id) || !selectedIsOpen.value) closeDetail()
}

function getDestIP(conn: Connection): string | null {
  const ip = conn.metadata.destinationIP
  return ip && isPublicIP(ip) ? ip : null
}

function destination(conn: Connection): string {
  const ip = conn.metadata.destinationIP
  if (!ip) return conn.metadata.destinationPort ? `端口 ${conn.metadata.destinationPort}` : '-'
  return withPort(ip, conn.metadata.destinationPort)
}

const geoInfo = ref<IPGeoInfo | null>(null)
const geoLoading = ref(false)
const geoError = ref(false)
let geoRequest = 0

async function loadGeo(ip: string) {
  const request = ++geoRequest
  geoInfo.value = null
  geoError.value = false
  geoLoading.value = true
  try {
    const info = await getGeoIPForIP(ip)
    if (request === geoRequest) geoInfo.value = info
  } catch {
    if (request === geoRequest) geoError.value = true
  } finally {
    if (request === geoRequest) geoLoading.value = false
  }
}

watch(selectedId, () => {
  geoRequest++
  geoInfo.value = null
  geoError.value = false
  geoLoading.value = false
  const conn = selectedConnection.value
  const ip = conn ? getDestIP(conn) : null
  if (ip) void loadGeo(ip)
})

function onGlobalKeydown(e: KeyboardEvent) {
  if (selectedId.value && e.key === 'Escape') {
    e.preventDefault()
    closeDetail()
    return
  }
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return
  if (!isRunning.value || selectedId.value) return
  e.preventDefault()
  searchInput.value?.focus()
  searchInput.value?.select()
}

onMounted(() => {
  start()
  window.addEventListener('keydown', onGlobalKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onGlobalKeydown)
})
</script>

<template>
  <div class="flex flex-col h-full gap-3">
    <span class="sr-only" aria-live="polite">{{ announcement }}</span>
    <ServiceNotice
      v-if="!isRunning"
      :state="serviceState"
      page-title="连接"
      stopped-detail="启动服务后即可查看实时连接。"
      starting-detail="核心启动后会自动显示连接。"
    />

    <template v-else>
    <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
      <div class="flex items-center gap-3">
        <h1 class="text-[26px] leading-tight tracking-tight font-bold shrink-0">连接</h1>
        <div class="tabs tabs-boxed tabs-sm" role="tablist" aria-label="连接视图" @keydown="onTabKeydown">
          <button
            id="connections-tab-active"
            type="button"
            role="tab"
            class="tab"
            :class="{ 'tab-active': activeTab === 'active' }"
            :aria-selected="activeTab === 'active'"
            aria-controls="connections-panel"
            :tabindex="activeTab === 'active' ? 0 : -1"
            @click="activeTab = 'active'"
          >
            活跃 ({{ activeList.length }})
          </button>
          <button
            id="connections-tab-closed"
            type="button"
            role="tab"
            class="tab"
            :class="{ 'tab-active': activeTab === 'closed' }"
            :aria-selected="activeTab === 'closed'"
            aria-controls="connections-panel"
            :tabindex="activeTab === 'closed' ? 0 : -1"
            @click="activeTab = 'closed'"
          >
            已关闭 ({{ closedList.length }})
          </button>
        </div>
      </div>
      <div class="ml-auto flex items-center gap-2">
        <span class="text-xs tabular-nums text-base-content/65" title="本次运行的累计流量">
          ↓ {{ formatBytes(shownDownTotal) }} · ↑ {{ formatBytes(shownUpTotal) }}
        </span>
        <button
          type="button"
          class="btn btn-sm btn-ghost btn-circle shrink-0"
          :class="{ 'bg-primary/[0.12] text-[color:var(--selection-ink)]': paused }"
          :aria-pressed="paused"
          aria-label="暂停刷新"
          :title="paused ? '继续刷新' : '暂停刷新'"
          @click="paused = !paused"
        >
          <svg v-if="!paused" class="h-4 w-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <rect x="4" y="3" width="2.6" height="10" rx="0.8" />
            <rect x="9.4" y="3" width="2.6" height="10" rx="0.8" />
          </svg>
          <svg v-else class="h-4 w-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <path d="M5 3.4v9.2a.7.7 0 0 0 1.06.6l7.4-4.6a.7.7 0 0 0 0-1.2l-7.4-4.6A.7.7 0 0 0 5 3.4z" />
          </svg>
        </button>
        <button
          v-if="activeTab === 'active'"
          type="button"
          class="btn btn-sm btn-glass btn-glass-danger shrink-0"
          :disabled="activeList.length === 0"
          @click="handleDisconnectMany"
        >
          {{ disconnectLabel }}
        </button>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <div class="relative flex min-w-0 flex-1 items-center">
        <input
          ref="searchInput"
          v-model="filterText"
          type="search"
          class="input input-sm input-bordered w-full min-w-0 pl-8 [&::-webkit-search-cancel-button]:appearance-none"
          :class="{ 'pr-8': filterText }"
          placeholder="搜索域名、IP、端口、进程、规则或节点"
          aria-label="搜索连接"
          aria-keyshortcuts="Control+F"
          title="搜索连接 (Ctrl+F)"
          @keydown.esc="onSearchEscape"
        />
        <svg class="pointer-events-none absolute left-2.5 z-[1] h-3.5 w-3.5 text-base-content/55" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
          <circle cx="7" cy="7" r="4.5" />
          <path d="m10.5 10.5 3 3" />
        </svg>
        <button
          v-if="filterText"
          type="button"
          class="absolute right-1.5 z-[1] flex h-5 w-5 items-center justify-center rounded-full text-base-content/55 transition-colors hover:bg-[var(--fill-strong)] hover:text-base-content focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          aria-label="清除搜索"
          @click="clearSearch"
        >
          <svg class="h-2.5 w-2.5" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
            <path d="M2 2l6 6M8 2 2 8" />
          </svg>
        </button>
      </div>
      <select v-model="sortKey" class="select select-sm select-bordered w-auto shrink-0" aria-label="排序方式">
        <option v-for="option in SORT_OPTIONS" :key="option.value" :value="option.value">{{ option.label }}</option>
      </select>
    </div>

    <div
      v-if="paused || (socketState === 'connecting' && connections.length > 0)"
      class="flex items-center gap-2 rounded-[var(--radius-control)] bg-[var(--fill)] px-3 py-1.5 text-xs text-base-content/80"
      role="status"
    >
      <template v-if="paused">
        <span>已暂停刷新，显示的是暂停时的连接。</span>
        <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="paused = false">继续刷新</button>
      </template>
      <template v-else>
        <span class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        <span>与核心的连接中断，正在重连；下面是中断前的数据。</span>
      </template>
    </div>

    <div
      id="connections-panel"
      role="tabpanel"
      :aria-labelledby="`connections-tab-${activeTab}`"
      class="surface-card flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      <div class="card-scroll min-h-0 flex-1 overflow-auto">
        <ul
          v-if="visibleList.length > 0"
          class="py-1"
          :aria-label="activeTab === 'active' ? '活跃连接' : '已关闭连接'"
          @mouseenter="listHovered = true"
          @mouseleave="listHovered = false"
          @focusin="listFocused = true"
          @focusout="onListFocusOut"
          @keydown="onListKeydown"
        >
          <li
            v-for="conn in visibleList"
            :key="conn.id"
            class="conn-row relative mx-1 grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-3 rounded-[var(--radius-control)] px-3 py-2 transition-colors hover:bg-base-content/[0.05]"
          >
            <button
              type="button"
              data-conn-row
              class="absolute inset-0 rounded-[inherit] focus-visible:outline-none"
              :tabindex="rowTabTarget === conn.id ? 0 : -1"
              :aria-label="rowLabel(conn)"
              @focus="focusedRowId = conn.id"
              @click="openDetail(conn)"
            ></button>
            <div class="pointer-events-none min-w-0">
              <div class="flex min-w-0 items-center gap-1.5 text-sm">
                <span
                  class="shrink-0 rounded-full px-1.5 py-px text-[11px] font-medium uppercase leading-4"
                  :class="conn.metadata.network === 'tcp'
                    ? 'bg-info/15 text-[color:var(--tcp-ink)]'
                    : 'bg-accent/15 text-[color:var(--udp-ink)]'"
                >{{ conn.metadata.network }}</span>
                <span class="min-w-0 truncate">{{ withPort(getHost(conn), conn.metadata.destinationPort) }}</span>
                <span v-if="getProcess(conn)" class="min-w-0 max-w-[45%] shrink-[2] truncate text-xs text-base-content/65">· {{ getProcess(conn) }}</span>
              </div>
              <div class="mt-0.5 truncate text-xs text-base-content/65">{{ routeLine(conn) }}</div>
            </div>
            <div class="pointer-events-none text-right text-xs tabular-nums leading-5">
              <div v-if="!conn.closedAt" class="whitespace-nowrap">↓ {{ formatSpeed(conn.downloadSpeed) }} · ↑ {{ formatSpeed(conn.uploadSpeed) }}</div>
              <div v-else class="whitespace-nowrap">↓ {{ formatBytes(conn.download) }} · ↑ {{ formatBytes(conn.upload) }}</div>
              <div class="whitespace-nowrap text-base-content/65">
                <template v-if="!conn.closedAt">{{ formatBytes(conn.download) }} / {{ formatBytes(conn.upload) }} · </template>{{ connectionDuration(conn) }}
              </div>
            </div>
            <button
              v-if="activeTab === 'active'"
              type="button"
              tabindex="-1"
              class="relative z-[1] btn btn-ghost btn-xs btn-circle text-[color:var(--danger-ink)]"
              :aria-label="`断开 ${getHost(conn)}`"
              title="断开连接 (Delete)"
              @click="disconnect(conn)"
            >
              <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
                <path d="M3 3l6 6M9 3 3 9" />
              </svg>
            </button>
            <span v-else class="w-6" aria-hidden="true"></span>
          </li>
        </ul>

        <div v-else-if="query" class="flex flex-col items-center gap-3 px-6 py-12 text-center" role="status">
          <p class="text-sm text-base-content/80">没有匹配“{{ filterText.trim() }}”的{{ activeTab === 'active' ? '活跃' : '已关闭' }}连接</p>
          <button type="button" class="btn btn-sm btn-glass px-5" @click="clearSearch">清除搜索</button>
        </div>
        <div
          v-else-if="activeTab === 'active' && socketState !== 'open'"
          class="flex items-center justify-center gap-2 py-12 text-sm text-base-content/65"
          role="status"
        >
          <span class="loading loading-spinner loading-sm" aria-hidden="true"></span>
          正在连接核心…
        </div>
        <div v-else class="flex flex-col items-center gap-1 px-6 py-12 text-center">
          <p class="text-sm text-base-content/80">{{ activeTab === 'active' ? '当前没有活跃连接' : '还没有已关闭的连接' }}</p>
          <p v-if="activeTab === 'closed'" class="text-xs text-base-content/65">连接结束后会出现在这里，最多保留 500 条。</p>
        </div>
      </div>
    </div>
    </template>

    <ConfirmDialog ref="confirmDialog" />

    <Teleport to="body">
    <Transition name="glass-pop">
    <div
      v-if="selectedConnection"
      class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      @click.self="closeDetail"
    >
      <div
        class="glass-popover w-full max-w-2xl max-h-[80vh] flex flex-col rounded-[var(--radius-panel)]"
        role="dialog"
        aria-modal="true"
        aria-labelledby="connection-detail-title"
      >
        <div class="flex items-center justify-between px-5 pt-4 pb-3 shrink-0">
          <h2 id="connection-detail-title" class="font-bold text-lg">
            连接详情<span v-if="!selectedIsOpen" class="ml-2 text-sm font-normal text-base-content/65">已关闭</span>
          </h2>
          <button
            ref="detailCloseButton"
            type="button"
            class="btn btn-sm btn-circle btn-ghost"
            aria-label="关闭详情"
            @click="closeDetail"
          >
            <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3 3 9" />
            </svg>
          </button>
        </div>

        <div class="flex-1 min-h-0 overflow-y-auto px-5 space-y-4">
          <div>
            <h3 class="text-sm font-semibold text-base-content/70 mb-2">基本信息</h3>
            <div class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/65">ID</span>
              <span class="settings-mono break-all">{{ selectedConnection.id }}</span>
              <span class="text-base-content/65">类型</span>
              <span>{{ selectedConnection.metadata.network }} / {{ selectedConnection.metadata.type }}</span>
              <span class="text-base-content/65">主机</span>
              <span class="break-all">{{ getHost(selectedConnection) }}</span>
              <span class="text-base-content/65">规则</span>
              <span class="break-words">{{ selectedConnection.rule || '默认出站' }}</span>
              <span v-if="selectedConnection.rulePayload" class="text-base-content/65">规则载荷</span>
              <span v-if="selectedConnection.rulePayload" class="break-words">{{ selectedConnection.rulePayload }}</span>
              <span class="text-base-content/65">链路</span>
              <span>{{ formatChains(selectedConnection.chains) }}</span>
              <span class="text-base-content/65">开始时间</span>
              <span class="tabular-nums">{{ new Date(selectedConnection.start).toLocaleString() }}</span>
              <span class="text-base-content/65">持续时长</span>
              <span class="tabular-nums">{{ connectionDuration(selectedConnection) }}</span>
            </div>
          </div>

          <div>
            <h3 class="text-sm font-semibold text-base-content/70 mb-2">网络信息</h3>
            <div class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/65">源地址</span>
              <span class="settings-mono break-all">{{ withPort(selectedConnection.metadata.sourceIP || '-', selectedConnection.metadata.sourcePort) }}</span>
              <span class="text-base-content/65">目标地址</span>
              <span class="settings-mono break-all">{{ destination(selectedConnection) }}</span>
              <span class="text-base-content/65">DNS 模式</span>
              <span>{{ selectedConnection.metadata.dnsMode || '-' }}</span>
              <span v-if="selectedConnection.metadata.sniffHost" class="text-base-content/65">嗅探主机</span>
              <span v-if="selectedConnection.metadata.sniffHost" class="break-all">{{ selectedConnection.metadata.sniffHost }}</span>
              <span class="text-base-content/65">进程</span>
              <span class="break-all">{{ getProcess(selectedConnection) || '-' }}</span>
              <span v-if="selectedConnection.metadata.processPath" class="text-base-content/65">进程路径</span>
              <span v-if="selectedConnection.metadata.processPath" class="settings-mono break-all">{{ selectedConnection.metadata.processPath }}</span>
            </div>
          </div>

          <div v-if="getDestIP(selectedConnection)">
            <h3 class="text-sm font-semibold text-base-content/70 mb-2">IP 地理信息</h3>
            <div v-if="geoLoading" class="text-sm text-base-content/65" role="status">
              <span class="loading loading-spinner loading-xs mr-1" aria-hidden="true"></span>正在查询…
            </div>
            <div v-else-if="geoError" class="flex items-center gap-2 text-sm text-base-content/65">
              查询失败
              <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="loadGeo(getDestIP(selectedConnection)!)">重试</button>
            </div>
            <div v-else-if="geoInfo" class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-sm">
              <span class="text-base-content/65">IP</span>
              <span class="settings-mono">{{ geoInfo.ip }}</span>
              <span class="text-base-content/65">ASN</span>
              <span>{{ geoInfo.asn ? `AS${geoInfo.asn}` : '-' }}</span>
              <span class="text-base-content/65">运营商</span>
              <span>{{ geoInfo.asnOrganization || geoInfo.organization || '-' }}</span>
              <span class="text-base-content/65">位置</span>
              <span>{{ [geoInfo.city, geoInfo.region, geoInfo.country].filter(Boolean).join(', ') || '-' }}</span>
            </div>
          </div>

          <div>
            <h3 class="text-sm font-semibold text-base-content/70 mb-2">流量信息</h3>
            <div class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-sm tabular-nums">
              <span class="text-base-content/65">下载总量</span>
              <span>{{ formatBytes(selectedConnection.download) }}</span>
              <span class="text-base-content/65">上传总量</span>
              <span>{{ formatBytes(selectedConnection.upload) }}</span>
              <template v-if="selectedIsOpen">
                <span class="text-base-content/65">下载速度</span>
                <span>{{ formatSpeed(selectedConnection.downloadSpeed) }}</span>
                <span class="text-base-content/65">上传速度</span>
                <span>{{ formatSpeed(selectedConnection.uploadSpeed) }}</span>
              </template>
            </div>
          </div>
        </div>

        <div class="flex justify-end gap-2 px-5 pt-3 pb-4 shrink-0">
          <button
            v-if="selectedIsOpen"
            type="button"
            class="btn btn-sm btn-glass btn-glass-danger"
            :disabled="closingIds.has(selectedConnection.id)"
            @click="disconnectSelected"
          >
            断开连接
          </button>
          <button type="button" class="btn btn-sm" @click="closeDetail">完成</button>
        </div>
      </div>
    </div>
    </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.conn-row:has(> [data-conn-row]:focus-visible) {
  outline: 2px solid oklch(var(--p));
  outline-offset: -2px;
}
</style>
