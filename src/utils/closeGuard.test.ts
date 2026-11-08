import { describe, expect, it } from 'vitest'
import { confirmWindowClose, registerCloseGuard } from './closeGuard'

describe('window close guard', () => {
  it('blocks closing while any guard says no, and lets go once it unregisters', async () => {
    const unregister = registerCloseGuard(async () => false)
    expect(await confirmWindowClose()).toBe(false)
    unregister()
    expect(await confirmWindowClose()).toBe(true)
  })
})
