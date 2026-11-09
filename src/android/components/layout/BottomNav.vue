<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { navItems } from './navItems'
import { backdropLensSupported, buildDisplacementMap, buildLensFilter, type MapImage } from '@/composables/useLiquidLens'
import { Spring } from '@/utils/spring'
import { goToPage, pagerIndex, requestReselect } from '@/android/stores/pager'
import { useConfigStore } from '@/stores/config'
import { isClearGlass } from '@/utils/glassTint'

const BAR_HEIGHT = 64
const BAR_PADDING = 4
const PILL_HEIGHT = 56
const PRESSED_SCALE = 78 / 56
const RUBBER_BAND = 4
const TAP_SLOP = 8
const CLICK_GUARD_MS = 700
const BAR_LENS = { height: 24, amount: 24 }
const PILL_LENS = { height: 10, amount: 14, spread: 0.08 }

const router = useRouter()
const { config } = useConfigStore()
const count = navItems.length
const uid = Math.random().toString(36).slice(2, 8)
const barLensId = `nav-bar-lens-${uid}`
const pillLensId = `nav-pill-lens-${uid}`

const bodyRef = ref<HTMLElement | null>(null)
const accentRef = ref<HTMLElement | null>(null)
const rowRef = ref<HTMLElement | null>(null)
const indicatorRef = ref<HTMLElement | null>(null)
const fillRef = ref<HTMLElement | null>(null)
const defsRef = ref<SVGSVGElement | null>(null)
const measured = ref(false)
const lensReady = ref(false)
const lensing = ref(false)

const activeIndex = computed(() => pagerIndex.value)

const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')
const reducedTransparency = window.matchMedia('(prefers-reduced-transparency: reduce)')

const position = new Spring(activeIndex.value, 1, 1000)
const press = new Spring(0, 1, 1000)
const scaleX = new Spring(1, 0.6, 250)
const scaleY = new Spring(1, 0.7, 250)
const velocity = new Spring(0, 0.5, 300, 0.01)
const offset = new Spring(0, 1, 300, 0.5)
const glow = new Spring(0, 0.5, 300)
const springs = [position, press, scaleX, scaleY, velocity, offset, glow]

let width = 0
let tabWidth = 0
let frame = 0
let lastTime = 0
let lastPosition = position.value
let pointerId: number | null = null
let lastX = 0
let travel = 0
let pointerEndedAt = -Infinity
let current = activeIndex.value
let releasing = false
const lensSupported = backdropLensSupported()
let pillScales: Array<{ element: Element; base: number }> = []
let resizeObserver: ResizeObserver | null = null

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

function easeOut(t: number) {
  return 1 - (1 - t) * (1 - t)
}

function pressTo(pressed: boolean) {
  const scale = pressed && !reducedMotion.matches ? PRESSED_SCALE : 1
  press.target = pressed ? 1 : 0
  scaleX.target = scale
  scaleY.target = scale
}

function schedule() {
  if (!frame) frame = requestAnimationFrame(tick)
}

function tick(now: number) {
  frame = 0
  const dt = lastTime ? Math.min((now - lastTime) / 1000, 1 / 20) : 1 / 60
  lastTime = now
  const still = reducedMotion.matches
  if (still) {
    for (const spring of springs) spring.snap()
  }
  position.step(dt)
  velocity.target = !still && (pointerId !== null || releasing)
    ? (position.value - lastPosition) / dt / Math.max(count - 1, 1)
    : 0
  lastPosition = position.value
  for (const spring of springs) {
    if (spring !== position) spring.step(dt)
  }
  if (releasing && Math.abs(position.value - position.target) < (count - 1) * 0.025) {
    releasing = false
    pressTo(false)
    if (still) press.snap()
  }
  render()
  if (pointerId !== null || releasing || springs.some((spring) => !spring.settled)) {
    schedule()
  } else {
    lastTime = 0
  }
}

function render() {
  const body = bodyRef.value
  const accent = accentRef.value
  const indicator = indicatorRef.value
  if (!body || !accent || !indicator || !width) return
  const still = reducedMotion.matches
  const p = clamp(press.value, 0, 1)
  const lift = still ? 0 : p
  const fraction = clamp(offset.value / width, -1, 1)
  const panel = still ? 0 : RUBBER_BAND * Math.sign(fraction) * easeOut(Math.abs(fraction))
  const v = velocity.value / 10
  const sx = scaleX.value / (1 - clamp(v * 0.75, -0.2, 0.2))
  const sy = scaleY.value * (1 - clamp(v * 0.25, -0.2, 0.2))
  const left = BAR_PADDING + position.value * tabWidth
  const center = left + tabWidth / 2

  body.style.setProperty('--panel', `${panel.toFixed(2)}px`)
  body.style.setProperty('--bar-scale', (1 + (16 / width) * lift).toFixed(4))
  body.style.setProperty('--press', p.toFixed(3))
  body.style.setProperty('--tab-scale', (1 + 0.2 * lift).toFixed(3))
  body.style.setProperty('--glow', clamp(glow.value, 0, 1).toFixed(3))
  body.style.setProperty('--glow-x', `${center.toFixed(1)}px`)
  const transform = `translate3d(${left.toFixed(2)}px, 0, 0) scale(${sx.toFixed(4)}, ${sy.toFixed(4)})`
  indicator.style.transform = transform
  if (fillRef.value) fillRef.value.style.transform = transform

  const w = tabWidth * sx
  const h = PILL_HEIGHT * sy
  const top = (BAR_HEIGHT - h) / 2
  accent.style.clipPath = `inset(${top.toFixed(2)}px ${(width - center - w / 2).toFixed(2)}px ${top.toFixed(2)}px ${(center - w / 2).toFixed(2)}px round ${(h / 2).toFixed(2)}px)`
  const row = rowRef.value
  if (row) row.style.clipPath = lensReady.value ? '' : outsidePill(center - w / 2, top, w, h)

  const lensOn = lensReady.value && p > 0.01
  if (lensing.value !== lensOn) lensing.value = lensOn
  for (const { element, base } of pillScales) element.setAttribute('scale', (base * p).toFixed(2))
}

function outsidePill(x: number, y: number, w: number, h: number) {
  const r = Math.min(w, h) / 2
  const x1 = x + w
  const y1 = y + h
  const n = (value: number) => value.toFixed(2)
  const pill = `M${n(x + r)} ${n(y)}H${n(x1 - r)}A${n(r)} ${n(r)} 0 0 1 ${n(x1)} ${n(y + r)}V${n(y1 - r)}A${n(r)} ${n(r)} 0 0 1 ${n(x1 - r)} ${n(y1)}H${n(x + r)}A${n(r)} ${n(r)} 0 0 1 ${n(x)} ${n(y1 - r)}V${n(y + r)}A${n(r)} ${n(r)} 0 0 1 ${n(x + r)} ${n(y)}Z`
  return `path(evenodd, "M0 0H${n(width)}V${BAR_HEIGHT}H0Z${pill}")`
}

function mapUrl(w: number, h: number, height: number, amount: number, depth: boolean) {
  const canvas = document.createElement('canvas')
  canvas.width = w
  canvas.height = h
  const context = canvas.getContext('2d')
  if (!context) return ''
  context.putImageData(new ImageData(buildDisplacementMap(w, h, h / 2, height, amount, undefined, depth), w, h), 0, 0)
  return canvas.toDataURL()
}

function lensImage(width: number, height: number, href: string): MapImage {
  return { x: 0, y: 0, width, height, href }
}

function rebuildLens() {
  const defs = defsRef.value
  if (!defs) return
  defs.replaceChildren()
  pillScales = []
  lensReady.value = false
  if (!lensSupported || !isClearGlass(config.value.glassTint) || reducedTransparency.matches || width < 2 || tabWidth < 2) return
  const barWidth = Math.round(width)
  const barMap = mapUrl(barWidth, BAR_HEIGHT, BAR_LENS.height, BAR_LENS.amount, false)
  const pillWidth = Math.round(tabWidth)
  const pillMap = mapUrl(pillWidth, PILL_HEIGHT, PILL_LENS.height, PILL_LENS.amount, true)
  if (!barMap || !pillMap) return
  defs.append(buildLensFilter(barWidth, BAR_HEIGHT, [lensImage(barWidth, BAR_HEIGHT, barMap)], barLensId, { chroma: false, amount: BAR_LENS.amount }))
  const pill = buildLensFilter(pillWidth, PILL_HEIGHT, [lensImage(pillWidth, PILL_HEIGHT, pillMap)], pillLensId, {
    amount: PILL_LENS.amount,
    spread: PILL_LENS.spread,
  })
  defs.append(pill)
  pillScales = [...pill.querySelectorAll('feDisplacementMap')].map((element) => ({
    element,
    base: Number(element.getAttribute('scale')) || 0,
  }))
  lensReady.value = true
}

function measure() {
  const body = bodyRef.value
  if (!body) return
  const next = body.offsetWidth
  if (next === width) return
  width = next
  tabWidth = Math.max(0, (width - BAR_PADDING * 2) / count)
  body.style.setProperty('--tab-w', `${tabWidth}px`)
  measured.value = width > 0
  rebuildLens()
  render()
}

function indexAt(clientX: number) {
  const body = bodyRef.value
  if (!body || !tabWidth) return current
  const x = clientX - body.getBoundingClientRect().left
  return clamp(Math.floor((x - BAR_PADDING) / tabWidth), 0, count - 1)
}

function navigate(index: number) {
  if (index === current) return
  current = index
  goToPage(router, navItems[index].path)
}

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0 || pointerId !== null) return
  pointerId = event.pointerId
  bodyRef.value?.setPointerCapture(event.pointerId)
  lastX = event.clientX
  travel = 0
  releasing = false
  position.target = indexAt(event.clientX)
  pressTo(true)
  glow.target = 1
  schedule()
}

function onPointerMove(event: PointerEvent) {
  if (event.pointerId !== pointerId) return
  const dx = event.clientX - lastX
  lastX = event.clientX
  travel += Math.abs(dx)
  if (!dx || !tabWidth) return
  position.target = clamp(position.target + dx / tabWidth, 0, count - 1)
  offset.snap(offset.value + dx)
  schedule()
}

function finish(event: PointerEvent, commit: boolean) {
  if (event.pointerId !== pointerId) return
  pointerId = null
  pointerEndedAt = performance.now()
  const target = commit ? clamp(Math.round(position.target), 0, count - 1) : current
  position.target = target
  offset.target = 0
  glow.target = 0
  releasing = true
  if (commit && target === current && travel < TAP_SLOP) requestReselect()
  else if (commit) navigate(target)
  schedule()
}

function onItemClick(index: number) {
  if (performance.now() - pointerEndedAt < CLICK_GUARD_MS) return
  if (index === current) {
    requestReselect()
    return
  }
  navigate(index)
  animateTo(index, true)
}

function animateTo(index: number, pressed: boolean) {
  if (pressed) pressTo(true)
  position.target = index
  releasing = true
  schedule()
}

watch(activeIndex, (index) => {
  if (index === current) return
  current = index
  animateTo(index, false)
})

function onTransparencyChange() {
  rebuildLens()
  render()
}

watch(() => isClearGlass(config.value.glassTint), onTransparencyChange)

onMounted(() => {
  measure()
  resizeObserver = new ResizeObserver(measure)
  if (bodyRef.value) resizeObserver.observe(bodyRef.value)
  reducedTransparency.addEventListener('change', onTransparencyChange)
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  reducedTransparency.removeEventListener('change', onTransparencyChange)
  if (frame) cancelAnimationFrame(frame)
})
</script>

<template>
  <nav class="liquid-nav" aria-label="主导航">
    <svg ref="defsRef" class="liquid-nav-defs" width="0" height="0" aria-hidden="true" />
    <div
      ref="bodyRef"
      class="liquid-nav-body"
      :class="{ 'is-measured': measured }"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="finish($event, true)"
      @pointercancel="finish($event, false)"
      @lostpointercapture.self="finish($event, false)"
    >
      <div class="liquid-nav-glass" :style="lensReady ? { filter: `url(#${barLensId})` } : undefined" />
      <div class="liquid-nav-rim" aria-hidden="true" />
      <div ref="fillRef" class="liquid-nav-pill-fill" aria-hidden="true" />
      <div ref="rowRef" class="liquid-nav-row">
        <button
          v-for="(item, index) in navItems"
          :key="item.path"
          type="button"
          class="liquid-nav-item"
          :aria-current="activeIndex === index ? 'page' : undefined"
          @click="onItemClick(index)"
        >
          <svg class="liquid-nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path v-for="(d, i) in item.icon" :key="i" :d="d" />
          </svg>
          <span class="liquid-nav-label">{{ item.label }}</span>
        </button>
      </div>
      <div ref="accentRef" class="liquid-nav-row is-accent" aria-hidden="true">
        <span v-for="item in navItems" :key="item.path" class="liquid-nav-item">
          <svg class="liquid-nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
            <path v-for="(d, i) in item.icon" :key="i" :d="d" />
          </svg>
          <span class="liquid-nav-label">{{ item.label }}</span>
        </span>
      </div>
      <div ref="indicatorRef" class="liquid-nav-indicator" aria-hidden="true">
        <div v-if="lensing" class="liquid-nav-pill-lens" :style="{ filter: `url(#${pillLensId})` }" />
        <div class="liquid-nav-pill-surface" />
      </div>
    </div>
  </nav>
</template>

<style scoped>
.liquid-nav {
  --nav-shadow: 0.1;
  --nav-tone: contrast(calc(1 - 0.6 * var(--glass-c, 0))) brightness(calc(1 / (1 - 0.3 * var(--glass-c, 0))));
  --nav-pill-base: calc(0.7 * var(--glass-c, 0));
  position: fixed;
  z-index: 40;
  right: calc(var(--safe-right) + 24px);
  bottom: max(calc(var(--safe-bottom) + 8px), 16px);
  left: calc(var(--safe-left) + 24px);
  max-width: 560px;
  height: 64px;
  margin-inline: auto;
}

:global([data-theme='dark'] .liquid-nav) {
  --nav-shadow: 0.2;
  --nav-tone: contrast(calc(1 - 0.5 * var(--glass-c, 0))) brightness(calc(1 - 0.4 * var(--glass-c, 0)));
}

@media (max-width: 359px) {
  .liquid-nav {
    right: calc(var(--safe-right) + 12px);
    left: calc(var(--safe-left) + 12px);
  }
}

.liquid-nav-defs {
  position: absolute;
  width: 0;
  height: 0;
  pointer-events: none;
}

.liquid-nav-body {
  position: relative;
  height: 100%;
  border-radius: 9999px;
  touch-action: none;
  user-select: none;
  -webkit-user-select: none;
  -webkit-tap-highlight-color: transparent;
  transform: translate3d(var(--panel, 0px), 0, 0) scale(var(--bar-scale, 1));
}

.liquid-nav-glass,
.liquid-nav-rim,
.liquid-nav-pill-lens,
.liquid-nav-pill-surface {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
}

.liquid-nav-glass {
  background: var(--glass-grain), rgb(var(--glass-rgb) / var(--glass-float-alpha));
  clip-path: inset(0 round 9999px);
  -webkit-backdrop-filter: saturate(var(--glass-saturate)) blur(var(--glass-blur)) var(--nav-tone,);
  backdrop-filter: saturate(var(--glass-saturate)) blur(var(--glass-blur)) var(--nav-tone,);
}

.liquid-nav-rim {
  box-shadow:
    0 4px 20px rgb(0 0 0 / var(--nav-shadow)),
    var(--glass-highlight),
    var(--glass-edge);
}

.liquid-nav-rim::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background:
    radial-gradient(
      circle 77px at var(--glow-x, 50%) 50%,
      rgb(255 255 255 / calc(0.12 * var(--glow, 0))) 0%,
      rgb(255 255 255 / calc(0.12 * var(--glow, 0))) 50%,
      transparent 100%
    ),
    rgb(255 255 255 / calc(0.06 * var(--glow, 0)));
}

.liquid-nav-row {
  position: absolute;
  inset: 0;
  display: flex;
  padding: 4px;
  color: oklch(var(--bc) / calc(0.72 + 0.28 * var(--glass-c, 0)));
  font-weight: 500;
}

.liquid-nav-row.is-accent {
  color: var(--selection-ink);
  pointer-events: none;
  clip-path: inset(0 100% 0 0);
}

:global([data-glass='clear'] .liquid-nav-icon) {
  stroke-width: 2;
}

:global([data-glass='clear'] .liquid-nav-label) {
  font-weight: 600;
}

.liquid-nav-item {
  display: flex;
  min-width: 0;
  flex: 1 1 0;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
  border-radius: 9999px;
}

.liquid-nav-item:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: -2px;
}

.liquid-nav-item[aria-current='page']:focus-visible {
  outline: none;
}

.liquid-nav-body:has(.liquid-nav-item[aria-current='page']:focus-visible) .liquid-nav-indicator {
  outline: 2px solid oklch(var(--p));
  outline-offset: 1px;
}

.liquid-nav-row.is-accent .liquid-nav-item {
  transform: scale(var(--tab-scale, 1));
}

.liquid-nav-icon {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
}

.liquid-nav-label {
  max-width: 100%;
  overflow: hidden;
  font-size: 12px;
  line-height: 15px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.liquid-nav-row.is-accent .liquid-nav-label {
  font-weight: 600;
}

.liquid-nav-indicator,
.liquid-nav-pill-fill {
  position: absolute;
  top: 4px;
  left: 0;
  width: var(--tab-w, 0px);
  height: 56px;
  border-radius: 9999px;
  opacity: 0;
  pointer-events: none;
  transform-origin: center;
  will-change: transform;
}

.liquid-nav-pill-fill {
  background:
    linear-gradient(oklch(var(--p) / calc(0.15 * (1 - var(--press, 0)))), oklch(var(--p) / calc(0.15 * (1 - var(--press, 0))))),
    rgb(var(--glass-rgb) / calc(var(--nav-pill-base, 0) * (1 - var(--press, 0))));
}

.liquid-nav-pill-lens {
  clip-path: inset(0 round 9999px);
  -webkit-backdrop-filter: saturate(1.1);
  backdrop-filter: saturate(1.1);
}

.liquid-nav-pill-surface {
  background: rgb(0 0 0 / calc(0.03 * var(--press, 0)));
  box-shadow:
    inset 0 calc(8px * var(--press, 0)) calc(8px * var(--press, 0)) rgb(0 0 0 / calc(0.15 * var(--press, 0))),
    inset 0 0 0 1px rgb(255 255 255 / calc(0.45 * var(--press, 0)));
}

.liquid-nav-body.is-measured :is(.liquid-nav-indicator, .liquid-nav-pill-fill) {
  opacity: 1;
}

@media (prefers-reduced-transparency: reduce) {
  .liquid-nav-glass {
    background: rgb(var(--glass-rgb));
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }
}
</style>
