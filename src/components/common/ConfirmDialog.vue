<script setup lang="ts">
import { ref } from 'vue'

const visible = ref(false)
const title = ref('')
const message = ref('')
const confirmText = ref('确定')
const cancelText = ref('取消')
const variant = ref<'default' | 'danger'>('default')
let resolveFn: ((value: boolean) => void) | null = null

function show(options: {
  title?: string
  message: string
  confirmText?: string
  cancelText?: string
  variant?: 'default' | 'danger'
}): Promise<boolean> {
  title.value = options.title ?? ''
  message.value = options.message
  confirmText.value = options.confirmText ?? '确定'
  cancelText.value = options.cancelText ?? '取消'
  variant.value = options.variant ?? 'default'
  visible.value = true
  return new Promise((resolve) => {
    resolveFn = resolve
  })
}

function confirm() {
  visible.value = false
  resolveFn?.(true)
  resolveFn = null
}

function cancel() {
  visible.value = false
  resolveFn?.(false)
  resolveFn = null
}

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
    <div class="glass-popover w-full max-w-sm rounded-[var(--radius-panel)] p-5 space-y-4">
      <h3 v-if="title" class="text-base font-semibold">{{ title }}</h3>
      <p class="text-sm text-base-content/80 whitespace-pre-line">{{ message }}</p>
      <div class="flex justify-end gap-2">
        <button class="btn btn-sm btn-ghost" @click="cancel">{{ cancelText }}</button>
        <button
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
