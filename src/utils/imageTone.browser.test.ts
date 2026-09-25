import { describe, expect, it } from 'vitest'
import { backdropTone, sampleImage, SAMPLE_SIZE } from './imageTone'
import { canvasToBlob } from './imageEdit'

async function wallpaper(colour: string): Promise<Blob> {
  const canvas = document.createElement('canvas')
  canvas.width = 640
  canvas.height = 360
  const context = canvas.getContext('2d')!
  context.fillStyle = colour
  context.fillRect(0, 0, 640, 360)
  return canvasToBlob(canvas, 'image/png')
}

describe('wallpaper sampling', () => {
  it('reads a small thumbnail of the image', async () => {
    const data = await sampleImage(await wallpaper('#102040'))
    expect(data.length).toBe(SAMPLE_SIZE * SAMPLE_SIZE * 4)
    expect([data[0], data[1], data[2]]).toEqual([16, 32, 64])
  })

  it('tells deep and pale wallpapers apart', async () => {
    expect(backdropTone(await sampleImage(await wallpaper('#141828')), 'light')).toBe('dark')
    expect(backdropTone(await sampleImage(await wallpaper('#f4efe6')), 'dark')).toBe('light')
  })
})
