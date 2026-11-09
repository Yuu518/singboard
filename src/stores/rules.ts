import { ref, computed } from 'vue'
import { fetchRuleProviders, fetchRules } from '@/api'
import type { Rule, RuleProvider } from '@/types'
import { getRequestErrorReason } from '@/utils/requestError'

const rules = ref<Rule[]>([])
const loading = ref(false)
const loaded = ref(false)
const loadError = ref('')
const filterText = ref('')
const ruleProviders = ref<RuleProvider[]>([])
const providersAvailable = computed(() => ruleProviders.value.length > 0)
let rulesRequest = 0
let providersRequest = 0

const filteredRules = computed(() => {
  const q = filterText.value.trim().toLowerCase()
  if (!q) return rules.value
  return rules.value.filter(
    (r) =>
      r.payload.toLowerCase().includes(q) ||
      r.proxy.toLowerCase().includes(q) ||
      r.type.toLowerCase().includes(q),
  )
})

export function useRulesStore() {
  async function loadRules(background = false): Promise<string> {
    const request = ++rulesRequest
    loading.value = true
    try {
      const { data } = await fetchRules()
      if (request !== rulesRequest) return ''
      rules.value = data.rules
      loadError.value = ''
      return ''
    } catch (error) {
      if (request !== rulesRequest) return ''
      const reason = getRequestErrorReason(error)
      if (!background || rules.value.length === 0) {
        rules.value = []
        loadError.value = reason
      }
      return reason
    } finally {
      if (request === rulesRequest) {
        loading.value = false
        loaded.value = true
      }
    }
  }

  async function loadProviders() {
    const request = ++providersRequest
    try {
      const { data } = await fetchRuleProviders()
      if (request === providersRequest) ruleProviders.value = Object.values(data?.providers ?? {})
    } catch {
      if (request === providersRequest) ruleProviders.value = []
    }
  }

  async function reloadAfterCoreReady() {
    await Promise.all([loadRules(), loadProviders()])
  }

  return {
    rules,
    filteredRules,
    loading,
    loaded,
    loadError,
    filterText,
    ruleProviders,
    providersAvailable,
    loadRules,
    loadProviders,
    reloadAfterCoreReady,
  }
}
