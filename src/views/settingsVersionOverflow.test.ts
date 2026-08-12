import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { describe, expect, it } from 'vitest'
import OverflowingText from '@/components/common/OverflowingText.vue'

function setSize(element: Element, property: 'clientWidth' | 'scrollWidth', value: number) {
  Object.defineProperty(element, property, { configurable: true, value })
}

describe('settings core version overflow', () => {
  it('routes the full core version through an overflow-aware scrolling view', () => {
    const source = readFileSync(resolve(process.cwd(), 'src/views/SettingsPage.vue'), 'utf8')

    expect(source).toContain("import OverflowingText from '@/components/common/OverflowingText.vue'")
    expect(source).toMatch(/<OverflowingText\s+:text="singboxVersion \|\| '未检测'"/)
  })

  it('scrolls only when the full version is wider than its viewport', async () => {
    const version = 'sing-box 1.14.0-beta.13-really-long-build-metadata'
    const wrapper = mount(OverflowingText, { props: { text: version } })
    const viewport = wrapper.get('.overflowing-text')
    const sample = wrapper.get('.overflowing-text-item')
    setSize(viewport.element, 'clientWidth', 120)
    setSize(sample.element, 'scrollWidth', 320)

    ;(wrapper.vm as unknown as { measure: () => void }).measure()
    await nextTick()

    expect(viewport.classes()).toContain('is-overflowing')
    expect(viewport.attributes('title')).toBe(version)
    expect(viewport.attributes('tabindex')).toBe('0')
    expect(wrapper.findAll('.overflowing-text-item')).toHaveLength(2)

    setSize(sample.element, 'scrollWidth', 80)
    ;(wrapper.vm as unknown as { measure: () => void }).measure()
    await nextTick()

    expect(viewport.classes()).not.toContain('is-overflowing')
    expect(viewport.attributes('tabindex')).toBeUndefined()
    expect(wrapper.findAll('.overflowing-text-item')).toHaveLength(1)
  })
})
