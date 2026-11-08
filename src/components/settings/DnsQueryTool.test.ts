import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick } from 'vue'

const dns = vi.hoisted(() => ({ queryDns: vi.fn() }))
vi.mock('@/api/dns', () => dns)
vi.mock('@/api/geoip', () => ({ getGeoIPForIP: vi.fn().mockResolvedValue({ country: 'US' }) }))

async function flush() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await nextTick()
  }
}

async function mountTool() {
  const { default: DnsQueryTool } = await import('./DnsQueryTool.vue')
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(DnsQueryTool)
  app.mount(host)
  await flush()
  return { host, unmount: () => { app.unmount(); host.remove() } }
}

function press(target: Element, key: string) {
  target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
}

beforeEach(() => {
  localStorage.clear()
  dns.queryDns.mockReset()
})

afterEach(() => {
  document.body.innerHTML = ''
})

describe('DnsQueryTool keyboard', () => {
  it('opens the record type list from the field and moves with arrow keys', async () => {
    const { host, unmount } = await mountTool()
    const input = host.querySelector<HTMLInputElement>('#settings-dns-type')!
    press(input, 'ArrowDown')
    await flush()

    const options = () => Array.from(host.querySelectorAll<HTMLElement>('[role="option"]'))
    expect(options().map((option) => option.textContent?.trim())).toEqual(['A', 'AAAA', 'CNAME'])
    expect(document.activeElement).toBe(options()[0])

    press(options()[0], 'ArrowDown')
    expect(document.activeElement).toBe(options()[1])
    press(options()[1], 'ArrowUp')
    press(options()[0], 'ArrowUp')
    expect(document.activeElement).toBe(options()[2])

    options()[2].click()
    await flush()
    expect(input.value).toBe('CNAME')
    expect(host.querySelector('[role="listbox"]')).toBeNull()
    expect(document.activeElement).toBe(input)
    unmount()
  })

  it('closes a list with Escape and returns focus to its field', async () => {
    const { host, unmount } = await mountTool()
    const input = host.querySelector<HTMLInputElement>('#settings-dns-server')!
    press(input, 'ArrowDown')
    await flush()
    const option = host.querySelector<HTMLElement>('[role="option"]')!
    press(option, 'Escape')
    await flush()
    expect(host.querySelector('[role="listbox"]')).toBeNull()
    expect(document.activeElement).toBe(input)
    unmount()
  })

  it('names history entries and lets them be removed from the keyboard', async () => {
    localStorage.setItem('singboard-dns-history', JSON.stringify([{ domain: 'a.test' }, { domain: 'b.test' }]))
    const { host, unmount } = await mountTool()
    const input = host.querySelector<HTMLInputElement>('#settings-dns-domain')!
    input.dispatchEvent(new Event('focus'))
    await flush()

    expect(host.querySelector('[aria-label="删除历史记录 a.test"]')).not.toBeNull()
    press(input, 'ArrowDown')
    await flush()
    const first = host.querySelector<HTMLElement>('[aria-label="查询 a.test"]')!
    expect(document.activeElement).toBe(first)
    press(first, 'Delete')
    await flush()

    expect(host.querySelector('[aria-label="查询 a.test"]')).toBeNull()
    expect(document.activeElement).toBe(host.querySelector('[aria-label="查询 b.test"]'))
    expect(host.querySelector('[role="status"]')?.textContent).toBe('已删除历史记录 a.test')
    unmount()
  })

  it('announces how many records a query returned', async () => {
    dns.queryDns.mockResolvedValue({
      source: 'system',
      server: '',
      records: [
        { name: 'example.com', record_type: 'A', ttl: 60, value: '93.184.216.34', ip: '93.184.216.34' },
        { name: 'example.com', record_type: 'A', ttl: 60, value: '93.184.216.35', ip: '93.184.216.35' },
      ],
    })
    const { host, unmount } = await mountTool()
    const input = host.querySelector<HTMLInputElement>('#settings-dns-domain')!
    input.value = 'example.com'
    input.dispatchEvent(new Event('input'))
    ;(host.querySelector('.settings-dns-submit') as HTMLButtonElement).click()
    await flush()
    expect(host.querySelector('[role="status"]')?.textContent).toBe('example.com 查询完成，共 2 条记录')
    unmount()
  })
})

describe('DnsQueryTool geo lookup', () => {
  it('keeps result IPs local until the user opts in', async () => {
    const { getGeoIPForIP } = await import('@/api/geoip')
    vi.mocked(getGeoIPForIP).mockClear()
    dns.queryDns.mockResolvedValue({
      source: 'system',
      server: '',
      records: [{ name: 'example.com', record_type: 'A', ttl: 60, value: '93.184.216.34', ip: '93.184.216.34' }],
    })
    const { host, unmount } = await mountTool()
    const input = host.querySelector<HTMLInputElement>('#settings-dns-domain')!
    input.value = 'example.com'
    input.dispatchEvent(new Event('input'))
    ;(host.querySelector('.settings-dns-submit') as HTMLButtonElement).click()
    await flush()
    expect(getGeoIPForIP).not.toHaveBeenCalled()
    expect(host.textContent).toContain('api.ip.sb')

    const toggle = host.querySelector<HTMLInputElement>('.settings-dns-geo input')!
    toggle.checked = true
    toggle.dispatchEvent(new Event('change'))
    await flush()
    expect(getGeoIPForIP).toHaveBeenCalledWith('93.184.216.34')
    expect(localStorage.getItem('singboard-dns-geo')).toBe('on')
    unmount()
  })

  it('ignores a slower earlier query once a newer one has answered', async () => {
    let slow!: (value: unknown) => void
    dns.queryDns
      .mockImplementationOnce(() => new Promise((resolve) => { slow = resolve }))
      .mockResolvedValueOnce({ source: 'system', server: '', records: [{ name: 'new.test', record_type: 'A', ttl: 60, value: '2.2.2.2' }] })
    const { host, unmount } = await mountTool()
    const input = host.querySelector<HTMLInputElement>('#settings-dns-domain')!
    input.value = 'old.test'
    input.dispatchEvent(new Event('input'))
    input.dispatchEvent(new KeyboardEvent('keyup', { key: 'Enter' }))
    input.value = 'new.test'
    input.dispatchEvent(new Event('input'))
    input.dispatchEvent(new KeyboardEvent('keyup', { key: 'Enter' }))
    await flush()
    slow({ source: 'system', server: '', records: [{ name: 'old.test', record_type: 'A', ttl: 60, value: '1.1.1.1' }] })
    await flush()
    expect(host.querySelector('.settings-dns-results')?.textContent).toContain('new.test')
    expect(host.querySelector('.settings-dns-results')?.textContent).not.toContain('old.test')
    unmount()
  })
})
