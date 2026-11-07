<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, computed, watch, watchEffect } from 'vue'
import { useRouter } from 'vue-router'
import { useProxiesStore } from '@/stores/proxies'
import { useServiceStore } from '@/stores/service'
import {
  fetchProxyProviders,
  updateProxyProvider,
  healthCheckProvider,
} from '@/api'
import type { ProxyProvider } from '@/types'
import { getRequestErrorReason } from '@/utils/requestError'
import { useToastStore } from '@/stores/toast'
import { formatBytes, formatLatency, formatDate, latencyColor, dotColor } from '@/utils/format'
import { batchUpdateProviders } from '@/utils/batchUpdate'
import { startCore } from '@/utils/coreControl'
import { isElevationCancelled } from '@/bridge/service'
import GroupSpeed from '@/components/proxies/GroupSpeed.vue'
import { appVisible } from '@/stores/appVisible'

const {
  proxyGroups,
  loading,
  loaded,
  loadError,
  loadProxies,
  switchProxy,
  testGroupNodes,
  testNodeLatency,
  getProxy,
  getLatency,
  getLatencyTime,
  getTestUrl,
  isIPv6,
} = useProxiesStore()

import { useConfigStore } from '@/stores/config'
import { useConnectionsStore } from '@/stores/connections'
const { config, clashApiUrl } = useConfigStore()
const { pushToast } = useToastStore()
const { serviceStatus, refresh: refreshService } = useServiceStore()
const router = useRouter()
const { start: startConnections } = useConnectionsStore()

const UNKNOWN_GRACE_MS = 5000
const serviceState = ref(serviceStatus.value.state)
let unknownSince = 0

watch(serviceStatus, (status) => {
  if (status.state !== 'unknown') {
    unknownSince = 0
    serviceState.value = status.state
    return
  }
  if (!unknownSince) unknownSince = Date.now()
  if (Date.now() - unknownSince >= UNKNOWN_GRACE_MS) serviceState.value = 'unknown'
})

const isRunning = computed(() => serviceState.value === 'running')

const coreWarmingUp = computed(() =>
  serviceStatus.value.state === 'running' && (serviceStatus.value.uptimeSeconds ?? Infinity) < 15
)

const serviceNotice = computed(() => {
  switch (serviceState.value) {
    case 'stopped':
      return { title: 'sing-box 服务已停止', detail: '启动服务后即可查看代理组和节点延迟。', action: 'start' as const }
    case 'starting':
      return { title: '正在启动 sing-box…', detail: '核心启动后会自动加载代理组。', action: null }
    case 'stopping':
      return { title: '正在停止 sing-box…', detail: '服务停止后可以在这里重新启动。', action: null }
    case 'not_installed':
      return { title: '尚未安装 singboard 服务', detail: '在设置中安装服务后，即可启动 sing-box。', action: 'settings' as const }
    default:
      return { title: '无法获取服务状态', detail: '可以在设置中查看服务状态和错误日志。', action: 'settings' as const }
  }
})

const startingService = ref(false)

async function handleStartService() {
  if (startingService.value) return
  startingService.value = true
  try {
    await startCore()
  } catch (e: any) {
    if (!isElevationCancelled(e)) pushToast({ message: '启动服务失败: ' + (e?.message || e), type: 'error' }, 6000)
  }
  await refreshService()
  startingService.value = false
}

const activeTab = ref<'groups' | 'providers'>('groups')

function onTabKeydown(e: KeyboardEvent) {
  if ((e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') || !showProvidersTab.value) return
  e.preventDefault()
  activeTab.value = activeTab.value === 'groups' ? 'providers' : 'groups'
  void nextTick(() => document.getElementById(`proxy-tab-${activeTab.value}`)?.focus())
}

const announcement = ref('')

function announce(message: string) {
  announcement.value = ''
  void nextTick(() => {
    announcement.value = message
  })
}

// 展开的代理组持久化到 localStorage
const EXPANDED_GROUPS_KEY = 'singboard-expanded-groups'

function loadExpandedGroups(): Set<string> {
  try {
    const saved = localStorage.getItem(EXPANDED_GROUPS_KEY)
    if (saved) {
      const parsed = JSON.parse(saved)
      if (Array.isArray(parsed)) {
        return new Set(parsed.filter((item): item is string => typeof item === 'string'))
      }
    }
  } catch { }
  return new Set()
}

const expandedGroups = ref<Set<string>>(loadExpandedGroups())
const testingGroups = ref<Set<string>>(new Set())
const testingNodes = ref<Set<string>>(new Set())

const proxyProviders = ref<ProxyProvider[]>([])
const providersAvailable = ref(false)
const updatingProviders = ref<Set<string>>(new Set())
const updatingAll = ref(false)
const healthCheckingProviders = ref<Set<string>>(new Set())
const expandedProviders = ref<Set<string>>(new Set())

const showProvidersTab = computed(() => providersAvailable.value && proxyProviders.value.length > 0)

watch(showProvidersTab, (show) => {
  if (!show) activeTab.value = 'groups'
})

function toggleProvider(name: string) {
  if (expandedProviders.value.has(name)) {
    expandedProviders.value.delete(name)
  } else {
    expandedProviders.value.add(name)
  }
}

const orderedGroups = computed(() => {
  const order = new Map((getProxy('GLOBAL')?.all ?? []).map((name, index) => [name, index]))
  return proxyGroups.value
    .filter((g) => g.name !== 'GLOBAL')
    .sort((a, b) => (order.get(a.name) ?? Infinity) - (order.get(b.name) ?? Infinity))
})

const searchText = ref('')
const searchInput = ref<HTMLInputElement | null>(null)

function clearSearch() {
  searchText.value = ''
  searchInput.value?.focus()
}

const searchQuery = computed(() => searchText.value.trim().toLowerCase())
const collapsedWhileSearching = ref<Set<string>>(new Set())

watch(searchQuery, () => {
  collapsedWhileSearching.value = new Set()
})

const SORT_BY_LATENCY_KEY = 'singboard-proxy-sort-latency'
const sortByLatency = ref(localStorage.getItem(SORT_BY_LATENCY_KEY) === '1')

watch(sortByLatency, (enabled) => {
  localStorage.setItem(SORT_BY_LATENCY_KEY, enabled ? '1' : '0')
})

function revealCurrent(name: string) {
  const now = getProxy(name)?.now
  const panel = Array.from(document.querySelectorAll<HTMLElement>('[data-group-panel]'))
    .find((element) => element.dataset.groupPanel === name)
  const target = panel && now
    ? Array.from(panel.querySelectorAll<HTMLElement>('[data-node]')).find((element) => element.dataset.node === now)
    : undefined
  if (!target) return
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  target.scrollIntoView({ block: 'nearest', behavior: reduceMotion ? 'auto' : 'smooth' })
}

function latencyRank(name: string): number {
  const delay = getLatency(name)
  if (delay > 0) return delay
  return delay < 0 ? Number.MAX_SAFE_INTEGER - 1 : Number.MAX_SAFE_INTEGER
}

function sortedByLatency(nodes: string[]): string[] {
  return [...nodes].sort((a, b) => latencyRank(a) - latencyRank(b))
}

const frozenOrders = ref<Record<string, string[]>>({})
const hoveredGroup = ref<string | null>(null)

function freezeOrder(name: string) {
  if (frozenOrders.value[name]) return
  const all = getProxy(name)?.all
  if (all) frozenOrders.value[name] = sortedByLatency(all)
}

const focusedGroup = ref<string | null>(null)

function releaseOrder(name: string) {
  if (testingGroups.value.has(name) || hoveredGroup.value === name || focusedGroup.value === name) return
  delete frozenOrders.value[name]
}

function onGroupEnter(name: string) {
  hoveredGroup.value = name
  freezeOrder(name)
}

function onGroupLeave(name: string) {
  if (hoveredGroup.value === name) hoveredGroup.value = null
  releaseOrder(name)
}

function onGroupFocusIn(name: string) {
  focusedGroup.value = name
  freezeOrder(name)
}

function onGroupFocusOut(e: FocusEvent, name: string) {
  if ((e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) return
  if (focusedGroup.value === name) focusedGroup.value = null
  releaseOrder(name)
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return
  if (!isRunning.value || orderedGroups.value.length === 0) return
  e.preventDefault()
  activeTab.value = 'groups'
  void nextTick(() => {
    searchInput.value?.focus()
    searchInput.value?.select()
  })
}

function onSearchEscape(e: KeyboardEvent) {
  if (searchText.value) searchText.value = ''
  else (e.target as HTMLInputElement).blur()
}

function orderedNodes(name: string, all: string[]): string[] {
  if (!sortByLatency.value) return all
  const frozen = frozenOrders.value[name]
  if (!frozen) return sortedByLatency(all)
  const present = new Set(all)
  const known = new Set(frozen)
  return [...frozen.filter((node) => present.has(node)), ...all.filter((node) => !known.has(node))]
}

const groupNodes = computed(() => {
  const query = searchQuery.value
  const map = new Map<string, string[]>()
  for (const group of orderedGroups.value) {
    const nodes = orderedNodes(group.name, group.all)
    map.set(
      group.name,
      query && !group.name.toLowerCase().includes(query)
        ? nodes.filter((name) => name.toLowerCase().includes(query))
        : nodes,
    )
  }
  return map
})

const filteredGroups = computed(() =>
  orderedGroups.value.filter((g) => (groupNodes.value.get(g.name)?.length ?? 0) > 0)
)

function nodesOf(name: string): string[] {
  return groupNodes.value.get(name) ?? []
}

function isExpanded(name: string): boolean {
  return searchQuery.value ? !collapsedWhileSearching.value.has(name) : expandedGroups.value.has(name)
}

function toggleGroup(name: string) {
  if (searchQuery.value) {
    if (collapsedWhileSearching.value.has(name)) collapsedWhileSearching.value.delete(name)
    else collapsedWhileSearching.value.add(name)
    return
  }
  if (expandedGroups.value.has(name)) {
    expandedGroups.value.delete(name)
  } else {
    expandedGroups.value.add(name)
    setTimeout(() => revealCurrent(name), 280)
  }
  localStorage.setItem(EXPANDED_GROUPS_KEY, JSON.stringify([...expandedGroups.value]))
}

const mountedGroups = ref<Set<string>>(new Set())

watchEffect(() => {
  for (const group of filteredGroups.value) {
    if (isExpanded(group.name) && !mountedGroups.value.has(group.name)) mountedGroups.value.add(group.name)
  }
})

function typeFormatter(type: string): string {
  type = type.toLowerCase()
  type = type.replace('shadowsocks', 'ss')
  type = type.replace('hysteria', 'hy')
  type = type.replace('wireguard', 'wg')
  return type
}

function getTypeDescription(nodeName: string): string {
  const proxy = getProxy(nodeName)
  if (!proxy) return ''
  const type = typeFormatter(proxy.type)
  const udpLabel = proxy.udp ? (proxy.xudp ? 'xudp' : 'udp') : ''
  const ipv6Label = config.value.ipv6TestEnabled && isIPv6(nodeName) ? 'IPv6' : ''
  return [type, udpLabel, ipv6Label].filter(Boolean).join(' / ')
}

function isDirectNode(nodeName: string): boolean {
  return getProxy(nodeName)?.type.toLowerCase() === 'direct'
}

function latencyHint(nodeName: string): string {
  const delay = getLatency(nodeName)
  if (delay < 0) return '未测速'
  const age = formatDate(getLatencyTime(nodeName))
  return age ? `${formatLatency(delay)} · ${age}测速` : formatLatency(delay)
}

function getLatencyLabel(nodeName: string): string {
  return formatLatency(getLatency(nodeName))
}

function shouldShowLatencyBadge(nodeName: string): boolean {
  return getLatency(nodeName) > 0 || !isDirectNode(nodeName)
}

function getLatencyBadgeClass(nodeName: string): string {
  return latencyColor(getLatency(nodeName))
}

function isSelectable(group: { type: string }): boolean {
  return group.type.toLowerCase() === 'selector'
}

const dotPreview = ref<{ group: string; name: string } | null>(null)

function previewNode(group: { name: string; now: string }): string {
  const preview = dotPreview.value
  if (!preview || preview.group !== group.name || preview.name === group.now) return ''
  return preview.name
}

function shownNode(group: { name: string; now: string }): string {
  return previewNode(group) || group.now
}

function latencySummary(names: string[]): string {
  let available = 0
  let failed = 0
  for (const name of names) {
    const delay = getLatency(name)
    if (delay > 0) available++
    else if (delay === 0) failed++
  }
  return `${available} 个节点可用，${failed} 个失败，${names.length - available - failed} 个未测`
}

const focusedNodes = ref<Record<string, string>>({})

const nodeTabTargets = computed(() => {
  const targets = new Map<string, string>()
  for (const group of filteredGroups.value) {
    const nodes = nodesOf(group.name)
    const focused = focusedNodes.value[group.name]
    targets.set(group.name, focused && nodes.includes(focused) ? focused : nodes.includes(group.now) ? group.now : nodes[0])
  }
  return targets
})

function onNodeGridKeydown(e: KeyboardEvent) {
  const grid = e.currentTarget as HTMLElement
  const items = Array.from(grid.querySelectorAll<HTMLElement>('[data-node-select]'))
  const index = items.indexOf(e.target as HTMLElement)
  if (index < 0) return
  const columns = getComputedStyle(grid).gridTemplateColumns.split(' ').filter(Boolean).length || 1
  let next: number
  switch (e.key) {
    case 'ArrowRight': next = index + 1; break
    case 'ArrowLeft': next = index - 1; break
    case 'ArrowDown': next = index + columns; break
    case 'ArrowUp': next = index - columns; break
    case 'Home': next = 0; break
    case 'End': next = items.length - 1; break
    default: return
  }
  e.preventDefault()
  items[Math.min(Math.max(next, 0), items.length - 1)].focus()
}

const lastSwitch = ref<{ group: string; from: string } | null>(null)
let undoTimer: ReturnType<typeof setTimeout> | null = null

function clearUndo() {
  if (undoTimer) clearTimeout(undoTimer)
  undoTimer = null
  lastSwitch.value = null
}

function undoSwitch() {
  const previous = lastSwitch.value
  clearUndo()
  if (previous) {
    void handleSelect(previous.group, previous.from, false).then(() => {
      if (getProxy(previous.group)?.now === previous.from) announce(`已撤销，切回 ${previous.from}`)
    })
  }
}

onBeforeUnmount(clearUndo)

async function handleSelect(group: string, name: string, undoable = true) {
  const from = getProxy(group)?.now
  if (from === name) return
  try {
    await switchProxy(group, name)
    clearUndo()
    if (undoable && from) {
      lastSwitch.value = { group, from }
      undoTimer = setTimeout(clearUndo, 8000)
      announce(`${group} 已切换到 ${name}，8 秒内可撤销`)
    }
  } catch (error) {
    pushToast({
      type: 'error',
      message: `切换节点失败\n${group} → ${name}\n原因: ${getRequestErrorReason(error)}`,
    })
  }
}

async function handleTestGroup(name: string) {
  if (testingGroups.value.has(name)) return
  freezeOrder(name)
  testingGroups.value.add(name)
  try {
    await testGroupNodes(name)
  } finally {
    testingGroups.value.delete(name)
    releaseOrder(name)
  }
}

async function handleTestNode(e: Event, nodeName: string, groupName?: string) {
  e.stopPropagation()
  testingNodes.value.add(nodeName)
  const { delay, reason } = await testNodeLatency(nodeName, groupName)
  if (delay === 0) {
    pushToast({
      type: 'error',
      message: `节点测速失败\n${nodeName}\n原因: ${reason}\n测速地址: ${getTestUrl(groupName)}`,
    })
  }
  testingNodes.value.delete(nodeName)
}

async function loadProviders() {
  try {
    const { data } = await fetchProxyProviders()
    proxyProviders.value = Object.values(data.providers).filter(
      (p) => p.name !== 'default' && p.vehicleType !== 'Compatible'
    )
    providersAvailable.value = true
  } catch {
    providersAvailable.value = false
  }
}

async function handleUpdateProvider(name: string) {
  if (updatingProviders.value.has(name)) return
  updatingProviders.value.add(name)
  try {
    await updateProxyProvider(name)
    await loadProviders()
    await loadProxies()
  } catch (error) {
    pushToast({
      type: 'error',
      message: `更新代理提供商失败\n${name}\n原因: ${getRequestErrorReason(error)}`,
    })
  }
  updatingProviders.value.delete(name)
}

async function handleUpdateAll() {
  updatingAll.value = true
  await batchUpdateProviders(proxyProviders.value, updateProxyProvider, '代理提供商')
  await loadProviders()
  await loadProxies()
  updatingAll.value = false
}

async function handleHealthCheck(name: string) {
  if (healthCheckingProviders.value.has(name)) return
  healthCheckingProviders.value.add(name)
  try {
    await healthCheckProvider(name)
    await loadProxies()
    const nodes = proxyProviders.value.find((p) => p.name === name)?.proxies ?? []
    const available = nodes.filter((n) => getLatency(n.name) > 0).length
    pushToast({ type: 'info', message: `${name} 健康检查完成\n可用 ${available} / ${nodes.length} 个节点` })
  } catch (error) {
    pushToast({
      type: 'error',
      message: `健康检查失败\n${name}\n原因: ${getRequestErrorReason(error)}`,
    })
  }
  healthCheckingProviders.value.delete(name)
}

function formatExpire(ts: number): string {
  if (!ts) return '无期限'
  const date = new Date(ts * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function testedCount(proxies: ProxyProvider['proxies']): number {
  return proxies?.filter((n) => getLatency(n.name) >= 0).length ?? 0
}

function formatUsage(info: ProxyProvider['subscriptionInfo']): string {
  if (!info?.Total) return ''
  const used = (info.Download ?? 0) + (info.Upload ?? 0)
  const pct = ((used / info.Total) * 100).toFixed(2)
  return `${formatBytes(used)} / ${formatBytes(info.Total)} (${pct}%)`
}

const REFRESH_INTERVAL_MS = 20_000
const SCROLL_KEY = 'singboard-proxies-scroll'
const groupsPanel = ref<HTMLElement | null>(null)
let refreshTimer: ReturnType<typeof setInterval> | null = null

function refreshInBackground() {
  if (!isRunning.value || !appVisible.value || loading.value) return
  void loadProxies(false, true)
}

onMounted(() => {
  if (isRunning.value) {
    loadProxies()
    loadProviders()
    startConnections()
  }
  refreshTimer = setInterval(refreshInBackground, REFRESH_INTERVAL_MS)
  window.addEventListener('keydown', onGlobalKeydown)
  const savedScroll = Number(sessionStorage.getItem(SCROLL_KEY))
  if (savedScroll > 0) {
    void nextTick(() => {
      if (groupsPanel.value) groupsPanel.value.scrollTop = savedScroll
    })
  }
})

onBeforeUnmount(() => {
  sessionStorage.setItem(SCROLL_KEY, String(groupsPanel.value?.scrollTop ?? 0))
  if (refreshTimer) clearInterval(refreshTimer)
  window.removeEventListener('keydown', onGlobalKeydown)
})

watch(appVisible, (visible) => {
  if (!visible || !isRunning.value) return
  refreshInBackground()
  void loadProviders()
})

watch(
  serviceState,
  (state, previous) => {
    if (state !== 'running' || previous === 'running') return
    loadProxies(previous !== 'unknown')
    loadProviders()
    startConnections()
  },
)

</script>

<template>
  <div class="flex flex-col h-full gap-3">
    <span class="sr-only" aria-live="polite">{{ announcement }}</span>
    <template v-if="!isRunning">
      <h1 class="sr-only">代理</h1>
      <div class="flex flex-col items-center justify-center flex-1 gap-4 px-6" role="status" aria-live="polite">
        <span v-if="!serviceNotice.action" class="loading loading-spinner loading-lg text-base-content/40" aria-hidden="true"></span>
        <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1" stroke="currentColor" class="w-16 h-16 text-base-content/40" aria-hidden="true">
          <path stroke-linecap="round" stroke-linejoin="round" d="M5.636 5.636a9 9 0 1012.728 0M12 3v9" />
        </svg>
        <div class="text-center space-y-1">
          <p class="text-lg font-medium text-base-content/80">{{ serviceNotice.title }}</p>
          <p class="text-sm text-base-content/65">{{ serviceNotice.detail }}</p>
        </div>
        <button
          v-if="serviceNotice.action === 'start'"
          type="button"
          class="btn btn-sm btn-primary px-5"
          :disabled="startingService"
          @click="handleStartService"
        >
          <span v-if="startingService" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          启动服务
        </button>
        <button
          v-else-if="serviceNotice.action === 'settings'"
          type="button"
          class="btn btn-sm btn-glass px-5"
          @click="router.push('/settings')"
        >
          前往设置
        </button>
      </div>
    </template>

    <template v-else>
    <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
      <div class="flex items-center gap-3">
        <h1 class="text-[26px] leading-tight tracking-tight font-bold shrink-0">代理</h1>
        <div class="tabs tabs-boxed tabs-sm" role="tablist" aria-label="代理视图" @keydown="onTabKeydown">
          <button
            id="proxy-tab-groups"
            type="button"
            role="tab"
            class="tab"
            :class="{ 'tab-active': activeTab === 'groups' }"
            :aria-selected="activeTab === 'groups'"
            aria-controls="proxy-panel-groups"
            :tabindex="activeTab === 'groups' ? 0 : -1"
            @click="activeTab = 'groups'"
          >
            代理组 ({{ orderedGroups.length }})
          </button>
          <button
            v-if="showProvidersTab"
            id="proxy-tab-providers"
            type="button"
            role="tab"
            class="tab"
            :class="{ 'tab-active': activeTab === 'providers' }"
            :aria-selected="activeTab === 'providers'"
            aria-controls="proxy-panel-providers"
            :tabindex="activeTab === 'providers' ? 0 : -1"
            @click="activeTab = 'providers'"
          >
            代理提供商 ({{ proxyProviders.length }})
          </button>
        </div>
        <button
          v-if="activeTab === 'providers'"
          type="button"
          class="btn btn-sm btn-ghost btn-circle shrink-0"
          :disabled="updatingAll || proxyProviders.length === 0"
          :aria-busy="updatingAll"
          aria-label="更新全部代理提供商"
          title="更新全部代理提供商"
          @click="handleUpdateAll"
        >
          <span v-if="updatingAll" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4" aria-hidden="true">
            <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
          </svg>
        </button>
      </div>
      <div v-if="activeTab === 'groups' && orderedGroups.length > 0" class="ml-auto flex min-w-0 items-center gap-2">
        <div class="relative flex min-w-0 items-center">
          <input
            ref="searchInput"
            v-model="searchText"
            type="search"
            class="input input-sm input-bordered w-48 min-w-0 pl-8 [&::-webkit-search-cancel-button]:appearance-none"
            :class="{ 'pr-8': searchText }"
            placeholder="搜索节点或代理组"
            aria-label="搜索节点或代理组"
            aria-keyshortcuts="Control+F"
            title="搜索节点或代理组 (Ctrl+F)"
            @keydown.esc="onSearchEscape"
          />
          <svg class="pointer-events-none absolute left-2.5 z-[1] h-3.5 w-3.5 text-base-content/55" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" />
            <path d="m10.5 10.5 3 3" />
          </svg>
          <button
            v-if="searchText"
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
        <button
          type="button"
          class="btn btn-sm btn-ghost btn-circle shrink-0"
          :class="{ 'bg-primary/[0.12] text-primary': sortByLatency }"
          :aria-pressed="sortByLatency"
          aria-label="按延迟排序节点"
          :title="sortByLatency ? '按延迟排序（已开启）' : '按延迟排序'"
          @click="sortByLatency = !sortByLatency"
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 4.5h14.25M3 9h9.75M3 13.5h9.75m4.5-4.5v12m0 0-3.75-3.75M17.25 21 21 17.25" />
          </svg>
        </button>
      </div>
    </div>

    <div
      v-show="activeTab === 'groups'"
      id="proxy-panel-groups"
      ref="groupsPanel"
      role="tabpanel"
      aria-labelledby="proxy-tab-groups"
      class="-mx-2 -mt-1 flex-1 overflow-auto px-2 pb-2 pt-1"
    >
      <div v-if="(loading || (loadError && coreWarmingUp)) && proxyGroups.length === 0" class="flex justify-center py-10" role="status">
        <span class="loading loading-spinner loading-md" aria-hidden="true"></span>
        <span class="sr-only">正在加载代理列表</span>
      </div>

      <div
        v-else-if="loadError && proxyGroups.length === 0"
        class="surface-card mx-auto mt-6 max-w-[520px] px-6 py-5"
        role="alert"
      >
        <p class="text-base font-semibold">无法读取代理列表</p>
        <p class="mt-1 text-sm text-base-content/70">
          请确认 sing-box 配置里开启了 clash_api，并且地址和密钥与当前的 Clash API 配置一致。
        </p>
        <dl class="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
          <dt class="text-base-content/65">地址</dt>
          <dd class="settings-mono break-all">{{ clashApiUrl || '未配置' }}</dd>
          <dt class="text-base-content/65">原因</dt>
          <dd class="break-words">{{ loadError }}</dd>
        </dl>
        <div class="mt-4 flex flex-wrap gap-2">
          <button type="button" class="btn btn-sm btn-primary px-5" @click="loadProxies()">重试</button>
          <button type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">检查 API 配置</button>
        </div>
      </div>

      <div
        v-else-if="loaded && orderedGroups.length === 0"
        class="flex flex-col items-center gap-1 px-6 py-16 text-center"
      >
        <p class="text-base font-medium text-base-content/80">当前配置里没有代理组</p>
        <p class="text-sm text-base-content/65">在 config.json 的 outbounds 中添加 selector 或 urltest 后，这里会列出它们。</p>
      </div>

      <div
        v-else-if="searchQuery && filteredGroups.length === 0"
        class="flex flex-col items-center gap-3 px-6 py-16 text-center"
        role="status"
      >
        <p class="text-base font-medium text-base-content/80">没有找到包含“{{ searchText.trim() }}”的节点或代理组</p>
        <button type="button" class="btn btn-sm btn-glass px-5" @click="clearSearch">清除搜索</button>
      </div>

      <div class="space-y-3">
      <div
        v-for="(group, groupIndex) in filteredGroups"
        :key="group.name"
        class="surface-card glass-hover overflow-hidden"
        @mouseenter="onGroupEnter(group.name)"
        @mouseleave="onGroupLeave(group.name)"
        @focusin="onGroupFocusIn(group.name)"
        @focusout="onGroupFocusOut($event, group.name)"
      >
        <div class="flex cursor-pointer items-start justify-between gap-3 px-5 pt-4 pb-4" @click="toggleGroup(group.name)">
          <div class="flex min-w-0 flex-1 flex-col gap-2">
            <h2 class="flex min-w-0">
              <button
                type="button"
                class="-mx-1 flex min-w-0 items-baseline gap-2 rounded-md px-1 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                :aria-expanded="isExpanded(group.name)"
                :aria-controls="`proxy-group-${groupIndex}`"
              >
                <span class="truncate font-semibold text-base">{{ group.name }}</span>
                <span class="shrink-0 text-xs text-base-content/65">
                  {{ group.type }} ({{ searchQuery ? `${nodesOf(group.name).length}/${group.all.length}` : group.all.length }})<template v-if="!isSelectable(group)"> · 自动</template>
                </span>
                <svg
                  class="h-3.5 w-3.5 shrink-0 self-center text-base-content/50 transition-transform duration-200"
                  :class="{ 'rotate-90': isExpanded(group.name) }"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <path d="M6 3.5 10.5 8 6 12.5" />
                </svg>
              </button>
            </h2>
            <div class="flex min-w-0 items-center gap-1.5 text-sm text-base-content/70">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor" class="w-3.5 h-3.5 shrink-0" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" />
              </svg>
              <template v-if="previewNode(group)">
                <span v-if="isSelectable(group)" class="shrink-0 text-[color:var(--selection-ink)]">点击切换到</span>
                <span class="truncate" :class="{ 'text-[color:var(--selection-ink)]': isSelectable(group) }">{{ previewNode(group) }}</span>
              </template>
              <span v-else class="truncate">{{ group.now }}</span>
              <span
                v-if="shownNode(group) && shouldShowLatencyBadge(shownNode(group))"
                class="shrink-0 text-xs leading-none px-2 py-0.5 rounded-full tabular-nums"
                :class="getLatencyBadgeClass(shownNode(group))"
                :title="latencyHint(shownNode(group))"
              >
                {{ getLatencyLabel(shownNode(group)) }}
              </span>
              <button
                v-if="lastSwitch?.group === group.name && !previewNode(group)"
                type="button"
                class="btn btn-xs btn-ghost h-5 min-h-0 shrink-0 px-2 text-[color:var(--selection-ink)]"
                @click.stop="undoSwitch"
              >
                撤销
              </button>
            </div>
            <span class="sr-only">{{ latencySummary(group.all) }}</span>
            <div
              class="proxy-dots -mx-1 -mb-1 -mt-2 cursor-default"
              :class="{ 'is-open': !isExpanded(group.name) }"
              aria-hidden="true"
              @click.stop
              @mouseleave="dotPreview = null"
            >
              <div class="proxy-collapse-inner">
                <div class="flex flex-wrap gap-1.5 px-1 pb-1 pt-2">
                  <span
                    v-for="(nodeName, index) in nodesOf(group.name)"
                    :key="nodeName"
                    class="proxy-dot w-3.5 h-3.5 rounded-full flex items-center justify-center"
                    :class="[
                      dotColor(getLatency(nodeName)),
                      isSelectable(group) ? 'cursor-pointer hover:scale-125 hover:ring-1 hover:ring-base-content/30' : '',
                      group.now === nodeName && getLatency(nodeName) === 0 ? 'outline outline-2 outline-offset-1 outline-base-content/60' : '',
                    ]"
                    :style="{ '--i': index }"
                    :title="`${nodeName}: ${latencyHint(nodeName)}`"
                    @mouseenter="isSelectable(group) && (dotPreview = { group: group.name, name: nodeName })"
                    @click="isSelectable(group) && handleSelect(group.name, nodeName)"
                  >
                    <span
                      v-if="group.now === nodeName && getLatency(nodeName) !== 0"
                      class="w-1.5 h-1.5 rounded-full"
                      :class="getLatency(nodeName) < 0 ? 'bg-base-content/60' : 'bg-white'"
                    ></span>
                    <svg
                      v-else-if="getLatency(nodeName) === 0"
                      class="h-2 w-2 text-white"
                      viewBox="0 0 8 8"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.5"
                      stroke-linecap="round"
                      aria-hidden="true"
                    >
                      <path d="M2 2l4 4M6 2 2 6" />
                    </svg>
                  </span>
                </div>
              </div>
            </div>
          </div>
          <div class="flex flex-col items-end gap-0.5 shrink-0">
            <button
              type="button"
              class="btn btn-xs btn-ghost min-w-0 px-2 h-6 min-h-0 disabled:bg-transparent"
              :disabled="testingGroups.has(group.name)"
              :aria-busy="testingGroups.has(group.name)"
              :aria-label="`延迟测试：${group.name} 全部节点`"
              @click.stop="handleTestGroup(group.name)"
              title="测试代理组节点延迟"
            >
              <span v-if="testingGroups.has(group.name)" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
              {{ testingGroups.has(group.name) ? '测速中' : '延迟测试' }}
            </button>
            <GroupSpeed :name="group.name" />
          </div>
        </div>

        <div :id="`proxy-group-${groupIndex}`" class="proxy-collapse" :class="{ 'is-open': isExpanded(group.name) }">
          <div class="proxy-collapse-inner">
          <div
            v-if="mountedGroups.has(group.name)"
            class="grid gap-2 px-5 pt-1 pb-4"
            style="grid-template-columns: repeat(auto-fill, minmax(180px, 1fr))"
            :role="isSelectable(group) ? 'listbox' : 'group'"
            :aria-label="`${group.name} 的节点`"
            :data-group-panel="group.name"
            @keydown="onNodeGridKeydown"
          >
            <div
              v-for="nodeName in nodesOf(group.name)"
              :key="nodeName"
              :data-node="nodeName"
              class="node-tile relative flex h-[76px] min-w-[180px] flex-col items-start gap-1 p-2.5 rounded-[var(--radius-tile)] text-xs transition-all duration-150 overflow-hidden"
              :class="[
                group.now === nodeName
                  ? 'bg-primary/15 text-[color:var(--selection-ink)] ring-1 ring-inset ring-primary/40'
                  : isSelectable(group) ? 'surface-fill surface-fill-hover' : 'surface-fill',
                isSelectable(group) ? 'cursor-pointer [&:has(>[data-node-select]:active)]:scale-[0.98]' : '',
              ]"
            >
              <button
                v-if="isSelectable(group)"
                type="button"
                data-node-select
                class="absolute inset-0 rounded-[inherit] focus-visible:outline-none"
                :tabindex="nodeTabTargets.get(group.name) === nodeName ? 0 : -1"
                role="option"
                :aria-selected="group.now === nodeName"
                :aria-label="`${nodeName}，${formatLatency(getLatency(nodeName))}`"
                :title="nodeName"
                @focus="focusedNodes[group.name] = nodeName"
                @click="handleSelect(group.name, nodeName)"
              ></button>
              <div class="w-full min-h-0 flex-1 line-clamp-2 break-words text-sm leading-tight" :title="isSelectable(group) ? undefined : nodeName">
                {{ nodeName }}
              </div>
              <div class="flex h-4 w-full shrink-0 items-center justify-between gap-2">
                <span
                  class="truncate text-xs tracking-tight"
                  :class="group.now === nodeName ? 'text-[color:var(--selection-ink)]' : 'text-base-content/65'"
                >{{ getTypeDescription(nodeName) }}</span>
                <button
                  v-if="shouldShowLatencyBadge(nodeName)"
                  type="button"
                  tabindex="-1"
                  class="relative z-[1] shrink-0 cursor-pointer text-xs leading-none px-2 py-0.5 rounded-full tabular-nums"
                  :class="getLatencyBadgeClass(nodeName)"
                  :aria-busy="testingNodes.has(nodeName)"
                  :aria-label="`${getLatencyLabel(nodeName)}，重新测试 ${nodeName} 延迟`"
                  @click="handleTestNode($event, nodeName, group.name)"
                  :title="`${latencyHint(nodeName)}，点击重新测速`"
                >
                  <span :class="{ invisible: testingNodes.has(nodeName) }">{{ getLatencyLabel(nodeName) }}</span>
                  <span v-if="testingNodes.has(nodeName)" class="loading loading-spinner absolute inset-0 m-auto h-2.5 w-2.5" aria-hidden="true"></span>
                </button>
              </div>
            </div>
          </div>
          </div>
        </div>
      </div>
      </div>
    </div>

    <div
      v-show="activeTab === 'providers'"
      id="proxy-panel-providers"
      role="tabpanel"
      aria-labelledby="proxy-tab-providers"
      class="-mx-2 -mt-1 flex-1 overflow-auto space-y-3 px-2 pb-2 pt-1"
    >
      <div v-if="proxyProviders.length === 0" class="flex items-center justify-center py-10 text-base-content/65">
        暂无代理提供商
      </div>

      <div
        v-for="(provider, providerIndex) in proxyProviders"
        :key="provider.name"
        class="surface-card glass-hover overflow-hidden"
      >
        <div class="flex cursor-pointer items-start justify-between gap-3 px-5 pt-4 pb-4" @click="toggleProvider(provider.name)">
          <div class="flex min-w-0 flex-1 flex-col gap-2">
            <h2 class="flex min-w-0">
              <button
                type="button"
                class="-mx-1 flex min-w-0 items-baseline gap-2 rounded-md px-1 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                :aria-expanded="expandedProviders.has(provider.name)"
                :aria-controls="`proxy-provider-${providerIndex}`"
              >
                <span class="truncate font-semibold text-base">{{ provider.name }}</span>
                <span class="shrink-0 text-xs text-base-content/65">{{ provider.proxies?.length ?? 0 }} 个节点 ({{ testedCount(provider.proxies) }} 已测试)</span>
                <svg
                  class="h-3.5 w-3.5 shrink-0 self-center text-base-content/50 transition-transform duration-200"
                  :class="{ 'rotate-90': expandedProviders.has(provider.name) }"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <path d="M6 3.5 10.5 8 6 12.5" />
                </svg>
              </button>
            </h2>
            <div v-if="provider.subscriptionInfo?.Total" class="text-xs text-base-content/65 space-y-0.5 tabular-nums">
              <div v-if="provider.subscriptionInfo!.Expire !== undefined">到期: {{ formatExpire(provider.subscriptionInfo!.Expire) }}</div>
              <div>{{ formatUsage(provider.subscriptionInfo) }}</div>
            </div>
            <div class="text-xs text-base-content/65">更新于 {{ formatDate(provider.updatedAt) }}</div>
            <span class="sr-only">{{ latencySummary((provider.proxies ?? []).map((n) => n.name)) }}</span>
            <div class="proxy-dots -mx-1 -mb-1 -mt-2" :class="{ 'is-open': !expandedProviders.has(provider.name) }" aria-hidden="true">
              <div class="proxy-collapse-inner">
                <div class="flex flex-wrap gap-1.5 px-1 pb-1 pt-2">
                  <span
                    v-for="(node, index) in provider.proxies"
                    :key="node.name"
                    class="proxy-dot w-3.5 h-3.5 rounded-full flex items-center justify-center"
                    :class="dotColor(getLatency(node.name))"
                    :style="{ '--i': index }"
                    :title="`${node.name}: ${latencyHint(node.name)}`"
                  >
                    <svg
                      v-if="getLatency(node.name) === 0"
                      class="h-2 w-2 text-white"
                      viewBox="0 0 8 8"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.5"
                      stroke-linecap="round"
                      aria-hidden="true"
                    >
                      <path d="M2 2l4 4M6 2 2 6" />
                    </svg>
                  </span>
                </div>
              </div>
            </div>
          </div>
          <div class="flex items-center gap-1 shrink-0">
            <button
              type="button"
              class="btn btn-ghost btn-xs btn-circle disabled:bg-transparent"
              :class="{ 'loading': healthCheckingProviders.has(provider.name) }"
              :disabled="healthCheckingProviders.has(provider.name)"
              :aria-busy="healthCheckingProviders.has(provider.name)"
              :aria-label="`对 ${provider.name} 做健康检查`"
              @click.stop="handleHealthCheck(provider.name)"
              title="健康检查"
            >
              <svg v-if="!healthCheckingProviders.has(provider.name)" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
              </svg>
            </button>
            <button
              v-if="provider.vehicleType !== 'Inline'"
              type="button"
              class="btn btn-ghost btn-xs btn-circle disabled:bg-transparent"
              :class="{ 'loading': updatingProviders.has(provider.name) }"
              :disabled="updatingProviders.has(provider.name)"
              :aria-busy="updatingProviders.has(provider.name)"
              :aria-label="`更新 ${provider.name}`"
              @click.stop="handleUpdateProvider(provider.name)"
              title="更新"
            >
              <svg v-if="!updatingProviders.has(provider.name)" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
              </svg>
            </button>
          </div>
        </div>

        <div :id="`proxy-provider-${providerIndex}`" class="proxy-collapse" :class="{ 'is-open': expandedProviders.has(provider.name) }">
          <div class="proxy-collapse-inner">
          <div class="grid gap-2 px-5 pb-4" style="grid-template-columns: repeat(auto-fill, minmax(180px, 1fr))">
            <div
              v-for="node in provider.proxies"
              :key="node.name"
              class="surface-fill flex h-[76px] min-w-[180px] flex-col items-start gap-1 p-2.5 rounded-[var(--radius-tile)] text-xs overflow-hidden"
            >
              <div class="w-full min-h-0 flex-1 line-clamp-2 break-words text-sm leading-tight" :title="node.name">
                {{ node.name }}
              </div>
              <div class="flex h-4 w-full shrink-0 items-center justify-between gap-2">
                <span class="truncate text-xs tracking-tight text-base-content/60">
                  {{ getTypeDescription(node.name) }}
                </span>
                <button
                  v-if="shouldShowLatencyBadge(node.name)"
                  type="button"
                  class="relative shrink-0 cursor-pointer text-xs leading-none px-2 py-0.5 rounded-full tabular-nums"
                  :class="getLatencyBadgeClass(node.name)"
                  :aria-busy="testingNodes.has(node.name)"
                  :aria-label="`${getLatencyLabel(node.name)}，重新测试 ${node.name} 延迟`"
                  @click.stop="handleTestNode($event, node.name, provider.name)"
                  :title="`${latencyHint(node.name)}，点击重新测速`"
                >
                  <span :class="{ invisible: testingNodes.has(node.name) }">{{ getLatencyLabel(node.name) }}</span>
                  <span v-if="testingNodes.has(node.name)" class="loading loading-spinner absolute inset-0 m-auto h-2.5 w-2.5" aria-hidden="true"></span>
                </button>
              </div>
            </div>
          </div>
          </div>
        </div>
      </div>
    </div>
    </template>
  </div>
</template>

<style scoped>
.node-tile:has(> [data-node-select]:focus-visible) {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}

.proxy-collapse {
  display: grid;
  grid-template-rows: 0fr;
  opacity: 0;
  visibility: hidden;
  transition:
    grid-template-rows 260ms cubic-bezier(0.32, 0.72, 0, 1),
    opacity 180ms ease,
    visibility 0s linear 260ms;
}

.proxy-collapse.is-open {
  grid-template-rows: 1fr;
  opacity: 1;
  visibility: visible;
  transition:
    grid-template-rows 260ms cubic-bezier(0.32, 0.72, 0, 1),
    opacity 220ms ease 40ms,
    visibility 0s;
}

.proxy-collapse-inner {
  min-height: 0;
  overflow: hidden;
}

.proxy-dots {
  display: grid;
  grid-template-rows: 0fr;
  visibility: hidden;
  transition:
    grid-template-rows 260ms cubic-bezier(0.32, 0.72, 0, 1),
    visibility 0s linear 260ms;
}

.proxy-dots.is-open {
  grid-template-rows: 1fr;
  visibility: visible;
  transition:
    grid-template-rows 260ms cubic-bezier(0.32, 0.72, 0, 1),
    visibility 0s;
}

.proxy-dot {
  opacity: 0;
  scale: 0.3;
  transition:
    opacity 100ms ease-out,
    scale 120ms ease-out,
    transform 150ms cubic-bezier(0.4, 0, 0.2, 1);
}

.proxy-dots.is-open .proxy-dot {
  --delay: calc(60ms + min(var(--i), 30) * 12ms);
  opacity: 1;
  scale: 1;
  transition:
    opacity 200ms ease-out var(--delay),
    scale 340ms cubic-bezier(0.34, 1.56, 0.64, 1) var(--delay),
    transform 150ms cubic-bezier(0.4, 0, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .proxy-collapse,
  .proxy-collapse.is-open,
  .proxy-dots,
  .proxy-dots.is-open,
  .proxy-dot,
  .proxy-dots.is-open .proxy-dot {
    transition: none;
  }
}
</style>
