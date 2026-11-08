<script setup lang="ts">
import { computed, ref, shallowRef, nextTick, onBeforeUnmount, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import ServiceNotice from '@/components/common/ServiceNotice.vue'
import { useSettledServiceState } from '@/composables/useSettledServiceState'
import { useVirtualList } from '@/composables/useVirtualList'
import { useConfigStore } from '@/stores/config'
import { LOG_LEVELS, useLogsStore } from '@/stores/logs'
import type { LogEntry } from '@/types'

const {
  logs,
  filteredLogs,
  levelFilter,
  levelChoice,
  coreFilterLevel,
  paused,
  newSincePause,
  filterText,
  socketState,
  socketFailures,
  setPaused,
  clear,
  restore,
} = useLogsStore()
const { clashApiUrl, activeClashApiId } = useConfigStore()
const { serviceState, isRunning } = useSettledServiceState()
const router = useRouter()

const query = computed(() => filterText.value.trim())
const apiUnreachable = computed(() => socketState.value === 'failed' || socketFailures.value >= 3)

const ROW_HEIGHT = 26
const autoScroll = ref(true)
const {
  container: logContainer,
  totalHeight,
  offsetY,
  visibleItems,
  syncScroll,
  offsetOf,
  indexAt,
} = useVirtualList({ items: filteredLogs, keyOf: (log) => log.seq ?? 0, rowHeight: ROW_HEIGHT })

const expanded = shallowRef(new Set<number>())

function toggleExpanded(log: LogEntry, fromKeyboard = false) {
  if (log.seq === undefined || (!fromKeyboard && window.getSelection()?.toString())) return
  activeSeq.value = log.seq
  const next = new Set(expanded.value)
  if (next.has(log.seq)) next.delete(log.seq)
  else next.add(log.seq)
  expanded.value = next
}

const levelOptions = computed(() => [
  { value: '', label: `跟随核心（${coreFilterLevel.value}）` },
  ...LOG_LEVELS.map((value) => ({ value, label: value })),
])
const levelFiltered = computed(() => levelFilter.value !== coreFilterLevel.value)

const clearedLogs = shallowRef<typeof logs.value | null>(null)
let undoTimer: ReturnType<typeof setTimeout> | null = null

function forgetCleared() {
  if (undoTimer) clearTimeout(undoTimer)
  undoTimer = null
  clearedLogs.value = null
}

const announcement = ref('')

function announce(message: string) {
  announcement.value = ''
  void nextTick(() => {
    announcement.value = message
  })
}

function countLines(list: LogEntry[]): number {
  let count = 0
  for (const entry of list) if (entry.type !== 'marker') count++
  return count
}

const totalCount = computed(() => countLines(logs.value))
const shownCount = computed(() => (filteredLogs.value === logs.value ? totalCount.value : countLines(filteredLogs.value)))

function clearLogs() {
  const removed = clear()
  forgetCleared()
  if (removed.length === 0) return
  clearedLogs.value = removed
  undoTimer = setTimeout(forgetCleared, 8000)
  announce(`已清空 ${countLines(removed)} 条日志，8 秒内可撤销`)
}

function undoClear() {
  const removed = clearedLogs.value
  forgetCleared()
  if (!removed) return
  restore(removed)
  announce(`已恢复 ${countLines(removed)} 条日志`)
}

onBeforeUnmount(forgetCleared)
watch(activeClashApiId, forgetCleared)

function togglePaused() {
  setPaused(!paused.value)
  announce(paused.value ? '已暂停刷新，新日志会在后台继续接收' : '已继续刷新')
}

watch(levelChoice, () => announce(`日志级别切换为 ${levelFilter.value}`))

watch(socketState, (state, previous) => {
  if (!isRunning.value) return
  if (state === 'reconnecting' && previous === 'open') announce('与核心的连接中断，正在重连')
  else if (state === 'open' && previous === 'reconnecting') announce('已重新连接核心')
})

watch(apiUnreachable, (unreachable) => {
  if (unreachable && isRunning.value) announce('无法连接 Clash API，请检查 API 配置')
})

const searchInput = ref<HTMLInputElement | null>(null)

function clearSearch() {
  filterText.value = ''
  searchInput.value?.focus()
}

function onSearchEscape(e: KeyboardEvent) {
  if (filterText.value) filterText.value = ''
  else (e.target as HTMLInputElement).blur()
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return
  if (!isRunning.value || document.querySelector('[role="alertdialog"]')) return
  e.preventDefault()
  searchInput.value?.focus()
  searchInput.value?.select()
}

onMounted(() => window.addEventListener('keydown', onGlobalKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onGlobalKeydown))

const activeSeq = ref<number | null>(null)
const focusInside = ref(false)
const lineCount = computed(() => countLines(filteredLogs.value))
const activeRendered = computed(() => visibleItems.value.some(({ item }) => item.seq === activeSeq.value))

function focusActiveRow() {
  void nextTick(() => {
    if (activeSeq.value === null) return
    logContainer.value?.querySelector<HTMLElement>(`[data-virtual-key="${activeSeq.value}"]`)?.focus()
  })
}

function onListFocusIn() {
  focusInside.value = true
}

function onListFocusOut(e: FocusEvent) {
  if (!logContainer.value?.contains(e.relatedTarget as Node | null)) focusInside.value = false
}

function lineIndexes(): number[] {
  const out: number[] = []
  filteredLogs.value.forEach((entry, index) => {
    if (entry.type !== 'marker') out.push(index)
  })
  return out
}

function revealIndex(index: number) {
  const el = logContainer.value
  if (!el) return
  const pad = 6
  const top = offsetOf(index) + pad
  const bottom = offsetOf(index + 1) + pad
  if (top < el.scrollTop) el.scrollTop = top - pad
  else if (bottom > el.scrollTop + el.clientHeight) el.scrollTop = bottom - el.clientHeight + pad
  handleScroll()
}

function onListFocus(e: FocusEvent) {
  if (e.target !== logContainer.value) return
  const el = logContainer.value
  const lines = lineIndexes()
  if (!el || lines.length === 0) return
  const top = indexAt(el.scrollTop)
  const bottom = indexAt(el.scrollTop + el.clientHeight - 1)
  const pick = [...lines].reverse().find((index) => index <= bottom && index >= top)
    ?? [...lines].reverse().find((index) => index <= bottom)
    ?? lines[0]
  activeSeq.value = filteredLogs.value[pick].seq ?? null
  focusActiveRow()
}

function onListKeydown(e: KeyboardEvent) {
  const lines = lineIndexes()
  if (lines.length === 0) return
  const current = activeSeq.value === null ? -1 : findBySeq(filteredLogs.value, activeSeq.value)
  let position = lines.indexOf(current)
  if (e.key === 'Enter' || e.key === ' ') {
    if (current < 0) return
    e.preventDefault()
    const entry = filteredLogs.value[current]
    toggleExpanded(entry, true)
    return
  }
  if (position < 0) position = lines.length - 1
  let next: number
  switch (e.key) {
    case 'ArrowDown': next = position + 1; break
    case 'ArrowUp': next = position - 1; break
    case 'PageDown': next = position + 10; break
    case 'PageUp': next = position - 10; break
    case 'Home': next = 0; break
    case 'End': next = lines.length - 1; break
    default: return
  }
  e.preventDefault()
  const index = lines[Math.min(Math.max(next, 0), lines.length - 1)]
  activeSeq.value = filteredLogs.value[index].seq ?? null
  revealIndex(index)
  focusActiveRow()
}

function scrollToLatest() {
  autoScroll.value = true
  void nextTick(() => {
    if (logContainer.value) logContainer.value.scrollTop = logContainer.value.scrollHeight
  })
}

function levelBadge(type: string): string {
  switch (type.toLowerCase()) {
    case 'error':
    case 'fatal':
    case 'panic': return 'bg-error/15 text-[color:var(--danger-ink)]'
    case 'warning':
    case 'warn': return 'bg-warning/15 text-[color:var(--latency-slow-ink)]'
    case 'info': return 'bg-base-content/[0.08] text-base-content/75'
    default: return 'bg-base-content/[0.05] text-base-content/65'
  }
}

function rowTint(type: string): string {
  switch (type.toLowerCase()) {
    case 'error':
    case 'fatal':
    case 'panic': return 'bg-error/[0.06]'
    case 'warning':
    case 'warn': return 'bg-warning/[0.06]'
    default: return ''
  }
}

let anchor: { seq: number; delta: number } | null = null

function rememberAnchor() {
  const el = logContainer.value
  const list = filteredLogs.value
  if (!el || list.length === 0) {
    anchor = null
    return
  }
  const index = indexAt(el.scrollTop)
  const seq = list[index]?.seq
  anchor = seq === undefined ? null : { seq, delta: el.scrollTop - offsetOf(index) }
}

function findBySeq(list: LogEntry[], seq: number): number {
  let lo = 0
  let hi = list.length - 1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const value = list[mid].seq ?? 0
    if (value === seq) return mid
    if (value < seq) lo = mid + 1
    else hi = mid - 1
  }
  return -1
}

watch(filteredLogs, (list) => {
  const kept = new Set<number>()
  for (const seq of expanded.value) if (findBySeq(list, seq) >= 0) kept.add(seq)
  if (kept.size !== expanded.value.size) expanded.value = kept

  const held = anchor
  void nextTick(() => {
    const el = logContainer.value
    if (!el) return
    if (autoScroll.value && !focusInside.value) {
      el.scrollTop = el.scrollHeight
    } else if (held) {
      const index = findBySeq(filteredLogs.value, held.seq)
      if (index >= 0) el.scrollTop = offsetOf(index) + held.delta
    }
    syncScroll()
    rememberAnchor()
  })
})

watch(logContainer, (el) => {
  if (!el || !autoScroll.value) return
  el.scrollTop = el.scrollHeight
  syncScroll()
  rememberAnchor()
}, { flush: 'post' })

function handleScroll() {
  const el = logContainer.value
  if (!el) return
  syncScroll()
  autoScroll.value = el.scrollHeight - el.scrollTop - el.clientHeight < 50
  rememberAnchor()
}
</script>

<template>
  <div class="flex flex-col h-full gap-3">
    <span class="sr-only" aria-live="polite">{{ announcement }}</span>
    <ServiceNotice
      v-if="!isRunning"
      :state="serviceState"
      page-title="日志"
      stopped-detail="启动服务后即可查看实时日志。"
      starting-detail="核心启动后会自动接收日志。"
    />

    <template v-else>
    <div class="flex items-center justify-between">
      <h1 class="text-[26px] leading-tight tracking-tight font-bold">
        日志
        <span class="ml-1 text-sm font-normal tabular-nums text-base-content/65">{{ shownCount === totalCount ? totalCount.toLocaleString() : `${shownCount.toLocaleString()} / ${totalCount.toLocaleString()}` }}</span>
      </h1>
      <div class="flex items-center gap-2">
        <select v-model="levelChoice" class="select select-sm select-bordered" aria-label="显示级别">
          <option v-for="option in levelOptions" :key="option.value" :value="option.value">{{ option.label }}</option>
        </select>
        <button
          type="button"
          class="btn btn-sm btn-ghost btn-circle shrink-0"
          :class="{ 'bg-primary/[0.12] text-[color:var(--selection-ink)]': paused }"
          :aria-pressed="paused"
          aria-label="暂停刷新"
          :title="paused ? '继续刷新' : '暂停刷新'"
          @click="togglePaused"
        >
          <svg v-if="!paused" class="h-4 w-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <rect x="4" y="3" width="2.6" height="10" rx="0.8" />
            <rect x="9.4" y="3" width="2.6" height="10" rx="0.8" />
          </svg>
          <svg v-else class="h-4 w-4" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <path d="M5 3.4v9.2a.7.7 0 0 0 1.06.6l7.4-4.6a.7.7 0 0 0 0-1.2l-7.4-4.6A.7.7 0 0 0 5 3.4z" />
          </svg>
        </button>
        <button type="button" class="btn btn-sm btn-ghost" :disabled="logs.length === 0 && !paused" @click="clearLogs">清空</button>
      </div>
    </div>

    <div class="relative flex items-center">
      <input
        ref="searchInput"
        v-model="filterText"
        type="search"
        class="input input-sm input-bordered w-full min-w-0 pl-8 [&::-webkit-search-cancel-button]:appearance-none"
        :class="{ 'pr-8': filterText }"
        placeholder="搜索日志内容"
        aria-label="搜索日志"
        aria-keyshortcuts="Control+F"
        title="搜索日志 (Ctrl+F)"
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

    <div
      v-if="paused || clearedLogs || ((socketState === 'reconnecting' || socketState === 'failed') && logs.length > 0)"
      class="surface-card flex items-center gap-2 px-3 py-1.5 text-xs"
    >
      <template v-if="paused">
        <span class="tabular-nums">已暂停刷新{{ newSincePause > 0 ? `，暂停后收到 ${newSincePause} 条新日志` : '' }}。</span>
        <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="togglePaused">继续刷新</button>
      </template>
      <template v-else-if="clearedLogs">
        <span class="tabular-nums">已清空 {{ countLines(clearedLogs) }} 条日志。</span>
        <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="undoClear">撤销</button>
      </template>
      <template v-else-if="apiUnreachable">
        <span>{{ socketState === 'failed' ? 'Clash API 地址无效，无法建立连接' : '无法连接 Clash API，正在重试' }}；下面是中断前的日志。</span>
        <button type="button" class="btn btn-xs btn-ghost h-5 min-h-0 px-2 text-[color:var(--selection-ink)]" @click="router.push('/settings')">检查 API 配置</button>
      </template>
      <template v-else>
        <span class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        <span>与核心的连接中断，正在重连；下面是中断前的日志。</span>
      </template>
    </div>

    <div class="surface-card relative flex min-h-0 flex-1 flex-col overflow-hidden">
      <div
        ref="logContainer"
        class="log-list card-scroll min-h-0 flex-1 overflow-auto py-1.5 focus-visible:outline-none"
        role="log"
        aria-live="off"
        aria-label="sing-box 日志"
        :tabindex="lineCount > 0 && !activeRendered ? 0 : -1"
        @scroll="handleScroll"
        @focus="onListFocus"
        @focusin="onListFocusIn"
        @focusout="onListFocusOut"
        @keydown="onListKeydown"
      >
        <div v-if="filteredLogs.length > 0" class="relative" :style="{ height: `${totalHeight}px` }">
          <div class="absolute inset-x-0 top-0" :style="{ transform: `translateY(${offsetY}px)` }">
            <template v-for="{ item: log } in visibleItems" :key="log.seq">
              <div
                v-if="log.type === 'marker'"
                :data-virtual-key="log.seq"
                class="flex h-[26px] items-center gap-3 px-3.5 text-[11.5px] text-base-content/65"
              >
                <span class="h-px flex-1 bg-[color:var(--hairline)]" aria-hidden="true"></span>
                <span class="shrink-0 tabular-nums">{{ log.payload }}</span>
                <span class="h-px flex-1 bg-[color:var(--hairline)]" aria-hidden="true"></span>
              </div>
              <div
                v-else
                :data-virtual-key="log.seq"
                role="button"
                :tabindex="log.seq === activeSeq ? 0 : -1"
                :aria-expanded="expanded.has(log.seq ?? 0)"
                class="log-row grid cursor-default grid-cols-[62px_52px_minmax(0,1fr)] gap-2.5 border-b-[0.5px] border-[color:var(--hairline)] px-3.5"
                :class="[rowTint(log.type), expanded.has(log.seq ?? 0) ? 'items-start py-1' : 'h-[26px] items-center']"
                @click="toggleExpanded(log)"
                @focus="activeSeq = log.seq ?? null"
              >
                <time class="settings-mono text-[11.5px] leading-[18px] text-base-content/65">{{ log.time }}</time>
                <span
                  class="flex h-[18px] items-center justify-center rounded-full text-[10.5px] font-semibold uppercase tracking-[0.04em]"
                  :class="levelBadge(log.type)"
                >{{ log.type }}</span>
                <span
                  class="settings-mono text-xs leading-[18px] text-base-content/90"
                  :class="expanded.has(log.seq ?? 0) ? 'whitespace-pre-wrap [overflow-wrap:anywhere]' : 'truncate'"
                >{{ log.payload }}</span>
              </div>
            </template>
          </div>
        </div>
        <template v-else>
          <div v-if="(query || levelFiltered) && logs.length > 0" class="flex flex-col items-center gap-3 px-6 py-12 text-center" role="status">
            <p class="text-sm text-base-content/80">
              <template v-if="query">没有匹配“{{ query }}”的日志</template>
              <template v-else>当前级别 {{ levelFilter }} 下没有日志</template>
            </p>
            <div class="flex gap-2">
              <button v-if="query" type="button" class="btn btn-sm btn-glass px-5" @click="clearSearch">清除搜索</button>
              <button v-if="levelFiltered" type="button" class="btn btn-sm btn-glass px-5" @click="levelChoice = ''">跟随核心级别</button>
            </div>
          </div>
          <div v-else-if="socketState !== 'open' && apiUnreachable" class="mx-auto mt-6 max-w-[520px] px-6 py-5" role="alert">
            <p class="text-base font-semibold">无法连接 Clash API</p>
            <p class="mt-1 text-sm text-base-content/70">
              请确认 sing-box 配置里开启了 clash_api，并且地址和密钥与当前的 Clash API 配置一致。
            </p>
            <dl class="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
              <dt class="text-base-content/65">地址</dt>
              <dd class="settings-mono break-all">{{ clashApiUrl || '未配置' }}</dd>
              <dt class="text-base-content/65">状态</dt>
              <dd>{{ socketState === 'failed' ? '地址无效，无法建立连接' : `已连续 ${socketFailures} 次连接失败，仍在自动重试` }}</dd>
            </dl>
            <div class="mt-4">
              <button type="button" class="btn btn-sm btn-glass px-5" @click="router.push('/settings')">检查 API 配置</button>
            </div>
          </div>
          <div v-else-if="socketState !== 'open'" class="flex items-center justify-center gap-2 py-12 text-sm text-base-content/65" role="status">
            <span class="loading loading-spinner loading-sm" aria-hidden="true"></span>
            正在连接核心…
          </div>
          <div v-else class="flex flex-col items-center gap-1 px-6 py-12 text-center">
            <p class="text-sm text-base-content/80">{{ paused ? '暂停前没有日志' : '已连接，暂时没有日志' }}</p>
            <p class="text-xs text-base-content/65">当前级别 {{ levelFilter }}{{ levelChoice ? '' : '（跟随核心）' }}，有新日志时会出现在这里。</p>
          </div>
        </template>
      </div>
      <button
        v-if="!autoScroll && filteredLogs.length > 0"
        type="button"
        class="btn btn-sm btn-glass absolute bottom-3 right-3 gap-1.5 px-3"
        @click="scrollToLatest"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M8 3v10M3.5 8.5 8 13l4.5-4.5" />
        </svg>
        回到最新
      </button>
    </div>
    </template>
  </div>
</template>

<style scoped>
.log-row:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: -2px;
}
</style>
