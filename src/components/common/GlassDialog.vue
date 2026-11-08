<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'

const props = withDefaults(defineProps<{
  open: boolean
  title: string
  busy?: boolean
  dismissOnBackdrop?: boolean
  size?: 'md' | 'lg'
}>(), { busy: false, dismissOnBackdrop: true, size: 'md' })

const emit = defineEmits<{ close: [] }>()

const titleId = useId()
const panel = ref<HTMLElement | null>(null)
let opener: HTMLElement | null = null

const FOCUSABLE = 'input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])'

function focusables(): HTMLElement[] {
  return Array.from(panel.value?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []).filter((element) => element.tabIndex >= 0)
}

function requestClose() {
  if (!props.busy) emit('close')
}

function onBackdrop() {
  if (props.dismissOnBackdrop) requestClose()
}

function onWindowKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape' || document.querySelector('[role="alertdialog"]')) return
  if (panel.value?.contains(e.target as Node)) return
  e.preventDefault()
  requestClose()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    requestClose()
    return
  }
  if (e.key !== 'Tab') return
  const items = focusables()
  if (items.length === 0) return
  const index = items.indexOf(document.activeElement as HTMLElement)
  const next = e.shiftKey ? (index <= 0 ? items.length - 1 : index - 1) : (index === items.length - 1 ? 0 : index + 1)
  e.preventDefault()
  items[next].focus()
}

watch(() => props.open, (open) => {
  if (open) window.addEventListener('keydown', onWindowKeydown)
  else window.removeEventListener('keydown', onWindowKeydown)
  if (open) {
    opener = document.activeElement as HTMLElement | null
    void nextTick(() => {
      const first = panel.value?.querySelector<HTMLElement>('input:not([disabled]), select:not([disabled]), textarea:not([disabled])')
      ;(first ?? focusables()[0])?.focus()
    })
    return
  }
  const target = opener
  opener = null
  void nextTick(() => {
    if (target?.isConnected) target.focus()
  })
}, { immediate: true })

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onWindowKeydown)
  if (opener?.isConnected) opener.focus()
})
</script>

<template>
  <Teleport to="body">
    <Transition name="glass-pop">
      <div
        v-if="open"
        class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
        @click.self="onBackdrop"
      >
        <div
          ref="panel"
          class="glass-popover w-full rounded-[var(--radius-panel)] p-5"
          :class="size === 'lg' ? 'max-w-3xl' : 'max-w-md'"
          role="dialog"
          aria-modal="true"
          :aria-labelledby="titleId"
          :aria-busy="busy || undefined"
          @keydown="onKeydown"
        >
          <h2 :id="titleId" class="mb-4 text-lg font-semibold">{{ title }}</h2>
          <slot />
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
