import { ref } from 'vue'
import type { Router } from 'vue-router'
import { resolveStartPage } from '@/android/components/layout/navItems'
import { useConfigStore } from '@/stores/config'

export const launchPage = resolveStartPage(useConfigStore().config.value.startPage)

export const pagerIndex = ref(0)
export const reselectRequest = ref(0)

export function requestReselect() {
  reselectRequest.value++
}

export function goToPage(router: Router, path: string) {
  const current = router.currentRoute.value.path
  if (path === current) return
  const start = launchPage
  if (path === start) {
    if (router.options.history.state.back === start) router.back()
    else void router.replace(start)
    return
  }
  if (current === start) void router.push(path)
  else void router.replace(path)
}
