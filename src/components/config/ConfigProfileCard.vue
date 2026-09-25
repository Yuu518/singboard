<script setup lang="ts">
import type { ConfigProfile } from '@/types'

defineProps<{
  profile: ConfigProfile
  isActive: boolean
}>()

defineEmits<{
  edit: []
  select: []
  delete: []
  update: []
  'edit-info': []
}>()

function formatDate(iso?: string): string {
  if (!iso) return ''
  try {
    const d = new Date(iso)
    return d.toLocaleString()
  } catch {
    return iso
  }
}
</script>

<template>
  <div
    class="surface-card p-4 outline outline-2 outline-offset-0 transition-[outline-color]"
    :class="isActive ? 'outline-primary' : 'outline-transparent'"
  >
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0 flex-1">
        <h3 class="font-medium truncate">{{ profile.name }}</h3>
        <div class="flex items-center gap-2 mt-1">
          <span
            class="badge badge-sm shrink-0"
            :class="profile.type === 'local' ? 'badge-info' : 'badge-accent'"
          >
            {{ profile.type === 'local' ? '本地' : '远程' }}
          </span>
          <span
            v-if="profile.type === 'local'"
            class="text-xs text-base-content/50 truncate"
            :title="profile.source"
          >
            {{ profile.source }}
          </span>
        </div>
        <div class="flex items-center gap-2 mt-1 text-xs text-base-content/40">
          <span v-if="profile.lastUpdated">{{ formatDate(profile.lastUpdated) }}</span>
          <span v-if="profile.type === 'remote' && profile.autoUpdateInterval > 0">· 每 {{ profile.autoUpdateInterval }} 小时自动更新</span>
        </div>
      </div>
      <button
        class="btn btn-xs shrink-0"
        :class="isActive ? 'btn-disabled text-base-content/40' : 'btn-primary'"
        :disabled="isActive"
        @click="$emit('select')"
      >
        {{ isActive ? '使用中' : '使用' }}
      </button>
    </div>
    <div class="flex items-center gap-2 mt-3">
      <button class="btn btn-xs btn-ghost" @click="$emit('edit')">编辑配置</button>
      <button
        v-if="profile.type === 'remote'"
        class="btn btn-xs btn-ghost"
        @click="$emit('update')"
      >
        更新
      </button>
      <button class="btn btn-xs btn-ghost" @click="$emit('edit-info')">修改信息</button>
      <div class="flex-1"></div>
      <button class="btn btn-xs btn-ghost text-error" @click="$emit('delete')">删除</button>
    </div>
  </div>
</template>
