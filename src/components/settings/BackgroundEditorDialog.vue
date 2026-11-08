<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useToastStore } from '@/stores/toast'
import GlassDialog from '@/components/common/GlassDialog.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import {
  canvasToBlob,
  centeredCrop,
  drawTransformed,
  flipEdit,
  initialEditState,
  moveCrop,
  renderEditCanvas,
  resizeCrop,
  rotateEdit,
  transformedSize,
  type CropHandle,
  type CropLimits,
  type CropRect,
  type EditState,
} from '@/utils/imageEdit'

const PREVIEW_EDGE = 1600
const MIN_CROP_PX = 64
const STAGE_GUTTER = 28
const KEY_STEP = 0.01
const ASPECT_OPTIONS = [
  { value: 'free', label: '自由' },
  { value: 'window', label: '适配窗口' },
] as const
const CORNER_HANDLES: CropHandle[] = ['nw', 'ne', 'sw', 'se']
const ALL_HANDLES: CropHandle[] = ['n', 's', 'e', 'w', ...CORNER_HANDLES]

const { pushToast } = useToastStore()
const visible = ref(false)
const busy = ref(false)
const state = ref<EditState>(initialEditState())
const aspectMode = ref<'free' | 'window'>('free')
const bitmap = shallowRef<ImageBitmap | null>(null)
const stageRef = ref<HTMLElement | null>(null)
const previewRef = ref<HTMLCanvasElement | null>(null)
const frameSize = ref({ width: 0, height: 0 })
let resolveFn: ((value: Blob | null) => void) | null = null
let stageObserver: ResizeObserver | null = null
let drag: { handle: CropHandle | 'move'; startX: number; startY: number; start: CropRect } | null = null
const cropAnnouncement = ref('')

const fullSize = computed<[number, number]>(() =>
  bitmap.value ? transformedSize(bitmap.value.width, bitmap.value.height, state.value.rotation) : [1, 1],
)

const handles = computed(() => (aspectMode.value === 'window' ? CORNER_HANDLES : ALL_HANDLES))

const cropStyle = computed(() => {
  const { x, y, w, h } = state.value.crop
  return { left: `${x * 100}%`, top: `${y * 100}%`, width: `${w * 100}%`, height: `${h * 100}%` }
})

function windowAspect(): number {
  const [width, height] = fullSize.value
  return (window.innerWidth / window.innerHeight) * (height / width)
}

function cropLimits(): CropLimits {
  const [width, height] = fullSize.value
  return {
    minW: Math.min(1, MIN_CROP_PX / width),
    minH: Math.min(1, MIN_CROP_PX / height),
    aspect: aspectMode.value === 'window' ? windowAspect() : undefined,
  }
}

function fitFrame() {
  const stage = stageRef.value
  if (!stage || !bitmap.value) return
  const [width, height] = fullSize.value
  const scale = Math.min((stage.clientWidth - STAGE_GUTTER) / width, (stage.clientHeight - STAGE_GUTTER) / height)
  frameSize.value = { width: Math.max(1, Math.floor(width * scale)), height: Math.max(1, Math.floor(height * scale)) }
}

function drawPreview() {
  const canvas = previewRef.value
  const source = bitmap.value
  if (!canvas || !source) return
  const [width, height] = fullSize.value
  const scale = Math.min(1, PREVIEW_EDGE / Math.max(width, height))
  canvas.width = Math.round(width * scale)
  canvas.height = Math.round(height * scale)
  const context = canvas.getContext('2d')
  if (!context) return
  context.clearRect(0, 0, canvas.width, canvas.height)
  drawTransformed(context, source, state.value, canvas.width, canvas.height)
}

function applyAspect() {
  if (aspectMode.value === 'window') state.value = { ...state.value, crop: centeredCrop(windowAspect()) }
}

watch(
  () => [state.value.rotation, state.value.flipX, state.value.flipY],
  () => {
    fitFrame()
    drawPreview()
  },
)

watch(aspectMode, applyAspect)

function rotate(clockwise: boolean) {
  state.value = rotateEdit(state.value, clockwise)
  applyAspect()
}

function flip(horizontal: boolean) {
  state.value = flipEdit(state.value, horizontal)
}

function reset() {
  state.value = initialEditState()
  applyAspect()
}

function startDrag(event: PointerEvent, handle: CropHandle | 'move') {
  if (event.button !== 0) return
  event.preventDefault()
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  drag = { handle, startX: event.clientX, startY: event.clientY, start: { ...state.value.crop } }
}

function onDrag(event: PointerEvent) {
  if (!drag || !frameSize.value.width) return
  const dx = (event.clientX - drag.startX) / frameSize.value.width
  const dy = (event.clientY - drag.startY) / frameSize.value.height
  const crop = drag.handle === 'move'
    ? moveCrop(drag.start, dx, dy)
    : resizeCrop(drag.start, drag.handle, dx, dy, cropLimits())
  state.value = { ...state.value, crop }
}

function endDrag() {
  drag = null
}

const ARROW_DELTAS: Record<string, [number, number]> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
}

function percent(value: number) {
  return Math.round(value * 100)
}

function describeCrop(crop: CropRect) {
  return `裁剪区域：距左 ${percent(crop.x)}%，距上 ${percent(crop.y)}%，宽 ${percent(crop.w)}%，高 ${percent(crop.h)}%`
}

function onCropKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault()
    void confirm()
    return
  }
  const delta = ARROW_DELTAS[event.key]
  if (!delta) return
  event.preventDefault()
  const dx = delta[0] * KEY_STEP
  const dy = delta[1] * KEY_STEP
  const current = state.value.crop
  const crop = event.shiftKey ? resizeCrop(current, 'se', dx, dy, cropLimits()) : moveCrop(current, dx, dy)
  state.value = { ...state.value, crop }
  cropAnnouncement.value = describeCrop(crop)
}

function finish(value: Blob | null) {
  visible.value = false
  cropAnnouncement.value = ''
  busy.value = false
  drag = null
  stageObserver?.disconnect()
  stageObserver = null
  bitmap.value?.close()
  bitmap.value = null
  resolveFn?.(value)
  resolveFn = null
}

async function edit(file: Blob): Promise<Blob | null> {
  if (resolveFn) finish(null)
  let decoded: ImageBitmap
  try {
    decoded = await createImageBitmap(file)
  } catch {
    throw new Error('无法读取这张图片')
  }
  bitmap.value = decoded
  state.value = initialEditState()
  aspectMode.value = 'free'
  visible.value = true
  await nextTick()
  stageObserver = new ResizeObserver(fitFrame)
  if (stageRef.value) stageObserver.observe(stageRef.value)
  fitFrame()
  drawPreview()
  return new Promise((resolve) => {
    resolveFn = resolve
  })
}

async function confirm() {
  const source = bitmap.value
  if (!source || busy.value) return
  busy.value = true
  try {
    const canvas = renderEditCanvas(source, source.width, source.height, state.value)
    finish(await canvasToBlob(canvas))
  } catch (error) {
    busy.value = false
    pushToast({ message: error instanceof Error ? error.message : '无法导出图片', type: 'error' })
  }
}

function cancel() {
  if (!busy.value) finish(null)
}

onBeforeUnmount(() => {
  if (resolveFn) finish(null)
})

defineExpose({ edit })
</script>

<template>
  <GlassDialog :open="visible" title="编辑背景" size="lg" :busy="busy" :dismiss-on-backdrop="false" @close="cancel">
    <div class="flex flex-col gap-3">
      <div ref="stageRef" class="editor-stage">
        <div
          class="editor-frame"
          :style="{ width: `${frameSize.width}px`, height: `${frameSize.height}px` }"
          @pointermove="onDrag"
          @pointerup="endDrag"
          @pointercancel="endDrag"
        >
          <canvas ref="previewRef" class="editor-preview" />
          <div class="editor-shade" aria-hidden="true">
            <div class="editor-shade-window" :style="cropStyle" />
          </div>
          <div
            class="editor-crop"
            :style="cropStyle"
            tabindex="0"
            role="group"
            aria-roledescription="裁剪框"
            aria-label="裁剪区域"
            aria-describedby="background-editor-keys"
            @pointerdown="startDrag($event, 'move')"
            @keydown="onCropKeydown"
          >
            <span
              v-for="handle in handles"
              :key="handle"
              class="editor-handle"
              :class="`editor-handle-${handle}`"
              aria-hidden="true"
              @pointerdown.stop="startDrag($event, handle)"
            />
          </div>
        </div>
      </div>
      <p class="sr-only" role="status" aria-live="polite">{{ cropAnnouncement }}</p>

      <div class="editor-toolbar">
        <div class="editor-group" role="group" aria-labelledby="background-editor-transform">
          <span id="background-editor-transform" class="editor-group-label">旋转与镜像</span>
          <div class="editor-tools">
            <button type="button" class="editor-tool" title="向左旋转" aria-label="向左旋转" @click="rotate(false)">
              <svg class="editor-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M4 8h7a5 5 0 1 1-4.6 7" /><path d="m7 5-3 3 3 3" /></svg>
            </button>
            <button type="button" class="editor-tool" title="向右旋转" aria-label="向右旋转" @click="rotate(true)">
              <svg class="editor-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M16 8H9a5 5 0 1 0 4.6 7" /><path d="m13 5 3 3-3 3" /></svg>
            </button>
            <button
              type="button"
              class="editor-tool"
              :class="{ 'is-active': state.flipX }"
              title="水平镜像"
              aria-label="水平镜像"
              :aria-pressed="state.flipX"
              @click="flip(true)"
            >
              <svg class="editor-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M10 3v14" /><path d="M7 6 3 14h4V6Z" /><path d="m13 6 4 8h-4V6Z" /></svg>
            </button>
            <button
              type="button"
              class="editor-tool"
              :class="{ 'is-active': state.flipY }"
              title="垂直镜像"
              aria-label="垂直镜像"
              :aria-pressed="state.flipY"
              @click="flip(false)"
            >
              <svg class="editor-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M3 10h14" /><path d="M6 7 14 3v4H6Z" /><path d="m6 13 8 4v-4H6Z" /></svg>
            </button>
          </div>
        </div>
        <span class="editor-separator" aria-hidden="true"></span>
        <div class="editor-group">
          <span id="background-editor-aspect" class="editor-group-label">比例</span>
          <SegmentedControl v-model="aspectMode" :options="ASPECT_OPTIONS" labelledby="background-editor-aspect" />
        </div>
        <span class="editor-separator" aria-hidden="true"></span>
        <button type="button" class="btn btn-sm btn-ghost editor-reset" @click="reset">重置</button>
      </div>

      <div class="editor-footer">
        <p id="background-editor-keys" class="editor-keys">
          裁剪框获得焦点时：<kbd>←↑→↓</kbd> 移动，<kbd>Shift</kbd>+方向键 调整大小，<kbd>Enter</kbd> 使用
        </p>
        <button type="button" class="btn btn-sm btn-ghost" :disabled="busy" @click="cancel">取消</button>
        <button type="button" class="btn btn-sm btn-primary" :disabled="busy" :aria-busy="busy" @click="confirm">
          <span v-if="busy" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          使用这张背景
        </button>
      </div>
    </div>
  </GlassDialog>
</template>

<style scoped>
.editor-stage {
  display: flex;
  height: min(58vh, 460px);
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-tile);
  background: var(--fill);
}

.editor-frame {
  position: relative;
  touch-action: none;
  user-select: none;
}

.editor-preview {
  display: block;
  width: 100%;
  height: 100%;
}

.editor-shade {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
}

.editor-shade-window {
  position: absolute;
  box-shadow: 0 0 0 9999px rgb(0 0 0 / 0.5);
}

.editor-crop {
  position: absolute;
  outline: 1px solid rgb(255 255 255 / 0.9);
  cursor: move;
  background-image:
    linear-gradient(to right, transparent calc(100% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(100% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(100% / 3 + 0.5px), transparent calc(100% / 3 + 0.5px), transparent calc(200% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(200% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(200% / 3 + 0.5px), transparent calc(200% / 3 + 0.5px)),
    linear-gradient(to bottom, transparent calc(100% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(100% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(100% / 3 + 0.5px), transparent calc(100% / 3 + 0.5px), transparent calc(200% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(200% / 3 - 0.5px), rgb(255 255 255 / 0.35) calc(200% / 3 + 0.5px), transparent calc(200% / 3 + 0.5px));
}

.editor-handle {
  position: absolute;
  width: 14px;
  height: 14px;
  margin: -7px 0 0 -7px;
  border-radius: 4px;
  background: #ffffff;
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.35);
}

.editor-handle-nw { top: 0; left: 0; cursor: nwse-resize; }
.editor-handle-ne { top: 0; left: 100%; cursor: nesw-resize; }
.editor-handle-sw { top: 100%; left: 0; cursor: nesw-resize; }
.editor-handle-se { top: 100%; left: 100%; cursor: nwse-resize; }
.editor-handle-n { top: 0; left: 50%; cursor: ns-resize; }
.editor-handle-s { top: 100%; left: 50%; cursor: ns-resize; }
.editor-handle-w { top: 50%; left: 0; cursor: ew-resize; }
.editor-handle-e { top: 50%; left: 100%; cursor: ew-resize; }

.editor-crop:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 3px;
}

.editor-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
}

.editor-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.editor-group-label {
  color: oklch(var(--bc) / 0.55);
  font-size: 12px;
}

.editor-tools {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: 9999px;
  background: var(--fill);
}

.editor-tool {
  display: inline-flex;
  width: 32px;
  height: 26px;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  color: oklch(var(--bc) / 0.7);
}

.editor-tool:hover {
  color: oklch(var(--bc));
}

.editor-tool.is-active {
  background: var(--segment-thumb);
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.12), 0 0 0 0.5px rgb(0 0 0 / 0.04);
  color: oklch(var(--bc));
}

.editor-tool:focus-visible {
  outline: 2px solid oklch(var(--p));
  outline-offset: 2px;
}

.editor-separator {
  width: 0.5px;
  height: 20px;
  background: var(--hairline);
}

.editor-reset {
  color: oklch(var(--bc) / 0.7);
}

.editor-footer {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding-top: 12px;
  border-top: 0.5px solid var(--hairline);
}

.editor-keys {
  min-width: 0;
  flex: 1 1 260px;
  margin: 0;
  color: oklch(var(--bc) / 0.55);
  font-size: 12px;
  line-height: 1.45;
}

.editor-keys kbd {
  padding: 1px 5px;
  border-radius: 5px;
  background: var(--fill);
  font-family: inherit;
  font-size: 11px;
}

.editor-icon {
  width: 17px;
  height: 17px;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 1.6;
}
</style>
