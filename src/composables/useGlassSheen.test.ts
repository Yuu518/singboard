import { describe, expect, it } from 'vitest'
import { rimAngle, unwrapAngle } from './useGlassSheen'

describe('glass sheen angle', () => {
  const rect = { left: 100, top: 100, width: 200, height: 100 }

  it('points the rim gradient from the element centre towards the pointer', () => {
    expect(rimAngle(rect, 200, 0)).toBeCloseTo(0, 2)
    expect(rimAngle(rect, 500, 150)).toBeCloseTo(90, 2)
    expect(rimAngle(rect, 200, 400)).toBeCloseTo(180, 2)
    expect(rimAngle(rect, 0, 150)).toBeCloseTo(270, 2)
  })

  it('takes the short way round when the angle wraps', () => {
    expect(unwrapAngle(350, 10)).toBe(370)
    expect(unwrapAngle(10, 350)).toBe(-10)
    expect(unwrapAngle(100, 120)).toBe(120)
    expect(unwrapAngle(725, 0)).toBe(720)
  })
})
