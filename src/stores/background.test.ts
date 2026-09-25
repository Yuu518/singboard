import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const storage = vi.hoisted(() => ({
  loadBackground: vi.fn(),
  saveBackground: vi.fn(),
  deleteBackground: vi.fn(),
}))

vi.mock('@/utils/backgroundStorage', () => storage)

const tone = vi.hoisted(() => ({ sampleImage: vi.fn() }))

vi.mock('@/utils/imageTone', () => tone)

function imageFile(size = 16, type = 'image/png') {
  return new File([new Uint8Array(size)], 'wallpaper.png', { type })
}

describe('background store', () => {
  let urlCount = 0

  beforeEach(() => {
    vi.resetModules()
    urlCount = 0
    storage.loadBackground.mockResolvedValue(null)
    storage.saveBackground.mockResolvedValue(undefined)
    storage.deleteBackground.mockResolvedValue(undefined)
    tone.sampleImage.mockResolvedValue(new Uint8ClampedArray([10, 20, 30, 255]))
    vi.stubGlobal('createImageBitmap', vi.fn().mockResolvedValue({ close: vi.fn() }))
    URL.createObjectURL = vi.fn(() => `blob:bg-${++urlCount}`)
    URL.revokeObjectURL = vi.fn()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.clearAllMocks()
    document.documentElement.removeAttribute('data-backdrop')
  })

  async function loadStore() {
    const { nextTick } = await import('vue')
    const { useBackgroundStore } = await import('./background')
    return { store: useBackgroundStore(), nextTick }
  }

  it('stores a picked image and marks the root element', async () => {
    const { store, nextTick } = await loadStore()
    const file = imageFile()
    await store.setBackground(file)
    await nextTick()

    expect(storage.saveBackground).toHaveBeenCalledWith(file)
    expect(store.backgroundUrl.value).toBe('blob:bg-1')
    expect(document.documentElement.getAttribute('data-backdrop')).toBe('image')
  })

  it('restores the default when the background is removed', async () => {
    const { store, nextTick } = await loadStore()
    await store.setBackground(imageFile())
    await store.removeBackground()
    await nextTick()

    expect(storage.deleteBackground).toHaveBeenCalled()
    expect(store.backgroundUrl.value).toBeNull()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:bg-1')
    expect(document.documentElement.hasAttribute('data-backdrop')).toBe(false)
  })

  it('rejects files that are not usable images', async () => {
    const { store } = await loadStore()

    await expect(store.setBackground(imageFile(16, 'text/plain'))).rejects.toThrow('请选择图片文件')
    await expect(store.setBackground(imageFile(50 * 1024 * 1024 + 1))).rejects.toThrow('图片不能超过 50MB')
    vi.mocked(createImageBitmap).mockRejectedValueOnce(new Error('broken'))
    await expect(store.setBackground(imageFile())).rejects.toThrow('无法读取这张图片')

    expect(storage.saveBackground).not.toHaveBeenCalled()
    expect(store.backgroundUrl.value).toBeNull()
  })

  it('samples the wallpaper for its tone and forgets it on removal', async () => {
    const { store } = await loadStore()
    const file = imageFile()
    await store.setBackground(file)
    await vi.waitFor(() => expect(store.backdropSample.value).not.toBeNull())

    expect(tone.sampleImage).toHaveBeenCalledWith(file)
    expect([...store.backdropSample.value!]).toEqual([10, 20, 30, 255])

    await store.removeBackground()
    expect(store.backdropSample.value).toBeNull()
  })

  it('ignores a sample that finishes after the background changed', async () => {
    let finishFirst: (data: Uint8ClampedArray) => void = () => {}
    tone.sampleImage.mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve }))
    const { store } = await loadStore()
    await store.setBackground(imageFile())
    await store.removeBackground()
    finishFirst(new Uint8ClampedArray([1, 2, 3, 255]))
    await Promise.resolve()

    expect(store.backdropSample.value).toBeNull()
  })

  it('loads the saved image on start', async () => {
    storage.loadBackground.mockResolvedValue(new Blob(['x'], { type: 'image/png' }))
    const { store, nextTick } = await loadStore()
    await store.init()
    await nextTick()

    expect(store.backgroundUrl.value).toBe('blob:bg-1')
    expect(document.documentElement.getAttribute('data-backdrop')).toBe('image')
  })
})
