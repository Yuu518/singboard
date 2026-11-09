<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'
import { useServiceStore } from '@/stores/service'

const props = defineProps<{ subject: string }>()

const { config } = useConfigStore()
const { serviceStatus, clashApiState } = useServiceStore()

const mask = computed(() => {
  if (serviceStatus.value.state !== 'running') {
    return config.value.rootMode
      ? { kind: 'stopped', title: '服务未运行', hint: `请先启动 sing-box 服务以查看${props.subject}` }
      : { kind: 'stopped', title: '未连接到 Clash API', hint: '请检查设置中的 Clash API 地址和端口' }
  }
  if (clashApiState.value === 'auth_failed') {
    return { kind: 'auth', title: 'Clash API 密钥不匹配', hint: '请在设置中检查 Clash API 密钥' }
  }
  if (clashApiState.value !== 'ready') {
    return { kind: 'starting', title: '服务启动中', hint: '正在等待 Clash API 就绪…' }
  }
  return null
})
</script>

<template>
  <div class="clash-gate-content" :inert="mask ? true : undefined" :aria-hidden="mask ? true : undefined">
    <slot />
  </div>
  <Transition name="clash-gate">
    <div v-if="mask" class="clash-gate-mask" :data-gate="mask.kind" role="status">
      <span v-if="mask.kind === 'starting'" class="loading loading-spinner w-10 h-10 text-base-content/60" aria-hidden="true"></span>
      <svg v-else xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1" stroke="currentColor" class="w-14 h-14 text-base-content/50" aria-hidden="true">
        <path stroke-linecap="round" stroke-linejoin="round" d="M5.636 5.636a9 9 0 1012.728 0M12 3v9" />
      </svg>
      <div class="text-center space-y-1">
        <p class="text-lg font-semibold text-base-content">{{ mask.title }}</p>
        <p class="text-sm text-base-content/60">{{ mask.hint }}</p>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.clash-gate-content {
  display: contents;
}

.clash-gate-mask {
  position: absolute;
  inset: 0;
  z-index: 10;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1rem;
  padding: 0 1.5rem;
  background: color-mix(in srgb, var(--page-bg) 55%, transparent);
  -webkit-backdrop-filter: blur(10px) saturate(1.1);
  backdrop-filter: blur(10px) saturate(1.1);
  touch-action: pan-x;
}

.clash-gate-enter-active,
.clash-gate-leave-active {
  transition: opacity 0.2s ease;
}

.clash-gate-enter-from,
.clash-gate-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .clash-gate-enter-active,
  .clash-gate-leave-active {
    transition: none;
  }
}
</style>
