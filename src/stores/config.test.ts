import { expect, it, vi } from 'vitest'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

it('drops the legacy custom service name when loading and saving settings', async () => {
  localStorage.setItem('singboard-config', JSON.stringify({ serviceName: 'custom-legacy-service', startupDelaySeconds: 45 }))
  const { useConfigStore } = await import('./config')
  const { config, updateConfig } = useConfigStore()
  expect(config.value).not.toHaveProperty('serviceName')
  expect(config.value.startupDelaySeconds).toBe(45)
  updateConfig({ startupDelaySeconds: 60 })
  await import('vue').then(({ nextTick }) => nextTick())
  expect(JSON.parse(localStorage.getItem('singboard-config')!)).not.toHaveProperty('serviceName')
  localStorage.clear()
})
