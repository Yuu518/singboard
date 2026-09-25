import { onBeforeUnmount } from 'vue'

const SHEEN_SELECTOR = '.glass-float, .glass-popover'
const ANGLE_PROPERTY = '--glass-rim-angle'
const REST_ANGLE = 135

interface Box {
  left: number
  top: number
  width: number
  height: number
}

export function rimAngle(rect: Box, x: number, y: number): number {
  const dx = x - (rect.left + rect.width / 2)
  const dy = y - (rect.top + rect.height / 2)
  const degrees = (Math.atan2(dy, dx) * 180) / Math.PI + 90
  return ((degrees % 360) + 360) % 360
}

export function unwrapAngle(prev: number, next: number): number {
  return prev + ((((next - prev) % 360) + 540) % 360) - 180
}

export function useGlassSheen() {
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')
  const angles = new WeakMap<HTMLElement, number>()
  let pointer: { x: number; y: number } | null = null
  let frame = 0
  let active = false

  function setAngle(element: HTMLElement, target: number) {
    const angle = unwrapAngle(angles.get(element) ?? REST_ANGLE, target)
    angles.set(element, angle)
    element.style.setProperty(ANGLE_PROPERTY, `${angle.toFixed(1)}deg`)
  }

  function paint() {
    frame = 0
    const current = pointer
    if (!current) return
    document.querySelectorAll<HTMLElement>(SHEEN_SELECTOR).forEach((element) => {
      setAngle(element, rimAngle(element.getBoundingClientRect(), current.x, current.y))
    })
  }

  function onPointerMove(event: PointerEvent) {
    pointer = { x: event.clientX, y: event.clientY }
    if (!frame) frame = requestAnimationFrame(paint)
  }

  function cancelFrame() {
    pointer = null
    if (frame) cancelAnimationFrame(frame)
    frame = 0
  }

  function rest() {
    cancelFrame()
    document.querySelectorAll<HTMLElement>(SHEEN_SELECTOR).forEach((element) => setAngle(element, REST_ANGLE))
  }

  function start() {
    if (active) return
    active = true
    window.addEventListener('pointermove', onPointerMove, { passive: true })
    document.documentElement.addEventListener('pointerleave', rest)
  }

  function stop() {
    if (!active) return
    active = false
    window.removeEventListener('pointermove', onPointerMove)
    document.documentElement.removeEventListener('pointerleave', rest)
    cancelFrame()
    document.querySelectorAll<HTMLElement>(SHEEN_SELECTOR).forEach((element) => {
      angles.delete(element)
      element.style.removeProperty(ANGLE_PROPERTY)
    })
  }

  function sync() {
    if (reducedMotion.matches) stop()
    else start()
  }

  sync()
  reducedMotion.addEventListener('change', sync)
  onBeforeUnmount(() => {
    reducedMotion.removeEventListener('change', sync)
    stop()
  })
}
