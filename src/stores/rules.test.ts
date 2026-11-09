import { afterEach, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({ fetchRules: vi.fn(), fetchRuleProviders: vi.fn() }))
vi.mock('@/api', () => api)
afterEach(() => { vi.resetAllMocks(); vi.resetModules() })

it('fills rules and providers once the core is ready after an early failed load', async () => {
  const { useRulesStore } = await import('./rules')
  const store = useRulesStore()

  api.fetchRules.mockRejectedValue(new Error('ECONNREFUSED'))
  api.fetchRuleProviders.mockRejectedValue(new Error('ECONNREFUSED'))
  await Promise.all([store.loadRules(), store.loadProviders()])
  expect(store.rules.value).toEqual([])
  expect(store.providersAvailable.value).toBe(false)

  const rule = { type: 'domain', payload: 'example.com', proxy: 'route(proxy)' }
  const provider = { name: 'geosite-cn', ruleCount: 1 }
  api.fetchRules.mockResolvedValue({ data: { rules: [rule] } })
  api.fetchRuleProviders.mockResolvedValue({ data: { providers: { [provider.name]: provider } } })
  await store.reloadAfterCoreReady()
  expect(store.rules.value).toEqual([rule])
  expect(store.ruleProviders.value).toEqual([provider])
  expect(store.providersAvailable.value).toBe(true)
  expect(store.loading.value).toBe(false)
})

it('ignores a stale failure that settles after a newer successful load', async () => {
  const { useRulesStore } = await import('./rules')
  const store = useRulesStore()

  let rejectStale!: (e: Error) => void
  api.fetchRules.mockReturnValueOnce(new Promise((_, reject) => { rejectStale = reject }))
  const stale = store.loadRules()

  const rule = { type: 'domain', payload: 'example.com', proxy: 'direct' }
  api.fetchRules.mockResolvedValueOnce({ data: { rules: [rule] } })
  await store.loadRules()

  rejectStale(new Error('ECONNREFUSED'))
  await stale
  expect(store.rules.value).toEqual([rule])
  expect(store.loading.value).toBe(false)
})
