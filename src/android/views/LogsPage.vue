<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, watch } from 'vue'
import { LOG_LEVELS, useLogsStore } from '@/stores/logs'
import { usePageActive } from '@/android/composables/usePageActive'
import type { LogEntry } from '@/types'

const {
  filteredLogs,
  levelFilter,
  levelChoice,
  paused,
  filterText,
  setPaused,
  clear,
} = useLogsStore()

const logContainer = ref<HTMLElement | null>(null)
const autoScroll = ref(true)

const levels = [...LOG_LEVELS].reverse()

function levelColor(type: string): string {
  switch (type.toLowerCase()) {
    case 'error':
    case 'fatal':
    case 'panic': return 'text-error'
    case 'warning':
    case 'warn': return 'text-warning'
    case 'info': return 'text-info'
    case 'debug': return 'text-success'
    case 'trace': return 'text-secondary'
    default: return 'text-base-content/50'
  }
}

const pageActive = usePageActive()
const frozenLogs = shallowRef<LogEntry[]>([])
watch(pageActive, (active) => {
  if (!active) frozenLogs.value = filteredLogs.value.slice()
}, { immediate: true })
const visibleLogs = computed(() => (pageActive.value ? filteredLogs.value : frozenLogs.value))

watch(() => [visibleLogs.value, visibleLogs.value.length], () => {
  if (autoScroll.value) {
    nextTick(() => {
      if (logContainer.value) {
        logContainer.value.scrollTop = logContainer.value.scrollHeight
      }
    })
  }
})

function handleScroll() {
  if (!logContainer.value) return
  const { scrollTop, scrollHeight, clientHeight } = logContainer.value
  autoScroll.value = scrollHeight - scrollTop - clientHeight < 50
}
</script>

<template>
  <div class="relative flex flex-col h-full gap-3">
    <div class="flex items-center justify-between">
      <h1 class="text-[26px] leading-tight tracking-tight font-bold">
        日志
        <span class="text-sm font-normal text-base-content/50">({{ visibleLogs.length }})</span>
      </h1>
      <div class="flex items-center gap-2">
        <select
          class="select select-xs select-bordered"
          :value="levelFilter"
          @change="levelChoice = ($event.target as HTMLSelectElement).value"
        >
          <option v-for="l in levels" :key="l" :value="l">{{ l }}</option>
        </select>
        <button
          class="btn btn-xs"
          :class="paused ? 'btn-warning' : 'btn-ghost'"
          @click="setPaused(!paused)"
        >
          {{ paused ? '继续' : '暂停' }}
        </button>
        <button class="btn btn-xs btn-ghost" @click="clear">清空</button>
      </div>
    </div>

    <input
      v-model="filterText"
      type="text"
      placeholder="搜索日志..."
      class="input input-sm input-bordered w-full"
    />

    <div
      ref="logContainer"
      class="under-nav min-h-0 flex-1 overflow-auto"
      @scroll="handleScroll"
    >
      <template v-for="(log, i) in visibleLogs" :key="log.seq ?? i">
      <p
        v-if="log.type === 'marker'"
        class="mb-2 px-3 py-1 text-center text-xs text-base-content/50 last:mb-0"
      >
        {{ log.payload }}
      </p>
      <article
        v-else
        class="surface-fill mb-2 rounded-[var(--radius-tile)] px-3 py-2.5 last:mb-0"
      >
        <div class="flex items-center gap-2 text-[11px] leading-none">
          <span class="shrink-0 tabular-nums text-base-content/30">
            {{ log.seq ?? i + 1 }}
          </span>
          <span
            class="shrink-0 font-semibold uppercase tracking-[0.04em]"
            :class="levelColor(log.type)"
          >
            {{ log.type }}
          </span>
          <time class="shrink-0 tabular-nums text-base-content/40">{{ log.time }}</time>
        </div>
        <div class="mt-2 whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-[13px] leading-5 text-base-content/90">
          {{ log.payload }}
        </div>
      </article>
      </template>
      <div v-if="visibleLogs.length === 0" class="py-10 text-center text-sm text-base-content/40">
        暂无日志
      </div>
    </div>

    <div v-if="!autoScroll" class="pointer-events-none absolute inset-x-0 bottom-2 z-10 flex justify-center">
      <button class="btn btn-xs btn-primary pointer-events-auto shadow-md" @click="autoScroll = true">
        跳转到底部
      </button>
    </div>
  </div>
</template>

<style scoped>
.under-nav {
  margin-bottom: calc(-1 * var(--nav-clearance, 0px));
  padding-bottom: calc(var(--nav-clearance, 0px) + 0.5rem);
}
</style>
