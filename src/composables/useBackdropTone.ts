import { watch } from 'vue'
import { useBackgroundStore } from '@/stores/background'
import { useConfigStore } from '@/stores/config'
import { backdropTone } from '@/utils/imageTone'

export function useBackdropTone() {
  const { backdropSample } = useBackgroundStore()
  const { resolvedTheme } = useConfigStore()

  watch(
    [backdropSample, resolvedTheme],
    ([sample, theme]) => {
      const root = document.documentElement
      if (sample) root.setAttribute('data-backdrop-tone', backdropTone(sample, theme === 'dark' ? 'dark' : 'light'))
      else root.removeAttribute('data-backdrop-tone')
    },
    { immediate: true },
  )
}
