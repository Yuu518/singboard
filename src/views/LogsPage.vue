<script setup lang="ts">
import { ref, nextTick, watch } from 'vue'
import { useLogsStore } from '@/stores/logs'

const {
  filteredLogs,
  logLevel,
  paused,
  filterText,
  clear,
  changeLevel,
} = useLogsStore()

const logContainer = ref<HTMLElement | null>(null)
const autoScroll = ref(true)

const levels = ['trace', 'debug', 'info', 'warn', 'error', 'fatal', 'panic']

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

watch(filteredLogs, () => {
  if (autoScroll.value) {
    nextTick(() => {
      if (logContainer.value) {
        logContainer.value.scrollTop = logContainer.value.scrollHeight
      }
    })
  }
}, { deep: true })

function handleScroll() {
  if (!logContainer.value) return
  const { scrollTop, scrollHeight, clientHeight } = logContainer.value
  autoScroll.value = scrollHeight - scrollTop - clientHeight < 50
}
</script>

<template>
  <div class="flex flex-col h-full gap-3">
    <div class="flex items-center justify-between">
      <h1 class="text-xl font-bold">
        日志
        <span class="text-sm font-normal text-base-content/50">({{ filteredLogs.length }})</span>
      </h1>
      <div class="flex items-center gap-2">
        <select
          class="select select-xs select-bordered"
          :value="logLevel"
          @change="changeLevel(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="l in levels" :key="l" :value="l">{{ l }}</option>
        </select>
        <button
          class="btn btn-xs"
          :class="paused ? 'btn-warning' : 'btn-ghost'"
          @click="paused = !paused"
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
      class="flex-1 min-h-0 overflow-auto rounded-xl bg-base-200 p-2"
      @scroll="handleScroll"
    >
      <article
        v-for="(log, i) in filteredLogs"
        :key="log.seq ?? i"
        class="mb-2 rounded-xl border border-base-300/50 bg-base-100 px-3 py-2.5 last:mb-0"
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
      <div v-if="filteredLogs.length === 0" class="py-10 text-center text-sm text-base-content/40">
        暂无日志
      </div>
    </div>

    <div v-if="!autoScroll" class="text-center">
      <button class="btn btn-xs btn-primary" @click="autoScroll = true">
        跳转到底部
      </button>
    </div>
  </div>
</template>
