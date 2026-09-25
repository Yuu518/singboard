import { describe, expect, it } from 'vitest'
import { initialEditState, renderEditCanvas, rotateEdit, flipEdit } from './imageEdit'

function redBlueSource(): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  canvas.width = 40
  canvas.height = 20
  const context = canvas.getContext('2d')!
  context.fillStyle = '#ff0000'
  context.fillRect(0, 0, 20, 20)
  context.fillStyle = '#0000ff'
  context.fillRect(20, 0, 20, 20)
  return canvas
}

function colourAt(canvas: HTMLCanvasElement, x: number, y: number): 'red' | 'blue' {
  const [r, , b] = canvas.getContext('2d')!.getImageData(x, y, 1, 1).data
  return r > b ? 'red' : 'blue'
}

describe('image edit rendering', () => {
  const source = redBlueSource()

  it('rotates the pixels a quarter turn clockwise', () => {
    const output = renderEditCanvas(source, 40, 20, rotateEdit(initialEditState(), true))
    expect([output.width, output.height]).toEqual([20, 40])
    expect(colourAt(output, 10, 5)).toBe('red')
    expect(colourAt(output, 10, 35)).toBe('blue')
  })

  it('mirrors the pixels horizontally', () => {
    const output = renderEditCanvas(source, 40, 20, flipEdit(initialEditState(), true))
    expect(colourAt(output, 5, 10)).toBe('blue')
    expect(colourAt(output, 35, 10)).toBe('red')
  })

  it('crops to the selected area of the transformed image', () => {
    const state = { ...initialEditState(), crop: { x: 0.5, y: 0, w: 0.5, h: 1 } }
    const output = renderEditCanvas(source, 40, 20, state)
    expect([output.width, output.height]).toEqual([20, 20])
    expect(colourAt(output, 10, 10)).toBe('blue')
  })

  it('crops after mirroring', () => {
    const mirrored = flipEdit(initialEditState(), true)
    const output = renderEditCanvas(source, 40, 20, { ...mirrored, crop: { x: 0, y: 0, w: 0.5, h: 1 } })
    expect(colourAt(output, 10, 10)).toBe('blue')
  })
})
