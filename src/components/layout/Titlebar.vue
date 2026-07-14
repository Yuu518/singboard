<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'

const appWindow = getCurrentWindow()

async function minimize() {
  await appWindow.minimize()
}
async function toggleMaximize() {
  await appWindow.toggleMaximize()
}
async function close() {
  await appWindow.close()
}
</script>

<template>
  <div
    data-tauri-drag-region
    class="flex items-center justify-between h-10 bg-base-200 border-b border-base-300 select-none px-3"
  >
    <div class="flex items-center gap-2" data-tauri-drag-region>
      <span class="text-sm font-semibold">singboard</span>
    </div>
    <div class="flex items-center gap-1">
      <button
        class="btn btn-ghost btn-xs btn-square"
        @click="minimize"
        title="最小化"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 12 12">
          <rect fill="currentColor" x="1" y="5.5" width="10" height="1" />
        </svg>
      </button>
      <button
        class="btn btn-ghost btn-xs btn-square"
        @click="toggleMaximize"
        title="最大化"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 12 12">
          <rect
            fill="none"
            stroke="currentColor"
            stroke-width="1"
            x="1.5"
            y="1.5"
            width="9"
            height="9"
            rx="1"
          />
        </svg>
      </button>
      <button
        class="btn btn-ghost btn-xs btn-square hover:bg-error hover:text-error-content"
        @click="close"
        title="关闭"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 12 12">
          <line stroke="currentColor" stroke-width="1.2" x1="2" y1="2" x2="10" y2="10" />
          <line stroke="currentColor" stroke-width="1.2" x1="10" y1="2" x2="2" y2="10" />
        </svg>
      </button>
    </div>
  </div>
</template>
