import { onBeforeUnmount, ref } from 'vue'

const FALLBACK_INK = [28, 28, 30]

export function readChartInk(root: Element = document.documentElement): number[] {
  const raw = getComputedStyle(root).getPropertyValue('--chart-ink-rgb').trim()
  const parts = raw.split(/[\s,]+/).map(Number)
  return parts.length === 3 && parts.every((n) => Number.isFinite(n)) ? parts : FALLBACK_INK
}

const FALLBACK_TOPOLOGY = [
  [88, 86, 214],
  [50, 173, 230],
  [175, 82, 222],
  [255, 45, 85],
]

export function readTopologyColors(root: Element = document.documentElement): number[][] {
  const style = getComputedStyle(root)
  return FALLBACK_TOPOLOGY.map((fallback, index) => {
    const parts = style.getPropertyValue(`--chart-topology-${index + 1}`).trim().split(/[\s,]+/).map(Number)
    return parts.length === 3 && parts.every((n) => Number.isFinite(n)) ? parts : fallback
  })
}

export function inkColor(rgb: readonly number[], alpha: number): string {
  return `rgba(${rgb[0]},${rgb[1]},${rgb[2]},${alpha})`
}

export const CHART_TOOLTIP_CSS = [
  'background: rgb(var(--glass-rgb) / var(--glass-pop-alpha))',
  'color: oklch(var(--bc))',
  'border-radius: var(--radius-tile)',
  'backdrop-filter: blur(var(--glass-pop-blur)) saturate(var(--glass-saturate))',
  'box-shadow: var(--glass-shadow-lg), var(--glass-edge)',
  'font-size: 12px',
  'line-height: 1.45',
].join('; ')

export function useReducedMotion() {
  const query = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-reduced-motion: reduce)') : null
  const reduced = ref(!!query?.matches)
  const sync = () => {
    reduced.value = !!query?.matches
  }
  query?.addEventListener?.('change', sync)
  onBeforeUnmount(() => query?.removeEventListener?.('change', sync))
  return reduced
}
