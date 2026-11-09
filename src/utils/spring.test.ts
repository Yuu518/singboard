import { describe, expect, it } from 'vitest'
import { Spring } from './spring'

function run(spring: Spring, seconds: number, frame = 1 / 60) {
  let peak = spring.value
  for (let t = 0; t < seconds; t += frame) {
    spring.step(frame)
    peak = Math.max(peak, spring.value)
  }
  return peak
}

describe('Spring', () => {
  it('settles on the target with critical damping and never overshoots', () => {
    const spring = new Spring(0, 1, 1000)
    spring.target = 3
    const peak = run(spring, 1)
    expect(spring.value).toBe(3)
    expect(spring.settled).toBe(true)
    expect(peak).toBeLessThanOrEqual(3)
  })

  it('overshoots when under-damped like the pressed indicator scale', () => {
    const spring = new Spring(1, 0.6, 250)
    spring.target = 78 / 56
    const peak = run(spring, 2)
    expect(peak).toBeGreaterThan(78 / 56)
    expect(spring.value).toBeCloseTo(78 / 56, 3)
  })

  it('snaps without animating', () => {
    const spring = new Spring(0, 1, 1000)
    spring.target = 5
    spring.step(1 / 60)
    spring.snap(2)
    expect(spring.value).toBe(2)
    expect(spring.target).toBe(2)
    expect(spring.velocity).toBe(0)
    expect(spring.settled).toBe(true)
  })

  it('stays stable with long frames', () => {
    const spring = new Spring(0, 1, 1000)
    spring.target = 1
    spring.step(0.5)
    expect(Number.isFinite(spring.value)).toBe(true)
    expect(spring.value).toBeCloseTo(1, 3)
  })
})
