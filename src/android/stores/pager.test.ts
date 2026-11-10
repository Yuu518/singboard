import { describe, expect, it, vi } from 'vitest'
import type { Router } from 'vue-router'
import { goToPage } from './pager'

function fakeRouter(current: string, back: string | null = null) {
  const router = {
    currentRoute: { value: { path: current } },
    options: { history: { state: { back } } },
    push: vi.fn(() => Promise.resolve()),
    replace: vi.fn(() => Promise.resolve()),
    back: vi.fn(),
  }
  return { router, go: (path: string) => goToPage(router as unknown as Router, path) }
}

describe('page navigation history', () => {
  it('pushes when leaving the home page so back returns home', () => {
    const { router, go } = fakeRouter('/home')
    go('/logs')
    expect(router.push).toHaveBeenCalledWith('/logs')
    expect(router.replace).not.toHaveBeenCalled()
  })

  it('replaces between other pages so history never grows past one entry above home', () => {
    const { router, go } = fakeRouter('/logs', '/home')
    go('/rules')
    expect(router.replace).toHaveBeenCalledWith('/rules')
    expect(router.push).not.toHaveBeenCalled()
  })

  it('steps back to the home entry instead of stacking a new one', () => {
    const { router, go } = fakeRouter('/rules', '/home')
    go('/home')
    expect(router.back).toHaveBeenCalledOnce()
    expect(router.push).not.toHaveBeenCalled()
    expect(router.replace).not.toHaveBeenCalled()
  })

  it('replaces with home when the app opened on another page', () => {
    const { router, go } = fakeRouter('/proxies')
    go('/home')
    expect(router.replace).toHaveBeenCalledWith('/home')
    expect(router.back).not.toHaveBeenCalled()
  })

  it('ignores the page that is already showing', () => {
    const { router, go } = fakeRouter('/logs', '/home')
    go('/logs')
    expect(router.push).not.toHaveBeenCalled()
    expect(router.replace).not.toHaveBeenCalled()
    expect(router.back).not.toHaveBeenCalled()
  })
})

describe('configured start page', () => {
  async function loadPager(startPage: string) {
    vi.resetModules()
    localStorage.setItem('singboard-config', JSON.stringify({ startPage }))
    const pager = await import('./pager')
    localStorage.clear()
    return pager
  }

  it('treats the chosen start page as the page back returns to', async () => {
    const { launchPage, goToPage: go } = await loadPager('/proxies')
    expect(launchPage).toBe('/proxies')

    const fromStart = fakeRouter('/proxies')
    go(fromStart.router as unknown as Router, '/logs')
    expect(fromStart.router.push).toHaveBeenCalledWith('/logs')

    const backToStart = fakeRouter('/logs', '/proxies')
    go(backToStart.router as unknown as Router, '/proxies')
    expect(backToStart.router.back).toHaveBeenCalledOnce()

    const fromHome = fakeRouter('/home')
    go(fromHome.router as unknown as Router, '/rules')
    expect(fromHome.router.replace).toHaveBeenCalledWith('/rules')
  })

  it('opens on the home page when nothing or an unknown page was saved', async () => {
    expect((await loadPager('')).launchPage).toBe('/home')
    expect((await loadPager('/config')).launchPage).toBe('/home')
  })
})
