export interface RemovedEntry<T> {
  key: string
  value: T
  index: number
}

export function removeEntry<T>(record: Record<string, T>, key: string): { next: Record<string, T>; removed: RemovedEntry<T> | null } {
  const entries = Object.entries(record)
  const index = entries.findIndex(([name]) => name === key)
  if (index < 0) return { next: record, removed: null }
  const [, value] = entries[index]
  entries.splice(index, 1)
  return { next: Object.fromEntries(entries), removed: { key, value, index } }
}

export function restoreEntry<T>(record: Record<string, T>, removed: RemovedEntry<T>): Record<string, T> {
  const entries = Object.entries(record).filter(([name]) => name !== removed.key)
  entries.splice(Math.min(removed.index, entries.length), 0, [removed.key, removed.value])
  return Object.fromEntries(entries)
}
