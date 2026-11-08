import { describe, expect, it } from 'vitest'
import { removeEntry, restoreEntry } from './orderedRecord'

describe('ordered record entries', () => {
  it('restores a removed entry at its original position', () => {
    const record = { a: '1', b: '2', c: '3' }
    const { next, removed } = removeEntry(record, 'b')
    expect(Object.keys(next)).toEqual(['a', 'c'])
    expect(Object.keys(restoreEntry(next, removed!))).toEqual(['a', 'b', 'c'])
  })

  it('appends when the list shrank below the old position', () => {
    const { next, removed } = removeEntry({ a: '1', b: '2', c: '3' }, 'c')
    const { next: smaller } = removeEntry(next, 'b')
    expect(restoreEntry(smaller, removed!)).toEqual({ a: '1', c: '3' })
  })

  it('replaces a group that was mapped again in the meantime', () => {
    const { next, removed } = removeEntry({ a: '1', b: '2' }, 'a')
    const restored = restoreEntry({ ...next, a: 'new' }, removed!)
    expect(restored).toEqual({ a: '1', b: '2' })
    expect(Object.keys(restored)).toEqual(['a', 'b'])
  })

  it('reports nothing removed for an unknown key', () => {
    const record = { a: '1' }
    expect(removeEntry(record, 'x')).toEqual({ next: record, removed: null })
  })
})
