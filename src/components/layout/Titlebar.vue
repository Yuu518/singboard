<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'

defineProps<{
  title: string
  scrolled: boolean
}>()

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
    class="titlebar flex h-12 items-center justify-between pl-5 pr-2.5 select-none"
    :class="{ 'is-scrolled': scrolled }"
  >
    <span class="titlebar-title pointer-events-none text-[15px] font-semibold tracking-tight">{{ title }}</span>
    <div class="flex items-center gap-1.5">
      <button class="titlebar-btn" title="最小化" @click="minimize">
        <svg class="h-3 w-3" viewBox="0 0 12 12" aria-hidden="true">
          <rect fill="currentColor" x="1.5" y="5.4" width="9" height="1.2" rx="0.6" />
        </svg>
      </button>
      <button class="titlebar-btn" title="最大化" @click="toggleMaximize">
        <svg class="h-3 w-3" viewBox="0 0 12 12" aria-hidden="true">
          <rect fill="none" stroke="currentColor" stroke-width="1.2" x="1.8" y="1.8" width="8.4" height="8.4" rx="2" />
        </svg>
      </button>
      <button class="titlebar-btn titlebar-btn-close" title="关闭" @click="close">
        <svg class="h-3 w-3" viewBox="0 0 12 12" aria-hidden="true">
          <path stroke="currentColor" stroke-width="1.3" stroke-linecap="round" d="M2.5 2.5l7 7M9.5 2.5l-7 7" />
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.titlebar::before {
  position: absolute;
  z-index: -1;
  inset: 0 0 -18px;
  background: var(--glass-grain), rgb(var(--glass-rgb) / var(--glass-bar-alpha));
  opacity: 0;
  -webkit-mask-image:
    linear-gradient(to bottom, #000 calc(100% - 26px), transparent),
    linear-gradient(to right, transparent, #000 28px);
  -webkit-mask-composite: source-in;
  mask-image:
    linear-gradient(to bottom, #000 calc(100% - 26px), transparent),
    linear-gradient(to right, transparent, #000 28px);
  mask-composite: intersect;
  pointer-events: none;
  -webkit-backdrop-filter: saturate(var(--glass-saturate)) blur(var(--glass-blur));
  backdrop-filter: saturate(var(--glass-saturate)) blur(var(--glass-blur));
  content: '';
  transition: opacity 200ms ease;
}

.titlebar.is-scrolled::before {
  opacity: 1;
}

.titlebar-title {
  opacity: 0;
  transform: translateY(4px);
  transition: opacity 200ms ease, transform 200ms ease;
}

.titlebar.is-scrolled .titlebar-title {
  opacity: 1;
  transform: none;
}

.titlebar-btn {
  display: inline-flex;
  width: 28px;
  height: 28px;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  background: var(--fill);
  color: oklch(var(--bc) / 0.7);
  transition: background-color 150ms ease, color 150ms ease, transform 150ms ease;
}

.titlebar-btn:hover {
  background: var(--fill-strong);
  color: oklch(var(--bc));
}

.titlebar-btn:active {
  transform: scale(0.94);
}

.titlebar-btn-close:hover {
  background: oklch(var(--er));
  color: oklch(var(--erc));
}

.titlebar-btn:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}
</style>
