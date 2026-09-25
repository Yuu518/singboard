export type Rotation = 0 | 90 | 180 | 270
export type CropHandle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw'

export interface CropRect {
  x: number
  y: number
  w: number
  h: number
}

export interface EditState {
  rotation: Rotation
  flipX: boolean
  flipY: boolean
  crop: CropRect
}

export interface CropLimits {
  minW: number
  minH: number
  aspect?: number
}

export const FULL_CROP: CropRect = { x: 0, y: 0, w: 1, h: 1 }

export function initialEditState(): EditState {
  return { rotation: 0, flipX: false, flipY: false, crop: { ...FULL_CROP } }
}

export function transformedSize(width: number, height: number, rotation: Rotation): [number, number] {
  return rotation % 180 === 0 ? [width, height] : [height, width]
}

export function rotateCrop(rect: CropRect, clockwise: boolean): CropRect {
  return clockwise
    ? { x: 1 - rect.y - rect.h, y: rect.x, w: rect.h, h: rect.w }
    : { x: rect.y, y: 1 - rect.x - rect.w, w: rect.h, h: rect.w }
}

export function flipCrop(rect: CropRect, horizontal: boolean): CropRect {
  return horizontal
    ? { ...rect, x: 1 - rect.x - rect.w }
    : { ...rect, y: 1 - rect.y - rect.h }
}

export function rotateEdit(state: EditState, clockwise: boolean): EditState {
  return {
    rotation: (((state.rotation + (clockwise ? 90 : -90)) % 360) + 360) % 360 as Rotation,
    flipX: state.flipY,
    flipY: state.flipX,
    crop: rotateCrop(state.crop, clockwise),
  }
}

export function flipEdit(state: EditState, horizontal: boolean): EditState {
  return {
    ...state,
    flipX: horizontal ? !state.flipX : state.flipX,
    flipY: horizontal ? state.flipY : !state.flipY,
    crop: flipCrop(state.crop, horizontal),
  }
}

export function centeredCrop(aspect: number): CropRect {
  const w = Math.min(1, aspect)
  const h = Math.min(1, w / aspect)
  const width = h * aspect
  return { x: (1 - width) / 2, y: (1 - h) / 2, w: width, h }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

export function moveCrop(rect: CropRect, dx: number, dy: number): CropRect {
  return { ...rect, x: clamp(rect.x + dx, 0, 1 - rect.w), y: clamp(rect.y + dy, 0, 1 - rect.h) }
}

export function resizeCrop(rect: CropRect, handle: CropHandle, dx: number, dy: number, limits: CropLimits): CropRect {
  const left = rect.x
  const top = rect.y
  const right = rect.x + rect.w
  const bottom = rect.y + rect.h
  const east = handle.includes('e')
  const west = handle.includes('w')
  const north = handle.includes('n')
  const south = handle.includes('s')

  if (limits.aspect && (east || west) && (north || south)) {
    const aspect = limits.aspect
    const wantW = east ? right + dx - left : right - (left + dx)
    const wantH = south ? bottom + dy - top : bottom - (top + dy)
    const maxW = Math.min(east ? 1 - left : right, (south ? 1 - top : bottom) * aspect)
    const minW = Math.min(maxW, Math.max(limits.minW, limits.minH * aspect))
    const w = clamp(Math.max(wantW, wantH * aspect), minW, maxW)
    const h = w / aspect
    return { x: east ? left : right - w, y: south ? top : bottom - h, w, h }
  }

  const nextLeft = west ? clamp(left + dx, 0, right - limits.minW) : left
  const nextRight = east ? clamp(right + dx, left + limits.minW, 1) : right
  const nextTop = north ? clamp(top + dy, 0, bottom - limits.minH) : top
  const nextBottom = south ? clamp(bottom + dy, top + limits.minH, 1) : bottom
  return { x: nextLeft, y: nextTop, w: nextRight - nextLeft, h: nextBottom - nextTop }
}

export function drawTransformed(
  context: CanvasRenderingContext2D,
  source: CanvasImageSource,
  state: Pick<EditState, 'rotation' | 'flipX' | 'flipY'>,
  width: number,
  height: number,
) {
  const [drawW, drawH] = transformedSize(width, height, state.rotation)
  context.save()
  context.translate(width / 2, height / 2)
  context.scale(state.flipX ? -1 : 1, state.flipY ? -1 : 1)
  context.rotate((state.rotation * Math.PI) / 180)
  context.drawImage(source, -drawW / 2, -drawH / 2, drawW, drawH)
  context.restore()
}

export function renderEditCanvas(
  source: CanvasImageSource,
  sourceWidth: number,
  sourceHeight: number,
  state: EditState,
  maxEdge = 4096,
): HTMLCanvasElement {
  const [fullW, fullH] = transformedSize(sourceWidth, sourceHeight, state.rotation)
  const cropW = state.crop.w * fullW
  const cropH = state.crop.h * fullH
  const scale = Math.min(1, maxEdge / Math.max(cropW, cropH))
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(cropW * scale))
  canvas.height = Math.max(1, Math.round(cropH * scale))
  const context = canvas.getContext('2d')
  if (!context) throw new Error('无法处理这张图片')
  context.imageSmoothingQuality = 'high'
  context.translate(-state.crop.x * fullW * scale, -state.crop.y * fullH * scale)
  drawTransformed(context, source, state, fullW * scale, fullH * scale)
  return canvas
}

export function canvasToBlob(canvas: HTMLCanvasElement, type = 'image/webp', quality = 0.92): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('无法导出图片'))), type, quality)
  })
}
