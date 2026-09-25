import { describe, expect, it } from 'vitest'
import { backdropTone, veiledLuminance } from './imageTone'

function solid(r: number, g: number, b: number, pixels = 4): Uint8ClampedArray {
  const data = new Uint8ClampedArray(pixels * 4)
  for (let i = 0; i < data.length; i += 4) data.set([r, g, b, 255], i)
  return data
}

describe('backdrop tone', () => {
  it('measures luminance through the theme veil', () => {
    expect(veiledLuminance(solid(0, 0, 0), 'dark')).toBeCloseTo(0, 5)
    expect(veiledLuminance(solid(255, 255, 255), 'light')).toBeCloseTo(1, 5)
    expect(veiledLuminance(solid(0, 0, 0), 'light')).toBeGreaterThan(0.05)
    expect(veiledLuminance(solid(255, 255, 255), 'dark')).toBeLessThan(0.5)
  })

  it('calls pale wallpapers light and deep ones dark in either theme', () => {
    expect(backdropTone(solid(240, 236, 230), 'light')).toBe('light')
    expect(backdropTone(solid(240, 236, 230), 'dark')).toBe('light')
    expect(backdropTone(solid(20, 24, 40), 'light')).toBe('dark')
    expect(backdropTone(solid(20, 24, 40), 'dark')).toBe('dark')
  })

  it('lets the veil tip a mid-tone wallpaper towards the theme', () => {
    const midTone = solid(118, 118, 118)
    expect(backdropTone(midTone, 'light')).toBe('light')
    expect(backdropTone(midTone, 'dark')).toBe('dark')
  })

  it('averages busy wallpapers instead of trusting one pixel', () => {
    const data = new Uint8ClampedArray([
      255, 255, 255, 255,
      0, 0, 0, 255,
      0, 0, 0, 255,
      0, 0, 0, 255,
    ])
    expect(backdropTone(data, 'dark')).toBe('dark')
  })
})
