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
    case 'fatal': return 'text-error'
    case 'warning':
    case 'warn': return 'text-warning'
    case 'info': return 'text-info'
    case 'debug': return 'text-base-content/50'
    default: return 'text-base-content/30'
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
      class="flex-1 overflow-auto bg-base-200 rounded-lg p-3 font-mono text-xs"
      @scroll="handleScroll"
    >
      <div v-for="(log, i) in filteredLogs" :key="i" class="py-0.5 flex gap-2">
        <span class="text-base-content/30 shrink-0 w-16">{{ log.time }}</span>
        <span class="shrink-0 w-12 font-semibold" :class="levelColor(log.type)">
          {{ log.type }}
        </span>
        <span class="break-all">{{ log.payload }}</span>
      </div>
      <div v-if="filteredLogs.length === 0" class="text-center text-base-content/40 py-10">
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
