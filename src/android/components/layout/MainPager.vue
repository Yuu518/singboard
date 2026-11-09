<script setup lang="ts">
import { computed, defineAsyncComponent, markRaw, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { navItems } from './navItems'
import PagerPage from './PagerPage.vue'
import { pagerIndex } from '@/android/stores/pager'
import { useConfigStore } from '@/stores/config'

const emit = defineEmits<{
  scrolled: [value: boolean]
}>()

const route = useRoute()
const router = useRouter()
const ClashApiGate = defineAsyncComponent(() => import('@/android/components/ClashApiGate.vue'))
const pages = navItems.map((item) => ({
  path: item.path,
  fill: item.fill === true,
  gate: item.gate,
  view: markRaw(defineAsyncComponent(item.load)),
}))
const count = pages.length
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')
const supportsScrollEnd = 'onscrollend' in window

const FADE_MS = 150

const { config } = useConfigStore()
const swipe = computed(() => config.value.pageSwipe)
const fadingIndex = ref(-1)
let fadeTimer = 0

const viewportRef = ref<HTMLElement | null>(null)
const settled = ref(0)
const loaded = reactive(new Set<number>())
const scrollTops = new Array<number>(count).fill(0)

let ready = false
let programmatic: number | null = null
let settleTimer = 0
let idleHandle = 0
let resizeObserver: ResizeObserver | null = null

function clamp(value: number) {
  return Math.min(count - 1, Math.max(0, value))
}

function indexOf(path: string) {
  return pages.findIndex((page) => page.path === path)
}

function pageWidth() {
  return viewportRef.value?.clientWidth || 1
}

function nearest() {
  return clamp(Math.round((viewportRef.value?.scrollLeft ?? 0) / pageWidth()))
}

function emitScrolled() {
  emit('scrolled', scrollTops[settled.value] > 0)
}

function settle(index: number) {
  programmatic = null
  settled.value = index
  pagerIndex.value = index
  loaded.add(index)
  emitScrolled()
  const path = pages[index].path
  if (route.path !== path) void router.push(path)
}

function jumpTo(index: number) {
  const viewport = viewportRef.value
  if (!viewport) return
  viewport.scrollTo({ left: index * pageWidth(), behavior: 'instant' })
  settle(index)
}

function slideTo(index: number) {
  const viewport = viewportRef.value
  if (!viewport) return
  loaded.add(index)
  pagerIndex.value = index
  const left = index * pageWidth()
  if (Math.abs(viewport.scrollLeft - left) < 1) {
    settle(index)
    return
  }
  if (reducedMotion.matches) {
    jumpTo(index)
    return
  }
  if (!swipe.value) {
    jumpTo(index)
    fadeIn(index)
    return
  }
  programmatic = index
  viewport.scrollTo({ left, behavior: 'smooth' })
}

function onScroll() {
  if (programmatic === null) {
    const index = nearest()
    if (index !== pagerIndex.value) {
      pagerIndex.value = index
      loaded.add(index)
    }
  }
  if (!supportsScrollEnd) {
    window.clearTimeout(settleTimer)
    settleTimer = window.setTimeout(onScrollEnd, 150)
  }
}

function onScrollEnd() {
  settle(programmatic ?? nearest())
}

function fadeIn(index: number) {
  window.clearTimeout(fadeTimer)
  fadingIndex.value = index
  fadeTimer = window.setTimeout(() => {
    fadingIndex.value = -1
  }, FADE_MS)
}

function onTouchStart() {
  if (!swipe.value) return
  programmatic = null
  for (const index of [settled.value - 1, settled.value + 1]) {
    if (index >= 0 && index < count) loaded.add(index)
  }
}

function onPageScroll(index: number, top: number) {
  scrollTops[index] = top
  if (index === settled.value) emitScrolled()
}

const supportsIdle = typeof window.requestIdleCallback === 'function'

function scheduleIdle(callback: () => void) {
  idleHandle = supportsIdle
    ? window.requestIdleCallback(callback, { timeout: 1500 })
    : window.setTimeout(callback, 200)
}

function cancelIdle() {
  if (supportsIdle) window.cancelIdleCallback(idleHandle)
  else window.clearTimeout(idleHandle)
}

function loadRemaining() {
  let next = -1
  for (let index = 0; index < count; index++) {
    if (loaded.has(index)) continue
    if (next < 0 || Math.abs(index - settled.value) < Math.abs(next - settled.value)) next = index
  }
  if (next < 0) return
  loaded.add(next)
  scheduleIdle(loadRemaining)
}

watch(
  () => route.path,
  (path) => {
    const index = indexOf(path)
    if (!ready || index < 0) return
    if (index === settled.value && programmatic === null && nearest() === index) return
    slideTo(index)
  },
)

onMounted(() => {
  void router.isReady().then(async () => {
    const index = Math.max(0, indexOf(route.path))
    loaded.add(index)
    settled.value = index
    pagerIndex.value = index
    await nextTick()
    jumpTo(index)
    ready = true
    scheduleIdle(loadRemaining)
  })
  resizeObserver = new ResizeObserver(() => {
    if (ready && programmatic === null) {
      viewportRef.value?.scrollTo({ left: settled.value * pageWidth(), behavior: 'instant' })
    }
  })
  if (viewportRef.value) resizeObserver.observe(viewportRef.value)
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  window.clearTimeout(settleTimer)
  window.clearTimeout(fadeTimer)
  cancelIdle()
})
</script>

<template>
  <main
    ref="viewportRef"
    class="app-main"
    :class="{ 'is-locked': !swipe }"
    @scroll.passive="onScroll"
    @scrollend="onScrollEnd"
    @touchstart.passive="onTouchStart"
  >
    <PagerPage
      v-for="(page, index) in pages"
      :key="page.path"
      :active="index === settled"
      :class="{ 'is-entering': index === fadingIndex }"
      :fill="page.fill"
      @scroll="onPageScroll(index, $event)"
    >
      <template v-if="loaded.has(index)">
        <ClashApiGate v-if="page.gate" :subject="page.gate">
          <component :is="page.view" />
        </ClashApiGate>
        <component :is="page.view" v-else />
      </template>
    </PagerPage>
  </main>
</template>

<style scoped>
.app-main {
  --nav-clearance: calc(max(var(--safe-bottom) + 8px, 16px) + 5rem);
  display: flex;
  height: 100%;
  overflow-x: auto;
  overflow-y: hidden;
  overscroll-behavior-x: contain;
  scroll-snap-type: x mandatory;
  scrollbar-width: none;
}

.app-main::-webkit-scrollbar {
  display: none;
}

.app-main.is-locked {
  overflow-x: hidden;
}

.is-entering {
  animation: pager-fade-in 150ms ease-out;
}

@keyframes pager-fade-in {
  from {
    opacity: 0;
  }

  to {
    opacity: 1;
  }
}

:global(.keyboard-open) .app-main {
  --nav-clearance: calc(var(--keyboard-inset, 0px) + 1rem);
}
</style>
