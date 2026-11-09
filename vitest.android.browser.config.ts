import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  define: {
    __ANDROID__: 'true',
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  test: {
    include: ['src/android/**/*.browser.test.ts'],
    setupFiles: ['vitest-browser-vue', './src/android/test/browserSetup.ts'],
    browser: {
      enabled: true,
      provider: 'playwright',
      headless: true,
      instances: [{
        browser: 'chromium',
        launch: { channel: 'chrome' },
      }],
    },
  },
})
