import { describe, expect, it } from 'vitest'
import { buildOffsets, visibleRange } from './useVirtualList'

describe('virtual list math', () => {
  it('accumulates row heights into offsets', () => {
    const offsets = buildOffsets(4, (i) => (i === 1 ? 60 : 26))
    expect(Array.from(offsets)).toEqual([0, 26, 86, 112, 138])
  })

  it('covers the viewport plus overscan', () => {
    const offsets = buildOffsets(1000, () => 26)
    expect(visibleRange(offsets, 0, 260, 5)).toEqual({ start: 0, end: 16 })
    expect(visibleRange(offsets, 2600, 260, 5)).toEqual({ start: 95, end: 116 })
  })

  it('accounts for a tall expanded row', () => {
    const offsets = buildOffsets(100, (i) => (i === 10 ? 400 : 26))
    expect(visibleRange(offsets, 300, 100, 0)).toEqual({ start: 10, end: 11 })
    expect(visibleRange(offsets, 650, 100, 0)).toEqual({ start: 10, end: 15 })
    expect(visibleRange(offsets, 660, 100, 0)).toEqual({ start: 11, end: 15 })
  })

  it('clamps to the end of the list and handles an empty list', () => {
    const offsets = buildOffsets(10, () => 26)
    expect(visibleRange(offsets, 10_000, 260, 3)).toEqual({ start: 6, end: 10 })
    expect(visibleRange(buildOffsets(0, () => 26), 0, 260, 3)).toEqual({ start: 0, end: 0 })
  })
})
