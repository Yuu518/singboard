<script setup lang="ts" generic="T extends string">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'

interface SettingsSelectOption<V extends string = string> {
  value: V
  label: string
  description?: string
}

const props = withDefaults(defineProps<{
  modelValue: T
  options: SettingsSelectOption<T>[]
  placeholder?: string
  ariaLabel?: string
  title?: string
  disabled?: boolean
}>(), {
  placeholder: '请选择',
  ariaLabel: undefined,
  title: undefined,
  disabled: false,
})

const emit = defineEmits<{
  'update:modelValue': [value: T]
}>()

const MENU_MAX_HEIGHT = 220
const MENU_OPTION_HEIGHT = 36

const rootEl = ref<HTMLElement | null>(null)
const triggerEl = ref<HTMLButtonElement | null>(null)
const menuEl = ref<HTMLElement | null>(null)
const open = ref(false)
const above = ref(false)

const selected = computed(() => props.options.find((option) => option.value === props.modelValue))

function optionButtons() {
  return Array.from(menuEl.value?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])
}

function shouldOpenAbove() {
  const trigger = triggerEl.value
  if (!trigger) return false
  const rect = trigger.getBoundingClientRect()
  const viewport = window.innerHeight
  const bottomLimit = Math.min(viewport, document.querySelector('.liquid-nav')?.getBoundingClientRect().top ?? viewport)
  const topLimit = Math.max(0, document.querySelector('.app-header')?.getBoundingClientRect().bottom ?? 0)
  const needed = Math.min(MENU_MAX_HEIGHT, viewport * 0.42, props.options.length * MENU_OPTION_HEIGHT + 8) + 4
  const spaceBelow = bottomLimit - rect.bottom
  const spaceAbove = rect.top - topLimit
  return spaceBelow < needed && spaceAbove > spaceBelow
}

async function openMenu(focusSelected = false) {
  if (props.disabled || open.value) return
  above.value = shouldOpenAbove()
  open.value = true
  await nextTick()
  const menu = menuEl.value
  const current = menu?.querySelector<HTMLButtonElement>('[aria-selected="true"]')
  if (menu && current) {
    menu.scrollTop = current.offsetTop - (menu.clientHeight - current.offsetHeight) / 2
  }
  if (focusSelected) {
    ;(current ?? optionButtons()[0])?.focus({ preventScroll: true })
  }
}

function closeMenu(restoreFocus = false) {
  if (!open.value) return
  open.value = false
  if (restoreFocus) triggerEl.value?.focus({ preventScroll: true })
}

function toggleMenu() {
  if (open.value) closeMenu()
  else void openMenu()
}

function choose(value: T) {
  closeMenu(true)
  if (value !== props.modelValue) emit('update:modelValue', value)
}

function handleTriggerKeydown(event: KeyboardEvent) {
  if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key)) {
    event.preventDefault()
    void openMenu(true)
  } else if (event.key === 'Escape') {
    closeMenu()
  }
}

function handleMenuKeydown(event: KeyboardEvent) {
  const items = optionButtons()
  const index = items.indexOf(document.activeElement as HTMLButtonElement)
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      items[Math.min(index + 1, items.length - 1)]?.focus()
      break
    case 'ArrowUp':
      event.preventDefault()
      items[Math.max(index - 1, 0)]?.focus()
      break
    case 'Home':
      event.preventDefault()
      items[0]?.focus()
      break
    case 'End':
      event.preventDefault()
      items[items.length - 1]?.focus()
      break
    case 'Escape':
      event.preventDefault()
      closeMenu(true)
      break
    case 'Tab':
      closeMenu()
      break
  }
}

function handleDocumentPointerDown(event: PointerEvent) {
  if (!rootEl.value?.contains(event.target as Node)) closeMenu()
}

watch(open, (value) => {
  if (value) document.addEventListener('pointerdown', handleDocumentPointerDown)
  else document.removeEventListener('pointerdown', handleDocumentPointerDown)
})

watch(() => props.disabled, (value) => {
  if (value) closeMenu()
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', handleDocumentPointerDown)
})
</script>

<template>
  <div ref="rootEl" class="settings-select" :class="{ 'is-open': open }">
    <button
      ref="triggerEl"
      type="button"
      class="input input-sm input-bordered settings-select-trigger"
      :title="title"
      :aria-label="ariaLabel"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :disabled="disabled"
      @click="toggleMenu"
      @keydown="handleTriggerKeydown"
    >
      <span class="settings-select-value" :class="{ 'is-placeholder': !selected }">
        {{ selected?.label ?? placeholder }}
      </span>
      <svg class="settings-select-chevron" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
        <path d="M2.5 4.25 6 7.75l3.5-3.5z" />
      </svg>
    </button>
    <div
      v-if="open"
      ref="menuEl"
      class="glass-popover settings-select-menu absolute z-20 w-full rounded-xl p-1"
      :class="{ 'is-above': above }"
      role="listbox"
      :aria-label="ariaLabel"
      @keydown="handleMenuKeydown"
    >
      <div v-if="options.length === 0" class="px-3 py-2 text-sm text-base-content/50">无可选项</div>
      <button
        v-for="option in options"
        :key="option.value"
        type="button"
        role="option"
        :aria-selected="option.value === modelValue"
        class="block w-full rounded-lg px-3 py-2 text-left text-sm hover:bg-base-content/[0.06]"
        @click="choose(option.value)"
      >
        <span class="block truncate">{{ option.label }}</span>
        <span v-if="option.description" class="block truncate text-xs text-base-content/50">{{ option.description }}</span>
      </button>
    </div>
  </div>
</template>
