<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import GlassDialog from '@/components/common/GlassDialog.vue'

withDefaults(defineProps<{ buttonClass?: string }>(), { buttonClass: 'settings-btn' })

const { config } = useConfigStore()
const { pushToast } = useToastStore()

const inputId = useId()
const descId = useId()
const errorId = useId()

const open = ref(false)
const draft = ref('')
const reveal = ref(false)
const error = ref('')

const hasToken = computed(() => !!config.value.githubToken)

function validate(value: string): string {
  if (!value) return ''
  if (!/^[A-Za-z0-9_]+$/.test(value)) return 'Token 只能包含字母、数字和下划线，请检查是否多复制了空格或引号'
  if (value.length < 20 || value.length > 255) return 'Token 长度不对，请重新复制完整的 Token'
  return ''
}

function show() {
  draft.value = config.value.githubToken
  reveal.value = false
  error.value = ''
  open.value = true
}

function close() {
  open.value = false
  draft.value = ''
}

function save() {
  const value = draft.value.trim()
  const problem = validate(value)
  if (problem) {
    error.value = problem
    return
  }
  const changed = value !== config.value.githubToken
  config.value.githubToken = value
  close()
  if (changed) pushToast({ message: value ? 'GitHub Token 已保存' : 'GitHub Token 已清除', type: 'info' })
}

function clear() {
  config.value.githubToken = ''
  close()
  pushToast({ message: 'GitHub Token 已清除', type: 'info' })
}
</script>

<template>
  <div class="settings-row">
    <div class="settings-row-copy">
      <strong>GitHub Token<span v-if="hasToken" class="github-token-pill">已填写</span></strong>
      <span>{{ hasToken ? '检查核心和面板更新时带上 Token，面板下载优先走 GitHub API。' : '可选。填写后检查核心和面板更新不受匿名限流影响。' }}</span>
    </div>
    <button type="button" class="btn btn-sm" :class="buttonClass" @click="show">
      {{ hasToken ? '修改' : '填写' }}
    </button>
  </div>

  <GlassDialog :open="open" title="GitHub Token" @close="close">
    <form class="space-y-4" novalidate @submit.prevent="save">
      <p :id="descId" class="text-sm leading-relaxed text-base-content/75">
        填写后，检查核心和面板更新都会带上这个 Token，不受匿名限流影响；下载面板更新也会优先通过 GitHub API，失败时再改走 Release 直链。Token 只保存在本机，只发给 api.github.com。
      </p>
      <div class="space-y-1.5">
        <label :for="inputId" class="block text-xs text-base-content/70">Token</label>
        <div class="flex gap-2">
          <input
            :id="inputId"
            v-model="draft"
            :type="reveal ? 'text' : 'password'"
            class="input input-sm input-bordered settings-mono min-w-0 flex-1"
            placeholder="github_pat_… 或 ghp_…"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            :aria-invalid="!!error"
            :aria-describedby="error ? `${descId} ${errorId}` : descId"
            @input="error = ''"
          />
          <button
            type="button"
            class="btn btn-sm btn-glass shrink-0"
            :aria-pressed="reveal"
            @click="reveal = !reveal"
          >
            {{ reveal ? '隐藏' : '显示' }}
          </button>
        </div>
        <p class="text-xs text-base-content/60">公开仓库不需要任何权限，建议创建一个不勾选权限的 Fine-grained Token。</p>
      </div>
      <p v-if="error" :id="errorId" class="text-xs text-[color:var(--danger-ink)]" role="alert">{{ error }}</p>
      <div class="flex items-center justify-end gap-2">
        <button v-if="hasToken" type="button" class="btn btn-sm btn-ghost github-token-clear mr-auto" @click="clear">清除 Token</button>
        <button type="button" class="btn btn-sm btn-ghost" @click="close">取消</button>
        <button type="submit" class="btn btn-sm btn-primary">保存</button>
      </div>
    </form>
  </GlassDialog>
</template>

<style scoped>
.github-token-pill {
  margin-left: 8px;
  padding: 1px 8px;
  border-radius: 9999px;
  background: var(--fill);
  color: oklch(var(--bc) / 0.7);
  font-size: 11px;
  font-weight: 500;
  line-height: 1.5;
  vertical-align: 1px;
}

.github-token-clear {
  color: var(--danger-ink);
}
</style>
