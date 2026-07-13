<script setup lang="ts">
import { computed, ref, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useServiceStore } from '@/stores/service'
import { useConfigStore } from '@/stores/config'
import { useToastStore } from '@/stores/toast'
import { getSingboxVersion } from '@/bridge/config'
import { startService, stopService } from '@/bridge/service'
import { normalizeVersionText, formatUptime } from '@/utils/format'

const route = useRoute()
const router = useRouter()
const { serviceStatus, statusText, refresh } = useServiceStore()
const { config } = useConfigStore()
const { pushToast } = useToastStore()
const singboxVersion = ref('')
const versionWrapEl = ref<HTMLElement | null>(null)
const versionTrackEl = ref<HTMLElement | null>(null)
const shouldScrollVersion = ref(false)
const versionOverflowPx = ref(0)
let resizeOb: ResizeObserver | null = null

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

async function refreshVersion() {
  if (serviceStatus.value.state !== 'running') {
    singboxVersion.value = ''
    return
  }
  const singboxPath = config.value.singboxPath?.trim()
  if (!singboxPath) {
    singboxVersion.value = ''
    return
  }
  try {
    const raw = await getSingboxVersion(singboxPath)
    singboxVersion.value = normalizeVersionText(raw)
  } catch {
    singboxVersion.value = ''
  }
}

function measureOverflow() {
  const wrap = versionWrapEl.value
  const track = versionTrackEl.value
  if (!wrap || !track || !singboxVersion.value) {
    shouldScrollVersion.value = false
    versionOverflowPx.value = 0
    return
  }
  const overflow = track.offsetWidth - wrap.clientWidth
  shouldScrollVersion.value = overflow > 2
  versionOverflowPx.value = Math.max(0, Math.ceil(overflow))
}

watch(
  () => [serviceStatus.value.state, config.value.singboxPath],
  () => {
    void refreshVersion()
  },
  { immediate: true },
)

watch(versionWrapEl, (el, oldEl) => {
  resizeOb?.disconnect()
  if (el) {
    resizeOb = new ResizeObserver(() => measureOverflow())
    resizeOb.observe(el)
  }
})

watch(singboxVersion, () => {
  requestAnimationFrame(() => measureOverflow())
})

onBeforeUnmount(() => {
  resizeOb?.disconnect()
  resizeOb = null
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

    <div class="p-3 border-t border-base-300">
      <div class="flex items-center gap-2 text-xs text-base-content/60 whitespace-nowrap overflow-hidden">
        <span
          class="inline-flex items-center gap-1.5 px-2 py-1 rounded-full font-medium shrink-0"
          :class="statusPillClass"
        >
          <span v-if="pillBusy" class="loading loading-spinner w-3 h-3 shrink-0"></span>
          <button
            v-else-if="serviceStatus.state === 'running' || serviceStatus.state === 'stopped'"
            class="inline-flex shrink-0 hover:opacity-60 transition-opacity"
            :title="serviceStatus.state === 'running' ? '停止服务' : '启动服务'"
            @click="toggleService"
          >
            <svg v-if="serviceStatus.state === 'running'" class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="6" width="12" height="12" rx="2" />
            </svg>
            <svg v-else class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5v14l11-7z" />
            </svg>
          </button>
          <span v-if="uptimeText" class="tabular-nums">{{ uptimeText }}</span>
          <span v-else-if="serviceStatus.state !== 'running'">{{ statusText }}</span>
        </span>
        <span
          v-if="serviceStatus.state === 'running' && singboxVersion"
          ref="versionWrapEl"
          class="version-wrap text-base-content/45"
          :class="{ scrolling: shouldScrollVersion }"
          :style="{ '--overflow-distance': versionOverflowPx }"
          :title="singboxVersion"
        >
          <span ref="versionTrackEl" class="version-track">
            <span class="version-item">{{ singboxVersion }}</span>
          </span>
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.version-wrap {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}

.version-track {
  display: inline-flex;
  align-items: center;
}

.version-wrap.scrolling .version-track {
  animation: version-pingpong 4.5s ease-in-out infinite alternate;
  will-change: transform;
}

.version-item {
  flex: 0 0 auto;
}

@keyframes version-pingpong {
  0% {
    transform: translateX(0);
  }
  100% {
    transform: translateX(calc(-1px * var(--overflow-distance, 0)));
  }
}
</style>
