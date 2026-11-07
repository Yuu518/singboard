import { vi } from 'vitest'
import { Storage } from 'happy-dom'

for (const name of ['localStorage', 'sessionStorage'] as const) {
  const storage = new Storage()
  for (const target of new Set([globalThis, window])) {
    Object.defineProperty(target, name, { configurable: true, value: storage })
  }
}

Object.defineProperty(window, 'matchMedia', {
  configurable: true,
  value: vi.fn((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})
