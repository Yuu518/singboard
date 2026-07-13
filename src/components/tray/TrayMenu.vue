<script setup lang="ts">
import { computed, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useServiceStore } from '@/stores/service'
import { useConfigStore } from '@/stores/config'
import { startService, stopService, restartService } from '@/bridge/service'
import { showMainWindow, quitApp } from '@/bridge/app'

const { serviceStatus, refresh } = useServiceStore()
const { serviceName } = useConfigStore()

const isRunning = computed(() => serviceStatus.value.state === 'running')
const busy = ref<'' | 'toggle' | 'restart'>('')

async function handleToggle() {
  if (busy.value) return
  busy.value = 'toggle'
  try {
    if (isRunning.value) {
      await stopService(serviceName.value)
    } else {
      await startService(serviceName.value)
    }
  } catch { }
  await refresh()
  busy.value = ''
}

async function handleRestart() {
  if (busy.value) return
  busy.value = 'restart'
  try {
    await restartService(serviceName.value)
  } catch { }
  await refresh()
  busy.value = ''
}

function openPanel() {
  showMainWindow().catch(() => { })
  getCurrentWindow().hide()
}

function handleQuit() {
  quitApp().catch(() => { })
}
</script>

<template>
  <div class="h-screen w-screen p-1.5">
    <div class="h-full flex flex-col bg-base-200 text-base-content border border-base-300 rounded-xl shadow-lg overflow-hidden select-none">
      <div class="flex items-center justify-center gap-3 flex-1">
        <button
          class="btn btn-circle btn-sm border-none text-white"
          :class="isRunning ? 'bg-success hover:bg-success/80' : 'bg-error hover:bg-error/80'"
          :title="isRunning ? '停止服务' : '启动服务'"
          @click="handleToggle"
        >
          <span v-if="busy === 'toggle'" class="loading loading-spinner loading-xs"></span>
          <svg v-else class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
            <line x1="12" y1="2" x2="12" y2="12" />
          </svg>
        </button>
        <button
          class="btn btn-circle btn-sm btn-ghost border border-base-content/20"
          title="重启服务"
          @click="handleRestart"
        >
          <span v-if="busy === 'restart'" class="loading loading-spinner loading-xs"></span>
          <svg v-else class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="1 4 1 10 7 10" />
            <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
          </svg>
        </button>
      </div>
      <div class="border-t border-base-300"></div>
      <button class="text-xs py-1.5 hover:bg-base-300 transition-colors" @click="openPanel">打开面板</button>
      <div class="border-t border-base-300"></div>
      <button class="text-xs py-1.5 hover:bg-base-300 transition-colors" @click="handleQuit">退出</button>
    </div>
  </div>
</template>

<style>
/* 托盘菜单窗口透明背景（该组件仅在 tray 窗口挂载） */
html,
body {
  background: transparent !important;
}
</style>
