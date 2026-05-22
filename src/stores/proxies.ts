import { ref, computed } from 'vue'
import { fetchProxies, fetchProxy, fetchProxyProviders, selectProxy, testLatency, testGroupLatency } from '@/api'
import { useConfigStore } from '@/stores/config'
import type { LatencyHistory, Proxy, ProxyGroup, ProxyProvider } from '@/types'

const proxyMap = ref<Record<string, Proxy>>({})
const providerList = ref<ProxyProvider[]>([])
const loading = ref(false)

function loadIPv6Map(): Record<string, boolean> {
  try {
    const stored = localStorage.getItem('singboard/ipv6-map')
    return stored ? JSON.parse(stored) : {}
  } catch {
    return {}
  }
}
function saveIPv6Map() {
  localStorage.setItem('singboard/ipv6-map', JSON.stringify(ipv6Map.value))
}
const ipv6Map = ref<Record<string, boolean>>(loadIPv6Map())

let fetchTime = 0
const LATENCY_TIMEOUT = 5000
const IPV6_TEST_URL = 'https://ipv6.google.com/generate_204'
const IPV6_TEST_TIMEOUT = 2000
const MAX_CONCURRENT = 8
const LATENCY_HISTORY_KEY = 'singboard/proxy-latency-history'

function getLastHistoryTime(history?: LatencyHistory[]): string {
  if (!history?.length) return ''
  return history[history.length - 1].time
}

function isTimeAfter(left: string, right: string): boolean {
  if (!right) return !!left
  if (!left) return false
  const leftTs = Date.parse(left)
  const rightTs = Date.parse(right)
  const leftValid = Number.isFinite(leftTs)
  const rightValid = Number.isFinite(rightTs)
  if (leftValid && rightValid) return leftTs > rightTs
  return left > right
}

function pickNewerHistory(a?: LatencyHistory[], b?: LatencyHistory[]): LatencyHistory[] {
  if (!a?.length) return b ?? []
  if (!b?.length) return a
  return isTimeAfter(getLastHistoryTime(a), getLastHistoryTime(b)) ? a : b
}

function loadLatencyHistoryMap(): Record<string, LatencyHistory[]> {
  try {
    const stored = localStorage.getItem(LATENCY_HISTORY_KEY)
    if (!stored) return {}
    const parsed = JSON.parse(stored)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}

    const map: Record<string, LatencyHistory[]> = {}
    for (const [name, rawHistory] of Object.entries(parsed as Record<string, unknown>)) {
      if (!Array.isArray(rawHistory)) continue
      const history = rawHistory.filter(
        (entry): entry is LatencyHistory =>
          !!entry
          && typeof entry === 'object'
          && typeof (entry as { time?: unknown }).time === 'string'
          && typeof (entry as { delay?: unknown }).delay === 'number'
      )
      if (history.length > 0) {
        map[name] = history
      }
    }
    return map
  } catch {
    return {}
  }
}

function saveLatencyHistoryMap(map: Record<string, LatencyHistory[]>) {
  const cleaned: Record<string, LatencyHistory[]> = {}
  for (const [name, history] of Object.entries(map)) {
    if (history?.length) {
      cleaned[name] = history
    }
  }

  if (Object.keys(cleaned).length === 0) {
    localStorage.removeItem(LATENCY_HISTORY_KEY)
    return
  }

  localStorage.setItem(LATENCY_HISTORY_KEY, JSON.stringify(cleaned))
}

function snapshotLatencyHistoryMap(): Record<string, LatencyHistory[]> {
  const map: Record<string, LatencyHistory[]> = {}
  for (const [name, proxy] of Object.entries(proxyMap.value)) {
    if (proxy.history?.length) {
      map[name] = proxy.history
    }
  }
  return map
}

let persistTimer: ReturnType<typeof setTimeout> | null = null

function persistLatencyHistoryMap() {
  if (persistTimer) return
  persistTimer = setTimeout(() => {
    persistTimer = null
    saveLatencyHistoryMap(snapshotLatencyHistoryMap())
  }, 500)
}

function flushLatencyHistoryMap() {
  if (persistTimer) {
    clearTimeout(persistTimer)
    persistTimer = null
  }
  saveLatencyHistoryMap(snapshotLatencyHistoryMap())
}

function appendProxyHistory(name: string, delay: number, time = new Date().toISOString()) {
  const proxy = proxyMap.value[name]
  if (!proxy) return
  if (!proxy.history) {
    proxy.history = []
  }
  proxy.history.push({ time, delay })
  if (proxy.history.length > 10) {
    proxy.history = proxy.history.slice(-10)
  }
  persistLatencyHistoryMap()
}

function loadPendingQueue(): string[] {
  try {
    const stored = localStorage.getItem('singboard/pending-test-queue')
    return stored ? JSON.parse(stored) : []
  } catch {
    return []
  }
}
function savePendingQueue(queue: string[]) {
  if (queue.length === 0) {
    localStorage.removeItem('singboard/pending-test-queue')
  } else {
    localStorage.setItem('singboard/pending-test-queue', JSON.stringify(queue))
  }
}
function loadPendingIPv6Queue(): string[] {
  try {
    const stored = localStorage.getItem('singboard/pending-ipv6-queue')
    return stored ? JSON.parse(stored) : []
  } catch {
    return []
  }
}
function savePendingIPv6Queue(queue: string[]) {
  if (queue.length === 0) {
    localStorage.removeItem('singboard/pending-ipv6-queue')
  } else {
    localStorage.setItem('singboard/pending-ipv6-queue', JSON.stringify(queue))
  }
}

const proxyGroups = computed<ProxyGroup[]>(() => {
  const groups: ProxyGroup[] = []
  for (const proxy of Object.values(proxyMap.value)) {
    if (proxy.all && proxy.all.length > 0) {
      groups.push(proxy as ProxyGroup)
    }
  }
  return groups
})

export function useProxiesStore() {
  function getTestUrl(groupName?: string): string {
    const { config } = useConfigStore()
    const defaultUrl = config.value.latencyTestUrl || 'https://www.gstatic.com/generate_204'
    if (!groupName) return defaultUrl

    const groupTestUrl = config.value.groupTestUrls[groupName]
    if (groupTestUrl) return groupTestUrl

    const proxyNode = proxyMap.value[groupName]
      || providerList.value.find(p => p.name === groupName)
    return proxyNode?.testUrl || defaultUrl
  }

  async function loadProxies(fresh = false) {
    loading.value = true
    const nowTime = Date.now()
    fetchTime = nowTime
    const previousMap = fresh ? {} : proxyMap.value
    const storedLatencyHistoryMap = fresh ? {} : loadLatencyHistoryMap()
    try {
      const [{ data }, providersRes] = await Promise.all([
        fetchProxies(),
        fetchProxyProviders().catch(() => null),
      ])

      if (fetchTime !== nowTime) return

      const providerProxies: Record<string, Proxy> = {}
      if (providersRes?.data?.providers) {
        const providers = Object.values(providersRes.data.providers).filter(
          (p) => p.name !== 'default' && p.vehicleType !== 'Compatible'
        )
        providerList.value = providers
        for (const provider of providers) {
          for (const proxy of provider.proxies) {
            providerProxies[proxy.name] = proxy
          }
        }
      } else {
        providerList.value = []
      }

      const merged = { ...data.proxies }
      for (const [name, providerProxy] of Object.entries(providerProxies)) {
        const existing = merged[name]
        if (existing) {
          merged[name] = {
            ...existing,
            ...providerProxy,
            history: pickNewerHistory(existing.history, providerProxy.history),
          }
        } else {
          merged[name] = providerProxy
        }
      }

      if (!fresh) {
        for (const [name, proxy] of Object.entries(merged)) {
          const previous = previousMap[name]
          const storedHistory = storedLatencyHistoryMap[name]
          if (!previous && !storedHistory) continue

          const localHistory = pickNewerHistory(previous?.history, storedHistory)
          merged[name] = {
            ...proxy,
            history: pickNewerHistory(proxy.history, localHistory),
          }
        }
      }
      proxyMap.value = merged
      if (fresh) {
        flushLatencyHistoryMap()
      } else {
        persistLatencyHistoryMap()
      }

      for (const [name, proxy] of Object.entries(proxyMap.value)) {
        const ipv6History = proxy.extra?.[IPV6_TEST_URL]?.history
        if (ipv6History?.length) {
          ipv6Map.value[name] = ipv6History[ipv6History.length - 1].delay > 0
        }
      }
      saveIPv6Map()
    } catch {
      if (fetchTime !== nowTime) return
      proxyMap.value = {}
    } finally {
      if (fetchTime === nowTime) {
        loading.value = false
      }
    }
  }

  async function switchProxy(group: string, name: string) {
    await selectProxy(group, name)
    if (proxyMap.value[group]) {
      proxyMap.value[group].now = name
    }
  }

  async function testNodeIPv6(name: string): Promise<boolean> {
    const nowNode = resolveNowNodeName(name)
    try {
      const { data } = await testLatency(nowNode, IPV6_TEST_URL, IPV6_TEST_TIMEOUT)
      const ok = data.delay > 0
      ipv6Map.value[nowNode] = ok
      saveIPv6Map()
      return ok
    } catch {
      ipv6Map.value[nowNode] = false
      saveIPv6Map()
      return false
    }
  }

  async function testNodeLatency(name: string, groupName?: string): Promise<number> {
    const { config } = useConfigStore()

    if (config.value.ipv6TestEnabled) {
      void testNodeIPv6(name)
    }

    try {
      const { data } = await testLatency(name, getTestUrl(groupName), LATENCY_TIMEOUT)
      const delay = data.delay
      appendProxyHistory(name, delay)
      return delay
    } catch {
      appendProxyHistory(name, 0)
      return 0
    }
  }

  async function testGroupNodes(groupName: string) {
    const { config } = useConfigStore()
    const group = proxyMap.value[groupName]
    if (!group?.all) return

    const type = group.type.toLowerCase()
    const url = getTestUrl(groupName)

    if (['selector', 'loadbalance', 'smart'].includes(type)) {
      const nodes = [...group.all]
      const run = async () => {
        while (nodes.length > 0) {
          const name = nodes.shift()!
          const now = new Date().toISOString()
          try {
            const { data } = await testLatency(name, url, LATENCY_TIMEOUT)
            appendProxyHistory(name, data.delay, now)
          } catch {
            appendProxyHistory(name, 0, now)
          }
          if (config.value.ipv6TestEnabled) {
            try {
              const { data } = await testLatency(name, IPV6_TEST_URL, IPV6_TEST_TIMEOUT)
              ipv6Map.value[resolveNowNodeName(name)] = data.delay > 0
            } catch {
              ipv6Map.value[resolveNowNodeName(name)] = false
            }
          }
        }
      }
      await Promise.all(Array.from({ length: Math.min(5, group.all.length) }, () => run()))
      if (config.value.ipv6TestEnabled) saveIPv6Map()
    } else {
      if (config.value.ipv6TestEnabled) {
        await testGroupLatency(groupName, IPV6_TEST_URL, IPV6_TEST_TIMEOUT)
          .then(({ data }) => {
            const results = data as Record<string, number>
            for (const [name, delay] of Object.entries(results)) {
              ipv6Map.value[resolveNowNodeName(name)] = delay > 0
            }
            saveIPv6Map()
          })
          .catch(() => {
            for (const name of group.all!) {
              ipv6Map.value[resolveNowNodeName(name)] = false
            }
            saveIPv6Map()
          })
      }
      const now = new Date().toISOString()
      try {
        const { data } = await testGroupLatency(groupName, url, Math.max(5000, LATENCY_TIMEOUT))
        const results = data as Record<string, number>
        const tested = new Set(Object.keys(results))
        for (const [name, delay] of Object.entries(results)) {
          appendProxyHistory(name, delay, now)
        }
        for (const name of group.all!) {
          if (!tested.has(name)) {
            appendProxyHistory(name, 0, now)
          }
        }
      } catch {
        for (const name of group.all!) {
          appendProxyHistory(name, 0, now)
        }
      }
    }

    flushLatencyHistoryMap()
    await loadProxies()
  }

  async function testAllNodes(names: string[]) {
    const queue = [...names]
    savePendingQueue(queue)
    const run = async () => {
      while (queue.length > 0) {
        const name = queue.shift()!
        savePendingQueue(queue)
        await testNodeLatency(name)
      }
    }
    const workers = Array.from({ length: Math.min(MAX_CONCURRENT, names.length) }, () => run())
    await Promise.all(workers)
    flushLatencyHistoryMap()
    savePendingQueue([])
  }

  async function testAllNodesIPv6(names: string[]) {
    const queue = [...names]
    savePendingIPv6Queue(queue)
    const run = async () => {
      while (queue.length > 0) {
        const name = queue.shift()!
        savePendingIPv6Queue(queue)
        await testNodeIPv6(name)
      }
    }
    const workers = Array.from({ length: Math.min(MAX_CONCURRENT, names.length) }, () => run())
    await Promise.all(workers)
    savePendingIPv6Queue([])
  }

  async function resumePendingTests() {
    const pendingQueue = loadPendingQueue()
    const pendingIPv6Queue = loadPendingIPv6Queue()
    const tasks: Promise<void>[] = []
    if (pendingQueue.length > 0) {
      tasks.push(testAllNodes(pendingQueue))
    }
    if (pendingIPv6Queue.length > 0) {
      tasks.push(testAllNodesIPv6(pendingIPv6Queue))
    }
    if (tasks.length > 0) {
      await Promise.all(tasks)
    }
  }

  function getProxy(name: string): Proxy | undefined {
    return proxyMap.value[name]
  }

  function getLatency(name: string): number {
    const resolved = resolveNowNodeName(name)
    const proxy = proxyMap.value[resolved]
    if (!proxy) return 0

    if (proxy.history?.length) {
      const last = proxy.history[proxy.history.length - 1]
      return last.delay
    }

    return 0
  }

  function resolveNowNodeName(name: string): string {
    let current = name
    const visited = new Set<string>()
    while (true) {
      if (visited.has(current)) return current
      visited.add(current)
      const proxy = proxyMap.value[current]
      if (!proxy?.now || proxy.now === current) return current
      current = proxy.now
    }
  }

  function isIPv6(name: string): boolean {
    const nowNode = resolveNowNodeName(name)
    return ipv6Map.value[nowNode] === true || ipv6Map.value[name] === true
  }

  async function pollAutoGroups() {
    const autoTypes = ['fallback', 'urltest']
    const autoGroups = proxyGroups.value.filter(g =>
      autoTypes.includes(g.type.toLowerCase())
    )

    for (const group of autoGroups) {
      try {
        const { data } = await fetchProxy(group.name)
        if (data.now && data.now !== proxyMap.value[group.name]?.now) {
          proxyMap.value[group.name].now = data.now
          testGroupNodes(group.name)
        }
      } catch {}
    }
  }

  return {
    proxyMap,
    proxyGroups,
    loading,
    loadProxies,
    switchProxy,
    pollAutoGroups,
    testNodeLatency,
    testGroupNodes,
    testAllNodes,
    testAllNodesIPv6,
    resumePendingTests,
    getProxy,
    getLatency,
    getTestUrl,
    isIPv6,
  }
}

