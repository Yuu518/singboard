import { ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { IS_ANDROID } from '@/platform'

export const appVisible = ref(IS_ANDROID ? document.visibilityState !== 'hidden' : true)

if (IS_ANDROID) {
  document.addEventListener('visibilitychange', () => {
    appVisible.value = document.visibilityState !== 'hidden'
  })
} else {
  try {
    void getCurrentWindow().listen<boolean>('window-visibility', (event) => {
      appVisible.value = event.payload
    }).catch(() => {})
  } catch {}
}
