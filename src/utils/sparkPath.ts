export interface SparkPoint {
  name: number
  value: number | null
}

export interface SparkOptions {
  width: number
  base: number
  extent: number
  direction: 1 | -1
  floor: number
}

export interface SparkGeometry {
  line: string
  area: string
  last: { x: number; y: number } | null
}

const EMPTY: SparkGeometry = { line: '', area: '', last: null }

export function isSample(point: SparkPoint): point is { name: number; value: number } {
  return point.name > 1e12 && typeof point.value === 'number' && Number.isFinite(point.value)
}

const round = (n: number) => Math.round(n * 10) / 10

export function sparkPath(points: readonly SparkPoint[], options: SparkOptions): SparkGeometry {
  const count = points.length
  if (count < 2) return EMPTY
  const step = options.width / (count - 1)
  const samples: { x: number; value: number }[] = []
  points.forEach((point, index) => {
    if (isSample(point)) samples.push({ x: index * step, value: Math.max(0, point.value) })
  })
  if (samples.length < 2) return EMPTY
  const max = Math.max(options.floor, ...samples.map((sample) => sample.value))
  const coords = samples.map((sample) => [round(sample.x), round(options.base - options.direction * (sample.value / max) * options.extent)])
  let line = `M${coords[0][0]},${coords[0][1]}`
  for (let i = 1; i < coords.length; i++) {
    const [x0, y0] = coords[i - 1]
    const [x1, y1] = coords[i]
    const mid = round((x0 + x1) / 2)
    line += ` C${mid},${y0} ${mid},${y1} ${x1},${y1}`
  }
  const first = coords[0]
  const end = coords[coords.length - 1]
  return {
    line,
    area: `${line} L${end[0]},${options.base} L${first[0]},${options.base} Z`,
    last: { x: end[0], y: end[1] },
  }
}
