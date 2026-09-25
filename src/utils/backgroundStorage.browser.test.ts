import { afterEach, describe, expect, it } from 'vitest'
import { deleteBackground, loadBackground, saveBackground } from './backgroundStorage'

describe('background storage', () => {
  afterEach(async () => {
    await deleteBackground()
  })

  it('round-trips the background image through IndexedDB', async () => {
    expect(await loadBackground()).toBeNull()

    await saveBackground(new Blob(['first'], { type: 'image/png' }))
    await saveBackground(new Blob(['second'], { type: 'image/webp' }))
    const stored = await loadBackground()
    expect(stored?.type).toBe('image/webp')
    expect(await stored?.text()).toBe('second')

    await deleteBackground()
    expect(await loadBackground()).toBeNull()
  })
})
