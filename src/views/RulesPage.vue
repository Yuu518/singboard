<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import { useRulesStore } from '@/stores/rules'
import { fetchRuleProviders, updateRuleProvider } from '@/api'
import type { Rule, RuleProvider } from '@/types'
import { getRequestErrorReason } from '@/utils/requestError'
import { useToastStore } from '@/stores/toast'
import { useConfigStore } from '@/stores/config'
import { useProxiesStore } from '@/stores/proxies'
import { srsMatchProvider, srsListProvider, getRunningConfigPath, readSingboxConfig } from '@/bridge/config'
import { parseJsonWithComments } from '@/utils/jsonc'
import { formatDate } from '@/utils/format'
import { batchUpdateProviders } from '@/utils/batchUpdate'
import ServiceNotice from '@/components/common/ServiceNotice.vue'
import { useSettledServiceState } from '@/composables/useSettledServiceState'

const { rules, loading, loaded, loadError, filterText, loadRules } = useRulesStore()
const { serviceState, isRunning } = useSettledServiceState()
const { proxyMap } = useProxiesStore()
const { pushToast } = useToastStore()
const { config, clashApiUrl } = useConfigStore()
const router = useRouter()

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

function isRejectNode(name: string): boolean {
  return name.toLowerCase().includes('reject')
}

function providerVehicle(provider: RuleProvider): string {
  return (provider.vehicleType || '').toLowerCase()
}

function isInlineProvider(provider: RuleProvider): boolean {
  return providerVehicle(provider) === 'inline'
}

function canOpenProvider(provider: RuleProvider): boolean {
  return !isInlineProvider(provider) && provider.behavior.toLowerCase() !== 'source'
}

const VEHICLE_LABELS: Record<string, string> = { remote: '远程', local: '本地', inline: '内联', http: '远程', file: '本地' }
const FORMAT_LABELS: Record<string, string> = { binary: '二进制', source: '源文件' }

function providerNameParts(name: string): { prefix: string; base: string } {
  const cut = name.lastIndexOf('/')
  return cut < 0 ? { prefix: '', base: name } : { prefix: name.slice(0, cut + 1), base: name.slice(cut + 1) }
}

function providerSource(provider: RuleProvider): string {
  const vehicle = VEHICLE_LABELS[providerVehicle(provider)] ?? provider.vehicleType
  const format = FORMAT_LABELS[provider.behavior.toLowerCase()] ?? provider.behavior
  return [vehicle, format].filter(Boolean).join(' · ')
}

function providerMeta(provider: RuleProvider): string {
  const updated = formatDate(provider.updatedAt)
  return [providerSource(provider), updated && `${updated}更新`].filter(Boolean).join(' · ') || '—'
}

const activeTab = ref<'rules' | 'providers'>('rules')

const announcement = ref('')

function announce(message: string) {
  announcement.value = ''
  void nextTick(() => {
    announcement.value = message
  })
}

const ruleProviders = ref<RuleProvider[]>([])
const providersAvailable = ref(false)
const updatingProviders = ref(new Set<string>())
const updatingAll = ref(false)
let providersRequest = 0

const providerByName = computed(() => {
  const map: Record<string, RuleProvider> = {}
  for (const provider of ruleProviders.value) map[provider.name] = provider
  return map
})

const searchableProviders = computed(() => ruleProviders.value.filter(canOpenProvider))

function onTabKeydown(e: KeyboardEvent) {
  if ((e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') || !providersAvailable.value) return
  e.preventDefault()
  activeTab.value = activeTab.value === 'rules' ? 'providers' : 'rules'
  void nextTick(() => document.getElementById(`rules-tab-${activeTab.value}`)?.focus())
}

watch(providersAvailable, (available) => {
  if (!available) activeTab.value = 'rules'
})

type MatchOutcome = { matched: Set<string>; failures: Map<string, string> }

const matchCache = new Map<string, Promise<MatchOutcome>>()

function srsFailureReason(error: unknown, tag: string): string {
  const raw = getRequestErrorReason(error)
  if (/no cache\.db found|cache\.db not found/i.test(raw)) return `找不到 cache.db，也找不到本地的 ${tag}.srs`
  if (/\.srs' not found/i.test(raw)) return `cache.db 和本地目录里都没有 ${tag} 的规则数据`
  if (/invalid SRS magic|unsupported SRS version/i.test(raw)) return `无法识别 ${tag} 的规则文件格式`
  return raw
}

function failureSummary(failures: Map<string, string>): string {
  const reasons = Array.from(new Set(failures.values()))
  const shown = reasons.slice(0, 2).join('；')
  return `${failures.size} 个规则提供商无法读取：${shown}${reasons.length > 2 ? ' 等' : ''}`
}

async function runProviderMatch(query: string, providers: RuleProvider[]): Promise<MatchOutcome> {
  let configPath = ''
  try {
    configPath = await getRunningConfigPath()
  } catch { }

  const matched = new Set<string>()
  const failures = new Map<string, string>()
  await Promise.all(
    providers.map(async (p) => {
      try {
        const hit = await srsMatchProvider(
          config.value.workingDir ?? '',
          configPath,
          config.value.singboxPath ?? '',
          p.name,
          query,
        )
        if (hit) matched.add(p.name)
      } catch (error) {
        failures.set(p.name, srsFailureReason(error, p.name))
      }
    }),
  )
  return { matched, failures }
}

function matchProviders(query: string): Promise<MatchOutcome> {
  const key = query.toLowerCase()
  const cached = matchCache.get(key)
  if (cached) return cached
  const pending = runProviderMatch(query, searchableProviders.value)
  matchCache.set(key, pending)
  if (matchCache.size > 30) {
    const oldest = matchCache.keys().next().value
    if (oldest !== undefined) matchCache.delete(oldest)
  }
  void pending.then((outcome) => {
    if (outcome.failures.size > 0 && matchCache.get(key) === pending) matchCache.delete(key)
  })
  return pending
}

function isLookupQuery(q: string): boolean {
  if (!q || /\s/.test(q)) return false
  return parseIpRange(q) !== null || /^(?:[a-z0-9_-]+\.)+[a-z0-9-]{2,}\.?$/i.test(q)
}

const RULE_SET_PATTERN = /rule_set=(\[[^\]]*\]|[^\s\]]+)/g

function ruleSetTags(payload: string): string[] {
  const tags: string[] = []
  for (const match of payload.matchAll(RULE_SET_PATTERN)) {
    const body = match[1].startsWith('[') ? match[1].slice(1, -1) : match[1]
    tags.push(...body.split(/\s+/).filter(Boolean))
  }
  return tags
}

type ConditionToken =
  | { kind: 'text' | 'word' | 'mono'; text: string }
  | { kind: 'set'; text: string; full: string; provider: RuleProvider | null }

const FIELD_LABELS: Record<string, string> = {
  domain: '域名',
  domain_suffix: '域名后缀',
  domain_keyword: '域名关键词',
  domain_regex: '域名正则',
  ip_cidr: 'IP 段',
  source_ip_cidr: '来源 IP 段',
  default_interface_address: '默认网卡地址',
  port: '端口',
  source_port: '来源端口',
  port_range: '端口范围',
  source_port_range: '来源端口范围',
  protocol: '协议',
  network: '网络',
  process_name: '进程',
  process_path: '进程路径',
  package_name: '应用',
  inbound: '入站',
  clash_mode: '模式',
  network_type: '网络类型',
  wifi_ssid: 'Wi-Fi',
  wifi_bssid: 'Wi-Fi BSSID',
  query_type: 'DNS 查询类型',
  process_path_regex: '进程路径正则',
  package_name_regex: '应用正则',
  network_is_expensive: '计费网络',
  network_is_constrained: '受限网络',
  user: '用户',
}

const MONO_FIELDS = new Set([
  'ip_cidr',
  'source_ip_cidr',
  'default_interface_address',
  'port',
  'source_port',
  'port_range',
  'source_port_range',
  'domain',
  'domain_suffix',
  'domain_regex',
  'process_path',
])
const UPPER_FIELDS = new Set(['protocol', 'network'])
const CONDITION_ITEM = /([a-z_]+)=(\[[^\]]*\]|\S+)/g

function conditionValues(raw: string): string[] {
  return raw.startsWith('[') ? raw.slice(1, -1).split(/\s+/).filter(Boolean) : [raw]
}

function conditionItemTokens(key: string, raw: string): ConditionToken[] {
  const list = conditionValues(raw)
  if (key === 'rule_set') {
    return list.map((full) => {
      const provider = providerByName.value[full]
      return {
        kind: 'set',
        full,
        text: full.slice(full.lastIndexOf('/') + 1),
        provider: provider && canOpenProvider(provider) ? provider : null,
      }
    })
  }
  if (key === 'ip_is_private') return [{ kind: 'text', text: raw === 'true' ? '私有 IP' : '非私有 IP' }]
  if (key === 'ip_version') return [{ kind: 'text', text: `IPv${raw}` }]
  const label = FIELD_LABELS[key]
  if (!label) return [{ kind: 'mono', text: `${key}=${raw}` }]
  const tokens: ConditionToken[] = [{ kind: 'word', text: label }]
  list.forEach((value, i) => {
    if (i > 0) tokens.push({ kind: 'word', text: '、' })
    tokens.push({
      kind: MONO_FIELDS.has(key) ? 'mono' : 'text',
      text: UPPER_FIELDS.has(key) ? value.toUpperCase() : value,
    })
  })
  return tokens
}

function conditionTermTokens(term: string): ConditionToken[] {
  let body = term.trim()
  const tokens: ConditionToken[] = []
  const negated = /^!\((.*)\)$/.exec(body)
  if (negated) {
    tokens.push({ kind: 'word', text: '不满足' })
    body = negated[1]
  } else if (/^\(.*\)$/.test(body)) {
    body = body.slice(1, -1)
  }
  const items = [...body.matchAll(CONDITION_ITEM)]
  if (items.length === 0) return [...tokens, { kind: 'mono', text: body }]
  items.forEach((match, i) => {
    if (i > 0) tokens.push({ kind: 'word', text: '，' })
    tokens.push(...conditionItemTokens(match[1], match[2]))
  })
  return tokens
}

function conditionTokens(payload: string): ConditionToken[] {
  if (!payload) return [{ kind: 'word', text: '所有流量' }]
  const tokens: ConditionToken[] = []
  for (const part of payload.split(/\s(&&|\|\|)\s/)) {
    if (part === '&&') tokens.push({ kind: 'word', text: '且' })
    else if (part === '||') tokens.push({ kind: 'word', text: '或' })
    else tokens.push(...conditionTermTokens(part))
  }
  return tokens
}

type RuleIcon = 'ban' | 'dns' | 'sniff' | 'resolve' | 'options'

type RuleAction =
  | { kind: 'route'; outbound: string; chain: string[] }
  | { kind: 'reject' | 'stop' | 'pass'; label: string; icon: RuleIcon }

function parseRuleAction(proxy: string): RuleAction {
  const route = /^route\((.*)\)$/.exec(proxy)
  if (route) {
    const outbound = route[1].split(',')[0].trim()
    return { kind: 'route', outbound, chain: resolveProxyChain(outbound) }
  }
  if (proxy === 'reject') return { kind: 'reject', label: '拒绝', icon: 'ban' }
  if (proxy === 'reject(drop)') return { kind: 'reject', label: '丢弃', icon: 'ban' }
  if (proxy.startsWith('reject')) return { kind: 'reject', label: '拒绝', icon: 'ban' }
  if (proxy === 'hijack-dns') return { kind: 'stop', label: '劫持 DNS', icon: 'dns' }
  if (proxy.startsWith('sniff')) return { kind: 'pass', label: '嗅探', icon: 'sniff' }
  if (proxy === 'resolve(match_only)') return { kind: 'pass', label: '解析 · 仅匹配', icon: 'resolve' }
  if (proxy.startsWith('resolve')) return { kind: 'pass', label: '解析', icon: 'resolve' }
  if (proxy.startsWith('route-options')) return { kind: 'pass', label: '路由选项', icon: 'options' }
  return { kind: 'route', outbound: proxy, chain: resolveProxyChain(proxy) }
}

const finalOutbound = ref('')
let finalRequest = 0

async function loadFinalOutbound() {
  const request = ++finalRequest
  let tag = ''
  try {
    const source = await readSingboxConfig(await getRunningConfigPath())
    const parsed = parseJsonWithComments<{ route?: { final?: unknown }; outbounds?: { tag?: unknown }[] }>(source)
    const final = parsed?.route?.final
    const first = parsed?.outbounds?.[0]?.tag
    tag = typeof final === 'string' && final ? final : typeof first === 'string' ? first : ''
  } catch {
    tag = ''
  }
  if (request === finalRequest) finalOutbound.value = tag
}

const finalChain = computed(() => (finalOutbound.value ? resolveProxyChain(finalOutbound.value) : []))

type ProviderUse = { index: number; target: string; chain: string[] }

const providerUsage = computed(() => {
  const usage: Record<string, ProviderUse[]> = {}
  rules.value.forEach((rule, index) => {
    const tags = ruleSetTags(rule.payload)
    if (tags.length === 0) return
    const action = parseRuleAction(rule.proxy)
    const use: ProviderUse = action.kind === 'route'
      ? { index, target: action.outbound, chain: action.chain }
      : { index, target: action.label, chain: [] }
    for (const tag of new Set(tags)) (usage[tag] ??= []).push(use)
  })
  return usage
})

function providerUsageText(name: string): string {
  const uses = providerUsage.value[name]
  if (!uses?.length) return '未被路由规则引用'
  if (uses.length === 1) return `第 ${uses[0].index + 1} 条 → ${uses[0].target}`
  return `第 ${uses.map((use) => use.index + 1).join('、')} 条`
}

function detailTypeLabel(type: string): string {
  return FIELD_LABELS[type] ?? type
}

const searchInput = ref<HTMLInputElement | null>(null)
const ruleQuery = computed(() => filterText.value.trim())
const ruleSetSearch = ref<{ query: string; outcome: MatchOutcome } | null>(null)
const ruleSetSearching = ref(false)
let ruleSetTimer: ReturnType<typeof setTimeout> | null = null
let ruleSetRequest = 0

watch(
  [ruleQuery, searchableProviders],
  ([q, providers]) => {
    if (ruleSetTimer) clearTimeout(ruleSetTimer)
    const request = ++ruleSetRequest
    if (!isLookupQuery(q) || providers.length === 0) {
      ruleSetSearch.value = null
      ruleSetSearching.value = false
      return
    }
    ruleSetSearching.value = true
    ruleSetTimer = setTimeout(async () => {
      const outcome = await matchProviders(q)
      if (request !== ruleSetRequest) return
      ruleSetSearch.value = { query: q, outcome }
      ruleSetSearching.value = false
    }, 400)
  },
  { immediate: true },
)

const matchedRuleSets = computed(() => {
  const search = ruleSetSearch.value
  return search && search.query === ruleQuery.value ? search.outcome.matched : null
})

const ruleSetFailureNote = computed(() => {
  const search = ruleSetSearch.value
  if (!search || search.query !== ruleQuery.value || search.outcome.failures.size === 0) return ''
  return failureSummary(search.outcome.failures)
})

type RuleRow = {
  rule: Rule
  index: number
  tokens: ConditionToken[]
  action: RuleAction
  detailTarget: RuleProvider | null
}

const ruleRows = computed<RuleRow[]>(() => {
  const q = ruleQuery.value.toLowerCase()
  const matchedSets = matchedRuleSets.value
  const rows: RuleRow[] = []
  rules.value.forEach((rule, index) => {
    const textHit = !q
      || rule.payload.toLowerCase().includes(q)
      || rule.proxy.toLowerCase().includes(q)
      || rule.type.toLowerCase().includes(q)
    const setHit = !!matchedSets && matchedSets.size > 0
      && ruleSetTags(rule.payload).some((tag) => matchedSets.has(tag))
    if (!textHit && !setHit) return
    const tokens = conditionTokens(rule.payload)
    const targets = tokens.flatMap((token) => (token.kind === 'set' && token.provider ? [token.provider] : []))
    rows.push({
      rule,
      index,
      tokens,
      action: parseRuleAction(rule.proxy),
      detailTarget: targets.length === 1 ? targets[0] : null,
    })
  })
  return rows
})

function clearRulesSearch() {
  filterText.value = ''
  searchInput.value?.focus()
}

function onRulesSearchEscape(e: KeyboardEvent) {
  if (filterText.value) filterText.value = ''
  else (e.target as HTMLInputElement).blur()
}

const providerSearchInput = ref<HTMLInputElement | null>(null)
const providerSearchText = ref('')
const providerQuery = computed(() => providerSearchText.value.trim())
const providerSearch = ref<{ query: string; outcome: MatchOutcome } | null>(null)
const providerSearching = ref(false)
let providerSearchTimer: ReturnType<typeof setTimeout> | null = null
let providerSearchRequest = 0

watch([providerQuery, searchableProviders], ([q, providers]) => {
  if (providerSearchTimer) clearTimeout(providerSearchTimer)
  const request = ++providerSearchRequest
  if (!q) {
    providerSearch.value = null
    providerSearching.value = false
    return
  }
  if (providers.length === 0) {
    providerSearch.value = { query: q, outcome: { matched: new Set(), failures: new Map() } }
    providerSearching.value = false
    return
  }
  providerSearching.value = true
  providerSearchTimer = setTimeout(async () => {
    const outcome = await matchProviders(q)
    if (request !== providerSearchRequest) return
    providerSearch.value = { query: q, outcome }
    providerSearching.value = false
  }, 500)
})

const displayedProviders = computed(() => {
  const search = providerSearch.value
  if (!providerQuery.value || !search) return ruleProviders.value
  return ruleProviders.value.filter((p) => search.outcome.matched.has(p.name))
})

const providerFailureNote = computed(() => {
  const failures = providerSearch.value?.outcome.failures
  return providerQuery.value && failures && failures.size > 0 ? failureSummary(failures) : ''
})

function clearProviderSearch() {
  providerSearchText.value = ''
  providerSearchInput.value?.focus()
}

function onProviderSearchEscape(e: KeyboardEvent) {
  if (providerSearchText.value) providerSearchText.value = ''
  else (e.target as HTMLInputElement).blur()
}

async function loadProviders(): Promise<string> {
  const request = ++providersRequest
  try {
    const { data } = await fetchRuleProviders()
    if (request !== providersRequest) return ''
    ruleProviders.value = Object.values(data.providers)
    providersAvailable.value = true
    matchCache.clear()
    return ''
  } catch (error) {
    if (request !== providersRequest) return ''
    if (!providersAvailable.value) ruleProviders.value = []
    return getRequestErrorReason(error)
  }
}

async function loadAll() {
  await Promise.all([loadRules(), loadProviders(), loadFinalOutbound()])
}

async function refreshAfterUpdate() {
  const reasons = (await Promise.all([loadProviders(), loadRules(true)])).filter(Boolean)
  if (reasons.length === 0) return
  pushToast({
    type: 'error',
    message: `规则提供商已更新，但刷新列表失败\n原因: ${Array.from(new Set(reasons)).join('；')}`,
  })
}

async function handleUpdateProvider(name: string) {
  if (updatingProviders.value.has(name) || updatingAll.value) return
  updatingProviders.value.add(name)
  try {
    await updateRuleProvider(name)
  } catch (error) {
    pushToast({
      type: 'error',
      message: `更新规则提供商失败\n${name}\n原因: ${getRequestErrorReason(error)}`,
    })
    updatingProviders.value.delete(name)
    return
  }
  announce(`已更新规则提供商 ${name}`)
  await refreshAfterUpdate()
  updatingProviders.value.delete(name)
}

async function handleUpdateAll() {
  if (updatingAll.value) return
  updatingAll.value = true
  const { failed, total } = await batchUpdateProviders(ruleProviders.value, updateRuleProvider, '规则提供商')
  if (failed === 0 && total > 0) announce(`已更新全部 ${total} 个规则提供商`)
  await refreshAfterUpdate()
  updatingAll.value = false
}

// ---- 规则详情弹窗 ----
const detailProvider = ref<RuleProvider | null>(null)
const detailUses = computed(() => (detailProvider.value ? providerUsage.value[detailProvider.value.name] ?? [] : []))
const detailName = computed(() => providerNameParts(detailProvider.value?.name ?? ''))
const detailLoading = ref(false)
const detailError = ref('')
const detailRules = ref<Array<{ type: string; value: string }>>([])
const detailFilterText = ref('')
const detailDialog = ref<HTMLElement | null>(null)
const detailSearchInput = ref<HTMLInputElement | null>(null)
let detailOpener: HTMLElement | null = null
let detailRequest = 0

// 弹窗搜索：调用 Rust 端 srsMatchProvider 做精确匹配
const detailMatchResult = ref<boolean | null>(null) // null=未搜索, true=匹配, false=未匹配
const detailMatchSearching = ref(false)
const detailMatchError = ref('')
let detailSearchTimer: ReturnType<typeof setTimeout> | null = null
let detailMatchRequest = 0

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
  const request = ++detailMatchRequest
  if (!q || !provider) {
    detailMatchResult.value = null
    detailMatchError.value = ''
    return
  }
  detailMatchSearching.value = true
  detailMatchResult.value = null
  detailMatchError.value = ''

  let configPath = ''
  try { configPath = await getRunningConfigPath() } catch {}

  try {
    const matched = await srsMatchProvider(
      config.value.workingDir ?? '',
      configPath,
      config.value.singboxPath ?? '',
      provider.name,
      q,
    )
    if (request === detailMatchRequest) detailMatchResult.value = matched
  } catch (error) {
    if (request === detailMatchRequest) detailMatchError.value = srsFailureReason(error, provider.name)
  } finally {
    if (request === detailMatchRequest) detailMatchSearching.value = false
  }
}

watch(detailFilterText, (val) => {
  if (detailSearchTimer) clearTimeout(detailSearchTimer)
  if (!val.trim()) {
    detailMatchRequest++
    detailMatchResult.value = null
    detailMatchError.value = ''
    detailMatchSearching.value = false
    return
  }
  detailSearchTimer = setTimeout(searchInDetail, 500)
})

const detailQuery = computed(() => detailFilterText.value.trim())

const detailMismatchNote = computed(() => {
  if (!detailQuery.value || detailMatchSearching.value) return ''
  if (detailMatchError.value) return detailMatchError.value
  if (detailMatchResult.value === null) return ''
  const shown = filteredDetailRules.value.length
  if (detailMatchResult.value && shown === 0) return '规则提供商命中，但列表里找不到对应条目，可能由正则或其他条件命中'
  if (!detailMatchResult.value && shown > 0) return '规则提供商未命中，列表中是文字包含该内容的条目'
  return ''
})

// 虚拟滚动
const ROW_HEIGHT = 28
const OVERSCAN = 10
const detailScrollTop = ref(0)
const detailContainerHeight = ref(400)
const detailScrollRef = ref<HTMLElement | null>(null)
let detailResizeObserver: ResizeObserver | null = null

watch(detailScrollRef, (el) => {
  detailResizeObserver?.disconnect()
  detailResizeObserver = null
  if (!el) return
  detailContainerHeight.value = el.clientHeight || 400
  if (typeof ResizeObserver === 'undefined') return
  detailResizeObserver = new ResizeObserver(() => {
    detailContainerHeight.value = el.clientHeight || 400
  })
  detailResizeObserver.observe(el)
})

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

async function loadDetailRules(provider: RuleProvider) {
  const request = ++detailRequest
  detailLoading.value = true
  detailError.value = ''
  detailRules.value = []

  let configPath = ''
  try {
    configPath = await getRunningConfigPath()
  } catch {}

  try {
    const entries = await srsListProvider(
      config.value.workingDir ?? '',
      configPath,
      config.value.singboxPath ?? '',
      provider.name,
    )
    if (request === detailRequest) detailRules.value = entries
  } catch (error) {
    if (request === detailRequest) detailError.value = srsFailureReason(error, provider.name)
  } finally {
    if (request === detailRequest) detailLoading.value = false
  }
}

function openProviderDetail(provider: RuleProvider) {
  if (!detailProvider.value) detailOpener = document.activeElement as HTMLElement | null
  detailProvider.value = provider
  detailFilterText.value = ''
  detailScrollTop.value = 0
  void nextTick(() => detailSearchInput.value?.focus())
  void loadDetailRules(provider)
}

function closeProviderDetail() {
  detailRequest++
  detailMatchRequest++
  detailProvider.value = null
  detailRules.value = []
  detailFilterText.value = ''
  detailError.value = ''
  detailLoading.value = false
  detailScrollTop.value = 0
  detailMatchResult.value = null
  detailMatchError.value = ''
  detailMatchSearching.value = false
  if (detailSearchTimer) { clearTimeout(detailSearchTimer); detailSearchTimer = null }
  if (filterTimer) { clearTimeout(filterTimer); filterTimer = null }
  const opener = detailOpener
  detailOpener = null
  void nextTick(() => {
    if (opener?.isConnected) opener.focus()
  })
}

function onDetailSearchEscape() {
  if (detailFilterText.value) detailFilterText.value = ''
  else closeProviderDetail()
}

function clearDetailSearch() {
  detailFilterText.value = ''
  detailSearchInput.value?.focus()
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

function isFindShortcut(e: KeyboardEvent): boolean {
  return (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'f'
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (detailProvider.value) {
    if (e.key === 'Escape') {
      e.preventDefault()
      closeProviderDetail()
    } else if (e.key === 'Tab') {
      trapDetailFocus(e)
    } else if (isFindShortcut(e)) {
      e.preventDefault()
      detailSearchInput.value?.focus()
      detailSearchInput.value?.select()
    }
    return
  }
  if (!isFindShortcut(e) || !isRunning.value || document.querySelector('[role="alertdialog"]')) return
  e.preventDefault()
  const input = activeTab.value === 'providers' ? providerSearchInput.value : searchInput.value
  input?.focus()
  input?.select()
}

watch(isRunning, (running) => {
  if (!running && detailProvider.value) closeProviderDetail()
})

watch(serviceState, (state, previous) => {
  if (state !== 'running' || previous === 'running') return
  void loadAll()
})

onMounted(() => {
  window.addEventListener('keydown', onGlobalKeydown)
  if (isRunning.value) void loadAll()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onGlobalKeydown)
  for (const timer of [ruleSetTimer, providerSearchTimer, detailSearchTimer, filterTimer]) {
    if (timer) clearTimeout(timer)
  }
  ruleSetRequest++
  providerSearchRequest++
  detailRequest++
  detailMatchRequest++
  detailResizeObserver?.disconnect()
})
</script>

<template>
  <div class="rules-page flex min-w-0 flex-col h-full gap-3">
    <span class="sr-only" aria-live="polite">{{ announcement }}</span>
    <ServiceNotice
      v-if="!isRunning"
      :state="serviceState"
      page-title="规则"
      stopped-detail="启动服务后即可查看路由规则和规则提供商。"
      starting-detail="核心启动后会自动加载规则。"
    />

    <template v-else>
    <div class="flex items-center">
      <div class="flex flex-wrap items-center gap-3">
        <h1 class="text-[26px] leading-tight tracking-tight font-bold shrink-0">规则</h1>
        <div class="tabs tabs-boxed tabs-sm" role="tablist" aria-label="规则视图" @keydown="onTabKeydown">
          <button
            id="rules-tab-rules"
            type="button"
            role="tab"
            class="tab"
            :class="{ 'tab-active': activeTab === 'rules' }"
            :aria-selected="activeTab === 'rules'"
            aria-controls="rules-panel-rules"
            :tabindex="activeTab === 'rules' ? 0 : -1"
            @click="activeTab = 'rules'"
          >
            规则<span class="ml-1 tabular-nums" aria-hidden="true">({{ rules.length }})</span>
          </button>
          <button
            v-if="providersAvailable"
            id="rules-tab-providers"
            type="button"
            role="tab"
            class="tab"
            :class="{ 'tab-active': activeTab === 'providers' }"
            :aria-selected="activeTab === 'providers'"
            aria-controls="rules-panel-providers"
            :tabindex="activeTab === 'providers' ? 0 : -1"
            @click="activeTab = 'providers'"
          >
            规则提供商<span class="ml-1 tabular-nums" aria-hidden="true">({{ ruleProviders.length }})</span>
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

    <div
      v-show="activeTab === 'rules'"
      id="rules-panel-rules"
      role="tabpanel"
      aria-labelledby="rules-tab-rules"
      class="flex min-h-0 flex-1 flex-col gap-3"
    >
      <div class="rules-toolbar">
        <div class="relative flex min-w-0 flex-1 items-center">
          <input
            ref="searchInput"
            v-model="filterText"
            type="search"
            class="input input-sm input-bordered rules-search pl-8 [&::-webkit-search-cancel-button]:appearance-none"
            :class="{ 'pr-8': filterText }"
            placeholder="搜索规则或出站，输入域名 / IP 时也会查找命中的规则提供商"
            aria-label="搜索规则"
            aria-keyshortcuts="Control+F"
            title="搜索规则 (Ctrl+F)"
            :aria-busy="ruleSetSearching"
            @keydown.esc="onRulesSearchEscape"
          />
          <span v-if="ruleSetSearching" class="loading loading-spinner loading-xs pointer-events-none absolute left-2.5 z-[1] text-base-content/55" aria-hidden="true"></span>
          <svg v-else class="pointer-events-none absolute left-2.5 z-[1] h-3.5 w-3.5 text-base-content/55" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" />
            <path d="m10.5 10.5 3 3" />
          </svg>
          <button
            v-if="filterText"
            type="button"
            class="absolute right-1 z-[1] flex h-6 w-6 items-center justify-center rounded-full text-base-content/55 transition-colors hover:bg-[var(--fill-strong)] hover:text-base-content focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
            aria-label="清除搜索"
            @click="clearRulesSearch"
          >
            <svg class="h-2.5 w-2.5" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
              <path d="M2 2l6 6M8 2 2 8" />
            </svg>
          </button>
        </div>
      </div>

      <p v-if="ruleSetFailureNote && ruleRows.length > 0" class="rules-note" role="status">
        {{ ruleSetFailureNote }}，结果可能不完整
      </p>

      <div class="surface-card flex min-h-0 flex-1 flex-col overflow-hidden">
        <div class="rules-table-container card-scroll">
          <ol v-if="ruleRows.length > 0" class="rules-flow" aria-label="规则列表，从上到下依次匹配">
            <li
              v-for="row in ruleRows"
              :key="row.index"
              class="rules-step"
              :class="[`rules-step--${row.action.kind}`, { 'cursor-pointer': row.detailTarget }]"
              :title="`${row.rule.payload || '所有流量'} → ${row.rule.proxy}`"
              @click="row.detailTarget && openProviderDetail(row.detailTarget)"
            >
              <span class="rules-rail" aria-hidden="true"><span class="rules-node"></span></span>
              <span class="rules-step-index">{{ row.index + 1 }}</span>
              <span class="rules-cond">
                <template v-for="(token, k) in row.tokens" :key="k">
                  <template v-if="token.kind === 'set'">
                    <button
                      v-if="token.provider"
                      type="button"
                      class="rules-set"
                      :class="{ 'rules-set-hit': matchedRuleSets?.has(token.full) }"
                      :aria-label="`查看规则提供商 ${token.full} 的详情`"
                      @click.stop="openProviderDetail(token.provider)"
                    >
                      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" aria-hidden="true"><path d="M8 2.5 13.5 5 8 7.5 2.5 5z" /><path d="m2.5 8 5.5 2.5L13.5 8" /><path d="m2.5 11 5.5 2.5 5.5-2.5" /></svg>
                      {{ token.text }}
                    </button>
                    <span v-else class="rules-set" :class="{ 'rules-set-hit': matchedRuleSets?.has(token.full) }">
                      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" aria-hidden="true"><path d="M8 2.5 13.5 5 8 7.5 2.5 5z" /><path d="m2.5 8 5.5 2.5L13.5 8" /><path d="m2.5 11 5.5 2.5 5.5-2.5" /></svg>
                      {{ token.text }}
                    </span>
                  </template>
                  <span v-else-if="token.kind === 'word'" class="rules-word">{{ token.text }}</span>
                  <span v-else-if="token.kind === 'mono'" class="rules-mono">{{ token.text }}</span>
                  <span v-else>{{ token.text }}</span>
                </template>
              </span>
              <span
                v-if="row.action.kind === 'route'"
                class="rules-dest"
                :title="row.action.chain.join(' › ')"
              >
                <span class="rules-dest-group">{{ row.action.outbound }}</span>
                <span
                  v-if="row.action.chain.length > 1"
                  class="rules-dest-node"
                  :class="{ 'rules-dest-node--reject': isRejectNode(row.action.chain[row.action.chain.length - 1]) }"
                >› {{ row.action.chain[row.action.chain.length - 1] }}</span>
              </span>
              <span v-else class="rules-act" :class="`rules-act--${row.action.kind}`">
                <svg v-if="row.action.icon === 'ban'" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><circle cx="8" cy="8" r="5.5" /><path d="m4.2 11.8 7.6-7.6" /></svg>
                <svg v-else-if="row.action.icon === 'dns'" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4 3 7l3 3" /><path d="M3 7h6.5a3.5 3.5 0 0 1 0 7H8" /></svg>
                <svg v-else-if="row.action.icon === 'sniff'" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M2.5 5.5v-3h3M10.5 2.5h3v3M13.5 10.5v3h-3M5.5 13.5h-3v-3" /><circle cx="8" cy="8" r="1.6" /></svg>
                <svg v-else-if="row.action.icon === 'resolve'" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><circle cx="8" cy="8" r="5.5" /><path d="M2.5 8h11M8 2.5c1.6 1.6 2.3 3.5 2.3 5.5S9.6 11.9 8 13.5C6.4 11.9 5.7 10 5.7 8S6.4 4.1 8 2.5z" /></svg>
                <svg v-else viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M3 5h10M3 11h10" /><circle cx="6" cy="5" r="1.5" /><circle cx="10" cy="11" r="1.5" /></svg>
                {{ row.action.label }}<span v-if="row.action.kind === 'pass'" class="sr-only">，处理后继续匹配下一条</span>
              </span>
            </li>
            <li
              v-if="!ruleQuery && finalChain.length > 0"
              class="rules-step rules-step--final"
              :title="finalChain.join(' › ')"
            >
              <span class="rules-rail" aria-hidden="true"><span class="rules-node"></span></span>
              <span class="rules-step-index"></span>
              <span class="rules-cond"><span class="rules-word">以上都没有命中</span></span>
              <span class="rules-dest">
                <span class="rules-dest-group">{{ finalChain[0] }}</span>
                <span
                  v-if="finalChain.length > 1"
                  class="rules-dest-node"
                  :class="{ 'rules-dest-node--reject': isRejectNode(finalChain[finalChain.length - 1]) }"
                >› {{ finalChain[finalChain.length - 1] }}</span>
              </span>
            </li>
          </ol>

          <div v-if="loading && rules.length === 0" class="rules-empty" role="status">
            <span class="loading loading-spinner loading-md" aria-hidden="true"></span>
            <span class="sr-only">正在加载规则</span>
          </div>

          <div
            v-else-if="loadError && rules.length === 0"
            class="mx-auto mt-6 max-w-[520px] px-6 py-5"
            role="alert"
          >
            <p class="text-base font-semibold">无法读取规则列表</p>
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
              <button type="button" class="btn btn-sm btn-primary px-5" @click="loadAll()">重试</button>
              <button type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">检查 API 配置</button>
            </div>
          </div>

          <div v-else-if="loaded && rules.length === 0" class="rules-empty-block">
            <p class="text-sm text-base-content/80">当前配置里没有路由规则</p>
            <p class="text-xs text-base-content/65">在 config.json 的 route.rules 中添加规则后，这里会按优先级列出它们。</p>
          </div>

          <div v-else-if="ruleRows.length === 0 && ruleSetSearching" class="rules-empty" role="status">
            <span class="loading loading-spinner loading-sm" aria-hidden="true"></span>
            <span class="ml-2">正在查找命中的规则提供商…</span>
          </div>

          <div v-else-if="ruleRows.length === 0 && ruleQuery" class="rules-empty-block" role="status">
            <p class="text-sm text-base-content/80">
              {{ isLookupQuery(ruleQuery) ? `没有规则或规则提供商命中“${ruleQuery}”` : `没有找到包含“${ruleQuery}”的规则` }}
            </p>
            <p v-if="ruleSetFailureNote" class="text-xs text-base-content/65">{{ ruleSetFailureNote }}</p>
            <button type="button" class="btn btn-sm btn-glass mt-1 px-5" @click="clearRulesSearch">清除搜索</button>
          </div>
        </div>
      </div>
    </div>

    <div
      v-if="providersAvailable"
      v-show="activeTab === 'providers'"
      id="rules-panel-providers"
      role="tabpanel"
      aria-labelledby="rules-tab-providers"
      class="flex min-h-0 flex-1 flex-col gap-3"
    >
      <div class="rules-toolbar">
        <div class="relative flex min-w-0 flex-1 items-center">
          <input
            ref="providerSearchInput"
            v-model="providerSearchText"
            type="search"
            class="input input-sm input-bordered rules-search pl-8 [&::-webkit-search-cancel-button]:appearance-none"
            :class="{ 'pr-8': providerSearchText }"
            placeholder="输入域名或 IP，查找命中的规则提供商"
            aria-label="按域名或 IP 查找规则提供商"
            aria-keyshortcuts="Control+F"
            title="按域名或 IP 查找规则提供商 (Ctrl+F)"
            :aria-busy="providerSearching"
            @keydown.esc="onProviderSearchEscape"
          />
          <span v-if="providerSearching" class="loading loading-spinner loading-xs pointer-events-none absolute left-2.5 z-[1] text-base-content/55" aria-hidden="true"></span>
          <svg v-else class="pointer-events-none absolute left-2.5 z-[1] h-3.5 w-3.5 text-base-content/55" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" />
            <path d="m10.5 10.5 3 3" />
          </svg>
          <button
            v-if="providerSearchText"
            type="button"
            class="absolute right-1 z-[1] flex h-6 w-6 items-center justify-center rounded-full text-base-content/55 transition-colors hover:bg-[var(--fill-strong)] hover:text-base-content focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
            aria-label="清除搜索"
            @click="clearProviderSearch"
          >
            <svg class="h-2.5 w-2.5" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
              <path d="M2 2l6 6M8 2 2 8" />
            </svg>
          </button>
        </div>
      </div>

      <p v-if="providerFailureNote && displayedProviders.length > 0" class="rules-note" role="status">
        {{ providerFailureNote }}，结果可能不完整
      </p>

      <div class="surface-card flex min-h-0 flex-1 flex-col overflow-hidden">
        <div class="rules-table-container card-scroll">
          <ul v-if="displayedProviders.length > 0" class="rules-flow" aria-label="规则提供商列表">
            <li
              v-for="provider in displayedProviders"
              :key="provider.name"
              class="rules-provider-line"
              :class="{ 'cursor-pointer': canOpenProvider(provider) }"
              @click="canOpenProvider(provider) && openProviderDetail(provider)"
            >
              <span class="rules-provider-tile" aria-hidden="true">
                <svg v-if="providerVehicle(provider) === 'remote' || providerVehicle(provider) === 'http'" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 12.5a3 3 0 0 1-.4-5.97A4 4 0 0 1 11.8 6a2.75 2.75 0 0 1 .2 5.5" /><path d="M8 8.5v5M6.2 11.7 8 13.5l1.8-1.8" /></svg>
                <svg v-else-if="isInlineProvider(provider)" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3c-1.5 0-2 .6-2 1.8v1.4C4 7.3 3.5 8 2.5 8 3.5 8 4 8.7 4 9.8v1.4C4 12.4 4.5 13 6 13M10 3c1.5 0 2 .6 2 1.8v1.4c0 1.1.5 1.8 1.5 1.8-1 0-1.5.7-1.5 1.8v1.4c0 1.2-.5 1.8-2 1.8" /></svg>
                <svg v-else viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M4 2.5h5l3 3v8H4z" /><path d="M9 2.5v3h3" /></svg>
              </span>
              <div class="min-w-0">
                <button
                  v-if="canOpenProvider(provider)"
                  type="button"
                  class="rules-provider-name block w-full rounded-sm text-left"
                  :title="provider.name"
                  :aria-label="`查看规则提供商 ${provider.name} 的详情`"
                  @click.stop="openProviderDetail(provider)"
                ><span v-if="providerNameParts(provider.name).prefix" class="rules-name-prefix">{{ providerNameParts(provider.name).prefix }}</span>{{ providerNameParts(provider.name).base }}</button>
                <div v-else class="rules-provider-name" :title="provider.name"><span v-if="providerNameParts(provider.name).prefix" class="rules-name-prefix">{{ providerNameParts(provider.name).prefix }}</span>{{ providerNameParts(provider.name).base }}</div>
                <div class="rules-provider-meta">
                  {{ providerMeta(provider) }} ·
                  <span :class="{ 'text-base-content/45': !providerUsage[provider.name] }">{{ providerUsageText(provider.name) }}</span>
                </div>
              </div>
              <span class="rules-provider-count"><b>{{ provider.ruleCount.toLocaleString() }}</b> 条</span>
              <button
                v-if="!isInlineProvider(provider)"
                type="button"
                class="btn btn-ghost btn-sm btn-circle"
                :aria-busy="updatingProviders.has(provider.name)"
                :disabled="updatingProviders.has(provider.name) || updatingAll"
                @click.stop="handleUpdateProvider(provider.name)"
                title="更新"
                :aria-label="`更新规则提供商 ${provider.name}`"
              >
                <span v-if="updatingProviders.has(provider.name)" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
                <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-3.5 h-3.5" aria-hidden="true">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.992 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
                </svg>
              </button>
              <span v-else aria-hidden="true"></span>
            </li>
          </ul>

          <div v-if="ruleProviders.length === 0" class="rules-empty-block">
            <p class="text-sm text-base-content/80">当前配置里没有规则提供商</p>
            <p class="text-xs text-base-content/65">在 config.json 的 route.rule_set 中添加规则集后，这里会列出它们。</p>
          </div>

          <div
            v-else-if="providerQuery && providerSearch && !providerSearching && displayedProviders.length === 0"
            class="rules-empty-block"
            role="status"
          >
            <p class="text-sm text-base-content/80">没有规则提供商命中“{{ providerQuery }}”</p>
            <p v-if="providerFailureNote" class="text-xs text-base-content/65">{{ providerFailureNote }}</p>
            <button type="button" class="btn btn-sm btn-glass mt-1 px-5" @click="clearProviderSearch">清除搜索</button>
          </div>
        </div>
      </div>
    </div>
    </template>

    <!-- 规则详情弹窗 -->
    <Teleport to="body">
    <Transition name="glass-pop">
    <div
      v-if="detailProvider"
      class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      @click.self="closeProviderDetail"
    >
      <div
        ref="detailDialog"
        class="glass-popover w-full max-w-2xl max-h-[80vh] flex flex-col rounded-[var(--radius-panel)]"
        role="dialog"
        aria-modal="true"
        aria-labelledby="rule-detail-title"
      >
        <div class="flex shrink-0 items-start gap-3 px-5 pt-5 pb-3">
          <span class="rules-provider-tile rules-provider-tile--lg" aria-hidden="true">
            <svg v-if="providerVehicle(detailProvider) === 'remote' || providerVehicle(detailProvider) === 'http'" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 12.5a3 3 0 0 1-.4-5.97A4 4 0 0 1 11.8 6a2.75 2.75 0 0 1 .2 5.5" /><path d="M8 8.5v5M6.2 11.7 8 13.5l1.8-1.8" /></svg>
            <svg v-else viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M4 2.5h5l3 3v8H4z" /><path d="M9 2.5v3h3" /></svg>
          </span>
          <div class="min-w-0 flex-1">
            <h2 id="rule-detail-title" class="truncate text-base font-semibold leading-[22px]" :title="detailProvider.name">
              <span v-if="detailName.prefix" class="rules-name-prefix">{{ detailName.prefix }}</span>{{ detailName.base }}
            </h2>
            <p class="mt-0.5 text-xs text-base-content/65">
              {{ providerSource(detailProvider) }} · <span class="tabular-nums">{{ detailProvider.ruleCount.toLocaleString() }}</span> 条<template v-if="detailProvider.updatedAt"> · {{ formatDate(detailProvider.updatedAt) }}更新</template>
            </p>
            <p v-if="detailUses.length === 1" class="rules-usage-pill" :title="[detailUses[0].target, ...detailUses[0].chain.slice(1)].join(' › ')">
              <span class="text-base-content/65">用于第 {{ detailUses[0].index + 1 }} 条规则 →</span>
              <span class="font-semibold">{{ detailUses[0].target }}</span>
              <span v-if="detailUses[0].chain.length > 1" class="min-w-0 truncate text-base-content/65">› {{ detailUses[0].chain[detailUses[0].chain.length - 1] }}</span>
            </p>
            <p v-else-if="detailUses.length > 1" class="rules-usage-pill">
              <span class="text-base-content/65">用于第 {{ detailUses.map((use) => use.index + 1).join('、') }} 条规则</span>
            </p>
          </div>
          <button
            type="button"
            class="btn btn-sm btn-circle btn-ghost shrink-0"
            aria-label="关闭规则详情"
            @click="closeProviderDetail"
          >
            <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3 3 9" />
            </svg>
          </button>
        </div>

        <div class="px-5 pb-2 shrink-0 flex items-center gap-2">
          <div class="relative flex min-w-0 flex-1 items-center">
            <input
              ref="detailSearchInput"
              v-model="detailFilterText"
              type="search"
              class="input input-sm input-bordered w-full min-w-0 pl-8 text-xs [&::-webkit-search-cancel-button]:appearance-none"
              :class="{ 'pr-8': detailFilterText }"
              placeholder="搜索规则内容，或输入域名 / IP 判断是否命中"
              aria-label="搜索这个规则提供商的规则"
              aria-keyshortcuts="Control+F"
              :aria-busy="detailMatchSearching"
              @keydown.esc.stop.prevent="onDetailSearchEscape"
            />
            <span v-if="detailMatchSearching" class="loading loading-spinner loading-xs pointer-events-none absolute left-2.5 z-[1] text-base-content/55" aria-hidden="true"></span>
            <svg v-else class="pointer-events-none absolute left-2.5 z-[1] h-3.5 w-3.5 text-base-content/55" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
              <circle cx="7" cy="7" r="4.5" />
              <path d="m10.5 10.5 3 3" />
            </svg>
            <button
              v-if="detailFilterText"
              type="button"
              class="absolute right-1 z-[1] flex h-6 w-6 items-center justify-center rounded-full text-base-content/55 transition-colors hover:bg-[var(--fill-strong)] hover:text-base-content focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              aria-label="清除搜索"
              @click="clearDetailSearch"
            >
              <svg class="h-2.5 w-2.5" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
                <path d="M2 2l6 6M8 2 2 8" />
              </svg>
            </button>
          </div>
          <span class="shrink-0" role="status">
            <span
              v-if="detailQuery && !detailMatchSearching && (detailMatchResult !== null || detailMatchError)"
              class="rules-match-pill"
              :class="{ 'rules-match-pill--hit': detailMatchResult && !detailMatchError }"
            >
              <svg v-if="detailMatchResult && !detailMatchError" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m3.5 8.5 3 3 6-7" /></svg>
              {{ detailMatchError ? '无法查询' : detailMatchResult ? '命中' : '未命中' }}
            </span>
          </span>
        </div>

        <div class="flex-1 flex flex-col px-5 pb-4 min-h-0">
          <div v-if="detailLoading" class="flex justify-center py-10" role="status">
            <span class="loading loading-spinner loading-md" aria-hidden="true"></span>
            <span class="sr-only">正在读取规则</span>
          </div>
          <div v-else-if="detailError" class="py-4" role="alert">
            <p class="text-sm font-medium text-[color:var(--danger-ink)]">无法读取这个规则提供商</p>
            <p class="mt-1 break-words text-xs text-base-content/70">{{ detailError }}</p>
            <button type="button" class="btn btn-sm btn-glass mt-3 px-5" @click="detailProvider && loadDetailRules(detailProvider)">重试</button>
          </div>
          <template v-else>
            <div
              ref="detailScrollRef"
              class="rules-entries card-scroll"
              @scroll="onDetailScroll"
            >
              <p v-if="filteredDetailRules.length === 0" class="py-10 text-center text-sm text-base-content/65">
                {{ detailRules.length === 0 ? '这个规则提供商没有规则' : `没有包含“${detailQuery}”的条目` }}
              </p>
              <div v-else :style="{ height: virtualSlice.totalHeight + 'px', position: 'relative' }">
                <div :style="{ transform: `translateY(${virtualSlice.offsetY}px)` }">
                  <div
                    v-for="(rule, j) in virtualSlice.items"
                    :key="virtualSlice.startIdx + j"
                    class="rules-entry"
                    :style="{ height: ROW_HEIGHT + 'px' }"
                  >
                    <span class="rules-entry-index">{{ virtualSlice.startIdx + j + 1 }}</span>
                    <span class="rules-entry-type">{{ filteredDetailRules[virtualSlice.startIdx + j - 1]?.type === rule.type && j > 0 ? '' : detailTypeLabel(rule.type) }}</span>
                    <span class="rules-entry-value" :title="`${detailTypeLabel(rule.type)} ${rule.value}`">{{ rule.value }}</span>
                  </div>
                </div>
              </div>
            </div>
            <p v-if="detailRules.length > 0" class="text-xs tabular-nums text-base-content/65 pt-2 shrink-0">
              <template v-if="detailQuery">
                显示 {{ filteredDetailRules.length }} / 共 {{ detailRules.length }} 条
              </template>
              <template v-else>
                共 {{ detailRules.length }} 条
              </template>
              <template v-if="detailMismatchNote"> · {{ detailMismatchNote }}</template>
            </p>
          </template>
        </div>
      </div>
    </div>
    </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.rules-toolbar {
  @apply flex shrink-0 items-center gap-2;
  min-height: 32px;
}

.rules-search {
  @apply w-full min-w-0 text-xs;
}

.rules-note {
  @apply -mt-1 shrink-0 px-1 text-xs text-base-content/70;
}

.rules-table-container {
  @apply flex-1 min-h-0 overflow-auto;
}

.rules-flow {
  @apply m-0 list-none p-1.5;
}

.rules-step {
  @apply relative grid items-center gap-x-3 rounded-[var(--radius-tile)] transition-colors hover:bg-base-content/[0.05];
  grid-template-columns: 14px 1.5rem minmax(0, 1fr) auto;
  min-height: 42px;
  padding: 6px 12px 6px 10px;
}

.rules-rail {
  @apply relative flex items-center justify-center self-stretch;
}

.rules-rail::before {
  content: '';
  position: absolute;
  top: -6px;
  bottom: -6px;
  left: 50%;
  width: 1.5px;
  transform: translateX(-50%);
  border-radius: 1px;
  background: oklch(var(--bc) / 0.2);
}

.rules-flow > .rules-step:first-child .rules-rail::before {
  top: 50%;
}

.rules-flow > .rules-step:last-child .rules-rail::before {
  bottom: 50%;
}

.rules-flow > .rules-step:only-child .rules-rail::before {
  display: none;
}

.rules-node {
  @apply relative z-[1] rounded-full;
  width: 9px;
  height: 9px;
  background: oklch(var(--bc) / 0.42);
}

.rules-step--pass .rules-node {
  background: rgb(var(--glass-rgb));
  box-shadow: inset 0 0 0 1.5px oklch(var(--bc) / 0.42);
}

.rules-step--reject .rules-node {
  background: #ff3b30;
  box-shadow: 0 0 0 3px rgb(255 59 48 / 0.22);
}

.rules-step--final {
  @apply mt-1 bg-[var(--fill)] hover:bg-[var(--fill-strong)];
}

.rules-step--final .rules-node {
  width: 11px;
  height: 11px;
  border-radius: 4px;
  background: oklch(var(--bc) / 0.7);
}

.rules-step-index {
  @apply text-right text-[11px] tabular-nums text-base-content/55;
}

.rules-cond {
  @apply min-w-0 truncate text-[13px] leading-[22px];
}

.rules-cond > * + * {
  margin-left: 5px;
}

.rules-word {
  @apply text-base-content/60;
}

.rules-mono {
  font-family: 'Cascadia Mono', 'SFMono-Regular', Consolas, monospace;
  @apply text-xs tabular-nums;
}

.rules-set {
  @apply inline-flex h-[22px] items-center gap-[5px] rounded-full bg-[var(--fill)] pl-2 pr-2.5 align-top text-xs font-medium text-base-content transition-colors;
}

.rules-set svg {
  @apply h-[13px] w-[13px] text-base-content/55;
}

button.rules-set:hover {
  @apply bg-[var(--fill-strong)];
}

button.rules-set:focus-visible {
  outline: 2px solid var(--primary-fill);
  outline-offset: 2px;
}

.rules-set-hit,
button.rules-set-hit:hover {
  @apply bg-[#007aff]/15 font-semibold;
  color: var(--selection-ink);
}

.rules-set-hit svg {
  color: currentColor;
}

.rules-dest {
  @apply inline-flex min-w-0 max-w-[22rem] items-baseline gap-1.5 justify-self-end whitespace-nowrap text-[13px];
}

.rules-dest-group {
  @apply shrink-0 font-semibold;
}

.rules-dest-node {
  @apply min-w-0 truncate text-base-content/60;
}

.rules-dest-node--reject {
  color: var(--danger-ink);
}

.rules-act {
  @apply inline-flex h-6 items-center gap-[5px] justify-self-end whitespace-nowrap rounded-full bg-[var(--fill)] pl-[9px] pr-[11px] text-xs font-medium text-base-content/75;
}

.rules-act svg {
  @apply h-[13px] w-[13px];
}

.rules-act--pass {
  @apply bg-base-content/[0.04] text-base-content/65;
}

.rules-act--reject {
  @apply bg-error/[0.12];
  color: var(--danger-ink);
}

.rules-provider-line {
  @apply grid items-center gap-x-3 rounded-[var(--radius-tile)] transition-colors hover:bg-base-content/[0.05];
  grid-template-columns: 30px minmax(0, 1fr) auto 2rem;
  min-height: 52px;
  padding: 7px 8px 7px 10px;
}

.rules-provider-tile {
  @apply flex shrink-0 items-center justify-center rounded-[var(--radius-control)] bg-[var(--fill)] text-base-content/60;
  width: 30px;
  height: 30px;
}

.rules-provider-tile svg {
  @apply h-4 w-4;
}

.rules-provider-tile--lg {
  @apply rounded-[var(--radius-tile)];
  width: 40px;
  height: 40px;
}

.rules-provider-tile--lg svg {
  @apply h-5 w-5;
}

.rules-provider-name {
  @apply truncate text-[13px] font-semibold leading-5 text-base-content transition-colors;
}

button.rules-provider-name:hover {
  color: var(--selection-ink);
}

button.rules-provider-name:hover .rules-name-prefix {
  color: inherit;
  opacity: 0.6;
}

.rules-provider-name:focus-visible {
  outline: 2px solid var(--primary-fill);
  outline-offset: 1px;
}

.rules-name-prefix {
  @apply font-normal text-base-content/45;
}

.rules-provider-meta {
  @apply truncate text-xs leading-[18px] text-base-content/65;
}

.rules-provider-count {
  @apply whitespace-nowrap text-xs tabular-nums text-base-content/55;
}

.rules-provider-count b {
  @apply text-[13px] font-semibold text-base-content;
}

.rules-usage-pill {
  @apply mt-1.5 inline-flex max-w-full items-baseline gap-[5px] whitespace-nowrap rounded-full bg-[var(--fill)] px-2.5 py-[3px] text-xs;
}

.rules-match-pill {
  @apply inline-flex h-[26px] items-center gap-1 rounded-full bg-base-content/[0.06] px-2.5 text-xs text-base-content/65;
}

.rules-match-pill svg {
  @apply h-[13px] w-[13px];
}

.rules-match-pill--hit {
  @apply bg-[#007aff]/15 pl-[9px] font-semibold;
  color: var(--selection-ink);
}

.rules-entries {
  @apply min-h-0 flex-1 overflow-auto rounded-[var(--radius-tile)] bg-[var(--fill)] px-1;
}

.rules-entry {
  @apply grid items-center gap-x-2.5 rounded-lg px-2.5 hover:bg-base-content/[0.05];
  grid-template-columns: 2.5rem 6rem minmax(0, 1fr);
}

.rules-entry-index {
  @apply text-right text-[11px] tabular-nums text-base-content/55;
}

.rules-entry-type {
  @apply truncate text-xs text-base-content/65;
}

.rules-entry-value {
  @apply truncate text-xs;
  font-family: 'Cascadia Mono', 'SFMono-Regular', Consolas, monospace;
}

.rules-empty {
  @apply flex items-center justify-center py-10 text-sm text-base-content/65;
}

.rules-empty-block {
  @apply flex flex-col items-center gap-1 px-6 py-12 text-center;
}

@media (prefers-reduced-motion: reduce) {
  .rules-step,
  .rules-set,
  .rules-provider-line {
    transition: none;
  }
}
</style>
