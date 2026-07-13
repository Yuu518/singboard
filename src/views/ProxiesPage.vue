<script setup lang="ts">
import { onMounted, ref, computed, watch } from 'vue'
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
import { formatSpeed, formatBytes, formatLatency, formatDate, latencyColor, dotColor } from '@/utils/format'
import { batchUpdateProviders } from '@/utils/batchUpdate'

const {
  proxyGroups,
  loading,
  loadProxies,
  switchProxy,
  testGroupNodes,
  testNodeLatency,
  getProxy,
  getLatency,
  getTestUrl,
  isIPv6,
} = useProxiesStore()

import { useConfigStore } from '@/stores/config'
import { useConnectionsStore } from '@/stores/connections'
const { config } = useConfigStore()
const { pushToast } = useToastStore()
const { serviceStatus } = useServiceStore()
const { connections, start: startConnections } = useConnectionsStore()

const groupSpeedMap = computed(() => {
  const map: Record<string, { down: number; up: number }> = {}
  for (const conn of connections.value) {
    if (!conn.chains) continue
    for (const chain of conn.chains) {
      if (!map[chain]) map[chain] = { down: 0, up: 0 }
      map[chain].down += conn.downloadSpeed ?? 0
      map[chain].up += conn.uploadSpeed ?? 0
    }
  }
  return map
})

function getGroupSpeed(groupName: string): { down: number; up: number } {
  return groupSpeedMap.value[groupName] ?? { down: 0, up: 0 }
}

const isRunning = computed(() => serviceStatus.value.state === 'running')

const activeTab = ref<'groups' | 'providers'>('groups')

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
const testingGroup = ref<string | null>(null)
const testingNodes = ref<Set<string>>(new Set())

const proxyProviders = ref<ProxyProvider[]>([])
const providersAvailable = ref(false)
const updatingProvider = ref<string | null>(null)
const updatingAll = ref(false)
const healthCheckingProvider = ref<string | null>(null)
const expandedProviders = ref<Set<string>>(new Set())

function toggleProvider(name: string) {
  if (expandedProviders.value.has(name)) {
    expandedProviders.value.delete(name)
  } else {
    expandedProviders.value.add(name)
  }
}

const filteredGroups = computed(() =>
  proxyGroups.value.filter((g) => g.name !== 'GLOBAL')
)

function toggleGroup(name: string) {
  if (expandedGroups.value.has(name)) {
    expandedGroups.value.delete(name)
  } else {
    expandedGroups.value.add(name)
  }
  localStorage.setItem(EXPANDED_GROUPS_KEY, JSON.stringify([...expandedGroups.value]))
}

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

function getLatencyLabel(nodeName: string): string {
  const delay = getLatency(nodeName)
  if (delay > 0) return formatLatency(delay)
  return isDirectNode(nodeName) ? '' : formatLatency(delay)
}

function shouldShowLatencyBadge(nodeName: string): boolean {
  return getLatency(nodeName) > 0 || !isDirectNode(nodeName)
}

function getLatencyBadgeClass(nodeName: string): string {
  if (!shouldShowLatencyBadge(nodeName)) {
    return 'h-4 w-4 bg-transparent p-0'
  }
  return latencyColor(getLatency(nodeName))
}

async function handleSelect(group: string, name: string) {
  await switchProxy(group, name)
}

async function handleTestGroup(name: string) {
  testingGroup.value = name
  await testGroupNodes(name)
  testingGroup.value = null
}

async function handleTestNode(e: Event, nodeName: string, groupName?: string) {
  e.stopPropagation()
  testingNodes.value.add(nodeName)
  const delay = await testNodeLatency(nodeName, groupName)
  if (delay === 0) {
    pushToast({
      type: 'error',
      message: `${nodeName}\n@${getTestUrl(groupName)}\n测速超时`,
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
  updatingProvider.value = name
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
  updatingProvider.value = null
}

async function handleUpdateAll() {
  updatingAll.value = true
  await batchUpdateProviders(proxyProviders.value, updateProxyProvider, '代理提供商')
  await loadProviders()
  await loadProxies()
  updatingAll.value = false
}

async function handleHealthCheck(name: string) {
  healthCheckingProvider.value = name
  try {
    await healthCheckProvider(name)
    await loadProxies()
  } catch {}
  healthCheckingProvider.value = null
}

function formatExpire(ts: number): string {
  if (!ts) return '无期限'
  return new Date(ts * 1000).toLocaleDateString()
}

function testedCount(proxies: ProxyProvider['proxies']): number {
  return proxies?.filter((n) => getLatency(n.name) > 0).length ?? 0
}

function formatUsage(info: ProxyProvider['subscriptionInfo']): string {
  if (!info?.Total) return ''
  const used = (info.Download ?? 0) + (info.Upload ?? 0)
  const pct = ((used / info.Total) * 100).toFixed(2)
  return `${formatBytes(used)} / ${formatBytes(info.Total)} ( ${pct}% )`
}

onMounted(() => {
  if (isRunning.value) {
    loadProxies()
    loadProviders()
    startConnections()
  }
})

watch(isRunning, (running) => {
  if (running) {
    loadProxies(true)
    loadProviders()
    startConnections()
  }
})

</script>

<template>
  <div class="flex flex-col h-full gap-3">
    <template v-if="!isRunning">
      <div class="flex flex-col items-center justify-center flex-1 gap-4 text-base-content/40">
        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1" stroke="currentColor" class="w-16 h-16">
          <path stroke-linecap="round" stroke-linejoin="round" d="M5.636 5.636a9 9 0 1012.728 0M12 3v9" />
        </svg>
        <div class="text-center space-y-1">
          <p class="text-lg font-medium">服务未启动</p>
          <p class="text-sm">请先启动 sing-box 服务以查看代理信息</p>
        </div>
      </div>
    </template>

    <template v-else>
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-3">
        <h1 class="text-xl font-bold shrink-0">代理</h1>
        <div class="tabs tabs-boxed tabs-sm">
          <a class="tab" :class="{ 'tab-active': activeTab === 'groups' }" @click="activeTab = 'groups'">
            代理 ({{ filteredGroups.length }})
          </a>
          <a
            v-if="providersAvailable"
            class="tab"
            :class="{ 'tab-active': activeTab === 'providers' }"
            @click="activeTab = 'providers'"
          >
            代理提供商 ({{ proxyProviders.length }})
          </a>
        </div>
      </div>
    </div>

    <div
      v-show="activeTab === 'groups'"
      class="flex-1 overflow-auto"
    >
      <div v-if="loading && proxyGroups.length === 0" class="flex justify-center py-10">
        <span class="loading loading-spinner loading-md"></span>
      </div>

      <div class="space-y-3">
      <div
        v-for="group in filteredGroups"
        :key="group.name"
        class="bg-base-200 rounded-lg overflow-hidden cursor-pointer hover:bg-base-300/30 transition-colors"
        @click="toggleGroup(group.name)"
      >
        <div class="flex items-start justify-between px-5 pt-4 pb-4">
          <div class="flex flex-col gap-2">
            <div class="flex items-baseline gap-2">
              <span class="font-semibold text-base">{{ group.name }}</span>
              <span class="text-xs text-base-content/40">{{ group.type }} ({{ group.all.length }})</span>
            </div>
            <div class="flex items-center gap-1.5 text-sm text-base-content/70">
              <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor" class="w-3.5 h-3.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" />
              </svg>
              <span class="truncate">{{ group.now }}</span>
              <span
                v-if="group.now && shouldShowLatencyBadge(group.now)"
                class="shrink-0 text-xs leading-none px-1.5 py-0.5 rounded"
                :class="getLatencyBadgeClass(group.now)"
              >
                {{ getLatencyLabel(group.now) }}
              </span>
            </div>
            <div v-show="!expandedGroups.has(group.name)" class="flex flex-wrap gap-1.5">
              <span
                v-for="nodeName in group.all"
                :key="nodeName"
                class="w-3.5 h-3.5 rounded-full cursor-pointer transition-transform hover:scale-125 hover:ring-1 hover:ring-base-content/30 flex items-center justify-center"
                :class="dotColor(getLatency(nodeName))"
                :title="nodeName + ': ' + formatLatency(getLatency(nodeName))"
                @click.stop="handleSelect(group.name, nodeName)"
              >
                <span v-if="group.now === nodeName" class="w-1.5 h-1.5 rounded-full bg-white"></span>
              </span>
            </div>
          </div>
          <div class="flex flex-col items-end gap-0.5 shrink-0">
            <button
              class="btn btn-xs btn-ghost min-w-0 px-2 h-6 min-h-0"
              :class="{ 'loading loading-xs': testingGroup === group.name }"
              @click.stop="handleTestGroup(group.name)"
              title="测试代理组节点延迟"
            >
              <template v-if="testingGroup !== group.name">延迟测试</template>
            </button>
            <span class="text-xs text-base-content/40 pr-1">↓{{ formatSpeed(getGroupSpeed(group.name).down) }}</span>
            <span class="text-xs text-base-content/40 pr-1">↑{{ formatSpeed(getGroupSpeed(group.name).up) }}</span>
          </div>
        </div>

        <div v-show="expandedGroups.has(group.name)" class="px-5 pb-4">
          <div class="grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(180px, 1fr))">
            <div
              v-for="nodeName in group.all"
              :key="nodeName"
              class="flex h-[70px] min-w-[180px] flex-col items-start gap-2 p-2 rounded-md text-xs transition-all cursor-pointer overflow-hidden"
              :class="
                group.now === nodeName
                  ? 'bg-primary/15 text-primary ring-1 ring-primary/30'
                  : 'bg-base-100 hover:bg-base-300'
              "
              @click.stop="handleSelect(group.name, nodeName)"
            >
              <div class="w-full flex-1 text-sm leading-tight break-all" :title="nodeName">
                {{ nodeName }}
              </div>
              <div class="flex h-4 w-full items-center justify-between">
                <span
                  class="truncate text-xs tracking-tight"
                  :class="group.now === nodeName ? 'text-primary/70' : 'text-base-content/60'"
                >{{ getTypeDescription(nodeName) }}</span>
                <button
                  class="shrink-0 cursor-pointer text-xs leading-none px-1.5 py-0.5 rounded"
                  :class="[getLatencyBadgeClass(nodeName), { 'loading loading-xs': testingNodes.has(nodeName) }]"
                  @click="handleTestNode($event, nodeName, group.name)"
                  title="点击测速"
                >
                  <template v-if="!testingNodes.has(nodeName)">{{ getLatencyLabel(nodeName) }}</template>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
      </div>
    </div>

    <div v-show="activeTab === 'providers'" class="flex-1 overflow-auto space-y-3">
      <div v-if="proxyProviders.length === 0" class="flex items-center justify-center py-10 text-base-content/40">
        暂无代理提供商
      </div>

      <div v-if="proxyProviders.length > 0" class="flex justify-end">
        <button
          class="btn btn-sm btn-ghost"
          :class="{ 'loading': updatingAll }"
          @click="handleUpdateAll"
          :disabled="updatingAll"
        >
          <template v-if="!updatingAll">全部更新</template>
        </button>
      </div>

      <div
        v-for="provider in proxyProviders"
        :key="provider.name"
        class="bg-base-200 rounded-lg overflow-hidden cursor-pointer hover:bg-base-300/30 transition-colors"
        @click="toggleProvider(provider.name)"
      >
        <div class="flex items-start justify-between px-5 pt-4 pb-4">
          <div class="flex flex-col gap-2">
            <div class="flex items-baseline gap-2">
              <span class="font-semibold text-base">{{ provider.name }}</span>
              <span class="text-xs text-base-content/40">{{ provider.proxies?.length ?? 0 }} 个节点 ({{ testedCount(provider.proxies) }} 已测试)</span>
            </div>
            <div v-if="provider.subscriptionInfo?.Total" class="text-xs text-base-content/50 space-y-0.5">
              <div v-if="provider.subscriptionInfo!.Expire">到期: {{ formatExpire(provider.subscriptionInfo!.Expire!) }}</div>
              <div>{{ formatUsage(provider.subscriptionInfo) }}</div>
            </div>
            <div class="text-xs text-base-content/40">更新于 {{ formatDate(provider.updatedAt) }}</div>
            <div v-show="!expandedProviders.has(provider.name)" class="flex flex-wrap gap-1.5">
              <span
                v-for="node in provider.proxies"
                :key="node.name"
                class="w-3.5 h-3.5 rounded-full"
                :class="dotColor(getLatency(node.name))"
                :title="node.name + ': ' + formatLatency(getLatency(node.name))"
              ></span>
            </div>
          </div>
          <div class="flex items-center gap-1 shrink-0">
            <button
              class="btn btn-ghost btn-xs btn-circle"
              :class="{ 'loading': healthCheckingProvider === provider.name }"
              @click.stop="handleHealthCheck(provider.name)"
              title="健康检查"
            >
              <svg v-if="healthCheckingProvider !== provider.name" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
              </svg>
            </button>
            <button
              v-if="provider.vehicleType !== 'Inline'"
              class="btn btn-ghost btn-xs btn-circle"
              :class="{ 'loading': updatingProvider === provider.name }"
              @click.stop="handleUpdateProvider(provider.name)"
              title="更新"
            >
              <svg v-if="updatingProvider !== provider.name" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
              </svg>
            </button>
          </div>
        </div>

        <div v-show="expandedProviders.has(provider.name)" class="px-5 pb-4">
          <div class="grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(180px, 1fr))">
            <div
              v-for="node in provider.proxies"
              :key="node.name"
              class="bg-base-100 flex h-[70px] min-w-[180px] flex-col items-start gap-2 p-2 rounded-md text-xs overflow-hidden"
            >
              <div class="w-full flex-1 text-sm leading-tight break-all" :title="node.name">
                {{ node.name }}
              </div>
              <div class="flex h-4 w-full items-center justify-between">
                <span class="truncate text-xs tracking-tight text-base-content/60">
                  {{ getTypeDescription(node.name) }}
                </span>
                <button
                  class="shrink-0 cursor-pointer text-xs leading-none px-1.5 py-0.5 rounded"
                  :class="[getLatencyBadgeClass(node.name), { 'loading loading-xs': testingNodes.has(node.name) }]"
                  @click.stop="handleTestNode($event, node.name, provider.name)"
                  title="点击测速"
                >
                  <template v-if="!testingNodes.has(node.name)">{{ getLatencyLabel(node.name) }}</template>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    </template>
  </div>
</template>
