<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = defineProps<{
  text: string
}>()

const viewportRef = ref<HTMLElement | null>(null)
const sampleRef = ref<HTMLElement | null>(null)
const overflowing = ref(false)
const durationSeconds = ref(8)
let resizeObserver: ResizeObserver | null = null

const trackStyle = computed(() => ({
  '--overflowing-text-duration': `${durationSeconds.value}s`,
}))

function measure() {
  const viewportWidth = viewportRef.value?.clientWidth ?? 0
  const textWidth = sampleRef.value?.scrollWidth ?? 0
  overflowing.value = viewportWidth > 0 && textWidth > viewportWidth
  durationSeconds.value = Math.max(8, Math.ceil(textWidth / 24))
}

function measureAfterRender() {
  void nextTick(measure)
}

onMounted(() => {
  measureAfterRender()
  if (typeof ResizeObserver === 'undefined') {
    window.addEventListener('resize', measureAfterRender)
    return
  }

  resizeObserver = new ResizeObserver(measureAfterRender)
  if (viewportRef.value) resizeObserver.observe(viewportRef.value)
  if (sampleRef.value) resizeObserver.observe(sampleRef.value)
})

watch(() => props.text, measureAfterRender)

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  window.removeEventListener('resize', measureAfterRender)
})

defineExpose({ measure })
</script>

<template>
  <span
    ref="viewportRef"
    class="overflowing-text"
    :class="{ 'is-overflowing': overflowing }"
    :title="text"
    :tabindex="overflowing ? 0 : undefined"
  >
    <span class="overflowing-text-track" :style="trackStyle">
      <span ref="sampleRef" class="overflowing-text-item">{{ text }}</span>
      <span v-if="overflowing" class="overflowing-text-item" aria-hidden="true">{{ text }}</span>
    </span>
  </span>
</template>

<style scoped>
.overflowing-text {
  display: block;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}

.overflowing-text-track {
  display: flex;
  width: max-content;
  min-width: 100%;
}

.overflowing-text-item {
  display: block;
  flex: none;
}

.overflowing-text.is-overflowing .overflowing-text-track {
  gap: 20px;
  min-width: max-content;
  animation: overflowing-text-scroll var(--overflowing-text-duration) linear infinite;
}

.overflowing-text.is-overflowing:hover .overflowing-text-track,
.overflowing-text.is-overflowing:focus:not(:focus-visible) .overflowing-text-track {
  animation-play-state: paused;
}

.overflowing-text.is-overflowing:focus-visible {
  overflow-x: auto;
  outline: 1px solid currentColor;
  outline-offset: -1px;
  scrollbar-width: thin;
}

.overflowing-text.is-overflowing:focus-visible .overflowing-text-track {
  animation: none;
  transform: none;
}

@keyframes overflowing-text-scroll {
  from { transform: translateX(0); }
  to { transform: translateX(calc(-50% - 10px)); }
}

@media (prefers-reduced-motion: reduce) {
  .overflowing-text.is-overflowing {
    overflow-x: auto;
    scrollbar-width: thin;
  }

  .overflowing-text.is-overflowing .overflowing-text-track {
    animation: none;
  }
}
</style>
