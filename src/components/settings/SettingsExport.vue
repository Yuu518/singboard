<script setup lang="ts">
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { IS_ANDROID } from '@/platform'
import { useConfigStore } from '@/stores/config'
import { writeSingboxConfig } from '@/bridge/config'

const { config } = useConfigStore()
const busy = ref(false)
const message = ref('')

async function exportSettings() {
  if (busy.value) return
  busy.value = true
  message.value = ''
  try {
    const settings = { ...config.value, githubToken: undefined }
    const content = JSON.stringify({ format: 'singboard-settings', version: 1, config: settings }, null, 2)
    if (IS_ANDROID) {
      const saved = await invoke<boolean>('export_native_settings', { content })
      if (!saved) return
    } else {
      const path = await save({ defaultPath: 'singboard-settings.json', filters: [{ name: 'JSON', extensions: ['json'] }] })
      if (!path) return
      await writeSingboxConfig(path, content)
    }
    message.value = '已导出设置'
  } catch (error) {
    message.value = String(error)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="settings-card">
    <button class="btn" :disabled="busy" @click="exportSettings">导出设置到原生版</button>
    <p>备份文件包含 API 密钥和订阅地址，请保存在私人位置。</p>
    <p v-if="message" role="status">{{ message }}</p>
  </div>
</template>
