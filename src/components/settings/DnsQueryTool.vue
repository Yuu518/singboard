<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { queryDns, type DnsRecord, type DnsQueryResult } from '@/api/dns'
import { getGeoIPForIP, type IPGeoInfo } from '@/api/geoip'
import { useToastStore } from '@/stores/toast'

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
const result = ref<DnsQueryResult | null>(null)
const records = ref<DnsRecordWithGeo[]>([])
const historyOpen = ref(false)
const typeMenuOpen = ref(false)
const dnsMenuOpen = ref(false)
const history = ref<DnsHistoryItem[]>(loadHistory())
const rootEl = ref<HTMLElement | null>(null)

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

function removeHistory(index: number) {
  history.value.splice(index, 1)
  if (history.value.length === 0) {
    historyOpen.value = false
  }
  saveHistory()
}

function applyHistory(item: DnsHistoryItem) {
  domain.value = item.domain
  historyOpen.value = false
}

function serverLabelFromValue(value: string) {
  return presetServers.find((preset) => preset.value === value)?.label ?? value
}

function resolveServerValue() {
  const value = server.value.trim()
  const preset = presetServers.find((item) => item.label === value || item.value === value)
  return preset?.value ?? value
}

async function enrichGeo(items: DnsRecordWithGeo[]) {
  await Promise.all(items.map(async (item) => {
    if (!item.ip) return
    item.geoLoading = true
    try {
      item.geo = await getGeoIPForIP(item.ip)
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

  loading.value = true
  try {
    const res = await queryDns(domainValue, recordType.value.trim().toUpperCase(), serverValue)
    result.value = res
    records.value = res.records
    pushHistory(domainValue)
    if (records.value.some((item) => item.ip)) {
      void enrichGeo(records.value)
    }
  } catch (e: any) {
    pushToast({ message: 'DNS 查询失败: ' + (e?.message || e), type: 'error' }, 6000)
  } finally {
    loading.value = false
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
  historyOpen.value = false
  dnsMenuOpen.value = false
  typeMenuOpen.value = !typeMenuOpen.value
}

function applyRecordType(type: string) {
  recordType.value = type
  typeMenuOpen.value = false
}

function toggleDnsMenu() {
  historyOpen.value = false
  typeMenuOpen.value = false
  dnsMenuOpen.value = !dnsMenuOpen.value
}

function applyDnsPreset(value: string) {
  server.value = serverLabelFromValue(value)
  dnsMenuOpen.value = false
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
  <div ref="rootEl" class="bg-base-200 rounded-lg p-4 space-y-3" @pointerdown="handleRootPointerDown">
    <h2 class="font-semibold text-sm">DNS 查询</h2>

    <div class="grid grid-cols-[minmax(11rem,1fr)_6rem_minmax(10rem,0.85fr)_auto] gap-2">
      <div class="relative" data-dns-history-area>
        <input
          v-model="domain"
          type="text"
          class="input input-sm input-bordered w-full"
          placeholder="www.google.com"
          @focus="historyOpen = history.length > 0"
          @keyup.enter="handleQuery"
          @keyup.escape="closeHistory"
        />
        <div
          v-if="historyOpen && history.length"
          class="absolute z-20 mt-1 w-full rounded-md bg-base-100 border border-base-300 shadow-lg py-1"
        >
          <div
            v-for="(item, index) in history"
            :key="item.domain"
            class="flex items-center gap-2 px-2 py-1.5 text-sm hover:bg-base-200"
          >
            <button class="flex-1 min-w-0 text-left" @click="applyHistory(item)">
              <span class="block truncate">{{ item.domain }}</span>
            </button>
            <button
              class="btn btn-ghost btn-xs btn-square min-h-0 h-5 w-5"
              title="删除"
              @click.stop="removeHistory(index)"
            >
              x
            </button>
          </div>
        </div>
      </div>

      <div class="relative" data-dns-type-area>
        <input
          v-model="recordType"
          type="text"
          class="input input-sm input-bordered w-full pr-8 uppercase"
          placeholder="A"
          @focus="closeHistory"
          @input="recordType = recordType.toUpperCase()"
          @keyup.enter="handleQuery"
          @keyup.escape="closeTypeMenu"
        />
        <button
          type="button"
          class="absolute right-0 top-0 flex h-full w-8 items-center justify-center text-base-content/70 focus:outline-none"
          title="选择记录类型"
          @click="toggleTypeMenu"
        >
          <svg class="w-3.5 h-3.5" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
            <path d="M2.5 4.25 6 7.75l3.5-3.5z" />
          </svg>
        </button>
        <div
          v-if="typeMenuOpen"
          class="absolute z-20 mt-1 w-full rounded-md bg-base-100 border border-base-300 shadow-lg py-1"
        >
          <button
            v-for="type in recordTypes"
            :key="type"
            type="button"
            class="block w-full px-3 py-2 text-left text-sm hover:bg-base-200"
            @mousedown.prevent="applyRecordType(type)"
          >
            {{ type }}
          </button>
        </div>
      </div>

      <div class="relative" data-dns-menu-area>
        <input
          v-model="server"
          type="text"
          class="input input-sm input-bordered w-full pr-9"
          placeholder="本机 DNS"
          @focus="closeHistory"
          @keyup.enter="handleQuery"
          @keyup.escape="closeDnsMenu"
        />
        <button
          type="button"
          class="absolute right-0 top-0 flex h-full w-9 items-center justify-center text-base-content/70 focus:outline-none"
          title="选择 DNS"
          @click="toggleDnsMenu"
        >
          <svg class="w-3.5 h-3.5" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
            <path d="M2.5 4.25 6 7.75l3.5-3.5z" />
          </svg>
        </button>
        <div
          v-if="dnsMenuOpen"
          class="absolute z-20 mt-1 w-full rounded-md bg-base-100 border border-base-300 shadow-lg py-1"
        >
          <button
            v-for="preset in presetServers"
            :key="preset.label"
            type="button"
            class="block w-full px-3 py-2 text-left text-sm hover:bg-base-200"
            @mousedown.prevent="applyDnsPreset(preset.value)"
          >
            <span class="block">{{ preset.label }}</span>
            <span v-if="preset.value" class="block text-xs text-base-content/50">{{ preset.value }}</span>
          </button>
        </div>
      </div>
      <button class="btn btn-sm btn-primary" :class="{ loading }" :disabled="loading" @click="handleQuery">
        DNS 查询
      </button>
    </div>

    <div v-if="result" class="rounded-md bg-base-100 border border-base-300 overflow-hidden">
      <div
        v-if="records.length === 0"
        class="px-3 py-6 text-center text-sm text-base-content/50"
      >
        无记录
      </div>
      <div
        v-for="item in records"
        :key="item.name + item.record_type + item.value"
        class="flex items-start gap-3 px-3 py-2.5 border-b border-base-300 last:border-b-0"
      >
        <div class="badge badge-sm badge-ghost shrink-0 mt-0.5">{{ item.record_type }}</div>
        <div class="min-w-0 flex-1">
          <div class="font-mono text-sm truncate" :title="item.name">{{ item.name }}</div>
          <div class="text-xs text-base-content/50">TTL {{ item.ttl }}</div>
          <div v-if="item.geo" class="text-xs text-base-content/60 mt-1 truncate">
            {{ [item.geo.country, item.geo.asnOrganization || item.geo.organization].filter(Boolean).join(' / ') }}
          </div>
        </div>
        <div class="font-mono text-sm text-right break-all max-w-[45%]">{{ item.value }}</div>
      </div>
      <div class="px-3 py-2 text-xs text-base-content/50 border-t border-base-300">
        {{ sourceLabel }}
      </div>
    </div>
  </div>
</template>
