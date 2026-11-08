<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import type { ConfigProfile } from '@/types'

const props = defineProps<{
  profile: ConfigProfile
  isActive: boolean
  hasBackup?: boolean
  busy?: '' | 'select' | 'update' | 'delete' | 'restore'
  locked?: boolean
}>()

const emit = defineEmits<{
  edit: []
  select: []
  delete: []
  update: []
  'edit-info': []
  restore: []
}>()

type CardEvent = 'edit' | 'select' | 'delete' | 'update' | 'edit-info' | 'restore'

function act(event: CardEvent) {
  if (props.locked) return
  ;(emit as (name: CardEvent) => void)(event)
}

const showFailure = ref(false)
const failureId = useId()

function formatDate(iso?: string): string {
  if (!iso) return ''
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

const isRemote = computed(() => props.profile.type === 'remote')

const sourceTitle = computed(() => {
  if (!isRemote.value) return props.profile.source
  try {
    const url = new URL(props.profile.source)
    return `${url.origin}${url.pathname}`
  } catch {
    return sourceLabel.value
  }
})

const sourceLabel = computed(() => {
  if (!isRemote.value) return props.profile.source
  try {
    return new URL(props.profile.source).host || props.profile.source
  } catch {
    return props.profile.source
  }
})

const timeLabel = computed(() => {
  const at = formatDate(props.profile.lastUpdated)
  const parts = at ? [`${isRemote.value ? '更新于' : '保存于'} ${at}`] : []
  const local = props.profile.lastLocalChangeAt
  if (isRemote.value && local && local > (props.profile.lastUpdated ?? '')) {
    parts.push(`本地改动于 ${formatDate(local)}，下次自动更新会覆盖`)
  }
  if (isRemote.value && props.profile.autoUpdateInterval > 0) parts.push(`每 ${props.profile.autoUpdateInterval} 小时自动更新`)
  return parts.join(' · ')
})

const backupLabel = computed(() => {
  const at = formatDate(props.profile.backupUpdatedAt)
  return at ? `换回 ${at} 的版本` : '换回上一版本'
})

const failureSummary = computed(() => {
  const error = props.profile.lastFetchError
  if (!error) return ''
  const line = error.split(/\r?\n/).map((part) => part.trim()).find(Boolean) ?? error
  const at = formatDate(props.profile.lastFetchFailedAt)
  return at ? `${at} 更新失败：${line}` : `更新失败：${line}`
})
</script>

<template>
  <div
    class="surface-card p-4"
    :class="isActive ? 'config-card-active' : ''"
    :data-profile-id="profile.id"
    :aria-current="isActive ? 'true' : undefined"
    :aria-busy="!!busy"
  >
    <div class="flex items-start justify-between gap-2">
      <h2 class="min-w-0 truncate text-[15px] font-semibold" :title="profile.name">{{ profile.name }}</h2>
      <span
        v-if="isActive"
        data-active-pill
        tabindex="-1"
        class="inline-flex h-6 shrink-0 items-center gap-1 rounded-full bg-primary/15 px-2.5 text-xs font-semibold text-[color:var(--selection-ink)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
      >
        <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M2.5 6.2 5 8.6l4.5-5" />
        </svg>
        <span class="sr-only">「{{ profile.name }}」</span>使用中
      </span>
      <button
        v-else
        type="button"
        class="btn btn-xs btn-glass shrink-0 px-3 text-[color:var(--selection-ink)]"
        :class="{ 'config-locked': locked }"
        :aria-disabled="locked || undefined"
        :aria-label="`使用「${profile.name}」`"
        @click="act('select')"
      >
        <span v-if="busy === 'select'" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        {{ busy === 'select' ? '校验中…' : '使用' }}
      </button>
    </div>

    <div class="mt-1.5 flex min-w-0 items-center gap-1.5 text-xs text-base-content/65">
      <span class="inline-flex h-5 shrink-0 items-center gap-1 rounded-full bg-base-content/[0.08] px-2 text-[11.5px] text-base-content/75">
        <svg v-if="isRemote" class="h-3 w-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M4.5 12.5h7a3 3 0 0 0 .4-6 4 4 0 0 0-7.7.8A2.6 2.6 0 0 0 4.5 12.5Z" />
        </svg>
        <svg v-else class="h-3 w-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.5 1.5h4.9A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5Z" />
        </svg>
        {{ isRemote ? '远程' : '本地' }}
      </span>
      <span class="settings-mono min-w-0 truncate text-[11.5px]" :title="sourceTitle">{{ sourceLabel }}</span>
    </div>

    <p v-if="timeLabel" class="mt-1 text-xs tabular-nums text-base-content/65">{{ timeLabel }}</p>
    <div v-if="failureSummary" class="mt-1 text-xs text-[color:var(--danger-ink)]">
      <div class="flex min-w-0 items-baseline gap-1">
        <span class="min-w-0" :class="showFailure ? '' : 'truncate'">保留现有配置 · {{ failureSummary }}</span>
        <button
          type="button"
          class="shrink-0 underline underline-offset-2"
          :aria-expanded="showFailure"
          :aria-controls="failureId"
          @click="showFailure = !showFailure"
        >
          {{ showFailure ? '收起' : '详情' }}
        </button>
      </div>
      <pre
        v-if="showFailure"
        :id="failureId"
        class="settings-mono mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-[var(--radius-tile)] bg-base-content/[0.05] p-2 text-[11px] text-base-content/80"
      >{{ profile.lastFetchError }}</pre>
    </div>

    <div class="mt-2.5 flex flex-wrap items-center gap-0.5">
      <button
        type="button"
        class="btn btn-xs btn-ghost"
        :class="{ 'config-locked': locked }"
        :aria-disabled="locked || undefined"
        :aria-label="`编辑「${profile.name}」`"
        data-action="edit"
        @click="act('edit')"
      >
        编辑
      </button>
      <button
        v-if="isRemote"
        type="button"
        class="btn btn-xs btn-ghost"
        :class="{ 'config-locked': locked }"
        :aria-disabled="locked || undefined"
        :aria-label="`更新「${profile.name}」`"
        @click="act('update')"
      >
        <span v-if="busy === 'update'" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        {{ busy === 'update' ? '更新中…' : '更新' }}
      </button>
      <button
        v-if="hasBackup"
        type="button"
        class="btn btn-xs btn-ghost"
        :class="{ 'config-locked': locked }"
        :aria-disabled="locked || undefined"
        :aria-label="`把「${profile.name}」${backupLabel}`"
        :title="`${backupLabel}，换回后还可以再换回来`"
        @click="act('restore')"
      >
        <span v-if="busy === 'restore'" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        {{ busy === 'restore' ? '换回中…' : backupLabel }}
      </button>
      <button
        type="button"
        class="btn btn-xs btn-ghost"
        :class="{ 'config-locked': locked }"
        :aria-disabled="locked || undefined"
        :aria-label="`修改「${profile.name}」的名称和来源`"
        @click="act('edit-info')"
      >
        修改信息
      </button>
      <button
        type="button"
        class="btn btn-xs btn-ghost ml-auto text-[color:var(--danger-ink)]"
        :class="{ 'config-locked': locked }"
        :aria-disabled="locked || undefined"
        :aria-label="`删除「${profile.name}」`"
        @click="act('delete')"
      >
        <span v-if="busy === 'delete'" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
        {{ busy === 'delete' ? '删除中…' : '删除' }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.config-locked {
  opacity: 0.5;
  cursor: not-allowed;
}

.config-card-active {
  background:
    linear-gradient(oklch(var(--p) / 0.1), oklch(var(--p) / 0.1)),
    var(--glass-grain),
    rgb(var(--glass-rgb) / var(--glass-card-alpha));
  box-shadow: inset 0 0 0 1px oklch(var(--p) / 0.4), var(--glass-highlight), var(--glass-card-shadow);
}
</style>
