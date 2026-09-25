<script setup lang="ts">
import { useToastStore } from '@/stores/toast'

const { toasts, removeToast, pauseToast, resumeToast } = useToastStore()

function accentClass(type?: 'error' | 'info') {
  return type === 'info' ? 'text-primary' : 'text-error'
}
</script>

<template>
  <div class="fixed right-4 top-14 z-[60] pointer-events-none space-y-2">
    <TransitionGroup name="toast">
      <div
        v-for="toast in toasts"
        :key="toast.id"
        class="glass-popover pointer-events-auto min-w-[260px] max-w-[420px] overflow-hidden rounded-[var(--radius-card)] px-3.5 pb-2.5 pt-3"
        :class="accentClass(toast.type)"
        @mouseenter="pauseToast(toast.id)"
        @mouseleave="resumeToast(toast.id)"
      >
        <div class="flex items-start gap-2.5">
          <span class="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-current">
            <svg class="h-3 w-3 text-white" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
              <template v-if="toast.type === 'info'">
                <path d="M6 5.5v3" />
                <path d="M6 3.4v.01" />
              </template>
              <template v-else>
                <path d="M6 3v3.6" />
                <path d="M6 8.8v.01" />
              </template>
            </svg>
          </span>
          <p class="flex-1 whitespace-pre-line text-sm leading-snug text-base-content">{{ toast.message }}</p>
          <button
            class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-base-content/50 transition-colors hover:bg-base-content/10 hover:text-base-content"
            aria-label="关闭"
            @click="removeToast(toast.id)"
          >
            <svg class="h-2.5 w-2.5" viewBox="0 0 10 10" aria-hidden="true">
              <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M2 2l6 6M8 2l-6 6" />
            </svg>
          </button>
        </div>
        <div class="mt-2.5 h-[3px] overflow-hidden rounded-full bg-base-content/10">
          <div
            class="h-full rounded-full bg-current transition-[width] duration-75 ease-linear"
            :style="{ width: `${Math.max(0, (toast.remainingMs / toast.duration) * 100)}%` }"
          />
        </div>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: opacity 220ms ease, transform 260ms cubic-bezier(0.32, 0.72, 0, 1);
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(-8px) scale(0.96);
}

@media (prefers-reduced-motion: reduce) {
  .toast-enter-active,
  .toast-leave-active {
    transition-duration: 0.01ms;
  }
}
</style>
