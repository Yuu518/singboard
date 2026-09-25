import { page, userEvent } from '@vitest/browser/context'
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from 'vitest-browser-vue'
import { defineComponent, ref } from 'vue'
import BackgroundEditorDialog from './BackgroundEditorDialog.vue'
import { canvasToBlob } from '@/utils/imageEdit'

type Editor = InstanceType<typeof BackgroundEditorDialog>

async function sampleImage(): Promise<Blob> {
  const canvas = document.createElement('canvas')
  canvas.width = 400
  canvas.height = 200
  const context = canvas.getContext('2d')!
  context.fillStyle = '#ff0000'
  context.fillRect(0, 0, 200, 200)
  context.fillStyle = '#0000ff'
  context.fillRect(200, 0, 200, 200)
  return canvasToBlob(canvas, 'image/png')
}

function mountEditor() {
  const editor = ref<Editor | null>(null)
  const Host = defineComponent({
    components: { BackgroundEditorDialog },
    setup: () => ({ editor }),
    template: '<BackgroundEditorDialog ref="editor" />',
  })
  render(Host)
  return () => editor.value!
}

afterEach(cleanup)

describe('background editor dialog', () => {
  it('applies the edits only after confirming', async () => {
    const editor = mountEditor()
    const pending = editor().edit(await sampleImage())

    await page.getByRole('button', { name: '向右旋转' }).click()
    await page.getByRole('button', { name: '水平镜像' }).click()
    await page.getByRole('button', { name: '确认' }).click()

    const result = await pending
    expect(result?.type).toBe('image/webp')
    const bitmap = await createImageBitmap(result!)
    expect([bitmap.width, bitmap.height]).toEqual([200, 400])
    bitmap.close()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
  })

  it('returns nothing when cancelled', async () => {
    const editor = mountEditor()
    const pending = editor().edit(await sampleImage())

    await page.getByRole('button', { name: '取消' }).click()

    expect(await pending).toBeNull()
  })

  it('crops to the area dragged out with a handle', async () => {
    const editor = mountEditor()
    const pending = editor().edit(await sampleImage())

    await expect.element(page.getByRole('dialog')).toBeVisible()
    const preview = document.querySelector<HTMLElement>('.editor-preview')!
    await expect.poll(() => preview.getBoundingClientRect().width).toBeGreaterThan(0)
    const { width, height } = preview.getBoundingClientRect()
    await userEvent.dragAndDrop(document.querySelector<HTMLElement>('.editor-handle-e')!, preview, {
      targetPosition: { x: width / 2, y: height / 2 },
    })
    await page.getByRole('button', { name: '确认' }).click()

    const bitmap = await createImageBitmap((await pending)!)
    expect(bitmap.width).toBeGreaterThan(180)
    expect(bitmap.width).toBeLessThan(220)
    expect(bitmap.height).toBe(200)
    bitmap.close()
  })

  it('locks the crop to the window shape', async () => {
    const editor = mountEditor()
    const pending = editor().edit(await sampleImage())

    await page.getByRole('radio', { name: '适配窗口' }).click()
    await page.getByRole('button', { name: '确认' }).click()

    const bitmap = await createImageBitmap((await pending)!)
    expect(bitmap.width / bitmap.height).toBeCloseTo(window.innerWidth / window.innerHeight, 1)
    bitmap.close()
  })
})
