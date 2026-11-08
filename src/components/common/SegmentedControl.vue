<script setup lang="ts" generic="T extends string">
import { computed, nextTick, ref } from 'vue'

const props = defineProps<{
  modelValue: T
  options: readonly { value: T; label: string }[]
  labelledby?: string
  label?: string
}>()

const emit = defineEmits<{ 'update:modelValue': [value: T] }>()

const root = ref<HTMLElement | null>(null)

const activeIndex = computed(() => {
  const index = props.options.findIndex((option) => option.value === props.modelValue)
  return index < 0 ? 0 : index
})

function select(index: number, moveFocus: boolean) {
  const option = props.options[index]
  if (!option) return
  if (option.value !== props.modelValue) emit('update:modelValue', option.value)
  if (!moveFocus) return
  void nextTick(() => root.value?.querySelectorAll<HTMLButtonElement>('[role="radio"]')[index]?.focus())
}

function onKeydown(event: KeyboardEvent, index: number) {
  const last = props.options.length - 1
  let next: number
  switch (event.key) {
    case 'ArrowRight':
    case 'ArrowDown': next = index >= last ? 0 : index + 1; break
    case 'ArrowLeft':
    case 'ArrowUp': next = index <= 0 ? last : index - 1; break
    case 'Home': next = 0; break
    case 'End': next = last; break
    default: return
  }
  event.preventDefault()
  select(next, true)
}
</script>

<template>
  <div ref="root" class="settings-segmented" role="radiogroup" :aria-labelledby="labelledby" :aria-label="label">
    <button
      v-for="(option, index) in options"
      :key="option.value"
      type="button"
      role="radio"
      :class="{ 'is-active': index === activeIndex && option.value === modelValue }"
      :aria-checked="option.value === modelValue"
      :tabindex="index === activeIndex ? 0 : -1"
      @click="select(index, false)"
      @keydown="onKeydown($event, index)"
    >
      {{ option.label }}
    </button>
  </div>
</template>
