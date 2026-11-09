<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useRulesStore } from '@/stores/rules'
import { updateRuleProvider } from '@/api'
import type { RuleProvider } from '@/types'
import { getRequestErrorReason } from '@/utils/requestError'
import { useToastStore } from '@/stores/toast'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'
import { useProxiesStore } from '@/stores/proxies'
import { srsMatchProvider, srsListProvider } from '@/bridge/config'
import { formatDate } from '@/utils/format'
import { batchUpdateProviders } from '@/utils/batchUpdate'
import { parseRuleAction, type RuleAction } from '@/utils/ruleDisplay'

const {
  rules,
  filteredRules,
  loading,
  filterText,
  ruleProviders,
  providersAvailable,
  loadRules,
  loadProviders,
} = useRulesStore()
const { clashApiReady } = useServiceStore()
const { proxyMap } = useProxiesStore()

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
  return 'bg-primary/15 text-primary'
}

type RuleTarget = { label: string; tone: string }

function ruleTargets(action: RuleAction): RuleTarget[] {
  if (action.kind === 'route') {
    return resolveProxyChain(action.outbound).map((node) => ({ label: node, tone: actionColor(node) }))
  }
  if (action.kind === 'direct') return [{ label: action.raw, tone: 'bg-success/15 text-success' }]
  if (action.kind === 'reject') return [{ label: action.raw, tone: 'bg-error/15 text-error' }]
  return [{ label: action.raw, tone: 'bg-base-content/10 text-base-content/60' }]
}

const ruleIndex = computed(() => new Map(rules.value.map((rule, i) => [rule, i + 1])))

const ruleRows = computed(() =>
  filteredRules.value.map((rule) => ({
    rule,
    index: ruleIndex.value.get(rule) ?? 0,
    targets: ruleTargets(parseRuleAction(rule.proxy)),
  })),
)

const canOpenProviders = computed(() => config.value.rootMode)

const activeTab = ref<'rules' | 'providers'>('rules')

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

  const configPath = config.value.configPath

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

watch(providersAvailable, (available) => {
  if (!available && activeTab.value === 'providers') {
    activeTab.value = 'rules'
  }
})

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

  const configPath = config.value.configPath

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

  try {
    const rules = await srsListProvider(
      config.value.workingDir ?? '',
      config.value.configPath,
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

watch(
  clashApiReady,
  (ready) => {
    if (!ready) return
    loadRules()
    loadProviders()
  },
  { immediate: true },
)
</script>

<template>
  <div class="rules-page flex min-w-0 flex-col h-full gap-3">
    <div class="flex items-center justify-between">
      <div class="flex flex-wrap items-center gap-3">
        <h1 class="text-[26px] leading-tight tracking-tight font-bold shrink-0">规则</h1>
        <div class="tabs tabs-boxed tabs-sm">
          <button
            type="button"
            class="tab"
            :class="{ 'tab-active': activeTab === 'rules' }"
            :aria-pressed="activeTab === 'rules'"
            @click="activeTab = 'rules'"
          >
            规则 ({{ filteredRules.length }})
          </button>
          <button
            v-if="providersAvailable"
            type="button"
            class="tab"
            :class="{ 'tab-active': activeTab === 'providers' }"
            :aria-pressed="activeTab === 'providers'"
            @click="activeTab = 'providers'"
          >
            规则提供商 ({{ ruleProviders.length }})
          </button>
        </div>
        <button
          v-if="activeTab === 'providers'"
          type="button"
          class="btn btn-sm btn-ghost btn-circle shrink-0"
          :disabled="updatingAll || ruleProviders.length === 0"
          :aria-busy="updatingAll"
          aria-label="更新全部规则提供商"
          title="更新全部规则提供商"
          @click="handleUpdateAll"
        >
          <span v-if="updatingAll" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4" aria-hidden="true">
            <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
          </svg>
        </button>
      </div>
    </div>

    <template v-if="activeTab === 'rules'">
      <div class="rules-toolbar">
        <input
          v-model="filterText"
          type="text"
          placeholder="搜索规则..."
          class="input input-sm input-bordered rules-search"
          aria-label="搜索规则"
        />
      </div>

      <div class="rules-scroll rules-card-scroll under-nav">
        <ol
          v-if="ruleRows.length > 0"
          class="rules-list"
          :style="{ '--rules-index-digits': String(rules.length).length }"
          aria-label="规则列表"
        >
          <li v-for="row in ruleRows" :key="row.index" class="surface-card rules-item">
            <span class="rules-index">{{ row.index }}</span>
            <span class="rules-badge rules-type">{{ row.rule.type }}</span>
            <p v-if="row.rule.payload" class="rules-payload">{{ row.rule.payload }}</p>
            <div class="rules-proxy-chain">
              <template v-for="(target, j) in row.targets" :key="j">
                <span v-if="j > 0" class="text-base-content/25" aria-hidden="true">›</span>
                <span class="rules-action-badge" :class="target.tone">{{ target.label }}</span>
              </template>
            </div>
          </li>
        </ol>

        <div v-if="loading" class="rules-empty" aria-label="正在加载规则">
          <span class="loading loading-spinner loading-md"></span>
        </div>

        <div v-else-if="ruleRows.length === 0" class="rules-empty">
          {{ filterText.trim() ? '未找到匹配规则' : '暂无规则' }}
        </div>
      </div>
    </template>

    <template v-if="activeTab === 'providers'">
      <div class="rules-toolbar">
        <div class="relative min-w-0 flex-1">
          <input
            v-model="providerSearchText"
            type="text"
            :placeholder="config.rootMode ? '搜索规则内容...' : '开启 root 模式后可搜索规则内容'"
            :disabled="!config.rootMode"
            class="input input-sm input-bordered rules-search pr-8"
            aria-label="搜索规则提供商内容"
          />
          <span
            v-if="providerSearching"
            class="loading loading-spinner loading-xs absolute right-2 top-1/2 -translate-y-1/2 text-base-content/40"
          ></span>
        </div>
      </div>

      <div class="rules-scroll rules-provider-list under-nav">
        <article
          v-for="provider in displayedProviders"
          :key="provider.name"
          class="surface-card rules-provider"
          :class="{ 'glass-hover cursor-pointer': canOpenProviders }"
          @click="canOpenProviders && openProviderDetail(provider)"
        >
          <div class="min-w-0 flex-1">
            <div class="rules-provider-title">
              <button
                v-if="canOpenProviders"
                type="button"
                class="rules-provider-name"
                :aria-label="`查看规则提供商 ${provider.name} 的详情`"
                @click.stop="openProviderDetail(provider)"
              >
                {{ provider.name }}
              </button>
              <span v-else class="rules-provider-name">{{ provider.name }}</span>
              <span class="rules-provider-count">{{ provider.ruleCount }} 规则</span>
            </div>
            <div class="rules-provider-kind">
              {{ [provider.behavior, provider.vehicleType].filter(Boolean).join(' · ') || '—' }}
            </div>
            <div class="rules-provider-time">
              {{ provider.updatedAt ? `更新于 ${formatDate(provider.updatedAt)}` : '未更新' }}
            </div>
          </div>
          <button
            v-if="provider.vehicleType !== 'Inline'"
            type="button"
            class="rules-provider-update glass-press"
            :aria-busy="updatingProvider === provider.name"
            :disabled="updatingProvider === provider.name || updatingAll"
            title="更新"
            :aria-label="`更新规则提供商 ${provider.name}`"
            @click.stop="handleUpdateProvider(provider.name)"
          >
            <span v-if="updatingProvider === provider.name" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
            <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.75" stroke="currentColor" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
            </svg>
          </button>
        </article>

        <div v-if="ruleProviders.length === 0" class="rules-empty">
          暂无规则提供商
        </div>

        <div
          v-else-if="providerSearchText.trim() && providerSearchDone && displayedProviders.length === 0"
          class="rules-empty"
        >
          未找到匹配规则
        </div>
      </div>
    </template>
  </div>

  <!-- 规则详情弹窗 -->
  <Transition name="glass-pop">
  <div
    v-if="detailProvider"
    class="glass-scrim safe-overlay fixed inset-0 z-50 flex items-center justify-center"
    @click.self="closeProviderDetail"
  >
    <div class="glass-popover w-full max-w-2xl max-h-[80vh] flex flex-col rounded-[var(--radius-panel)]">
      <div class="flex items-start justify-between px-5 pt-4 pb-3 shrink-0">
        <div class="flex flex-col gap-1.5">
          <div class="flex items-baseline gap-2">
            <span class="font-semibold text-base">{{ detailProvider.name }}</span>
            <span class="text-xs text-base-content/50">{{ detailProvider.ruleCount }} 条规则</span>
          </div>
          <div class="flex items-center gap-1.5">
            <span v-if="detailProvider.behavior" class="rules-badge">{{ detailProvider.behavior }}</span>
            <span v-if="detailProvider.vehicleType" class="rules-badge">{{ detailProvider.vehicleType }}</span>
            <span class="text-xs text-base-content/40">{{ formatDate(detailProvider.updatedAt) }}</span>
          </div>
        </div>
        <button class="btn btn-sm btn-circle btn-ghost" aria-label="关闭规则详情" @click="closeProviderDetail">✕</button>
      </div>

      <div class="px-5 pb-2 shrink-0 flex items-center gap-2">
        <div class="relative flex-1">
          <input
            v-model="detailFilterText"
            type="text"
            :placeholder="config.rootMode ? '搜索规则内容...' : '开启 root 模式后可搜索规则内容'"
            :disabled="!config.rootMode"
            class="input input-sm input-bordered w-full"
          />
          <span
            v-if="detailMatchSearching"
            class="loading loading-spinner loading-xs absolute right-2 top-1/2 -translate-y-1/2 text-base-content/40"
          ></span>
        </div>
        <span
          v-if="detailFilterText.trim() && !detailMatchSearching && detailMatchResult !== null"
          class="text-xs leading-none px-2.5 py-1 rounded-full shrink-0"
          :class="detailMatchResult ? 'bg-success/15 text-success' : 'bg-base-content/10 text-base-content/40'"
        >{{ detailMatchResult ? '匹配' : '未匹配' }}</span>
      </div>

      <div class="flex-1 flex flex-col px-5 pb-4 min-h-0">
        <div v-if="detailLoading" class="flex justify-center py-10">
          <span class="loading loading-spinner loading-md"></span>
        </div>
        <div v-else-if="detailError" class="text-sm text-error py-4">{{ detailError }}</div>
        <template v-else>
          <div class="surface-fill flex text-xs font-semibold text-base-content/60 rounded-[var(--radius-control)] px-2 shrink-0" :style="{ height: ROW_HEIGHT + 'px', lineHeight: ROW_HEIGHT + 'px' }">
            <span class="hidden w-12 shrink-0 sm:inline">#</span>
            <span class="w-24 shrink-0 sm:w-28">类型</span>
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
                  class="flex items-center rounded-lg px-2 text-xs hover:bg-base-content/[0.05]"
                  :style="{ height: ROW_HEIGHT + 'px' }"
                >
                  <span class="hidden w-12 shrink-0 text-base-content/30 sm:inline">{{ virtualSlice.startIdx + j + 1 }}</span>
                  <span class="w-24 shrink-0 sm:w-28">
                    <span class="rules-badge rules-badge-fixed" :title="rule.type">{{ rule.type }}</span>
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
  </Transition>
</template>

<style scoped>
.rules-toolbar {
  @apply flex shrink-0 items-center gap-2;
  min-height: 32px;
}

.rules-search {
  @apply w-full min-w-0 text-xs;
}

.rules-scroll {
  @apply flex-1 min-h-0 overflow-y-auto overflow-x-hidden;
}

.rules-card-scroll {
  margin-inline: -0.5rem;
  padding: 0.25rem 0.5rem 0.5rem;
}

.rules-list {
  @apply flex flex-col gap-2;
  margin: 0;
  padding: 0;
  list-style: none;
}

.rules-item {
  display: grid;
  flex-shrink: 0;
  grid-template-columns: auto minmax(0, 1fr);
  column-gap: 8px;
  row-gap: 6px;
  align-items: center;
  padding: 12px 14px;
}

.rules-item > * {
  grid-column: 2;
}

.rules-item > .rules-index {
  grid-column: 1;
  grid-row: 1;
}

.rules-index {
  @apply text-right tabular-nums text-xs text-base-content/40;
  min-width: calc(var(--rules-index-digits, 1) * 1ch);
}

.rules-type {
  justify-self: start;
}

.rules-payload {
  @apply m-0 text-[13px] font-medium leading-snug text-base-content/85;
  overflow-wrap: anywhere;
}

.rules-badge,
.rules-action-badge {
  @apply inline-flex max-w-full items-center rounded-full px-2 text-xs;
  min-height: 20px;
  line-height: 16px;
  overflow-wrap: anywhere;
}

.rules-badge-fixed {
  @apply block truncate;
  overflow-wrap: normal;
}

.rules-badge {
  @apply bg-base-content/10 text-base-content/60;
}

.rules-proxy-chain {
  @apply flex min-w-0 flex-wrap items-center gap-1 text-xs;
}

.rules-provider-list {
  @apply flex flex-col gap-3;
  margin-inline: -0.5rem;
  padding: 0.25rem 0.5rem 0.5rem;
}

.rules-provider {
  @apply flex shrink-0 items-end gap-3;
  padding: 14px 16px;
}

.rules-provider-title {
  @apply flex min-w-0 items-baseline gap-2;
}

.rules-provider-name {
  @apply min-w-0 text-left text-base font-semibold leading-snug;
  overflow-wrap: anywhere;
}

button.rules-provider-name:focus-visible {
  @apply rounded-sm outline-none ring-1 ring-primary;
}

.rules-provider-count {
  @apply shrink-0 text-xs tabular-nums text-base-content/45;
}

.rules-provider-kind {
  @apply mt-1.5 text-[11px] font-semibold uppercase tracking-wider text-base-content/50;
}

.rules-provider-time {
  @apply mt-1 text-xs text-base-content/45;
}

.rules-provider-update {
  @apply inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-base-content/60;
  background: var(--fill);
}

.rules-provider-update svg {
  @apply h-4 w-4;
}

.rules-provider-update:disabled {
  @apply opacity-60;
}

.rules-empty {
  @apply flex items-center justify-center py-10 text-sm text-base-content/40;
}

.under-nav {
  margin-bottom: calc(-1 * var(--nav-clearance, 0px));
  padding-bottom: calc(var(--nav-clearance, 0px) + 0.5rem);
}
</style>
