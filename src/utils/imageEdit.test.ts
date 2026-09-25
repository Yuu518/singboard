import { describe, expect, it } from 'vitest'
import {
  centeredCrop,
  flipEdit,
  initialEditState,
  moveCrop,
  resizeCrop,
  rotateCrop,
  rotateEdit,
  transformedSize,
  type CropRect,
} from './imageEdit'

function expectRect(actual: CropRect, expected: CropRect) {
  expect(actual.x).toBeCloseTo(expected.x, 6)
  expect(actual.y).toBeCloseTo(expected.y, 6)
  expect(actual.w).toBeCloseTo(expected.w, 6)
  expect(actual.h).toBeCloseTo(expected.h, 6)
}

describe('image edit geometry', () => {
  const crop = { x: 0.1, y: 0.2, w: 0.3, h: 0.4 }

  it('swaps the output size for quarter turns', () => {
    expect(transformedSize(400, 300, 0)).toEqual([400, 300])
    expect(transformedSize(400, 300, 90)).toEqual([300, 400])
    expect(transformedSize(400, 300, 180)).toEqual([400, 300])
    expect(transformedSize(400, 300, 270)).toEqual([300, 400])
  })

  it('keeps the crop on the same part of the image when rotating', () => {
    expectRect(rotateCrop(crop, true), { x: 0.4, y: 0.1, w: 0.4, h: 0.3 })
    expectRect(rotateCrop(rotateCrop(crop, true), false), crop)
    let turned = crop
    for (let i = 0; i < 4; i++) turned = rotateCrop(turned, true)
    expectRect(turned, crop)
  })

  it('wraps the rotation and carries flips across quarter turns', () => {
    const flipped = flipEdit(initialEditState(), true)
    expectRect(flipped.crop, { x: 0, y: 0, w: 1, h: 1 })
    const turned = rotateEdit(flipped, true)
    expect(turned.rotation).toBe(90)
    expect(turned.flipX).toBe(false)
    expect(turned.flipY).toBe(true)
    expect(rotateEdit(initialEditState(), false).rotation).toBe(270)
  })

  it('mirrors the crop with the image', () => {
    expectRect(flipEdit({ ...initialEditState(), crop }, true).crop, { x: 0.6, y: 0.2, w: 0.3, h: 0.4 })
    expectRect(flipEdit({ ...initialEditState(), crop }, false).crop, { x: 0.1, y: 0.4, w: 0.3, h: 0.4 })
  })

  it('centres the largest crop with the requested aspect', () => {
    expectRect(centeredCrop(2), { x: 0, y: 0.25, w: 1, h: 0.5 })
    expectRect(centeredCrop(0.5), { x: 0.25, y: 0, w: 0.5, h: 1 })
    expectRect(centeredCrop(1), { x: 0, y: 0, w: 1, h: 1 })
  })

  it('keeps a moved crop inside the image', () => {
    expectRect(moveCrop(crop, 1, -1), { x: 0.7, y: 0, w: 0.3, h: 0.4 })
  })

  it('resizes freely from any edge within the image and minimum size', () => {
    const limits = { minW: 0.1, minH: 0.1 }
    expectRect(resizeCrop(crop, 'se', 0.1, 0.1, limits), { x: 0.1, y: 0.2, w: 0.4, h: 0.5 })
    expectRect(resizeCrop(crop, 'w', -0.5, 0, limits), { x: 0, y: 0.2, w: 0.4, h: 0.4 })
    expectRect(resizeCrop(crop, 'n', 0, 0.9, limits), { x: 0.1, y: 0.5, w: 0.3, h: 0.1 })
    expectRect(resizeCrop(crop, 'e', 5, 5, limits), { x: 0.1, y: 0.2, w: 0.9, h: 0.4 })
  })

  it('keeps the aspect when a corner is dragged with a locked ratio', () => {
    const square = { x: 0.2, y: 0.2, w: 0.4, h: 0.4 }
    const limits = { minW: 0.05, minH: 0.05, aspect: 1 }
    expectRect(resizeCrop(square, 'se', 0.1, 0.02, limits), { x: 0.2, y: 0.2, w: 0.5, h: 0.5 })
    expectRect(resizeCrop(square, 'nw', -0.5, -0.5, limits), { x: 0, y: 0, w: 0.6, h: 0.6 })
    expectRect(resizeCrop(square, 'se', 1, 1, limits), { x: 0.2, y: 0.2, w: 0.8, h: 0.8 })
  })
})
