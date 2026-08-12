import { page } from '@vitest/browser/context'
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from 'vitest-browser-vue'

const LoadingButton = {
  template: `
    <div class="settings-page">
      <button class="btn btn-sm btn-route" disabled aria-busy="true">
        <span class="loading loading-spinner loading-xs settings-update-spinner" aria-hidden="true"></span>
        <span>检查中</span>
      </button>
    </div>
  `,
}

afterEach(cleanup)

describe('update spinner browser style', () => {
  it('keeps the spinner shadow and filter clear', () => {
    render(LoadingButton)
    const spinner = page.getByText('检查中').element().previousElementSibling as HTMLElement
    const style = getComputedStyle(spinner)

    expect(style.boxShadow).toBe('none')
    expect(style.filter).toBe('none')
  })
})
