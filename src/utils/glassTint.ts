export const CLEAR_GLASS = 0
export const BALANCED_GLASS = 0.5
export const TINTED_GLASS = 1
export const GLASS_TICKS = [CLEAR_GLASS, BALANCED_GLASS, TINTED_GLASS] as const

const SNAP_HUNDREDTHS = 4

function round(value: number): number {
  return Math.round(value * 100) / 100
}

export function normalizeGlassTint(value: unknown, legacyMode?: unknown): number {
  if (typeof value === 'number' && Number.isFinite(value)) return round(Math.min(TINTED_GLASS, Math.max(CLEAR_GLASS, value)))
  return legacyMode === 'clear' ? CLEAR_GLASS : TINTED_GLASS
}

export function snapGlassTint(value: number): number {
  const tint = normalizeGlassTint(value)
  for (const tick of GLASS_TICKS) {
    if (Math.round(Math.abs(tint - tick) * 100) <= SNAP_HUNDREDTHS) return tick
  }
  return tint
}

export function isClearGlass(tint: number): boolean {
  return tint < BALANCED_GLASS
}

export function glassTintLabel(tint: number): string {
  if (tint === CLEAR_GLASS) return '通透'
  if (tint === BALANCED_GLASS) return '适中'
  if (tint === TINTED_GLASS) return '模糊'
  return isClearGlass(tint) ? '偏通透' : '偏模糊'
}

export function applyGlassTint(tint: number, root: HTMLElement = document.documentElement) {
  root.style.setProperty('--glass-t', String(tint))
  root.setAttribute('data-glass', isClearGlass(tint) ? 'clear' : 'blur')
}
