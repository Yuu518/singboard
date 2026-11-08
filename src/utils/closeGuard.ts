type Guard = () => Promise<boolean> | boolean

const guards = new Set<Guard>()

export function registerCloseGuard(guard: Guard): () => void {
  guards.add(guard)
  return () => guards.delete(guard)
}

export async function confirmWindowClose(): Promise<boolean> {
  for (const guard of [...guards]) {
    if (!(await guard())) return false
  }
  return true
}
