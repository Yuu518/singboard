import { describe, expect, it } from 'vitest'
import { createApp, defineComponent, h, nextTick, ref } from 'vue'
import GlassTintSlider from './GlassTintSlider.vue'

function mount(initial: number) {
  const value = ref(initial)
  const commits: number[] = []
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(defineComponent({
    setup: () => () => h('div', [
      h('span', { id: 'glass-label' }, '玻璃效果'),
      h(GlassTintSlider, {
        modelValue: value.value,
        labelledby: 'glass-label',
        'onUpdate:modelValue': (next: number) => {
          commits.push(next)
          value.value = next
        },
      }),
    ]),
  }))
  app.mount(host)
  const input = host.querySelector<HTMLInputElement>('input[type="range"]')!
  return { value, commits, input, unmount: () => { app.unmount(); host.remove() } }
}

function drag(input: HTMLInputElement, to: number) {
  input.value = String(to)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

function release(input: HTMLInputElement) {
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

function press(input: HTMLInputElement, key: string) {
  input.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
}

const root = document.documentElement

describe('GlassTintSlider', () => {
  it('describes its value to assistive technology', async () => {
    const { input, unmount } = mount(0.5)
    expect(input.getAttribute('aria-labelledby')).toBe('glass-label')
    expect(input.getAttribute('aria-valuetext')).toBe('适中，50%')
    unmount()
  })

  it('previews while dragging and saves only when released', async () => {
    const { input, commits, unmount } = mount(1)
    drag(input, 0.3)
    await nextTick()
    expect(root.style.getPropertyValue('--glass-t')).toBe('0.3')
    expect(root.getAttribute('data-glass')).toBe('clear')
    expect(commits).toEqual([])
    release(input)
    expect(commits).toEqual([0.3])
    unmount()
  })

  it('snaps to the nearby mark while dragging', async () => {
    const { input, commits, unmount } = mount(1)
    drag(input, 0.47)
    await nextTick()
    expect(input.value).toBe('0.5')
    expect(input.getAttribute('aria-valuetext')).toBe('适中，50%')
    release(input)
    expect(commits).toEqual([0.5])
    unmount()
  })

  it('steps with the arrow keys and jumps to the ends with Home and End', async () => {
    const { input, commits, unmount } = mount(0.5)
    press(input, 'ArrowLeft')
    await nextTick()
    press(input, 'Home')
    await nextTick()
    press(input, 'End')
    await nextTick()
    expect(commits).toEqual([0.45, 0, 1])
    unmount()
  })

  it('puts the saved value back if it unmounts mid-drag', async () => {
    const { input, unmount } = mount(1)
    drag(input, 0.2)
    await nextTick()
    expect(root.style.getPropertyValue('--glass-t')).toBe('0.2')
    unmount()
    expect(root.style.getPropertyValue('--glass-t')).toBe('1')
    expect(root.getAttribute('data-glass')).toBe('blur')
  })
})
