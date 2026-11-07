<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref } from 'vue'

const visible = ref(false)
const title = ref('')
const message = ref('')
const confirmText = ref('确定')
const cancelText = ref('取消')
const variant = ref<'default' | 'danger'>('default')
let resolveFn: ((value: boolean) => void) | null = null
let opener: HTMLElement | null = null

const cancelButton = ref<HTMLButtonElement | null>(null)
const confirmButton = ref<HTMLButtonElement | null>(null)

function show(options: {
  title?: string
  message: string
  confirmText?: string
  cancelText?: string
  variant?: 'default' | 'danger'
}): Promise<boolean> {
  resolveFn?.(false)
  if (!visible.value) opener = document.activeElement as HTMLElement | null
  title.value = options.title ?? ''
  message.value = options.message
  confirmText.value = options.confirmText ?? '确定'
  cancelText.value = options.cancelText ?? '取消'
  variant.value = options.variant ?? 'default'
  visible.value = true
  window.addEventListener('keydown', onKeydown, true)
  void nextTick(() => (variant.value === 'danger' ? cancelButton.value : confirmButton.value)?.focus())
  return new Promise((resolve) => {
    resolveFn = resolve
  })
}

function finish(result: boolean) {
  visible.value = false
  window.removeEventListener('keydown', onKeydown, true)
  resolveFn?.(result)
  resolveFn = null
  const target = opener
  opener = null
  void nextTick(() => {
    if (target?.isConnected) target.focus()
  })
}

function confirm() {
  finish(true)
}

function cancel() {
  finish(false)
}

function onKeydown(e: KeyboardEvent) {
  if (!visible.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    cancel()
    return
  }
  if (e.key !== 'Tab') return
  const buttons = [cancelButton.value, confirmButton.value].filter((b): b is HTMLButtonElement => !!b)
  if (buttons.length === 0) return
  e.preventDefault()
  const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
  const next = e.shiftKey ? index - 1 : index + 1
  buttons[(next + buttons.length) % buttons.length].focus()
}

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown, true)
})

defineExpose({ show })
</script>

<template>
  <Teleport to="body">
  <Transition name="glass-pop">
  <div
    v-if="visible"
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="cancel"
  >
    <div
      class="glass-popover w-full max-w-sm rounded-[var(--radius-panel)] p-5 space-y-4"
      role="alertdialog"
      aria-modal="true"
      :aria-labelledby="title ? 'confirm-dialog-title' : undefined"
      aria-describedby="confirm-dialog-message"
    >
      <h3 v-if="title" id="confirm-dialog-title" class="text-base font-semibold">{{ title }}</h3>
      <p id="confirm-dialog-message" class="text-sm text-base-content/80 whitespace-pre-line">{{ message }}</p>
      <div class="flex justify-end gap-2">
        <button ref="cancelButton" type="button" class="btn btn-sm btn-ghost" @click="cancel">{{ cancelText }}</button>
        <button
          ref="confirmButton"
          type="button"
          class="btn btn-sm"
          :class="variant === 'danger' ? 'btn-error' : 'btn-primary'"
          @click="confirm"
        >
          {{ confirmText }}
        </button>
      </div>
    </div>
  </div>
  </Transition>
  </Teleport>
</template>
