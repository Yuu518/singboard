import { page } from '@vitest/browser/context'
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from 'vitest-browser-vue'
import OverflowingText from './OverflowingText.vue'

afterEach(cleanup)

describe('OverflowingText browser behavior', () => {
  it('animates a long version only when it overflows', async () => {
    const container = document.createElement('div')
    container.style.width = '120px'
    document.body.append(container)

    render(OverflowingText, {
      container,
      props: { text: 'sing-box 1.14.0-beta.13-really-long-build-metadata.windows-amd64' },
    })

    await expect.element(page.getByTitle(/sing-box 1\.14\.0/)).toHaveClass(/is-overflowing/)
    const track = document.querySelector<HTMLElement>('.overflowing-text-track')!
    expect(getComputedStyle(track).animationName).toContain('overflowing-text-scroll')
    const startX = new DOMMatrixReadOnly(getComputedStyle(track).transform).m41
    await new Promise((resolve) => setTimeout(resolve, 120))
    const movedX = new DOMMatrixReadOnly(getComputedStyle(track).transform).m41
    expect(Math.abs(movedX - startX)).toBeGreaterThan(0.5)

    const viewport = page.getByTitle(/sing-box 1\.14\.0/)
    ;(viewport.element() as HTMLElement).focus()
    expect(getComputedStyle(track).animationName).toBe('none')
    expect(getComputedStyle(viewport.element()).overflowX).toBe('auto')
    expect((viewport.element() as HTMLElement).scrollWidth).toBeGreaterThan(
      (viewport.element() as HTMLElement).clientWidth,
    )

    container.remove()
  })

  it('does not animate a short version', async () => {
    const container = document.createElement('div')
    container.style.width = '240px'
    document.body.append(container)

    render(OverflowingText, { container, props: { text: '1.14.0' } })

    await expect.element(page.getByTitle('1.14.0')).not.toHaveClass(/is-overflowing/)
    const track = document.querySelector<HTMLElement>('.overflowing-text-track')!
    expect(getComputedStyle(track).animationName).toBe('none')

    container.remove()
  })
})
