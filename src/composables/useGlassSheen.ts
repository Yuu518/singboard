import { onBeforeUnmount } from 'vue'

const SHEEN_SELECTOR = '.glass-float, .glass-popover, .surface-card, .settings-card'
export const SHEEN_REACH = 160
const LEAVE_MARGIN = 4

interface Box {
  left: number
  top: number
  width: number
  height: number
}

export function isOnScreen(rect: Box, viewportWidth: number, viewportHeight: number): boolean {
  return (
    rect.width > 0
    && rect.height > 0
    && rect.left + rect.width > 0
    && rect.top + rect.height > 0
    && rect.left < viewportWidth
    && rect.top < viewportHeight
  )
}

export function sheenGlow(rect: Box, x: number, y: number): number {
  const outsideX = Math.max(0, rect.left - x, x - (rect.left + rect.width))
  const outsideY = Math.max(0, rect.top - y, y - (rect.top + rect.height))
  const t = Math.min(1, Math.max(0, 1 - Math.hypot(outsideX, outsideY) / SHEEN_REACH))
  return t * t * (3 - 2 * t)
}

export function isInsideWindow(x: number, y: number, width: number, height: number): boolean {
  return x > LEAVE_MARGIN && y > LEAVE_MARGIN && x < width - LEAVE_MARGIN && y < height - LEAVE_MARGIN
}

export function useGlassSheen() {
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')
  const glows = new WeakMap<HTMLElement, number>()
  let pointer: { x: number; y: number } | null = null
  let frame = 0
  let active = false

  function setGlow(element: HTMLElement, glow: number) {
    if ((glows.get(element) ?? 0) === glow) return
    glows.set(element, glow)
    element.style.setProperty('--glass-rim-glow', glow.toFixed(3))
  }

  function paint() {
    frame = 0
    const current = pointer
    if (!current) return
    const { innerWidth, innerHeight } = window
    document.querySelectorAll<HTMLElement>(SHEEN_SELECTOR).forEach((element) => {
      const rect = element.getBoundingClientRect()
      const glow = isOnScreen(rect, innerWidth, innerHeight) ? sheenGlow(rect, current.x, current.y) : 0
      if (glow > 0) {
        element.style.setProperty('--glass-rim-x', `${(current.x - rect.left).toFixed(1)}px`)
        element.style.setProperty('--glass-rim-y', `${(current.y - rect.top).toFixed(1)}px`)
      }
      setGlow(element, glow)
    })
  }

  function schedule() {
    if (pointer && !frame) frame = requestAnimationFrame(paint)
  }

  function onPointerMove(event: PointerEvent) {
    pointer = { x: event.clientX, y: event.clientY }
    schedule()
  }

  function fadeOut() {
    pointer = null
    if (frame) cancelAnimationFrame(frame)
    frame = 0
    document.querySelectorAll<HTMLElement>(SHEEN_SELECTOR).forEach((element) => setGlow(element, 0))
  }

  function onPointerLeave(event: PointerEvent) {
    if (!isInsideWindow(event.clientX, event.clientY, window.innerWidth, window.innerHeight)) fadeOut()
  }

  function start() {
    if (active) return
    active = true
    window.addEventListener('pointermove', onPointerMove, { passive: true })
    document.addEventListener('scroll', schedule, { capture: true, passive: true })
    document.documentElement.addEventListener('pointerleave', onPointerLeave)
  }

  function stop() {
    if (!active) return
    active = false
    window.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('scroll', schedule, { capture: true })
    document.documentElement.removeEventListener('pointerleave', onPointerLeave)
    fadeOut()
    document.querySelectorAll<HTMLElement>(SHEEN_SELECTOR).forEach((element) => {
      glows.delete(element)
      element.style.removeProperty('--glass-rim-glow')
      element.style.removeProperty('--glass-rim-x')
      element.style.removeProperty('--glass-rim-y')
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
