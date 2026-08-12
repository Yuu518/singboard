<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'

import { useRulesStore } from '@/stores/rules'
import { fetchRuleProviders, updateRuleProvider } from '@/api'
import type { RuleProvider } from '@/types'
import { getRequestErrorReason } from '@/utils/requestError'
import { useToastStore } from '@/stores/toast'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useProxiesStore } from '@/stores/proxies'
import { srsMatchProvider, srsListProvider, getRunningConfigPath } from '@/bridge/config'
import { formatDate } from '@/utils/format'
import { batchUpdateProviders } from '@/utils/batchUpdate'

const { filteredRules, loading, filterText, loadRules } = useRulesStore()
const { serviceStatus } = useServiceStore()
const { proxyMap } = useProxiesStore()

const SPECIAL_ACTIONS = new Set(['sniff', 'hijack-dns', 'resolve', 'resolve(match_only)'])

function resolveProxyChain(name: string): string[] {
  const chain: string[] = [name]
  const visited = new Set<string>()
  let current = name
  while (true) {
    if (visited.has(current)) break
    visited.add(current)
    const proxy = proxyMap.value[current]
    if (!proxy?.now || proxy.now === current) break
    current = proxy.now
    chain.push(current)
  }
  return chain
}

function actionColor(name: string): string {
  const lower = name.toLowerCase()
  if (lower.includes('reject')) return 'bg-error/15 text-error'
  if (lower === 'direct') return 'bg-success/15 text-success'
  if (SPECIAL_ACTIONS.has(lower)) return 'bg-base-content/10 text-base-content/60'
  return 'bg-primary/15 text-primary'
}

function canOpenProvider(provider: RuleProvider): boolean {
  return provider.vehicleType !== 'Inline' && provider.behavior.toLowerCase() !== 'source'
}
const isRunning = computed(() => serviceStatus.value.state === 'running')

const activeTab = ref<'rules' | 'providers'>('rules')

const ruleProviders = ref<RuleProvider[]>([])
const providersAvailable = ref(false)
const updatingProvider = ref<string | null>(null)
const updatingAll = ref(false)
const { pushToast } = useToastStore()
const { config } = useConfigStore()

const providerSearchText = ref('')
const providerSearching = ref(false)
// undefined = not searched, -1 = error/not found, false = no match, true = matched
const providerMatchCounts = ref<Record<string, boolean | -1>>({})
const providerSearchDone = ref(false)
let searchTimer: ReturnType<typeof setTimeout> | null = null

const displayedProviders = computed(() => {
  const q = providerSearchText.value.trim()
  if (!q) return ruleProviders.value
  if (!providerSearchDone.value) return ruleProviders.value
  return ruleProviders.value.filter((p) => providerMatchCounts.value[p.name] === true)
})

async function searchInProviders() {
  const q = providerSearchText.value.trim()
  if (!q) {
    providerMatchCounts.value = {}
    providerSearchDone.value = false
    return
  }
  providerSearching.value = true
  providerSearchDone.value = false

  let configPath = ''
  try {
    configPath = await getRunningConfigPath()
  } catch { }

  const results: Record<string, boolean | -1> = {}
  await Promise.allSettled(
    ruleProviders.value.map(async (p) => {
      try {
        results[p.name] = await srsMatchProvider(
          config.value.workingDir ?? '',
          configPath,
          config.value.singboxPath ?? '',
          p.name,
          q,
        )
      } catch (e) {
        console.error(`[srs-search] ${p.name}:`, e)
        results[p.name] = -1
      }
    }),
  )
  providerMatchCounts.value = results
  providerSearchDone.value = true
  providerSearching.value = false
}

watch(providerSearchText, (val) => {
  if (searchTimer) clearTimeout(searchTimer)
  if (!val.trim()) {
    providerMatchCounts.value = {}
    providerSearchDone.value = false
    return
  }
  searchTimer = setTimeout(searchInProviders, 500)
})

async function loadProviders() {
  try {
    const { data } = await fetchRuleProviders()
    ruleProviders.value = Object.values(data.providers)
    providersAvailable.value = true
  } catch {
    providersAvailable.value = false
  }
}

async function handleUpdateProvider(name: string) {
  updatingProvider.value = name
  try {
    await updateRuleProvider(name)
    await loadProviders()
    await loadRules()
  } catch (error) {
    pushToast({
      type: 'error',
      message: `更新规则提供商失败\n${name}\n原因: ${getRequestErrorReason(error)}`,
    })
  }
  updatingProvider.value = null
}

async function handleUpdateAll() {
  updatingAll.value = true
  await batchUpdateProviders(ruleProviders.value, updateRuleProvider, '规则提供商')
  await loadProviders()
  await loadRules()
  updatingAll.value = false
}

// ---- 规则详情弹窗 ----
const detailProvider = ref<RuleProvider | null>(null)
const detailLoading = ref(false)
const detailError = ref('')
const detailRules = ref<Array<{ type: string; value: string }>>([])
const detailFilterText = ref('')

// 弹窗搜索：调用 Rust 端 srsMatchProvider 做精确匹配
const detailMatchResult = ref<boolean | null>(null) // null=未搜索, true=匹配, false=未匹配
const detailMatchSearching = ref(false)
let detailSearchTimer: ReturnType<typeof setTimeout> | null = null

const filteredDetailRules = ref<Array<{ type: string; value: string }>>([])
let filterTimer: ReturnType<typeof setTimeout> | null = null

// 地址区间（闭区间），v 标识地址族
type IpRange = { v: 4 | 6; from: bigint; to: bigint }

function parseIPv4(ip: string): bigint | null {
  const parts = ip.split('.')
  if (parts.length !== 4) return null
  let n = 0n
  for (const p of parts) {
    if (!/^\d{1,3}$/.test(p)) return null
    const v = Number(p)
    if (v > 255) return null
    n = (n << 8n) | BigInt(v)
  }
  return n
}

function parseIPv6(ip: string): bigint | null {
  if (!ip.includes(':')) return null
  const double = ip.indexOf('::')
  if (double !== ip.lastIndexOf('::')) return null
  const headText = double < 0 ? ip : ip.slice(0, double)
  const tailText = double < 0 ? '' : ip.slice(double + 2)

  const expand = (text: string): bigint[] | null => {
    if (!text) return []
    const out: bigint[] = []
    const groups = text.split(':')
    for (let i = 0; i < groups.length; i++) {
      const g = groups[i]
      if (g.includes('.')) {
        // 内嵌 IPv4 只能出现在末尾
        if (i !== groups.length - 1) return null
        const v4 = parseIPv4(g)
        if (v4 === null) return null
        out.push(v4 >> 16n, v4 & 0xffffn)
        continue
      }
      if (!/^[0-9a-f]{1,4}$/i.test(g)) return null
      out.push(BigInt('0x' + g))
    }
    return out
  }

  const head = expand(headText)
  const tail = expand(tailText)
  if (!head || !tail) return null
  const total = head.length + tail.length
  if (double < 0 ? total !== 8 : total > 7) return null
  const groups = [...head, ...Array<bigint>(8 - total).fill(0n), ...tail]
  return groups.reduce((acc, g) => (acc << 16n) | g, 0n)
}

function maskRange(n: bigint, bits: 32 | 128, prefix: number): IpRange {
  const host = BigInt(bits - prefix)
  const from = (n >> host) << host
  return { v: bits === 32 ? 4 : 6, from, to: from | ((1n << host) - 1n) }
}

function parsePartialIp(text: string): IpRange | null {
  if (text.includes(':')) {
    const body = text.endsWith(':') && !text.endsWith('::') ? text.slice(0, -1) : text
    if (body.includes('::')) return null
    const groups = body.split(':')
    if (groups.length < 2 || groups.length >= 8) return null
    let n = 0n
    for (const g of groups) {
      if (!/^[0-9a-f]{1,4}$/i.test(g)) return null
      n = (n << 16n) | BigInt('0x' + g)
    }
    return maskRange(n << BigInt(16 * (8 - groups.length)), 128, 16 * groups.length)
  }
  const parts = text.split('.')
  if (parts.length < 2 || parts.length >= 4) return null
  let n = 0n
  for (const p of parts) {
    if (!/^\d{1,3}$/.test(p)) return null
    const v = Number(p)
    if (v > 255) return null
    n = (n << 8n) | BigInt(v)
  }
  return maskRange(n << BigInt(8 * (4 - parts.length)), 32, 8 * parts.length)
}

// 支持单个地址、CIDR，以及截断的前缀写法
function parseIpRange(text: string): IpRange | null {
  const s = text.trim()
  if (!s) return null
  const slash = s.indexOf('/')
  if (slash >= 0) {
    const addr = s.slice(0, slash).trim()
    const prefixText = s.slice(slash + 1).trim()
    if (!/^\d{1,3}$/.test(prefixText)) return null
    const prefix = Number(prefixText)
    const v4 = parseIPv4(addr)
    if (v4 !== null) return prefix <= 32 ? maskRange(v4, 32, prefix) : null
    const v6 = parseIPv6(addr)
    if (v6 !== null) return prefix <= 128 ? maskRange(v6, 128, prefix) : null
    return null
  }
  const v4 = parseIPv4(s)
  if (v4 !== null) return maskRange(v4, 32, 32)
  const v6 = parseIPv6(s)
  if (v6 !== null) return maskRange(v6, 128, 128)
  return parsePartialIp(s)
}

function runDetailFilter() {
  const q = detailFilterText.value.trim().toLowerCase()
  if (!q) {
    filteredDetailRules.value = detailRules.value
    return
  }
  const qRange = parseIpRange(q)
  filteredDetailRules.value = detailRules.value.filter((r) => {
    // 文本包含
    if (r.value.toLowerCase().includes(q) || r.type.toLowerCase().includes(q)) return true
    // IP CIDR 语义匹配：查询区间与规则区间有交集即命中
    if (qRange && (r.type === 'ip_cidr' || r.type === 'source_ip_cidr')) {
      const ruleRange = parseIpRange(r.value)
      if (!ruleRange || ruleRange.v !== qRange.v) return false
      return qRange.from <= ruleRange.to && qRange.to >= ruleRange.from
    }
    // 域名语义匹配
    if (!qRange) {
      const val = r.value.toLowerCase()
      if (r.type === 'domain') return q === val
      if (r.type === 'domain_suffix') return q.endsWith(val) || q.endsWith('.' + val.replace(/^\./, ''))
      if (r.type === 'domain_keyword') return q.includes(val)
    }
    return false
  })
}

watch(detailFilterText, () => {
  if (filterTimer) clearTimeout(filterTimer)
  filterTimer = setTimeout(runDetailFilter, 300)
})

watch(detailRules, () => {
  filteredDetailRules.value = detailRules.value
})

async function searchInDetail() {
  const q = detailFilterText.value.trim()
  const provider = detailProvider.value
  if (!q || !provider) {
    detailMatchResult.value = null
    return
  }
  detailMatchSearching.value = true
  detailMatchResult.value = null

  let configPath = ''
  try { configPath = await getRunningConfigPath() } catch {}

  try {
    detailMatchResult.value = await srsMatchProvider(
      config.value.workingDir ?? '',
      configPath,
      config.value.singboxPath ?? '',
      provider.name,
      q,
    )
  } catch {
    detailMatchResult.value = null
  } finally {
    detailMatchSearching.value = false
  }
}

watch(detailFilterText, (val) => {
  if (detailSearchTimer) clearTimeout(detailSearchTimer)
  if (!val.trim()) {
    detailMatchResult.value = null
    detailMatchSearching.value = false
    return
  }
  detailSearchTimer = setTimeout(searchInDetail, 500)
})

// 虚拟滚动
const ROW_HEIGHT = 28
const OVERSCAN = 10
const detailScrollTop = ref(0)
const detailContainerHeight = ref(400)
const detailScrollRef = ref<HTMLElement | null>(null)

const virtualSlice = computed(() => {
  const items = filteredDetailRules.value
  const total = items.length
  const startIdx = Math.max(0, Math.floor(detailScrollTop.value / ROW_HEIGHT) - OVERSCAN)
  const visibleCount = Math.ceil(detailContainerHeight.value / ROW_HEIGHT) + OVERSCAN * 2
  const endIdx = Math.min(total, startIdx + visibleCount)
  return {
    items: items.slice(startIdx, endIdx),
    startIdx,
    totalHeight: total * ROW_HEIGHT,
    offsetY: startIdx * ROW_HEIGHT,
  }
})

function onDetailScroll(e: Event) {
  const el = e.target as HTMLElement
  detailScrollTop.value = el.scrollTop
}

async function openProviderDetail(provider: RuleProvider) {
  detailProvider.value = provider
  detailLoading.value = true
  detailError.value = ''
  detailRules.value = []
  detailFilterText.value = ''

  let configPath = ''
  try {
    configPath = await getRunningConfigPath()
  } catch {}

  try {
    const rules = await srsListProvider(
      config.value.workingDir ?? '',
      configPath,
      config.value.singboxPath ?? '',
      provider.name,
    )
    detailRules.value = rules
  } catch (e: any) {
    detailError.value = e?.message || String(e)
  } finally {
    detailLoading.value = false
  }
}

function closeProviderDetail() {
  detailProvider.value = null
  detailRules.value = []
  detailFilterText.value = ''
  detailError.value = ''
  detailScrollTop.value = 0
  detailMatchResult.value = null
  detailMatchSearching.value = false
  if (detailSearchTimer) { clearTimeout(detailSearchTimer); detailSearchTimer = null }
  if (filterTimer) { clearTimeout(filterTimer); filterTimer = null }
}

onMounted(() => {
  if (isRunning.value) {
    loadRules()
    loadProviders()
  }
})

watch(isRunning, (running) => {
  if (running) {
    loadRules()
    loadProviders()
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
          <p class="text-sm">请先启动 sing-box 服务以查看规则信息</p>
        </div>
      </div>
    </template>

    <template v-else>
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-3">
        <h1 class="text-xl font-bold shrink-0">规则</h1>
        <div class="tabs tabs-boxed tabs-sm">
          <a class="tab" :class="{ 'tab-active': activeTab === 'rules' }" @click="activeTab = 'rules'">
            规则 ({{ filteredRules.length }})
          </a>
          <a
            v-if="providersAvailable"
            class="tab"
            :class="{ 'tab-active': activeTab === 'providers' }"
            @click="activeTab = 'providers'"
          >
            规则提供商 ({{ ruleProviders.length }})
          </a>
        </div>
      </div>
    </div>

    <template v-if="activeTab === 'rules'">
      <input
        v-model="filterText"
        type="text"
        placeholder="搜索规则..."
        class="input input-sm input-bordered w-full"
        aria-label="搜索规则"
      />

      <div class="flex-1 min-h-0 overflow-auto rounded-lg border border-base-content/10 bg-base-100">
        <table v-if="filteredRules.length > 0" class="table table-xs table-pin-rows min-w-[680px]">
          <thead>
            <tr class="bg-base-200 border-b border-base-content/20">
              <th scope="col" class="z-20 w-12 bg-base-200 text-right">#</th>
              <th scope="col" class="z-20 w-36 bg-base-200">类型</th>
              <th scope="col" class="z-20 bg-base-200">规则内容</th>
              <th scope="col" class="z-20 bg-base-200">代理链</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(rule, i) in filteredRules"
              :key="i"
              class="hover:bg-base-200/50 transition-colors"
            >
              <td class="text-right tabular-nums text-base-content/30">{{ i + 1 }}</td>
              <td>
                <span class="inline-block whitespace-nowrap rounded bg-base-content/10 px-1.5 py-0.5 text-xs leading-none text-base-content/60">
                  {{ rule.type }}
                </span>
              </td>
              <td class="max-w-[32rem] text-xs text-base-content/60" :title="rule.payload">
                <span class="block truncate">{{ rule.payload || '—' }}</span>
              </td>
              <td>
                <div class="flex items-center gap-1 whitespace-nowrap text-xs">
                  <template v-for="(node, j) in resolveProxyChain(rule.proxy)" :key="j">
                    <span v-if="j > 0" class="text-base-content/20">›</span>
                    <span class="rounded px-1.5 py-0.5 leading-none" :class="actionColor(node)">{{ node }}</span>
                  </template>
                </div>
              </td>
            </tr>
          </tbody>
        </table>

        <div v-if="loading" class="flex justify-center py-10" aria-label="正在加载规则">
          <span class="loading loading-spinner loading-md"></span>
        </div>

        <div
          v-else-if="filteredRules.length === 0"
          class="flex items-center justify-center py-10 text-sm text-base-content/40"
        >
          {{ filterText.trim() ? '未找到匹配规则' : '暂无规则' }}
        </div>
      </div>
    </template>

    <template v-if="activeTab === 'providers'">
      <div v-if="ruleProviders.length > 0" class="flex items-center gap-2">
        <div class="relative flex-1">
          <input
            v-model="providerSearchText"
            type="text"
            placeholder="搜索规则内容..."
            class="input input-sm input-bordered w-full"
            aria-label="搜索规则提供商内容"
          />
          <span
            v-if="providerSearching"
            class="loading loading-spinner loading-xs absolute right-2 top-1/2 -translate-y-1/2 text-base-content/40"
          ></span>
        </div>
        <button
          class="btn btn-sm btn-ghost shrink-0"
          :class="{ 'loading': updatingAll }"
          @click="handleUpdateAll"
          :disabled="updatingAll"
          aria-label="更新全部规则提供商"
        >
          <template v-if="!updatingAll">全部更新</template>
        </button>
      </div>

      <div class="flex-1 min-h-0 overflow-auto rounded-lg border border-base-content/10 bg-base-100">
        <table v-if="displayedProviders.length > 0" class="table table-xs table-pin-rows min-w-[680px]">
          <thead>
            <tr class="bg-base-200 border-b border-base-content/20">
              <th scope="col" class="z-20 w-12 bg-base-200 text-right">#</th>
              <th scope="col" class="z-20 bg-base-200">提供商</th>
              <th scope="col" class="z-20 w-20 bg-base-200 text-center">规则数</th>
              <th scope="col" class="z-20 w-28 bg-base-200 text-center">格式</th>
              <th scope="col" class="z-20 w-28 bg-base-200 text-center">载入方式</th>
              <th scope="col" class="z-20 w-28 bg-base-200">更新时间</th>
              <th scope="col" class="z-20 w-12 bg-base-200 text-center">
                <span class="sr-only">操作</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(provider, i) in displayedProviders"
              :key="provider.name"
              class="transition-colors"
              :class="canOpenProvider(provider) ? 'cursor-pointer hover:bg-base-200/50' : 'hover:bg-base-200/50'"
              @click="canOpenProvider(provider) && openProviderDetail(provider)"
            >
              <td class="text-right tabular-nums text-base-content/30">{{ i + 1 }}</td>
              <td class="max-w-[24rem]" :title="provider.name">
                <button
                  v-if="canOpenProvider(provider)"
                  class="block w-full truncate rounded-sm text-left text-sm font-medium hover:text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
                  :aria-label="`查看规则提供商 ${provider.name} 的详情`"
                  @click.stop="openProviderDetail(provider)"
                >
                  {{ provider.name }}
                </button>
                <span v-else class="block truncate text-sm font-medium">{{ provider.name }}</span>
              </td>
              <td class="text-center tabular-nums text-xs text-base-content/60">{{ provider.ruleCount }}</td>
              <td class="text-center">
                <span v-if="provider.behavior" class="inline-block whitespace-nowrap rounded bg-base-content/10 px-1.5 py-0.5 text-xs leading-none text-base-content/60">
                  {{ provider.behavior }}
                </span>
                <span v-else class="text-base-content/30">—</span>
              </td>
              <td class="text-center">
                <span v-if="provider.vehicleType" class="inline-block whitespace-nowrap rounded border border-base-content/20 px-1.5 py-0.5 text-xs leading-none text-base-content/60">
                  {{ provider.vehicleType }}
                </span>
                <span v-else class="text-base-content/30">—</span>
              </td>
              <td class="whitespace-nowrap text-xs text-base-content/40">
                {{ formatDate(provider.updatedAt) || '—' }}
              </td>
              <td class="text-center">
                <button
                  v-if="provider.vehicleType !== 'Inline'"
                  class="btn btn-ghost btn-xs btn-circle"
                  :class="{ 'loading': updatingProvider === provider.name }"
                  @click.stop="handleUpdateProvider(provider.name)"
                  title="更新"
                  :aria-label="`更新规则提供商 ${provider.name}`"
                >
                  <svg v-if="updatingProvider !== provider.name" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
                  </svg>
                </button>
              </td>
            </tr>
          </tbody>
        </table>

        <div
          v-if="ruleProviders.length === 0"
          class="flex items-center justify-center py-10 text-sm text-base-content/40"
        >
          暂无规则提供商
        </div>

        <div
          v-else-if="providerSearchText.trim() && providerSearchDone && displayedProviders.length === 0"
          class="flex items-center justify-center py-10 text-sm text-base-content/40"
        >
          未找到匹配规则
        </div>
      </div>
    </template>
    </template>
  </div>

  <!-- 规则详情弹窗 -->
  <div
    v-if="detailProvider"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="closeProviderDetail"
  >
    <div class="w-full max-w-2xl max-h-[80vh] flex flex-col rounded-lg bg-base-100 shadow-xl">
      <div class="flex items-start justify-between px-5 pt-4 pb-3 shrink-0">
        <div class="flex flex-col gap-1.5">
          <div class="flex items-baseline gap-2">
            <span class="font-semibold text-base">{{ detailProvider.name }}</span>
            <span class="text-xs text-base-content/50">{{ detailProvider.ruleCount }} 条规则</span>
          </div>
          <div class="flex items-center gap-1.5">
            <span v-if="detailProvider.behavior" class="text-xs leading-none px-1.5 py-0.5 rounded bg-base-content/10 text-base-content/60">{{ detailProvider.behavior }}</span>
            <span v-if="detailProvider.vehicleType" class="text-xs leading-none px-1.5 py-0.5 rounded border border-base-content/20 text-base-content/60">{{ detailProvider.vehicleType }}</span>
            <span class="text-xs text-base-content/40">{{ formatDate(detailProvider.updatedAt) }}</span>
          </div>
        </div>
        <button class="btn btn-sm btn-circle btn-ghost" @click="closeProviderDetail">✕</button>
      </div>

      <div class="px-5 pb-2 shrink-0 flex items-center gap-2">
        <div class="relative flex-1">
          <input
            v-model="detailFilterText"
            type="text"
            placeholder="搜索规则内容..."
            class="input input-sm input-bordered w-full"
          />
          <span
            v-if="detailMatchSearching"
            class="loading loading-spinner loading-xs absolute right-2 top-1/2 -translate-y-1/2 text-base-content/40"
          ></span>
        </div>
        <span
          v-if="detailFilterText.trim() && !detailMatchSearching && detailMatchResult !== null"
          class="text-xs leading-none px-2 py-1 rounded shrink-0"
          :class="detailMatchResult ? 'bg-success/15 text-success' : 'bg-base-content/10 text-base-content/40'"
        >{{ detailMatchResult ? '匹配' : '未匹配' }}</span>
      </div>

      <div class="flex-1 flex flex-col px-5 pb-4 min-h-0">
        <div v-if="detailLoading" class="flex justify-center py-10">
          <span class="loading loading-spinner loading-md"></span>
        </div>
        <div v-else-if="detailError" class="text-sm text-error py-4">{{ detailError }}</div>
        <template v-else>
          <div class="flex text-xs font-semibold text-base-content/60 bg-base-200 rounded-t px-2 shrink-0" :style="{ height: ROW_HEIGHT + 'px', lineHeight: ROW_HEIGHT + 'px' }">
            <span class="w-12 shrink-0">#</span>
            <span class="w-28 shrink-0">类型</span>
            <span class="flex-1">内容</span>
          </div>
          <div
            ref="detailScrollRef"
            class="flex-1 overflow-auto min-h-0"
            @scroll="onDetailScroll"
          >
            <div :style="{ height: virtualSlice.totalHeight + 'px', position: 'relative' }">
              <div :style="{ transform: `translateY(${virtualSlice.offsetY}px)` }">
                <div
                  v-for="(rule, j) in virtualSlice.items"
                  :key="virtualSlice.startIdx + j"
                  class="flex items-center px-2 text-xs hover:bg-base-200/50"
                  :style="{ height: ROW_HEIGHT + 'px' }"
                >
                  <span class="w-12 shrink-0 text-base-content/30">{{ virtualSlice.startIdx + j + 1 }}</span>
                  <span class="w-28 shrink-0">
                    <span class="leading-none px-1.5 py-0.5 rounded bg-base-content/10 text-base-content/60 whitespace-nowrap">{{ rule.type }}</span>
                  </span>
                  <span class="flex-1 truncate" :title="rule.value">{{ rule.value }}</span>
                </div>
              </div>
            </div>
          </div>
          <div v-if="detailRules.length > 0" class="text-xs text-base-content/40 pt-2 shrink-0">
            <template v-if="detailFilterText.trim()">
              显示 {{ filteredDetailRules.length }} / 共 {{ detailRules.length }} 条
            </template>
            <template v-else>
              共 {{ detailRules.length }} 条
            </template>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>
