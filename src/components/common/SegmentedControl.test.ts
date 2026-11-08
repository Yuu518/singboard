import { describe, expect, it } from 'vitest'
import { createApp, defineComponent, h, nextTick, ref } from 'vue'
import SegmentedControl from './SegmentedControl.vue'

const options = [
  { value: 'auto', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
] as const

function mount(initial: string) {
  const value = ref(initial)
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(defineComponent({
    setup: () => () => h(SegmentedControl, {
      modelValue: value.value,
      options,
      label: '界面主题',
      'onUpdate:modelValue': (next: string) => { value.value = next },
    }),
  }))
  app.mount(host)
  const radios = () => Array.from(host.querySelectorAll<HTMLButtonElement>('[role="radio"]'))
  return { value, radios, unmount: () => { app.unmount(); host.remove() } }
}

function press(target: HTMLElement, key: string) {
  target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
}

describe('SegmentedControl', () => {
  it('keeps one tab stop on the selected option', () => {
    const { radios, unmount } = mount('light')
    expect(radios().map((radio) => radio.tabIndex)).toEqual([-1, 0, -1])
    expect(radios()[1].getAttribute('aria-checked')).toBe('true')
    unmount()
  })

  it('selects and focuses with arrow keys, wrapping at the ends', async () => {
    const { value, radios, unmount } = mount('auto')
    press(radios()[0], 'ArrowRight')
    await nextTick()
    await nextTick()
    expect(value.value).toBe('light')
    expect(document.activeElement).toBe(radios()[1])

    press(radios()[1], 'End')
    await nextTick()
    expect(value.value).toBe('dark')
    press(radios()[2], 'ArrowRight')
    await nextTick()
    expect(value.value).toBe('auto')
    press(radios()[0], 'ArrowUp')
    await nextTick()
    expect(value.value).toBe('dark')
    expect(radios().map((radio) => radio.tabIndex)).toEqual([-1, -1, 0])
    unmount()
  })
})
