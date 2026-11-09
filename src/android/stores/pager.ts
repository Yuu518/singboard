import { ref } from 'vue'
import type { Router } from 'vue-router'
import { navItems } from '@/android/components/layout/navItems'

export const pagerIndex = ref(0)
export const reselectRequest = ref(0)

export function requestReselect() {
  reselectRequest.value++
}

export function goToPage(router: Router, path: string) {
  const current = router.currentRoute.value.path
  if (path === current) return
  const start = navItems[0].path
  if (path === start) {
    if (router.options.history.state.back === start) router.back()
    else void router.replace(start)
    return
  }
  if (current === start) void router.push(path)
  else void router.replace(path)
}
