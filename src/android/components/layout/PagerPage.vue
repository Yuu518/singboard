<script setup lang="ts">
import { computed, provide } from 'vue'
import { PAGE_ACTIVE } from '@/android/composables/usePageActive'

const props = defineProps<{
  active: boolean
  fill?: boolean
}>()

const emit = defineEmits<{
  scroll: [top: number]
}>()

provide(PAGE_ACTIVE, computed(() => props.active))

function onScroll(event: Event) {
  emit('scroll', (event.target as HTMLElement).scrollTop)
}
</script>

<template>
  <section
    class="pager-page"
    :class="{ 'is-fill': fill }"
    :inert="!active || undefined"
    :aria-hidden="!active || undefined"
    @scroll.passive="onScroll"
  >
    <div class="mx-auto max-w-5xl" :class="{ 'h-full': fill }">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.pager-page {
  position: relative;
  width: 100%;
  height: 100%;
  flex: 0 0 100%;
  overflow-x: hidden;
  overflow-y: auto;
  padding: calc(var(--safe-top) + 3.25rem) calc(var(--safe-right) + 0.75rem) var(--nav-clearance)
    calc(var(--safe-left) + 0.75rem);
  scroll-snap-align: start;
  scroll-snap-stop: always;
}

@media (min-width: 768px) {
  .pager-page {
    padding-right: calc(var(--safe-right) + 1.5rem);
    padding-left: calc(var(--safe-left) + 1.5rem);
  }
}
</style>
