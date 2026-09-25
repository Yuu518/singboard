import { describe, expect, it } from 'vitest'
import { isInsideWindow, isOnScreen, SHEEN_REACH, sheenGlow } from './useGlassSheen'

describe('glass sheen visibility', () => {
  it('skips elements outside the viewport', () => {
    expect(isOnScreen({ left: 10, top: 10, width: 100, height: 50 }, 800, 600)).toBe(true)
    expect(isOnScreen({ left: 10, top: -60, width: 100, height: 50 }, 800, 600)).toBe(false)
    expect(isOnScreen({ left: 10, top: 600, width: 100, height: 50 }, 800, 600)).toBe(false)
    expect(isOnScreen({ left: -120, top: 10, width: 100, height: 50 }, 800, 600)).toBe(false)
    expect(isOnScreen({ left: 10, top: 10, width: 0, height: 0 }, 800, 600)).toBe(false)
  })
})

describe('glass sheen glow', () => {
  const rect = { left: 100, top: 100, width: 700, height: 60 }

  it('is fully lit anywhere over the element', () => {
    expect(sheenGlow(rect, 100, 100)).toBe(1)
    expect(sheenGlow(rect, 450, 130)).toBe(1)
    expect(sheenGlow(rect, 800, 160)).toBe(1)
  })

  it('fades out smoothly with distance from the nearest edge', () => {
    expect(sheenGlow(rect, 450, 100 - SHEEN_REACH / 2)).toBeCloseTo(0.5, 5)
    expect(sheenGlow(rect, 800 + SHEEN_REACH / 2, 130)).toBeCloseTo(0.5, 5)
    expect(sheenGlow(rect, 450, 100 - SHEEN_REACH)).toBe(0)
    expect(sheenGlow(rect, 450, 100 - SHEEN_REACH - 50)).toBe(0)
  })

  it('changes gradually as the pointer slides along a long edge', () => {
    let previous = sheenGlow(rect, 100, 60)
    for (let x = 110; x <= 800; x += 10) {
      const glow = sheenGlow(rect, x, 60)
      expect(Math.abs(glow - previous)).toBeLessThan(0.05)
      previous = glow
    }
  })

  it('treats drag regions inside the window as still inside', () => {
    expect(isInsideWindow(300, 20, 1100, 700)).toBe(true)
    expect(isInsideWindow(0, 20, 1100, 700)).toBe(false)
    expect(isInsideWindow(300, 699, 1100, 700)).toBe(false)
  })
})
