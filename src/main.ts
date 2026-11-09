import { IS_ANDROID } from '@/platform'

if (IS_ANDROID) {
  void import('@/android/main')
} else {
  void import('./desktopMain')
}
