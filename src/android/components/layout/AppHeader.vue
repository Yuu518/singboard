<script setup lang="ts">
import { useRoute } from 'vue-router'
import StatusPill from './StatusPill.vue'

defineProps<{
  title: string
  scrolled: boolean
}>()

const route = useRoute()
</script>

<template>
  <header class="app-header" :class="{ 'is-scrolled': scrolled }">
    <div class="mx-auto flex h-12 max-w-5xl items-center justify-between gap-3 px-1">
      <div class="flex min-w-0 items-center gap-2">
        <img src="/favicon.png" alt="" class="h-6 w-6 shrink-0 rounded-md" />
        <span class="truncate text-[15px] font-semibold tracking-tight">{{ scrolled ? title : 'singboard' }}</span>
      </div>
      <StatusPill v-if="route.path !== '/home'" class="shrink-0" />
    </div>
  </header>
</template>

<style scoped>
.app-header {
  position: fixed;
  top: 0;
  right: 0;
  left: 0;
  z-index: 20;
  padding-top: var(--safe-top);
  padding-right: calc(var(--safe-right) + 0.75rem);
  padding-left: calc(var(--safe-left) + 0.75rem);
  transition: background-color 200ms ease, box-shadow 200ms ease;
}

@media (min-width: 768px) {
  .app-header {
    padding-right: calc(var(--safe-right) + 1.5rem);
    padding-left: calc(var(--safe-left) + 1.5rem);
  }
}

:global(html[data-backdrop='image'] .app-header),
.app-header.is-scrolled {
  background: rgb(var(--glass-rgb) / var(--glass-bar-alpha));
  box-shadow: 0 0.5px 0 var(--hairline);
  -webkit-backdrop-filter: saturate(var(--glass-saturate)) blur(var(--glass-blur));
  backdrop-filter: saturate(var(--glass-saturate)) blur(var(--glass-blur));
}
</style>
