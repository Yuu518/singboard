import { computed, ref, watch } from 'vue'
import { useServiceStore } from '@/stores/service'

const UNKNOWN_GRACE_MS = 5000

export function useSettledServiceState() {
  const { serviceStatus } = useServiceStore()
  const serviceState = ref(serviceStatus.value.state)
  let unknownSince = 0

  watch(serviceStatus, (status) => {
    if (status.state !== 'unknown') {
      unknownSince = 0
      serviceState.value = status.state
      return
    }
    if (!unknownSince) unknownSince = Date.now()
    if (Date.now() - unknownSince >= UNKNOWN_GRACE_MS) serviceState.value = 'unknown'
  })

  const isRunning = computed(() => serviceState.value === 'running')

  return { serviceState, isRunning }
}
