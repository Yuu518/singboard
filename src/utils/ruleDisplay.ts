export type RuleActionKind = 'route' | 'direct' | 'reject' | 'action'

export interface RuleAction {
  kind: RuleActionKind
  name: string
  outbound: string
  args: string[]
  raw: string
}

const ACTION_NAMES = new Set([
  'route',
  'route-options',
  'bypass',
  'direct',
  'reject',
  'hijack-dns',
  'sniff',
  'resolve',
  'predefined',
  'evaluate',
  'respond',
])

const ACTION_PATTERN = /^([a-z-]+)(?:\((.*)\))?$/s

export function parseRuleAction(raw: string): RuleAction {
  const text = raw.trim()
  const match = ACTION_PATTERN.exec(text)
  if (!match || !ACTION_NAMES.has(match[1])) {
    return { kind: 'route', name: 'route', outbound: text, args: [], raw: text }
  }
  const name = match[1]
  const args = match[2] ? match[2].split(',').filter(Boolean) : []
  if ((name === 'route' || name === 'bypass') && args.length > 0) {
    return { kind: 'route', name, outbound: args[0], args: args.slice(1), raw: text }
  }
  if (name === 'direct') return { kind: 'direct', name, outbound: '', args, raw: text }
  if (name === 'reject') return { kind: 'reject', name, outbound: '', args, raw: text }
  return { kind: 'action', name, outbound: '', args, raw: text }
}
