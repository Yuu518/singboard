import { describe, expect, it, vi } from 'vitest'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

const MAP = { x: 0, y: 0, width: 200, height: 120, href: 'data:image/png;base64,' }

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

describe('liquid lens map tiles', () => {
  const compose = async (width: number, height: number, radius: number, band: number) => {
    const { buildDisplacementMap, mapTiles } = await import('./useLiquidLens')
    const out = new Uint8ClampedArray(width * height * 4)
    for (let i = 0; i < out.length; i += 4) out.set([128, 128, 0, 255], i)
    const tiles = mapTiles(width, height, radius, band)
    for (const tile of tiles) {
      const { source } = tile
      const data = buildDisplacementMap(source.mapWidth, source.mapHeight, radius, band, 24, source)
      for (let y = 0; y < tile.height; y++) {
        for (let x = 0; x < tile.width; x++) {
          const sx = Math.min(source.width - 1, Math.floor((x * source.width) / tile.width))
          const sy = Math.min(source.height - 1, Math.floor((y * source.height) / tile.height))
          const s = (sy * source.width + sx) * 4
          out.set(data.subarray(s, s + 4), ((tile.y + y) * width + tile.x + x) * 4)
        }
      }
    }
    return { out, tiles, full: buildDisplacementMap(width, height, radius, band, 24) }
  }

  it('rebuilds a large map from small corner and edge tiles', async () => {
    const { out, tiles, full } = await compose(640, 300, 18, 24)
    expect(tiles).toHaveLength(8)
    expect(Math.max(...tiles.map((tile) => tile.source.width * tile.source.height))).toBeLessThan(40 * 40)
    let worst = 0
    for (let i = 0; i < full.length; i++) worst = Math.max(worst, Math.abs(full[i] - out[i]))
    expect(worst).toBeLessThanOrEqual(1)
  })

  it('shares the same tiles across sizes', async () => {
    const { mapTiles } = await import('./useLiquidLens')
    const sources = (width: number, height: number) => mapTiles(width, height, 18).map((tile) => JSON.stringify(tile.source))
    expect(sources(640, 300)).toEqual(sources(1090, 78))
  })

  it('falls back to one full map for small surfaces', async () => {
    const { out, tiles, full } = await compose(60, 50, 18, 24)
    expect(tiles).toHaveLength(1)
    expect([...out]).toEqual([...full])
  })
})

describe('liquid lens filter', () => {
  it('lays the map tiles over a neutral base', async () => {
    const { buildLensFilter } = await import('./useLiquidLens')
    const tiles = [MAP, { ...MAP, x: 10, width: 20, href: 'data:image/png;base64,AA' }]
    const filter = buildLensFilter(200, 120, tiles, 'tiled-lens', { chroma: false })
    const images = [...filter.querySelectorAll('feImage')]
    expect(images.map((node) => [node.getAttribute('x'), node.getAttribute('width'), node.getAttribute('href')])).toEqual([
      ['0', '200', MAP.href],
      ['10', '20', 'data:image/png;base64,AA'],
    ])
    const merge = filter.querySelector('feMerge')!
    expect(merge.getAttribute('result')).toBe('map')
    expect([...merge.children].map((node) => node.getAttribute('in'))).toEqual(['map-base', 'map-0', 'map-1'])
    expect(filter.querySelector('feFlood')!.getAttribute('result')).toBe('map-base')
  })

  it('splits the refraction into three channels with rising strength', async () => {
    const { buildLensFilter, REFRACTION_AMOUNT, CHROMA_SPREAD } = await import('./useLiquidLens')
    const filter = buildLensFilter(200, 120, [MAP], 'lens-test')
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

describe('liquid lens targets', () => {
  it('only refracts cards when a background is set', async () => {
    const { lensSelector } = await import('./useLiquidLens')
    expect(lensSelector(false)).toBe('.glass-float, .glass-popover')
    expect(lensSelector(true)).toBe('.glass-float, .glass-popover, .surface-card, .settings-card')
  })
})

describe('card lens filter', () => {
  it('uses a single displacement pass without colour fringing', async () => {
    const { buildLensFilter, REFRACTION_AMOUNT } = await import('./useLiquidLens')
    const filter = buildLensFilter(200, 120, [MAP], 'card-lens', { chroma: false })
    const shifts = [...filter.querySelectorAll('feDisplacementMap')]
    expect(shifts).toHaveLength(1)
    expect(Number(shifts[0].getAttribute('scale'))).toBe(REFRACTION_AMOUNT * 2)
    expect(filter.querySelectorAll('feColorMatrix, feBlend')).toHaveLength(0)
    expect(filter.lastElementChild).toBe(shifts[0])
  })
})

describe('popover lens filter', () => {
  it('keeps the rim sharp and frosts the middle', async () => {
    const { buildLensFilter } = await import('./useLiquidLens')
    const filter = buildLensFilter(200, 120, [MAP], 'pop-lens', { frost: 20 })
    const blur = filter.querySelector('feGaussianBlur')!
    expect(blur.getAttribute('in')).toBe('SourceGraphic')
    expect(blur.getAttribute('stdDeviation')).toBe('20')
    const [rim, over] = [...filter.querySelectorAll('feComposite')]
    expect(rim.getAttribute('in')).toBe('refracted-soft')
    expect(rim.getAttribute('operator')).toBe('in')
    expect(over.getAttribute('in2')).toBe(blur.getAttribute('result'))
    expect(over.getAttribute('operator')).toBe('over')
    expect(filter.lastElementChild).toBe(over)
    expect(filter.querySelectorAll('feDisplacementMap')).toHaveLength(3)
  })

  it('stores the rim band as an edge mask in the blue channel', async () => {
    const { buildDisplacementMap } = await import('./useLiquidLens')
    const data = buildDisplacementMap(200, 120, 20, 24, 24)
    const blue = (x: number, y: number) => data[(y * 200 + x) * 4 + 2]
    expect(blue(0, 60)).toBeGreaterThan(240)
    expect(blue(12, 60)).toBeGreaterThan(90)
    expect(blue(12, 60)).toBeLessThan(170)
    expect(blue(100, 60)).toBe(0)
  })
})

describe('lens strength', () => {
  it('scales every displacement pass with the requested amount', async () => {
    const { buildLensFilter, CHROMA_SPREAD } = await import('./useLiquidLens')
    const filter = buildLensFilter(200, 120, [MAP], 'soft-lens', { amount: 12, frost: 4 })
    expect([...filter.querySelectorAll('feDisplacementMap')].map((node) => Number(node.getAttribute('scale')))).toEqual([
      24,
      24 * (1 + CHROMA_SPREAD),
      24 * (1 + 2 * CHROMA_SPREAD),
    ])
    const soften = [...filter.querySelectorAll('feGaussianBlur')].find((node) => node.getAttribute('in') === 'refracted')
    expect(Number(soften?.getAttribute('stdDeviation'))).toBeGreaterThan(0)
    expect(Number(soften?.getAttribute('stdDeviation'))).toBeLessThan(2)
  })
})
