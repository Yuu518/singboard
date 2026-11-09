import { computed, inject, type ComputedRef, type InjectionKey } from 'vue'

export const PAGE_ACTIVE: InjectionKey<ComputedRef<boolean>> = Symbol('page-active')

const alwaysActive = computed(() => true)

export function usePageActive(): ComputedRef<boolean> {
  return inject(PAGE_ACTIVE, alwaysActive)
}
