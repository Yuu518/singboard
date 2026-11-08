import { describe, expect, it } from 'vitest'
import { escapeHtml } from './html'
import { buildTopology, topologyHeight, topologyTooltip } from './topology'
import type { Connection } from '@/types'

const PAYLOAD = '<img src=x onerror="alert(1)">'

function conn(sourceIP: string, rule: string, chains: string[], rulePayload = ''): Connection {
  return { id: `${sourceIP}-${rule}-${chains.join('/')}`, metadata: { sourceIP }, rule, rulePayload, chains } as unknown as Connection
}

describe('escapeHtml', () => {
  it('escapes every character that can open markup or an attribute', () => {
    expect(escapeHtml(`a&b<c>d"e'f\`g`)).toBe('a&amp;b&lt;c&gt;d&quot;e&#39;f&#96;g')
  })

  it('renders nullish values as empty text', () => {
    expect(escapeHtml(undefined)).toBe('')
    expect(escapeHtml(null)).toBe('')
    expect(escapeHtml(3)).toBe('3')
  })
})

describe('buildTopology', () => {
  it('places each hop in its own column with depth, not layer', () => {
    const data = buildTopology([
      conn('10.0.0.2', 'DOMAIN-SUFFIX', ['HK 01', 'Proxy'], 'google.com'),
      conn('10.0.0.2', 'MATCH', ['DIRECT']),
    ])
    const byName = Object.fromEntries(data.nodes.map((n) => [n.name, n]))
    expect(byName['10.0.0.2'].depth).toBe(0)
    expect(byName['DOMAIN-SUFFIX: google.com'].depth).toBe(1)
    expect(byName.Proxy.depth).toBe(2)
    expect(byName['HK 01'].depth).toBe(3)
    expect(byName.DIRECT.depth).toBe(3)
    expect(data.nodes.every((n) => !('layer' in n))).toBe(true)
    expect(data.connectionCount).toBe(2)
  })

  it('keeps the same name in different columns apart so the graph never loops', () => {
    const data = buildTopology([conn('DIRECT', 'MATCH', ['DIRECT'])])
    const ids = data.nodes.map((n) => n.id)
    expect(new Set(ids).size).toBe(ids.length)
    expect(data.nodes.filter((n) => n.name === 'DIRECT').map((n) => n.depth)).toEqual([0, 3])
  })

  it('counts connections on nodes and links', () => {
    const data = buildTopology([
      conn('a', 'MATCH', ['DIRECT']),
      conn('a', 'MATCH', ['DIRECT']),
      conn('b', 'MATCH', ['DIRECT']),
    ])
    expect(data.nodes.find((n) => n.name === 'MATCH')?.count).toBe(3)
    const fromA = data.links.find((l) => l.source === data.nodes.find((n) => n.name === 'a')!.id)
    expect(fromA?.originalValue).toBe(2)
  })

  it('merges the quietest nodes of a crowded column into one other node', () => {
    const conns = Array.from({ length: 12 }, (_, i) => conn(`10.0.0.${i}`, 'MATCH', ['DIRECT']))
    conns.push(conn('10.0.0.0', 'MATCH', ['DIRECT']), conn('10.0.0.0', 'MATCH', ['DIRECT']))
    const data = buildTopology(conns, 4)
    const sources = data.nodes.filter((n) => n.depth === 0)
    expect(sources).toHaveLength(4)
    expect(sources.some((n) => n.name === '10.0.0.0')).toBe(true)
    const other = sources.find((n) => n.merged > 0)!
    expect(other.name).toBe('其他 9 个')
    expect(other.count).toBe(9)
    const otherLink = data.links.find((l) => l.source === other.id)!
    expect(otherLink.originalValue).toBe(9)
    expect(data.links.every((l) => data.nodes.some((n) => n.id === l.source) && data.nodes.some((n) => n.id === l.target))).toBe(true)
  })

  it('skips connections without a chain', () => {
    expect(buildTopology([conn('a', 'MATCH', [])]).nodes).toEqual([])
  })
})

describe('topologyHeight', () => {
  it('grows with the busiest column within limits', () => {
    const small = buildTopology([conn('a', 'MATCH', ['DIRECT'])])
    expect(topologyHeight(small)).toBe(160)
    const many = buildTopology(Array.from({ length: 30 }, (_, i) => conn(`s${i}`, `R${i}`, [`N${i}`])))
    expect(topologyHeight(many)).toBe(8 * 28 + 24)
    expect(topologyHeight(many, { max: 200 })).toBe(200)
  })
})

describe('topologyTooltip', () => {
  it('escapes node names and types taken from remote outbound tags', () => {
    const html = topologyTooltip({ dataType: 'node', data: { name: PAYLOAD, nodeType: '<b>x</b>', count: 3 } }, [])
    expect(html).not.toContain('<img')
    expect(html).not.toContain('<b>')
    expect(html).toContain('&lt;img src=x onerror=&quot;alert(1)&quot;&gt;')
    expect(html).toContain('<br/>类型: &lt;b&gt;x&lt;/b&gt;<br/>连接数: 3')
  })

  it('escapes both ends of an edge', () => {
    const nodes = [
      { id: 'r', name: 'MATCH: <script>' },
      { id: 'x', name: PAYLOAD },
    ]
    const html = topologyTooltip({ dataType: 'edge', data: { source: 'r', target: 'x', originalValue: 4, value: 6 } }, nodes)
    expect(html).toBe('MATCH: &lt;script&gt; → &lt;img src=x onerror=&quot;alert(1)&quot;&gt;<br/>连接数: 4')
  })

  it('falls back to the count when an edge end is unknown', () => {
    expect(topologyTooltip({ dataType: 'edge', data: { source: 'a', target: 'b', value: 2 } }, [])).toBe('连接数: 2')
  })

  it('labels a node without a type as unknown', () => {
    expect(topologyTooltip({ dataType: 'node', data: { name: 'direct' } }, [])).toBe('direct<br/>类型: 未知')
  })

  it('returns nothing for other targets', () => {
    expect(topologyTooltip({}, [])).toBe('')
  })
})
