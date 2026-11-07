<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { matchesFilter, useConnectionsStore } from '@/stores/connections'
import { useConfigStore } from '@/stores/config'
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
  socketFailures,
  start,
  closeConnection,
  closeConnections,
  closeAllConnections,
} = useConnectionsStore()
const { clashApiUrl } = useConfigStore()
const { pushToast } = useToastStore()
const { serviceState, isRunning } = useSettledServiceState()
const router = useRouter()

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

const apiUnreachable = computed(() => socketFailures.value >= 3)

const paused = ref(false)
const pausedSnapshot = ref<{ active: Connection[]; closed: Connection[]; down: number; up: number } | null>(null)

watch(paused, (on) => {
  pausedSnapshot.value = on
    ? { active: connections.value, closed: closedConnections.value, down: downloadTotal.value, up: uploadTotal.value }
    : null
})

const shownDownTotal = computed(() => pausedSnapshot.value?.down ?? downloadTotal.value)
const shownUpTotal = computed(() => pausedSnapshot.value?.up ?? uploadTotal.value)

const CLOSING_TIMEOUT_MS = 4000
const closingIds = ref<Set<string>>(new Set())

watch(connections, (list) => {
  if (closingIds.value.size === 0 || paused.value) return
  const present = new Set(list.map((c) => c.id))
  for (const id of closingIds.value) {
    if (!present.has(id)) closingIds.value.delete(id)
  }
})

function markClosing(ids: string[]) {
  for (const id of ids) closingIds.value.add(id)
  setTimeout(() => {
    for (const id of ids) closingIds.value.delete(id)
  }, CLOSING_TIMEOUT_MS)
}

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

const sortOptions = computed(() =>
  activeTab.value === 'closed'
    ? SORT_OPTIONS.filter((o) => o.value !== 'download' && o.value !== 'upload')
    : SORT_OPTIONS
)

const effectiveSort = computed<SortKey>(() =>
  sortOptions.value.some((o) => o.value === sortKey.value) ? sortKey.value : 'recent'
)

function startTime(conn: Connection): number {
  return new Date(conn.start).getTime() || 0
}

function durationMs(conn: Connection): number {
  return (conn.closedAt ?? Date.now()) - startTime(conn)
}

function compareConnections(a: Connection, b: Connection, closed: boolean): number {
  let diff = 0
  switch (effectiveSort.value) {
    case 'recent': diff = closed ? (b.closedAt ?? 0) - (a.closedAt ?? 0) : startTime(b) - startTime(a); break
    case 'duration': diff = durationMs(b) - durationMs(a); break
    case 'download': diff = (b.downloadSpeed ?? 0) - (a.downloadSpeed ?? 0); break
    case 'upload': diff = (b.uploadSpeed ?? 0) - (a.uploadSpeed ?? 0); break
    case 'downloadTotal': diff = (b.download ?? 0) - (a.download ?? 0); break
    case 'uploadTotal': diff = (b.upload ?? 0) - (a.upload ?? 0); break
    case 'host': diff = getHost(a).localeCompare(getHost(b)); break
  }
  return diff || startTime(b) - startTime(a) || a.id.localeCompare(b.id)
}

type RowState = 'live' | 'closing' | 'ended'

interface Row {
  conn: Connection
  state: RowState
}

const listEl = ref<HTMLElement | null>(null)
const listHovered = ref(false)
const listFocused = ref(false)
const frozenRows = ref<Map<string, Connection> | null>(null)

function prepareList(source: Connection[], closed: boolean): Row[] {
  const q = query.value
  const live = source
    .filter((c) => !closingIds.value.has(c.id) && (!q || matchesFilter(c, q)))
    .sort((a, b) => compareConnections(a, b, closed))
  const frozen = frozenRows.value
  if (!frozen) return live.map((conn) => ({ conn, state: 'live' }))
  const pending = new Map(live.map((conn) => [conn.id, conn]))
  const rows: Row[] = []
  for (const [id, snapshot] of frozen) {
    const conn = pending.get(id)
    if (conn) {
      rows.push({ conn, state: 'live' })
      pending.delete(id)
    } else {
      rows.push({ conn: snapshot, state: closingIds.value.has(id) ? 'closing' : 'ended' })
    }
  }
  for (const conn of live) {
    if (pending.has(conn.id)) rows.push({ conn, state: 'live' })
  }
  return rows
}

const activeRows = computed(() => prepareList(pausedSnapshot.value?.active ?? connections.value, false))
const closedRows = computed(() => prepareList(pausedSnapshot.value?.closed ?? closedConnections.value, true))
const visibleRows = computed(() => (activeTab.value === 'active' ? activeRows.value : closedRows.value))
const activeLive = computed(() => activeRows.value.filter((row) => row.state === 'live').map((row) => row.conn))
const closedCount = computed(() => closedRows.value.filter((row) => row.state === 'live').length)

function snapshotRows(): Map<string, Connection> {
  return new Map(visibleRows.value.filter((row) => row.state === 'live').map((row) => [row.conn.id, row.conn]))
}

watch([listHovered, listFocused], ([hovered, focused]) => {
  if (!hovered && !focused) {
    frozenRows.value = null
  } else if (!frozenRows.value) {
    frozenRows.value = snapshotRows()
  }
})

watch([activeTab, effectiveSort, query], () => {
  if (!frozenRows.value) return
  frozenRows.value = null
  frozenRows.value = snapshotRows()
})

watch(visibleRows, (rows) => {
  if (rows.length === 0) {
    listHovered.value = false
    listFocused.value = false
    return
  }
  if (!listFocused.value) return
  void nextTick(() => {
    if (listFocused.value && !listEl.value?.contains(document.activeElement)) listFocused.value = false
  })
})

function onListFocusOut(e: FocusEvent) {
  if ((e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) return
  listFocused.value = false
}

const focusedRowId = ref<string | null>(null)

const rowTabTarget = computed(() => {
  const rows = visibleRows.value
  if (focusedRowId.value && rows.some((row) => row.conn.id === focusedRowId.value)) return focusedRowId.value
  return rows[0]?.conn.id ?? null
})

function focusCurrentRow() {
  listEl.value?.querySelector<HTMLElement>('[data-conn-row][tabindex="0"]')?.focus()
}

function onListKeydown(e: KeyboardEvent) {
  const list = e.currentTarget as HTMLElement
  const buttons = Array.from(list.querySelectorAll<HTMLElement>('[data-conn-row]'))
  const index = buttons.indexOf(e.target as HTMLElement)
  if (index < 0) return
  if (e.key === 'Delete') {
    e.preventDefault()
    const row = visibleRows.value[index]
    if (e.repeat || activeTab.value !== 'active' || !row || row.state !== 'live') return
    const neighbour = index < buttons.length - 1 ? index + 1 : index - 1
    if (neighbour >= 0) buttons[neighbour].focus()
    void disconnect(row.conn)
    return
  }
  let next: number
  switch (e.key) {
    case 'ArrowDown': next = index + 1; break
    case 'ArrowUp': next = index - 1; break
    case 'PageDown': next = index + 10; break
    case 'PageUp': next = index - 10; break
    case 'Home': next = 0; break
    case 'End': next = buttons.length - 1; break
    default: return
  }
  e.preventDefault()
  buttons[Math.min(Math.max(next, 0), buttons.length - 1)].focus()
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

function ruleText(conn: Connection): string {
  return conn.rule || 'final'
}

function connectionDuration(conn: Connection): string {
  const text = formatDuration(conn.start, conn.closedAt)
  return conn.closedInBackground ? `≥ ${text}` : text
}

function rowStateText(row: Row): string {
  if (row.state === 'closing') return '正在断开…'
  if (row.state === 'ended') return '已结束'
  return ''
}

function rowLabel(row: Row): string {
  const conn = row.conn
  return [
    `${conn.metadata.network.toUpperCase()} ${withPort(getHost(conn), conn.metadata.destinationPort)}`,
    getProcess(conn),
    formatChains(conn.chains),
    `规则 ${ruleText(conn)}`,
    rowStateText(row),
  ].filter(Boolean).join('，')
}

function isIPAddress(str: string): boolean {
  return /^[\d.]+$/.test(str) || str.includes(':')
}

function isPublicIPv4(ip: string): boolean {
  const [a, b] = ip.split('.').map((part) => parseInt(part, 10))
  if (Number.isNaN(a) || Number.isNaN(b)) return false
  return !(a === 0 || a === 10 || a === 127 || a >= 224
    || (a === 100 && b >= 64 && b <= 127)
    || (a === 169 && b === 254)
    || (a === 172 && b >= 16 && b <= 31)
    || (a === 192 && b === 168)
    || (a === 198 && (b === 18 || b === 19)))
}

function isPublicIP(ip: string): boolean {
  if (!ip.includes(':')) return isPublicIPv4(ip)
  const lower = ip.toLowerCase()
  if (lower.startsWith('::ffff:') && lower.includes('.')) return isPublicIPv4(lower.slice(7))
  return !(lower === '::' || lower === '::1' || /^f[c-f]/.test(lower))
}

async function disconnect(conn: Connection) {
  if (closingIds.value.has(conn.id)) return
  markClosing([conn.id])
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
  query.value ? `断开筛选结果 (${activeLive.value.length})` : '断开全部'
)

async function handleDisconnectMany() {
  const targets = activeLive.value
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
  markClosing(ids)
  try {
    if (filtered) {
      const failed = await closeConnections(ids)
      for (const id of failed) closingIds.value.delete(id)
      if (failed.length > 0) {
        pushToast({ type: 'error', message: `有 ${failed.length} 个连接没能断开，已重新显示在列表里。` })
      }
      announce(`已断开 ${ids.length - failed.length} 个连接`)
    } else {
      await closeAllConnections()
      announce(`已断开 ${ids.length} 个连接`)
    }
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
const detailDialog = ref<HTMLElement | null>(null)
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

watch(selectedIsOpen, (open, wasOpen) => {
  if (selectedId.value && wasOpen && !open) announce('这个连接已经结束')
})

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
  void nextTick(() => {
    if (opener?.isConnected) opener.focus()
    else focusCurrentRow()
  })
}

async function disconnectSelected() {
  const conn = selectedConnection.value
  if (!conn) return
  await disconnect(conn)
  if (closingIds.value.has(conn.id) || !selectedIsOpen.value) closeDetail()
}

function trapDetailFocus(e: KeyboardEvent) {
  const focusables = Array.from(detailDialog.value?.querySelectorAll<HTMLElement>(
    'button:not([disabled]), [href], input, select, [tabindex]:not([tabindex="-1"])',
  ) ?? [])
  if (focusables.length === 0) return
  const index = focusables.indexOf(document.activeElement as HTMLElement)
  if (e.shiftKey && index <= 0) {
    e.preventDefault()
    focusables[focusables.length - 1].focus()
  } else if (!e.shiftKey && (index === -1 || index === focusables.length - 1)) {
    e.preventDefault()
    focusables[0].focus()
  }
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
const geoError = ref('')
let geoRequest = 0

async function loadGeo(ip: string) {
  const request = ++geoRequest
  geoInfo.value = null
  geoError.value = ''
  geoLoading.value = true
  try {
    const info = await getGeoIPForIP(ip)
    if (request === geoRequest) geoInfo.value = info
  } catch (error) {
    if (request === geoRequest) geoError.value = getRequestErrorReason(error)
  } finally {
    if (request === geoRequest) geoLoading.value = false
  }
}

watch(selectedId, () => {
  geoRequest++
  geoInfo.value = null
  geoError.value = ''
  geoLoading.value = false
  const conn = selectedConnection.value
  const ip = conn ? getDestIP(conn) : null
  if (ip) void loadGeo(ip)
})

function onGlobalKeydown(e: KeyboardEvent) {
  if (selectedId.value) {
    if (e.key === 'Escape') {
      e.preventDefault()
      closeDetail()
    } else if (e.key === 'Tab') {
      trapDetailFocus(e)
    }
    return
  }
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return
  if (!isRunning.value || document.querySelector('[role="alertdialog"]')) return
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
            活跃<span class="ml-1 tabular-nums" aria-hidden="true">({{ activeLive.length }})</span>
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
            已关闭<span class="ml-1 tabular-nums" aria-hidden="true">({{ closedCount }})</span>
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
          type="button"
          class="btn btn-sm btn-glass btn-glass-danger shrink-0"
          :class="{ invisible: activeTab !== 'active' }"
          :disabled="activeTab !== 'active' || activeLive.length === 0"
          :aria-hidden="activeTab !== 'active'"
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
          class="absolute right-1 z-[1] flex h-6 w-6 items-center justify-center rounded-full text-base-content/55 transition-colors hover:bg-[var(--fill-strong)] hover:text-base-content focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          aria-label="清除搜索"
          @click="clearSearch"
        >
          <svg class="h-2.5 w-2.5" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
            <path d="M2 2l6 6M8 2 2 8" />
          </svg>
        </button>
      </div>
      <select
        class="select select-sm select-bordered w-auto shrink-0"
        aria-label="排序方式"
        :value="effectiveSort"
        @change="sortKey = ($event.target as HTMLSelectElement).value as SortKey"
      >
        <option v-for="option in sortOptions" :key="option.value" :value="option.value">{{ option.label }}</option>
      </select>
    </div>

    <div
      v-if="paused || (socketState === 'reconnecting' && connections.length > 0)"
      class="surface-card flex items-center gap-2 px-3 py-1.5 text-xs"
      role="status"
    >
      <template v-if="paused">
        <span>已暂停刷新，显示的是暂停时的连接。</span>
        <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="paused = false">继续刷新</button>
      </template>
      <template v-else-if="apiUnreachable">
        <span>无法连接 Clash API，正在重试；下面是中断前的数据。</span>
        <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="router.push('/settings')">检查 API 配置</button>
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
          v-if="visibleRows.length > 0"
          ref="listEl"
          class="py-1"
          :aria-label="activeTab === 'active' ? '活跃连接' : '已关闭连接'"
          @mouseenter="listHovered = true"
          @mouseleave="listHovered = false"
          @focusin="listFocused = true"
          @focusout="onListFocusOut"
          @keydown="onListKeydown"
        >
          <li
            v-for="row in visibleRows"
            :key="row.conn.id"
            class="conn-row relative mx-1 grid grid-cols-[minmax(0,1fr)_13rem_1.5rem] items-center gap-x-3 rounded-[var(--radius-control)] px-3 py-2 transition-[background-color,opacity] hover:bg-base-content/[0.05]"
            :class="{ 'opacity-50': row.state !== 'live' }"
          >
            <button
              type="button"
              data-conn-row
              class="absolute inset-0 rounded-[inherit] focus-visible:outline-none"
              :tabindex="rowTabTarget === row.conn.id ? 0 : -1"
              :aria-label="rowLabel(row)"
              :aria-keyshortcuts="activeTab === 'active' ? 'Enter Delete' : 'Enter'"
              @focus="focusedRowId = row.conn.id"
              @click="openDetail(row.conn)"
            ></button>
            <div class="pointer-events-none min-w-0" aria-hidden="true">
              <div class="flex min-w-0 items-center gap-1.5 text-sm">
                <span
                  class="shrink-0 rounded-full px-1.5 text-xs font-medium uppercase leading-5"
                  :class="row.conn.metadata.network === 'tcp'
                    ? 'bg-info/15 text-[color:var(--tcp-ink)]'
                    : 'bg-accent/15 text-[color:var(--udp-ink)]'"
                >{{ row.conn.metadata.network }}</span>
                <span class="min-w-0 truncate">{{ withPort(getHost(row.conn), row.conn.metadata.destinationPort) }}</span>
                <span v-if="getProcess(row.conn)" class="min-w-0 max-w-[45%] shrink-[2] truncate text-xs text-base-content/65">· {{ getProcess(row.conn) }}</span>
              </div>
              <div class="mt-0.5 flex min-w-0 gap-1.5 text-xs text-base-content/65">
                <span class="max-w-[70%] shrink-0 truncate">{{ formatChains(row.conn.chains) }}</span>
                <span class="min-w-0 truncate">· {{ ruleText(row.conn) }}</span>
              </div>
            </div>
            <div class="pointer-events-none min-w-0 text-right text-xs tabular-nums leading-5" aria-hidden="true">
              <div v-if="row.state !== 'live'" class="truncate">{{ rowStateText(row) }}</div>
              <div v-else-if="!row.conn.closedAt" class="truncate">↓ {{ formatSpeed(row.conn.downloadSpeed) }} · ↑ {{ formatSpeed(row.conn.uploadSpeed) }}</div>
              <div v-else class="truncate">↓ {{ formatBytes(row.conn.download) }} · ↑ {{ formatBytes(row.conn.upload) }}</div>
              <div class="truncate text-base-content/65" :title="row.conn.closedInBackground ? '在窗口隐藏期间结束，确切结束时间未知' : undefined">
                <template v-if="!row.conn.closedAt">{{ formatBytes(row.conn.download) }} / {{ formatBytes(row.conn.upload) }} · </template>{{ connectionDuration(row.conn) }}
              </div>
            </div>
            <button
              v-if="activeTab === 'active'"
              type="button"
              tabindex="-1"
              class="relative z-[1] btn btn-ghost btn-xs btn-circle text-[color:var(--danger-ink)] disabled:bg-transparent"
              :disabled="row.state !== 'live'"
              :aria-label="`断开 ${getHost(row.conn)}`"
              title="断开连接 (Delete)"
              @click="disconnect(row.conn)"
            >
              <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
                <path d="M3 3l6 6M9 3 3 9" />
              </svg>
            </button>
            <span v-else aria-hidden="true"></span>
          </li>
        </ul>

        <div v-else-if="query" class="flex flex-col items-center gap-3 px-6 py-12 text-center" role="status">
          <p class="text-sm text-base-content/80">没有匹配“{{ filterText.trim() }}”的{{ activeTab === 'active' ? '活跃' : '已关闭' }}连接</p>
          <button type="button" class="btn btn-sm btn-glass px-5" @click="clearSearch">清除搜索</button>
        </div>
        <div
          v-else-if="activeTab === 'active' && socketState !== 'open' && apiUnreachable"
          class="mx-auto mt-6 max-w-[520px] px-6 py-5"
          role="alert"
        >
          <p class="text-base font-semibold">无法连接 Clash API</p>
          <p class="mt-1 text-sm text-base-content/70">
            请确认 sing-box 配置里开启了 clash_api，并且地址和密钥与当前的 Clash API 配置一致。
          </p>
          <dl class="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
            <dt class="text-base-content/65">地址</dt>
            <dd class="settings-mono break-all">{{ clashApiUrl || '未配置' }}</dd>
            <dt class="text-base-content/65">状态</dt>
            <dd>已连续 {{ socketFailures }} 次连接失败，仍在自动重试</dd>
          </dl>
          <div class="mt-4">
            <button type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">检查 API 配置</button>
          </div>
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
        ref="detailDialog"
        class="glass-popover w-full max-w-2xl max-h-[80vh] flex flex-col rounded-[var(--radius-panel)]"
        role="dialog"
        aria-modal="true"
        aria-labelledby="connection-detail-title"
      >
        <div class="flex items-center justify-between gap-3 px-5 pt-4 pb-3 shrink-0">
          <h2 id="connection-detail-title" class="min-w-0 truncate font-bold text-lg">
            {{ getHost(selectedConnection) }}<span v-if="!selectedIsOpen" class="ml-2 text-sm font-normal text-base-content/65">已关闭</span>
          </h2>
          <button
            ref="detailCloseButton"
            type="button"
            class="btn btn-sm btn-circle btn-ghost shrink-0"
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
              <span>{{ selectedConnection.metadata.network.toUpperCase() }} / {{ selectedConnection.metadata.type }}</span>
              <span class="text-base-content/65">主机</span>
              <span class="break-all">{{ getHost(selectedConnection) }}</span>
              <span class="text-base-content/65">规则</span>
              <span class="break-words">{{ ruleText(selectedConnection) }}</span>
              <span v-if="selectedConnection.rulePayload" class="text-base-content/65">规则载荷</span>
              <span v-if="selectedConnection.rulePayload" class="break-words">{{ selectedConnection.rulePayload }}</span>
              <span class="text-base-content/65">链路</span>
              <span>{{ formatChains(selectedConnection.chains) }}</span>
              <span class="text-base-content/65">开始时间</span>
              <span class="tabular-nums">{{ new Date(selectedConnection.start).toLocaleString() }}</span>
              <span class="text-base-content/65">持续时长</span>
              <span class="tabular-nums">
                {{ connectionDuration(selectedConnection) }}<template v-if="selectedConnection.closedInBackground">（在窗口隐藏期间结束，确切时间未知）</template>
              </span>
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
            <div v-else-if="geoError" class="flex flex-wrap items-center gap-2 text-sm text-base-content/65">
              查询失败：{{ geoError }}
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
