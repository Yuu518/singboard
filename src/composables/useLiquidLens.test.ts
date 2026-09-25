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
