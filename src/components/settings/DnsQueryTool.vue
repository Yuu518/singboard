<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { queryDns, type DnsRecord, type DnsQueryResult } from '@/api/dns'
import { getGeoIPForIP, type IPGeoInfo } from '@/api/geoip'
import { useToastStore } from '@/stores/toast'
import { failureMessage } from '@/utils/humanError'

type DnsRecordWithGeo = DnsRecord & {
  geo?: IPGeoInfo
  geoLoading?: boolean
}

interface DnsHistoryItem {
  domain: string
}

const HISTORY_KEY = 'singboard-dns-history'
const HISTORY_LIMIT = 3
const recordTypes = ['A', 'AAAA', 'CNAME']
const presetServers = [
  { label: 'Google', value: '8.8.8.8' },
  { label: 'Cloudflare', value: '1.1.1.1' },
  { label: 'AliDNS', value: '223.5.5.5' },
  { label: 'DNSPod', value: '119.29.29.29' },
]

const { pushToast } = useToastStore()
const domain = ref('')
const recordType = ref('A')
const server = ref('')
const loading = ref(false)
const GEO_KEY = 'singboard-dns-geo'
const geoEnabled = ref(localStorage.getItem(GEO_KEY) === 'on')

function setGeoEnabled(event: Event) {
  const enabled = (event.target as HTMLInputElement).checked
  geoEnabled.value = enabled
  try {
    localStorage.setItem(GEO_KEY, enabled ? 'on' : 'off')
  } catch { }
  if (enabled && records.value.some((item) => item.ip && !item.geo)) void enrichGeo(records.value, queryGeneration)
}
const result = ref<DnsQueryResult | null>(null)
const records = ref<DnsRecordWithGeo[]>([])
const historyOpen = ref(false)
const typeMenuOpen = ref(false)
const dnsMenuOpen = ref(false)
const history = ref<DnsHistoryItem[]>(loadHistory())
const rootEl = ref<HTMLElement | null>(null)
const domainInput = ref<HTMLInputElement | null>(null)
const typeInput = ref<HTMLInputElement | null>(null)
const serverInput = ref<HTMLInputElement | null>(null)
const historyList = ref<HTMLElement | null>(null)
const typeList = ref<HTMLElement | null>(null)
const serverList = ref<HTMLElement | null>(null)
const announcement = ref('')

type MenuName = 'history' | 'type' | 'server'

function menuState(menu: MenuName) {
  if (menu === 'history') return { open: historyOpen, list: historyList, input: domainInput }
  if (menu === 'type') return { open: typeMenuOpen, list: typeList, input: typeInput }
  return { open: dnsMenuOpen, list: serverList, input: serverInput }
}

function menuOptions(menu: MenuName): HTMLButtonElement[] {
  return Array.from(menuState(menu).list.value?.querySelectorAll<HTMLButtonElement>('[data-menu-option]') ?? [])
}

async function openMenu(menu: MenuName) {
  if (menu === 'history' && history.value.length === 0) return
  historyOpen.value = menu === 'history'
  typeMenuOpen.value = menu === 'type'
  dnsMenuOpen.value = menu === 'server'
  await nextTick()
  const options = menuOptions(menu)
  const selected = options.find((option) => option.getAttribute('aria-selected') === 'true')
  ;(selected ?? options[0])?.focus()
}

let skipHistoryOnFocus = false

function onDomainFocus() {
  if (skipHistoryOnFocus) {
    skipHistoryOnFocus = false
    return
  }
  historyOpen.value = history.value.length > 0
}

function closeMenu(menu: MenuName, returnFocus = true) {
  const state = menuState(menu)
  state.open.value = false
  if (!returnFocus) return
  void nextTick(() => {
    if (menu === 'history' && document.activeElement !== state.input.value) skipHistoryOnFocus = true
    state.input.value?.focus()
  })
}

function onInputKeydown(event: KeyboardEvent, menu: MenuName) {
  if (event.key !== 'ArrowDown') return
  event.preventDefault()
  void openMenu(menu)
}

function onOptionKeydown(event: KeyboardEvent, menu: MenuName) {
  const options = menuOptions(menu)
  const index = options.indexOf(event.target as HTMLButtonElement)
  if (index < 0) return
  let next = -1
  switch (event.key) {
    case 'ArrowDown': next = index === options.length - 1 ? 0 : index + 1; break
    case 'ArrowUp': next = index === 0 ? options.length - 1 : index - 1; break
    case 'Home': next = 0; break
    case 'End': next = options.length - 1; break
    case 'Escape':
      event.preventDefault()
      event.stopPropagation()
      closeMenu(menu)
      return
    case 'Tab':
      closeMenu(menu, false)
      return
    case 'Delete':
      if (menu === 'history') {
        event.preventDefault()
        void removeHistoryAt(index, true)
      }
      return
    default:
      return
  }
  event.preventDefault()
  options[next]?.focus()
}

function announce(message: string) {
  announcement.value = ''
  void nextTick(() => { announcement.value = message })
}

const sourceLabel = computed(() => {
  if (!result.value) return ''
  return result.value.source === 'custom'
    ? `指定 DNS: ${result.value.server}`
    : '本机当前 DNS'
})

function loadHistory(): DnsHistoryItem[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(HISTORY_KEY) || '[]')
    return Array.isArray(parsed) ? parsed.slice(0, HISTORY_LIMIT) : []
  } catch {
    return []
  }
}

function saveHistory() {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(history.value.slice(0, HISTORY_LIMIT)))
}

function pushHistory(domainValue: string) {
  history.value = [
    { domain: domainValue },
    ...history.value.filter((item) => item.domain !== domainValue),
  ].slice(0, HISTORY_LIMIT)
  saveHistory()
}

async function removeHistoryAt(index: number, keepFocus: boolean) {
  const [removed] = history.value.splice(index, 1)
  saveHistory()
  if (removed) announce(`已删除历史记录 ${removed.domain}`)
  if (history.value.length === 0) {
    closeMenu('history')
    return
  }
  if (!keepFocus) return
  await nextTick()
  const options = menuOptions('history')
  options[Math.min(index, options.length - 1)]?.focus()
}

function applyHistory(item: DnsHistoryItem) {
  domain.value = item.domain
  closeMenu('history')
}

function serverLabelFromValue(value: string) {
  return presetServers.find((preset) => preset.value === value)?.label ?? value
}

function resolveServerValue() {
  const value = server.value.trim()
  const preset = presetServers.find((item) => item.label === value || item.value === value)
  return preset?.value ?? value
}

let queryGeneration = 0

async function enrichGeo(items: DnsRecordWithGeo[], token: number) {
  await Promise.all(items.map(async (item) => {
    if (!item.ip) return
    item.geoLoading = true
    try {
      const geo = await getGeoIPForIP(item.ip)
      if (token === queryGeneration) item.geo = geo
    } catch {
      item.geo = undefined
    } finally {
      item.geoLoading = false
    }
  }))
}

async function handleQuery() {
  historyOpen.value = false
  const domainValue = domain.value.trim()
  const serverValue = resolveServerValue()
  if (!domainValue) {
    pushToast({ message: '请输入域名。', type: 'error' })
    return
  }

  const token = ++queryGeneration
  const typeValue = recordType.value.trim().toUpperCase()
  loading.value = true
  announce(`正在查询 ${domainValue}…`)
  try {
    const res = await queryDns(domainValue, typeValue, serverValue)
    if (token !== queryGeneration) return
    result.value = res
    records.value = res.records
    pushHistory(domainValue)
    announce(res.records.length ? `${domainValue} 查询完成，共 ${res.records.length} 条记录` : `${domainValue} 没有 ${typeValue} 记录`)
    if (geoEnabled.value && records.value.some((item) => item.ip)) {
      void enrichGeo(records.value, token)
    }
  } catch (error) {
    if (token !== queryGeneration) return
    pushToast({ message: failureMessage(`查询 ${domainValue} 失败`, error), type: 'error' }, 6000)
    announce(`查询 ${domainValue} 失败`)
  } finally {
    if (token === queryGeneration) loading.value = false
  }
}

function closeHistory() {
  historyOpen.value = false
}

function closeDnsMenu() {
  dnsMenuOpen.value = false
}

function closeTypeMenu() {
  typeMenuOpen.value = false
}

function toggleTypeMenu() {
  if (typeMenuOpen.value) {
    typeMenuOpen.value = false
    return
  }
  void openMenu('type')
}

function applyRecordType(type: string) {
  recordType.value = type
  closeMenu('type')
}

function toggleDnsMenu() {
  if (dnsMenuOpen.value) {
    dnsMenuOpen.value = false
    return
  }
  void openMenu('server')
}

function applyDnsPreset(value: string) {
  server.value = serverLabelFromValue(value)
  closeMenu('server')
}

function handleRootPointerDown(event: PointerEvent) {
  const target = event.target as HTMLElement | null
  if (!target?.closest('[data-dns-history-area]')) {
    historyOpen.value = false
  }
  if (!target?.closest('[data-dns-type-area]')) {
    typeMenuOpen.value = false
  }
  if (!target?.closest('[data-dns-menu-area]')) {
    dnsMenuOpen.value = false
  }
}

function handleDocumentPointerDown(event: PointerEvent) {
  const root = rootEl.value
  if (!root || root.contains(event.target as Node)) return
  historyOpen.value = false
  typeMenuOpen.value = false
  dnsMenuOpen.value = false
}

onMounted(() => {
  document.addEventListener('pointerdown', handleDocumentPointerDown)
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', handleDocumentPointerDown)
})
</script>

<template>
  <div ref="rootEl" class="settings-card settings-dns-card" @pointerdown="handleRootPointerDown">
    <p class="sr-only" role="status" aria-live="polite">{{ announcement }}</p>
    <div class="settings-dns-grid">
      <div class="settings-dns-field settings-dns-domain" data-dns-history-area>
        <label for="settings-dns-domain">域名</label>
        <input
          id="settings-dns-domain"
          ref="domainInput"
          v-model="domain"
          type="text"
          class="input input-sm input-bordered w-full settings-mono"
          placeholder="www.google.com"
          aria-haspopup="true"
          :aria-expanded="historyOpen && history.length > 0"
          :aria-controls="historyOpen ? 'settings-dns-history' : undefined"
          @focus="onDomainFocus"
          @keydown="onInputKeydown($event, 'history')"
          @keyup.enter="handleQuery"
          @keyup.escape="closeHistory"
        />
        <div
          v-if="historyOpen && history.length"
          id="settings-dns-history"
          ref="historyList"
          class="glass-popover absolute z-20 mt-1 w-full rounded-xl p-1"
          role="group"
          aria-label="最近查询的域名"
        >
          <div
            v-for="(item, index) in history"
            :key="item.domain"
            class="flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm hover:bg-base-content/[0.06] focus-within:bg-base-content/[0.06]"
          >
            <button
              type="button"
              class="flex-1 min-w-0 text-left focus:outline-none"
              data-menu-option
              :aria-label="`查询 ${item.domain}`"
              @click="applyHistory(item)"
              @keydown="onOptionKeydown($event, 'history')"
            >
              <span class="block truncate">{{ item.domain }}</span>
            </button>
            <button
              type="button"
              class="btn btn-ghost btn-xs btn-square min-h-0 h-5 w-5 text-base-content/60"
              title="删除这条历史记录"
              :aria-label="`删除历史记录 ${item.domain}`"
              @click.stop="removeHistoryAt(index, false)"
            >
              <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="m3 3 6 6M9 3 3 9" /></svg>
            </button>
          </div>
        </div>
      </div>

      <div class="settings-dns-field settings-dns-type" data-dns-type-area>
        <label for="settings-dns-type">记录</label>
        <input
          id="settings-dns-type"
          ref="typeInput"
          v-model="recordType"
          type="text"
          class="input input-sm input-bordered w-full pr-8 uppercase settings-mono"
          placeholder="A"
          @focus="closeHistory"
          @input="recordType = recordType.toUpperCase()"
          @keydown="onInputKeydown($event, 'type')"
          @keyup.enter="handleQuery"
          @keyup.escape="closeTypeMenu"
        />
        <button
          type="button"
          class="settings-dns-menu-button absolute right-0 flex w-8 items-center justify-center text-base-content/70 focus:outline-none"
          title="选择记录类型"
          aria-label="选择记录类型"
          aria-haspopup="listbox"
          :aria-expanded="typeMenuOpen"
          @click="toggleTypeMenu"
        >
          <svg class="w-3.5 h-3.5" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
            <path d="M2.5 4.25 6 7.75l3.5-3.5z" />
          </svg>
        </button>
        <div
          v-if="typeMenuOpen"
          ref="typeList"
          class="glass-popover absolute z-20 mt-1 w-full rounded-xl p-1"
          role="listbox"
          aria-label="记录类型"
        >
          <button
            v-for="type in recordTypes"
            :key="type"
            type="button"
            role="option"
            data-menu-option
            tabindex="-1"
            :aria-selected="recordType === type"
            class="block w-full rounded-lg px-3 py-2 text-left text-sm hover:bg-base-content/[0.06] focus:bg-base-content/[0.08] focus:outline-none"
            @click="applyRecordType(type)"
            @keydown="onOptionKeydown($event, 'type')"
          >
            {{ type }}
          </button>
        </div>
      </div>

      <div class="settings-dns-field settings-dns-server" data-dns-menu-area>
        <label for="settings-dns-server">解析器</label>
        <input
          id="settings-dns-server"
          ref="serverInput"
          v-model="server"
          type="text"
          class="input input-sm input-bordered w-full pr-9 settings-mono"
          placeholder="本机 DNS"
          @focus="closeHistory"
          @keydown="onInputKeydown($event, 'server')"
          @keyup.enter="handleQuery"
          @keyup.escape="closeDnsMenu"
        />
        <button
          type="button"
          class="settings-dns-menu-button absolute right-0 flex w-9 items-center justify-center text-base-content/70 focus:outline-none"
          title="选择 DNS"
          aria-label="选择 DNS 解析器"
          aria-haspopup="listbox"
          :aria-expanded="dnsMenuOpen"
          @click="toggleDnsMenu"
        >
          <svg class="w-3.5 h-3.5" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
            <path d="M2.5 4.25 6 7.75l3.5-3.5z" />
          </svg>
        </button>
        <div
          v-if="dnsMenuOpen"
          ref="serverList"
          class="glass-popover absolute z-20 mt-1 w-full rounded-xl p-1"
          role="listbox"
          aria-label="DNS 解析器"
        >
          <button
            v-for="preset in presetServers"
            :key="preset.label"
            type="button"
            role="option"
            data-menu-option
            tabindex="-1"
            :aria-selected="server === serverLabelFromValue(preset.value)"
            class="block w-full rounded-lg px-3 py-2 text-left text-sm hover:bg-base-content/[0.06] focus:bg-base-content/[0.08] focus:outline-none"
            @click="applyDnsPreset(preset.value)"
            @keydown="onOptionKeydown($event, 'server')"
          >
            <span class="block">{{ preset.label }}</span>
            <span v-if="preset.value" class="block text-xs text-base-content/50">{{ preset.value }}</span>
          </button>
        </div>
      </div>
      <button type="button" class="btn btn-sm settings-btn settings-dns-submit" :disabled="loading" :aria-busy="loading" @click="handleQuery">
        <span v-if="loading" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        查询
      </button>
    </div>

    <div v-if="result" class="settings-dns-results">
      <div
        v-if="records.length === 0"
        class="px-3 py-6 text-center text-sm text-base-content/50"
      >
        无记录
      </div>
      <div
        v-for="item in records"
        :key="item.name + item.record_type + item.value"
        class="settings-dns-record"
      >
        <div class="badge badge-sm badge-ghost shrink-0 mt-0.5">{{ item.record_type }}</div>
        <div class="min-w-0 flex-1">
          <div class="font-mono text-sm truncate" :title="item.name">{{ item.name }}</div>
          <div class="text-xs text-base-content/50">TTL {{ item.ttl }}</div>
          <div v-if="item.geo" class="text-xs text-base-content/60 mt-1 truncate">
            {{ [item.geo.country, item.geo.asnOrganization || item.geo.organization].filter(Boolean).join(' / ') }}
          </div>
        </div>
        <div class="settings-dns-value settings-mono">{{ item.value }}</div>
      </div>
      <div class="px-3 py-2 text-xs text-base-content/50 border-t border-[color:var(--hairline)]">
        {{ sourceLabel }}
      </div>
    </div>

    <label class="settings-row settings-toggle-row settings-dns-geo">
      <span class="settings-row-copy">
        <strong>显示 IP 归属地</strong>
        <span>开启后会把查询到的 IP 发送给 api.ip.sb 查询归属地。</span>
      </span>
      <input type="checkbox" class="toggle toggle-sm toggle-primary" :checked="geoEnabled" @change="setGeoEnabled" />
    </label>
  </div>
</template>
