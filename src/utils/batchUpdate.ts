import { getRequestErrorReason } from './requestError'
import { useToastStore } from '@/stores/toast'

export async function batchUpdateProviders<T extends { name: string; vehicleType?: string }>(
  providers: T[],
  updateFn: (name: string) => Promise<unknown>,
  label: string,
): Promise<{ failed: number; total: number }> {
  const { pushToast } = useToastStore()
  const updatable = providers.filter((p) => p.vehicleType !== 'Inline')
  const results = await Promise.allSettled(updatable.map((p) => updateFn(p.name)))
  const failed = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected')
  if (failed.length > 0) {
    const reasons = Array.from(new Set(failed.map((r) => getRequestErrorReason(r.reason))))
    pushToast({
      type: 'error',
      message: `部分${label}更新失败 (${failed.length}/${updatable.length})\n原因: ${reasons.join('；')}`,
    })
  }
  return { failed: failed.length, total: updatable.length }
}
