<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useServiceStore } from '@/stores/service'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import { startService, stopService } from '@/bridge/service'
import { formatUptime } from '@/utils/format'

const route = useRoute()
const router = useRouter()
const { serviceStatus, statusText, refresh } = useServiceStore()
const { config } = useConfigStore()
const { pushToast } = useToastStore()

const navItems = [
  { path: '/overview', label: '概览', icon: 'chart' },
  { path: '/proxies', label: '代理', icon: 'proxy' },
  { path: '/connections', label: '连接', icon: 'connection' },
  { path: '/logs', label: '日志', icon: 'log' },
  { path: '/rules', label: '规则', icon: 'rule' },
  { path: '/config', label: '配置', icon: 'config' },
  { path: '/settings', label: '设置', icon: 'settings' },
]

const currentPath = computed(() => route.path)

function navigate(path: string) {
  router.push(path)
}

const uptimeText = computed(() => {
  if (serviceStatus.value.state !== 'running') return ''
  const s = serviceStatus.value.uptimeSeconds
  return typeof s === 'number' ? formatUptime(s) : ''
})

const togglingService = ref(false)
const pillBusy = computed(() =>
  togglingService.value
  || serviceStatus.value.state === 'starting'
  || serviceStatus.value.state === 'stopping',
)

async function toggleService() {
  if (togglingService.value) return
  const state = serviceStatus.value.state
  if (state !== 'running' && state !== 'stopped') return
  togglingService.value = true
  const name = config.value.serviceName.trim()
  try {
    if (serviceStatus.value.state === 'running') {
      await stopService(name)
    } else {
      await startService(name)
    }
  } catch (e: any) {
    pushToast({ message: '服务操作失败: ' + (e?.message || e), type: 'error' }, 6000)
  }
  await refresh()
  togglingService.value = false
}

const statusPillClass = computed(() => {
  switch (serviceStatus.value.state) {
    case 'running': return 'bg-success/15 text-success'
    case 'stopped': return 'bg-error/15 text-error'
    case 'starting':
    case 'stopping': return 'bg-warning/15 text-warning'
    default: return 'bg-base-content/10 text-base-content/60'
  }
})
</script>

<template>
  <div class="flex flex-col w-48 bg-base-200 border-r border-base-300 h-full">
    <nav class="flex-1 py-2 overflow-y-auto">
      <button
        v-for="item in navItems"
        :key="item.path"
        class="w-full flex items-center gap-3 px-4 py-2.5 text-sm transition-colors"
        :class="
          currentPath === item.path
            ? 'bg-primary/10 text-primary font-medium border-r-2 border-primary'
            : 'hover:bg-base-300 text-base-content/70'
        "
        @click="navigate(item.path)"
      >
        <span class="w-5 text-center emoji-font">
          <template v-if="item.icon === 'chart'">📊</template>
          <template v-else-if="item.icon === 'proxy'">🔀</template>
          <template v-else-if="item.icon === 'connection'">🔗</template>
          <template v-else-if="item.icon === 'log'">📝</template>
          <template v-else-if="item.icon === 'rule'">📋</template>
          <template v-else-if="item.icon === 'config'">📄</template>
          <template v-else-if="item.icon === 'settings'">⚙️</template>
        </span>
        <span>{{ item.label }}</span>
      </button>
    </nav>

    <div class="p-3">
      <button
        class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium transition-opacity hover:opacity-75"
        :class="statusPillClass"
        :title="serviceStatus.state === 'running' ? '点击停止服务' : '点击启动服务'"
        :disabled="pillBusy"
        @click="toggleService"
      >
        <span v-if="pillBusy" class="loading loading-spinner w-3 h-3 shrink-0"></span>
        <span v-else class="w-1.5 h-1.5 rounded-full bg-current shrink-0"></span>
        <span v-if="uptimeText" class="tabular-nums">{{ uptimeText }}</span>
        <span v-else>{{ statusText }}</span>
      </button>
    </div>
  </div>
</template>
