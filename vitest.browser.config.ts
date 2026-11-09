import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  define: {
    __ANDROID__: 'false',
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  test: {
    include: ['src/**/*.browser.test.ts'],
    exclude: ['src/android/**'],
    setupFiles: ['vitest-browser-vue', './src/test/browserSetup.ts'],
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
