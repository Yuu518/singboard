import { describe, expect, it, vi } from 'vitest'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

describe('liquid lens displacement map', () => {
  const pixel = (data: Uint8ClampedArray, width: number, x: number, y: number) => {
    const i = (y * width + x) * 4
    return [data[i], data[i + 1]]
  }

  it('leaves the centre untouched and pulls the edges inward', async () => {
    const { buildDisplacementMap } = await import('./useLiquidLens')
    const width = 200
    const height = 120
    const data = buildDisplacementMap(width, height, 20)

    const [cx, cy] = pixel(data, width, 100, 60)
    expect(Math.abs(cx - 127.5)).toBeLessThan(1)
    expect(Math.abs(cy - 127.5)).toBeLessThan(1)

    expect(pixel(data, width, 0, 60)[0]).toBeGreaterThan(200)
    expect(pixel(data, width, width - 1, 60)[0]).toBeLessThan(55)
    expect(pixel(data, width, 100, 0)[1]).toBeGreaterThan(200)
    expect(pixel(data, width, 100, height - 1)[1]).toBeLessThan(55)
  })

  it('fades the refraction out across the refraction band', async () => {
    const { buildDisplacementMap } = await import('./useLiquidLens')
    const data = buildDisplacementMap(200, 120, 20, 24, 24)
    const edge = pixel(data, 200, 0, 60)[0]
    const middle = pixel(data, 200, 12, 60)[0]
    const inner = pixel(data, 200, 30, 60)[0]
    expect(edge).toBeGreaterThan(middle)
    expect(middle).toBeGreaterThan(127)
    expect(Math.abs(inner - 127.5)).toBeLessThan(1)
  })
})

describe('liquid lens filter', () => {
  it('splits the refraction into three channels with rising strength', async () => {
    const { buildLensFilter, REFRACTION_AMOUNT, CHROMA_SPREAD } = await import('./useLiquidLens')
    const filter = buildLensFilter(200, 120, 'data:image/png;base64,', 'lens-test')
    const base = REFRACTION_AMOUNT * 2

    const shifts = [...filter.querySelectorAll('feDisplacementMap')]
    expect(shifts.map((node) => Number(node.getAttribute('scale')))).toEqual([
      base,
      base * (1 + CHROMA_SPREAD),
      base * (1 + 2 * CHROMA_SPREAD),
    ])
    for (const node of shifts) {
      expect(node.getAttribute('in')).toBe('SourceGraphic')
      expect(node.getAttribute('in2')).toBe('map')
    }

    const matrices = [...filter.querySelectorAll('feColorMatrix')].map((node) =>
      node.getAttribute('values')!.split(' ').map(Number),
    )
    expect(matrices).toHaveLength(3)
    matrices.forEach((values, channel) => {
      for (let row = 0; row < 3; row++) {
        expect(values.slice(row * 5, row * 5 + 3)).toEqual([0, 1, 2].map((col) => (row === channel && col === channel ? 1 : 0)))
      }
      expect(values.slice(15)).toEqual([0, 0, 0, 1, 0])
    })

    const blends = [...filter.querySelectorAll('feBlend')]
    expect(blends).toHaveLength(2)
    expect(blends.every((node) => node.getAttribute('mode') === 'screen')).toBe(true)
    expect(filter.lastElementChild).toBe(blends[1])
  })
})
