import { afterEach, describe, expect, it, vi } from 'vitest'
import { render } from 'vitest-browser-vue'
import { defineComponent, h, type Component } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { usePageActive } from '@/android/composables/usePageActive'
import { keyboardVisible } from '@/android/stores/keyboard'
import { pagerIndex, requestReselect } from '@/android/stores/pager'
import { useConfigStore } from '@/stores/config'

function page(name: string): Component {
  return defineComponent({
    setup() {
      const active = usePageActive()
      return () => h('div', { 'data-page': name, 'data-active': String(active.value), style: 'height: 2000px' }, name)
    },
  })
}

vi.mock('./navItems', () => ({
  navItems: ['a', 'b', 'c'].map((name) => ({
    path: `/${name}`,
    label: name,
    icon: [],
    load: () => Promise.resolve(page(name)),
  })),
}))

const { default: MainPager } = await import('./MainPager.vue')

const WIDTH = 400

async function mountPager(start: string): Promise<{ router: Router; viewport: HTMLElement }> {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: ['/a', '/b', '/c'].map((path) => ({ path, component: { render: () => null } })),
  })
  await router.push(start)
  const Host = defineComponent({
    setup: () => () => h('div', { style: `width: ${WIDTH}px; height: 600px` }, [h(MainPager)]),
  })
  const screen = render(Host, { global: { plugins: [router] } })
  const viewport = screen.container.querySelector('main') as HTMLElement
  await expect.poll(() => viewport.querySelectorAll('[data-page]').length).toBe(3)
  return { router, viewport }
}

function activePages(viewport: HTMLElement): string[] {
  return [...viewport.querySelectorAll<HTMLElement>('[data-active="true"]')].map((el) => el.dataset.page!)
}

const { config } = useConfigStore()

afterEach(() => {
  document.body.innerHTML = ''
  config.value.pageSwipe = false
  keyboardVisible.value = false
  document.documentElement.style.removeProperty('--keyboard-inset')
})

describe('main pager', () => {
  it('opens on the routed page and keeps every page mounted', async () => {
    const { viewport } = await mountPager('/b')

    await expect.poll(() => viewport.scrollLeft).toBe(WIDTH)
    expect(pagerIndex.value).toBe(1)
    expect(activePages(viewport)).toEqual(['b'])
    const sections = viewport.querySelectorAll('section')
    expect(sections[0].hasAttribute('inert')).toBe(true)
    expect(sections[1].hasAttribute('inert')).toBe(false)
  })

  it('slides to a page when the route changes without remounting it', async () => {
    config.value.pageSwipe = true
    const { router, viewport } = await mountPager('/a')
    const pageC = viewport.querySelector('[data-page="c"]')

    await router.push('/c')

    await expect.poll(() => viewport.scrollLeft, { timeout: 3000 }).toBe(WIDTH * 2)
    await expect.poll(() => activePages(viewport)).toEqual(['c'])
    expect(viewport.querySelector('[data-page="c"]')).toBe(pageC)
  })

  it('updates the route after a swipe settles on another page', async () => {
    config.value.pageSwipe = true
    const { router, viewport } = await mountPager('/a')
    await expect.poll(() => pagerIndex.value).toBe(0)

    viewport.scrollTo({ left: WIDTH, behavior: 'instant' })

    await expect.poll(() => router.currentRoute.value.path).toBe('/b')
    expect(pagerIndex.value).toBe(1)
    await expect.poll(() => activePages(viewport)).toEqual(['b'])
  })

  it('locks horizontal swiping by default and jumps straight to the tapped page', async () => {
    const { router, viewport } = await mountPager('/a')
    expect(viewport.classList.contains('is-locked')).toBe(true)
    expect(getComputedStyle(viewport).overflowX).toBe('hidden')
    const pageC = viewport.querySelector('[data-page="c"]')

    await router.push('/c')

    expect(viewport.scrollLeft).toBe(WIDTH * 2)
    expect(activePages(viewport)).toEqual(['c'])
    expect(viewport.querySelector('[data-page="c"]')).toBe(pageC)
    expect(viewport.querySelectorAll('section')[2].classList.contains('is-entering')).toBe(true)
    await expect.poll(() => viewport.querySelectorAll('section')[2].classList.contains('is-entering')).toBe(false)
  })

  it('makes the pages clear the keyboard instead of the hidden nav while it is open', async () => {
    const { viewport } = await mountPager('/a')
    const section = viewport.querySelector('section') as HTMLElement
    const resting = getComputedStyle(section).paddingBottom

    document.documentElement.style.setProperty('--keyboard-inset', '300px')
    keyboardVisible.value = true
    await expect.poll(() => getComputedStyle(section).paddingBottom).toBe('316px')

    keyboardVisible.value = false
    await expect.poll(() => getComputedStyle(section).paddingBottom).toBe(resting)
  })

  it('scrolls the current page back to the top when its nav item is picked again', async () => {
    const { viewport } = await mountPager('/b')
    const section = viewport.querySelectorAll('section')[1] as HTMLElement
    section.scrollTo({ top: 600, behavior: 'instant' })
    expect(section.scrollTop).toBe(600)

    requestReselect()

    await expect.poll(() => section.scrollTop, { timeout: 3000 }).toBe(0)
  })

  it('unlocks swiping when the setting is turned on', async () => {
    const { viewport } = await mountPager('/a')
    config.value.pageSwipe = true
    await expect.poll(() => viewport.classList.contains('is-locked')).toBe(false)
    expect(getComputedStyle(viewport).overflowX).toBe('auto')
  })
})
