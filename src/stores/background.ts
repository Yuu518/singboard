import { computed, ref, shallowRef, watch } from 'vue'
import { deleteBackground, loadBackground, saveBackground } from '@/utils/backgroundStorage'
import { sampleImage } from '@/utils/imageTone'

export const MAX_BACKGROUND_BYTES = 50 * 1024 * 1024

const backgroundUrl = ref<string | null>(null)
const hasBackground = computed(() => backgroundUrl.value !== null)
const backdropSample = shallowRef<Uint8ClampedArray | null>(null)
let revision = 0
let initialized = false

function applyUrl(blob: Blob | null) {
  const previous = backgroundUrl.value
  backgroundUrl.value = blob ? URL.createObjectURL(blob) : null
  if (previous) URL.revokeObjectURL(previous)
  backdropSample.value = null
  if (!blob) return
  const current = revision
  sampleImage(blob)
    .then((data) => {
      if (current === revision) backdropSample.value = data
    })
    .catch(() => {})
}

watch(
  hasBackground,
  (active) => {
    if (active) document.documentElement.setAttribute('data-backdrop', 'image')
    else document.documentElement.removeAttribute('data-backdrop')
  },
  { immediate: true },
)

async function init() {
  if (initialized) return
  initialized = true
  const current = revision
  try {
    const blob = await loadBackground()
    if (current === revision) applyUrl(blob)
  } catch {
    if (current === revision) applyUrl(null)
  }
}

export function assertBackgroundFile(file: Blob) {
  if (!file.type.startsWith('image/')) throw new Error('请选择图片文件')
  if (file.size > MAX_BACKGROUND_BYTES) throw new Error('图片不能超过 50MB')
}

async function setBackground(file: Blob) {
  assertBackgroundFile(file)
  try {
    const bitmap = await createImageBitmap(file)
    bitmap.close()
  } catch {
    throw new Error('无法读取这张图片')
  }
  revision++
  try {
    await saveBackground(file)
  } catch {
    throw new Error('保存背景失败')
  }
  applyUrl(file)
}

async function removeBackground() {
  revision++
  try {
    await deleteBackground()
  } catch {
    throw new Error('删除背景失败')
  }
  applyUrl(null)
}

export function useBackgroundStore() {
  return {
    backgroundUrl,
    hasBackground,
    backdropSample,
    init,
    setBackground,
    removeBackground,
  }
}
