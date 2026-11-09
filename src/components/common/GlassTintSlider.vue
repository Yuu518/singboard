<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { GLASS_TICKS, applyGlassTint, glassTintLabel, snapGlassTint } from '@/utils/glassTint'

const props = defineProps<{
  modelValue: number
  labelledby?: string
}>()

const emit = defineEmits<{ 'update:modelValue': [value: number] }>()

const KEY_STEP = 0.05
const PAGE_STEP = 0.25

const draft = ref(props.modelValue)

watch(() => props.modelValue, (value) => {
  draft.value = value
})

const valueText = computed(() => `${glassTintLabel(draft.value)}，${Math.round(draft.value * 100)}%`)

function preview(input: HTMLInputElement, value: number) {
  const tint = snapGlassTint(value)
  if (Number(input.value) !== tint) input.value = String(tint)
  draft.value = tint
  applyGlassTint(tint)
  return tint
}

function commit(tint: number) {
  if (tint !== props.modelValue) emit('update:modelValue', tint)
}

function onInput(event: Event) {
  const input = event.target as HTMLInputElement
  preview(input, Number(input.value))
}

function onChange(event: Event) {
  const input = event.target as HTMLInputElement
  commit(preview(input, Number(input.value)))
}

function onKeydown(event: KeyboardEvent) {
  const input = event.target as HTMLInputElement
  let next: number
  switch (event.key) {
    case 'ArrowRight':
    case 'ArrowUp': next = draft.value + KEY_STEP; break
    case 'ArrowLeft':
    case 'ArrowDown': next = draft.value - KEY_STEP; break
    case 'PageUp': next = draft.value + PAGE_STEP; break
    case 'PageDown': next = draft.value - PAGE_STEP; break
    case 'Home': next = 0; break
    case 'End': next = 1; break
    default: return
  }
  event.preventDefault()
  commit(preview(input, next))
}

onBeforeUnmount(() => {
  if (draft.value !== props.modelValue) applyGlassTint(props.modelValue)
})
</script>

<template>
  <div class="glass-tint" :style="{ '--tint': draft }">
    <svg class="glass-tint-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="2.75" y="4.75" width="13" height="10" rx="3.25" />
      <rect x="8.25" y="9.25" width="13" height="10" rx="3.25" />
    </svg>
    <div class="glass-tint-track">
      <input
        class="glass-tint-input"
        type="range"
        min="0"
        max="1"
        step="0.01"
        :value="draft"
        :aria-labelledby="labelledby"
        :aria-valuetext="valueText"
        @input="onInput"
        @change="onChange"
        @keydown="onKeydown"
      />
      <span
        v-for="tick in GLASS_TICKS"
        :key="tick"
        class="glass-tint-tick"
        :class="{ 'is-covered': Math.abs(draft - tick) < 0.1 }"
        :style="{ '--at': tick }"
        aria-hidden="true"
      />
    </div>
    <svg class="glass-tint-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="2.75" y="4.75" width="13" height="10" rx="3.25" class="is-back" />
      <rect x="8.25" y="9.25" width="13" height="10" rx="3.25" class="is-front" />
    </svg>
  </div>
</template>

<style scoped>
.glass-tint {
  --thumb-w: 40px;
  --thumb-h: 26px;
  --track-h: 4px;
  --center: calc(var(--thumb-w) / 2 + (100% - var(--thumb-w)) * var(--tint));
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 12px;
}

.glass-tint-icon {
  width: 22px;
  height: 22px;
  flex: none;
  color: oklch(var(--bc) / 0.55);
  stroke: currentColor;
  stroke-width: 1.5;
}

.glass-tint-icon .is-back {
  fill: currentColor;
  fill-opacity: 0.35;
}

.glass-tint-icon .is-front {
  fill: currentColor;
}

.glass-tint-track {
  position: relative;
  min-width: 0;
  flex: 1;
}

.glass-tint-input {
  display: block;
  width: 100%;
  height: 36px;
  margin: 0;
  background: transparent;
  cursor: pointer;
  touch-action: pan-y;
  -webkit-appearance: none;
  appearance: none;
  -webkit-tap-highlight-color: transparent;
}

.glass-tint-input:focus {
  outline: none;
}

.glass-tint-input::-webkit-slider-runnable-track {
  height: var(--track-h);
  border-radius: 9999px;
  background: linear-gradient(to right, oklch(var(--p)) 0 var(--center), oklch(var(--bc) / 0.2) var(--center) 100%);
}

.glass-tint-input::-webkit-slider-thumb {
  width: var(--thumb-w);
  height: var(--thumb-h);
  margin-top: calc((var(--track-h) - var(--thumb-h)) / 2);
  border-radius: 9999px;
  background: #ffffff;
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.12), 0 3px 10px -2px rgb(0 0 0 / 0.18), 0 0 0 0.5px rgb(0 0 0 / 0.06);
  transition: transform 220ms cubic-bezier(0.34, 1.56, 0.64, 1), box-shadow 180ms ease;
  -webkit-appearance: none;
  appearance: none;
}

.glass-tint-input:active::-webkit-slider-thumb {
  transform: scale(1.15);
}

.glass-tint-input:focus-visible::-webkit-slider-thumb {
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.12), 0 3px 10px -2px rgb(0 0 0 / 0.18), 0 0 0 2px var(--page-bg), 0 0 0 4px oklch(var(--p));
}

.glass-tint-tick {
  position: absolute;
  top: calc(50% + 9px);
  left: calc(var(--thumb-w) / 2 + (100% - var(--thumb-w)) * var(--at));
  width: 4px;
  height: 4px;
  margin-left: -2px;
  border-radius: 9999px;
  background: oklch(var(--bc) / 0.3);
  pointer-events: none;
  transition: opacity 150ms ease;
}

.glass-tint-tick.is-covered {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .glass-tint-input::-webkit-slider-thumb,
  .glass-tint-tick {
    transition: none;
  }

  .glass-tint-input:active::-webkit-slider-thumb {
    transform: none;
  }
}

@media (prefers-contrast: more) {
  .glass-tint-input::-webkit-slider-runnable-track {
    background: linear-gradient(to right, oklch(var(--p)) 0 var(--center), oklch(var(--bc) / 0.35) var(--center) 100%);
  }
}
</style>
