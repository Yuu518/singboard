<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId } from 'vue'

export interface ConfirmDetail {
  label: string
  value: string
  mono?: boolean
}

export interface ConfirmLink {
  label: string
  open: () => void
}

export interface ConfirmOptions {
  title?: string
  message: string
  details?: ConfirmDetail[]
  warning?: string
  link?: ConfirmLink
  confirmText?: string
  cancelText?: string
  variant?: 'default' | 'danger'
}

const titleId = useId()
const messageId = useId()
const visible = ref(false)
const title = ref('')
const message = ref('')
const details = ref<ConfirmDetail[]>([])
const warning = ref('')
const link = ref<ConfirmLink | null>(null)
const confirmText = ref('确定')
const cancelText = ref('取消')
const variant = ref<'default' | 'danger'>('default')
let resolveFn: ((value: boolean) => void) | null = null
let opener: HTMLElement | null = null

const panel = ref<HTMLElement | null>(null)
const cancelButton = ref<HTMLButtonElement | null>(null)
const confirmButton = ref<HTMLButtonElement | null>(null)

function show(options: ConfirmOptions): Promise<boolean> {
  resolveFn?.(false)
  if (!visible.value) opener = document.activeElement as HTMLElement | null
  title.value = options.title ?? ''
  message.value = options.message
  details.value = options.details ?? []
  warning.value = options.warning ?? ''
  link.value = options.link ?? null
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
  const items = Array.from(panel.value?.querySelectorAll<HTMLElement>('button:not([disabled])') ?? [])
  if (items.length === 0) return
  e.preventDefault()
  const index = items.indexOf(document.activeElement as HTMLElement)
  const next = e.shiftKey ? index - 1 : index + 1
  items[(next + items.length) % items.length].focus()
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
      ref="panel"
      class="glass-popover w-full max-w-sm rounded-[var(--radius-panel)] p-5 space-y-4"
      role="alertdialog"
      aria-modal="true"
      :aria-labelledby="title ? titleId : undefined"
      :aria-describedby="messageId"
    >
      <h2 v-if="title" :id="titleId" class="text-lg font-semibold">{{ title }}</h2>
      <dl v-if="details.length" class="confirm-details">
        <template v-for="detail in details" :key="detail.label">
          <dt>{{ detail.label }}</dt>
          <dd :class="{ 'confirm-mono': detail.mono }">{{ detail.value }}</dd>
        </template>
      </dl>
      <p :id="messageId" class="text-sm text-base-content/80 whitespace-pre-line">{{ message }}</p>
      <p v-if="warning" class="confirm-warning text-sm" role="note">{{ warning }}</p>
      <div class="flex items-center justify-end gap-2">
        <button v-if="link" type="button" class="confirm-link mr-auto" @click="link.open()">
          {{ link.label }}
          <svg viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="M4.5 2.5h5v5M9.5 2.5l-7 7" /></svg>
        </button>
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

<style scoped>
.confirm-details {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 6px 14px;
  margin: 0;
  font-size: 13px;
  line-height: 1.4;
}

.confirm-details dt {
  color: oklch(var(--bc) / 0.55);
}

.confirm-details dd {
  min-width: 0;
  margin: 0;
  overflow-wrap: anywhere;
}

.confirm-mono {
  font-family: 'Cascadia Mono', 'SFMono-Regular', Consolas, monospace;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.confirm-warning {
  margin: 0;
  color: var(--danger-ink);
  line-height: 1.5;
}

.confirm-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 4px;
  border-radius: 6px;
  color: var(--selection-ink);
  font-size: 13px;
  font-weight: 500;
}

.confirm-link:hover {
  text-decoration: underline;
}

.confirm-link:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}

.confirm-link svg {
  width: 11px;
  height: 11px;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 1.4;
}
</style>
