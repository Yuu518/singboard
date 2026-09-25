import { onBeforeUnmount, watch } from 'vue'
import { useConfigStore } from '@/stores/config'

const LENS_SELECTOR = '.glass-float, .glass-popover'
const REFRACTION_HEIGHT = 24
export const REFRACTION_AMOUNT = 24
export const CHROMA_SPREAD = 0.06
const SVG_NS = 'http://www.w3.org/2000/svg'
const CHANNELS = [
  { name: 'r', matrix: '1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0' },
  { name: 'g', matrix: '0 0 0 0 0 0 1 0 0 0 0 0 0 0 0 0 0 0 1 0' },
  { name: 'b', matrix: '0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 0 0 0 1 0' },
] as const

function circleMap(x: number): number {
  return 1 - Math.sqrt(Math.max(0, 1 - x * x))
}

function sdRoundedRect(x: number, y: number, halfW: number, halfH: number, radius: number): number {
  const cx = Math.abs(x) - (halfW - radius)
  const cy = Math.abs(y) - (halfH - radius)
  const outside = Math.hypot(Math.max(cx, 0), Math.max(cy, 0)) - radius
  const inside = Math.min(Math.max(cx, cy), 0)
  return outside + inside
}

function gradRoundedRect(x: number, y: number, halfW: number, halfH: number, radius: number): [number, number] {
  const cx = Math.abs(x) - (halfW - radius)
  const cy = Math.abs(y) - (halfH - radius)
  const sx = Math.sign(x) || 1
  const sy = Math.sign(y) || 1
  if (cx >= 0 || cy >= 0) {
    const gx = Math.max(cx, 0)
    const gy = Math.max(cy, 0)
    const len = Math.hypot(gx, gy) || 1
    return [(sx * gx) / len, (sy * gy) / len]
  }
  return cx >= cy ? [sx, 0] : [0, sy]
}

export function buildDisplacementMap(
  width: number,
  height: number,
  radius: number,
  refractionHeight = REFRACTION_HEIGHT,
  refractionAmount = REFRACTION_AMOUNT,
): Uint8ClampedArray {
  const data = new Uint8ClampedArray(width * height * 4)
  const halfW = width / 2
  const halfH = height / 2
  const r = Math.min(radius, halfW, halfH)
  const gradRadius = Math.min(r * 1.5, halfW, halfH)
  for (let py = 0; py < height; py++) {
    for (let px = 0; px < width; px++) {
      const x = px + 0.5 - halfW
      const y = py + 0.5 - halfH
      let dx = 0
      let dy = 0
      const sd = Math.min(sdRoundedRect(x, y, halfW, halfH, r), 0)
      if (-sd < refractionHeight) {
        const d = circleMap(1 - -sd / refractionHeight) * refractionAmount
        const [gx, gy] = gradRoundedRect(x, y, halfW, halfH, gradRadius)
        dx = -d * gx
        dy = -d * gy
      }
      const i = (py * width + px) * 4
      data[i] = 255 * (0.5 + dx / (2 * refractionAmount))
      data[i + 1] = 255 * (0.5 + dy / (2 * refractionAmount))
      data[i + 2] = 128
      data[i + 3] = 255
    }
  }
  return data
}

function svgElement(tag: string, attributes: Record<string, string | number>): SVGElement {
  const element = document.createElementNS(SVG_NS, tag)
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, String(value))
  return element
}

export function buildLensFilter(width: number, height: number, href: string, id: string): SVGFilterElement {
  const filter = svgElement('filter', {
    id,
    filterUnits: 'userSpaceOnUse',
    primitiveUnits: 'userSpaceOnUse',
    'color-interpolation-filters': 'sRGB',
    x: 0,
    y: 0,
    width,
    height,
  }) as SVGFilterElement
  filter.append(svgElement('feImage', { x: 0, y: 0, width, height, preserveAspectRatio: 'none', result: 'map', href }))
  CHANNELS.forEach((channel, index) => {
    filter.append(
      svgElement('feDisplacementMap', {
        in: 'SourceGraphic',
        in2: 'map',
        scale: REFRACTION_AMOUNT * 2 * (1 + index * CHROMA_SPREAD),
        xChannelSelector: 'R',
        yChannelSelector: 'G',
        result: `${channel.name}-shift`,
      }),
      svgElement('feColorMatrix', {
        in: `${channel.name}-shift`,
        type: 'matrix',
        values: channel.matrix,
        result: channel.name,
      }),
    )
  })
  filter.append(
    svgElement('feBlend', { in: 'g', in2: 'b', mode: 'screen', result: 'gb' }),
    svgElement('feBlend', { in: 'r', in2: 'gb', mode: 'screen' }),
  )
  return filter
}

interface LensEntry {
  filter: SVGFilterElement | null
  key: string
}

export function useLiquidLens() {
  const { config } = useConfigStore()
  const entries = new Map<HTMLElement, LensEntry>()
  const reducedTransparency = window.matchMedia('(prefers-reduced-transparency: reduce)')
  let svg: SVGSVGElement | null = null
  let mutationObserver: MutationObserver | null = null
  let resizeObserver: ResizeObserver | null = null
  let scanFrame = 0
  let counter = 0

  function ensureSvg(): SVGSVGElement {
    if (!svg) {
      svg = document.createElementNS(SVG_NS, 'svg')
      svg.setAttribute('aria-hidden', 'true')
      svg.setAttribute('width', '0')
      svg.setAttribute('height', '0')
      svg.style.position = 'absolute'
      svg.style.pointerEvents = 'none'
      document.body.appendChild(svg)
    }
    return svg
  }

  function renderMap(width: number, height: number, radius: number): string {
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = height
    const context = canvas.getContext('2d')
    if (!context) return ''
    context.putImageData(new ImageData(buildDisplacementMap(width, height, radius), width, height), 0, 0)
    return canvas.toDataURL()
  }

  async function createFilter(width: number, height: number, href: string): Promise<SVGFilterElement> {
    const preload = new Image()
    preload.src = href
    await preload.decode().catch(() => {})
    const filter = buildLensFilter(width, height, href, `liquid-lens-${++counter}`)
    ensureSvg().appendChild(filter)
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))
    return filter
  }

  async function update(element: HTMLElement) {
    const width = Math.round(element.offsetWidth)
    const height = Math.round(element.offsetHeight)
    if (width < 2 || height < 2) return
    const radius = parseFloat(getComputedStyle(element).borderTopLeftRadius) || 0
    const key = `${width}x${height}@${radius}`
    let entry = entries.get(element)
    if (entry?.key === key) return
    const href = renderMap(width, height, radius)
    if (!href) return
    if (!entry) {
      entry = { filter: null, key }
      entries.set(element, entry)
      resizeObserver?.observe(element)
    }
    entry.key = key
    const filter = await createFilter(width, height, href)
    if (entries.get(element) !== entry || entry.key !== key) {
      filter.remove()
      return
    }
    element.style.setProperty('--glass-lens', `url(#${filter.id})`)
    entry.filter?.remove()
    entry.filter = filter
  }

  function release(element: HTMLElement) {
    const entry = entries.get(element)
    if (!entry) return
    entry.filter?.remove()
    resizeObserver?.unobserve(element)
    element.style.removeProperty('--glass-lens')
    entries.delete(element)
  }

  function scan() {
    scanFrame = 0
    for (const element of [...entries.keys()]) {
      if (!element.isConnected) release(element)
    }
    document.querySelectorAll<HTMLElement>(LENS_SELECTOR).forEach((element) => void update(element))
  }

  function scheduleScan() {
    if (!scanFrame) scanFrame = requestAnimationFrame(scan)
  }

  function start() {
    if (mutationObserver) return
    resizeObserver = new ResizeObserver((records) => {
      for (const record of records) void update(record.target as HTMLElement)
    })
    mutationObserver = new MutationObserver(scheduleScan)
    mutationObserver.observe(document.body, { childList: true, subtree: true })
    scheduleScan()
  }

  function stop() {
    mutationObserver?.disconnect()
    resizeObserver?.disconnect()
    mutationObserver = null
    resizeObserver = null
    if (scanFrame) cancelAnimationFrame(scanFrame)
    scanFrame = 0
    for (const element of [...entries.keys()]) release(element)
    svg?.remove()
    svg = null
  }

  function sync() {
    if (config.value.glassMode === 'clear' && !reducedTransparency.matches) start()
    else stop()
  }

  watch(() => config.value.glassMode, sync, { immediate: true })
  reducedTransparency.addEventListener('change', sync)
  onBeforeUnmount(() => {
    reducedTransparency.removeEventListener('change', sync)
    stop()
  })
}
