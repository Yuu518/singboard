export type Tone = 'light' | 'dark'

export const SAMPLE_SIZE = 32
export const BACKDROP_VEIL: Record<Tone, { value: number; alpha: number }> = {
  light: { value: 255, alpha: 0.28 },
  dark: { value: 0, alpha: 0.38 },
}
const INK_CROSSOVER = 0.179

function toLinear(channel: number): number {
  const value = channel / 255
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
}

export function veiledLuminance(data: Uint8ClampedArray, theme: Tone): number {
  const { value, alpha } = BACKDROP_VEIL[theme]
  const veil = (channel: number) => toLinear(channel * (1 - alpha) + value * alpha)
  let total = 0
  let count = 0
  for (let i = 0; i < data.length; i += 4) {
    total += 0.2126 * veil(data[i]) + 0.7152 * veil(data[i + 1]) + 0.0722 * veil(data[i + 2])
    count++
  }
  return count ? total / count : 0
}

export function backdropTone(data: Uint8ClampedArray, theme: Tone): Tone {
  return veiledLuminance(data, theme) > INK_CROSSOVER ? 'light' : 'dark'
}

export async function sampleImage(blob: Blob): Promise<Uint8ClampedArray> {
  const bitmap = await createImageBitmap(blob, {
    resizeWidth: SAMPLE_SIZE,
    resizeHeight: SAMPLE_SIZE,
    resizeQuality: 'medium',
  })
  try {
    const canvas = document.createElement('canvas')
    canvas.width = SAMPLE_SIZE
    canvas.height = SAMPLE_SIZE
    const context = canvas.getContext('2d')
    if (!context) throw new Error('canvas unavailable')
    context.drawImage(bitmap, 0, 0)
    return context.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE).data
  } finally {
    bitmap.close()
  }
}
