import { computed, onBeforeUnmount, onMounted, onUpdated, ref, shallowRef, watch, type Ref } from 'vue'

export function buildOffsets(count: number, heightAt: (index: number) => number): Float64Array {
  const offsets = new Float64Array(count + 1)
  for (let i = 0; i < count; i++) offsets[i + 1] = offsets[i] + heightAt(i)
  return offsets
}

function lastStartingAtOrBefore(offsets: Float64Array, y: number): number {
  let lo = 0
  let hi = offsets.length - 2
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1
    if (offsets[mid] <= y) lo = mid
    else hi = mid - 1
  }
  return Math.max(0, lo)
}

export function visibleRange(offsets: Float64Array, scrollTop: number, viewport: number, overscan: number) {
  const count = offsets.length - 1
  if (count <= 0) return { start: 0, end: 0 }
  const first = lastStartingAtOrBefore(offsets, scrollTop)
  const last = lastStartingAtOrBefore(offsets, scrollTop + Math.max(viewport, 1))
  return {
    start: Math.max(0, first - overscan),
    end: Math.min(count, last + 1 + overscan),
  }
}

export function useVirtualList<T>(options: {
  items: Ref<T[]>
  keyOf: (item: T) => string | number
  rowHeight: number
  overscan?: number
}) {
  const { items, keyOf, rowHeight } = options
  const overscan = options.overscan ?? 12
  const container = ref<HTMLElement | null>(null)
  const scrollTop = ref(0)
  const viewport = ref(0)
  const measured = shallowRef(new Map<string, number>())

  const offsets = computed(() => {
    const list = items.value
    const heights = measured.value
    return buildOffsets(list.length, (i) => heights.get(String(keyOf(list[i]))) ?? rowHeight)
  })
  const totalHeight = computed(() => offsets.value[offsets.value.length - 1])
  const range = computed(() => visibleRange(offsets.value, scrollTop.value, viewport.value, overscan))
  const offsetY = computed(() => offsets.value[range.value.start] ?? 0)
  const visibleItems = computed(() =>
    items.value.slice(range.value.start, range.value.end).map((item, i) => ({ item, index: range.value.start + i })),
  )

  function setHeight(rawKey: string | number, height: number) {
    const key = String(rawKey)
    const current = measured.value.get(key) ?? rowHeight
    if (Math.abs(current - height) < 0.5) return
    const next = new Map(measured.value)
    if (Math.abs(height - rowHeight) < 0.5) next.delete(key)
    else next.set(key, height)
    measured.value = next
  }

  function offsetOf(index: number): number {
    return offsets.value[Math.min(Math.max(index, 0), offsets.value.length - 1)]
  }

  function indexAt(y: number): number {
    return lastStartingAtOrBefore(offsets.value, y)
  }

  function syncScroll() {
    const el = container.value
    if (!el) return
    scrollTop.value = el.scrollTop
    viewport.value = el.clientHeight
  }

  let rowObserver: ResizeObserver | null = null
  let viewportObserver: ResizeObserver | null = null

  function observeRows() {
    const el = container.value
    if (!el || !rowObserver) return
    rowObserver.disconnect()
    el.querySelectorAll<HTMLElement>('[data-virtual-key]').forEach((row) => rowObserver?.observe(row))
  }

  onMounted(() => {
    if (typeof ResizeObserver !== 'undefined') {
      rowObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const row = entry.target as HTMLElement
          const key = row.dataset.virtualKey
          if (key !== undefined) setHeight(key, row.offsetHeight)
        }
      })
      viewportObserver = new ResizeObserver(syncScroll)
      if (container.value) viewportObserver.observe(container.value)
    }
    syncScroll()
    observeRows()
  })

  watch(container, (el, previous) => {
    if (previous) viewportObserver?.unobserve(previous)
    if (el) viewportObserver?.observe(el)
    syncScroll()
  }, { flush: 'post' })

  onUpdated(observeRows)

  onBeforeUnmount(() => {
    rowObserver?.disconnect()
    viewportObserver?.disconnect()
  })

  return {
    container,
    totalHeight,
    offsetY,
    visibleItems,
    syncScroll,
    offsetOf,
    indexAt,
    setHeight,
  }
}
