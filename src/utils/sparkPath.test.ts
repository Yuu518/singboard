import { describe, expect, it } from 'vitest'
import { sparkPath } from './sparkPath'

const now = Date.now()

describe('sparkPath', () => {
  it('draws nothing until there are two real samples', () => {
    const placeholders = [{ name: 0, value: null }, { name: 1, value: null }, { name: 2, value: 0 }]
    expect(sparkPath(placeholders, { width: 100, base: 50, extent: 40, direction: 1, floor: 1 }).line).toBe('')
    expect(sparkPath([{ name: 0, value: null }, { name: now, value: 5 }], { width: 100, base: 50, extent: 40, direction: 1, floor: 1 }).line).toBe('')
  })

  it('places samples by their slot so history grows in from the right', () => {
    const points = [{ name: 0, value: null }, { name: 1, value: null }, { name: now, value: 10 }, { name: now + 1000, value: 20 }]
    const geometry = sparkPath(points, { width: 300, base: 100, extent: 80, direction: 1, floor: 1 })
    expect(geometry.line.startsWith('M200,60')).toBe(true)
    expect(geometry.last).toEqual({ x: 300, y: 20 })
    expect(geometry.area.endsWith('L300,100 L200,100 Z')).toBe(true)
  })

  it('keeps small values small against the floor and mirrors downward', () => {
    const points = [{ name: now, value: 1 }, { name: now + 1000, value: 1 }]
    const up = sparkPath(points, { width: 100, base: 50, extent: 40, direction: 1, floor: 100 })
    const down = sparkPath(points, { width: 100, base: 50, extent: 40, direction: -1, floor: 100 })
    expect(up.last).toEqual({ x: 100, y: 49.6 })
    expect(down.last).toEqual({ x: 100, y: 50.4 })
  })
})
