import { describe, expect, it } from 'vitest'
import { applyGlassTint, glassTintLabel, isClearGlass, normalizeGlassTint, snapGlassTint } from './glassTint'

describe('glass tint', () => {
  it('clamps and rounds stored values, falling back to the old two-way mode', () => {
    expect(normalizeGlassTint(0.333)).toBe(0.33)
    expect(normalizeGlassTint(-1)).toBe(0)
    expect(normalizeGlassTint(2)).toBe(1)
    expect(normalizeGlassTint(Number.NaN, 'clear')).toBe(0)
    expect(normalizeGlassTint(undefined, 'clear')).toBe(0)
    expect(normalizeGlassTint(undefined, 'blur')).toBe(1)
    expect(normalizeGlassTint(undefined)).toBe(1)
  })

  it('snaps to the clear, balanced and tinted marks when close to them', () => {
    expect(snapGlassTint(0.03)).toBe(0)
    expect(snapGlassTint(0.47)).toBe(0.5)
    expect(snapGlassTint(0.54)).toBe(0.5)
    expect(snapGlassTint(0.97)).toBe(1)
    expect(snapGlassTint(0.3)).toBe(0.3)
    expect(snapGlassTint(0.62)).toBe(0.62)
  })

  it('names the marks and the ranges between them', () => {
    expect(glassTintLabel(0)).toBe('通透')
    expect(glassTintLabel(0.5)).toBe('适中')
    expect(glassTintLabel(1)).toBe('模糊')
    expect(glassTintLabel(0.2)).toBe('偏通透')
    expect(glassTintLabel(0.8)).toBe('偏模糊')
  })

  it('treats the left half as clear glass and writes the tint onto the root element', () => {
    expect(isClearGlass(0.49)).toBe(true)
    expect(isClearGlass(0.5)).toBe(false)
    const root = document.createElement('div')
    applyGlassTint(0.25, root)
    expect(root.style.getPropertyValue('--glass-t')).toBe('0.25')
    expect(root.getAttribute('data-glass')).toBe('clear')
    applyGlassTint(0.75, root)
    expect(root.getAttribute('data-glass')).toBe('blur')
  })
})
