<script setup lang="ts">
import { computed } from 'vue'
import { chainSpeeds } from '@/stores/connections'
import { formatSpeed } from '@/utils/format'

const props = defineProps<{ name: string }>()

const speed = computed(() => chainSpeeds.value[props.name] ?? { down: 0, up: 0 })
const idle = computed(() => speed.value.down === 0 && speed.value.up === 0)
</script>

<template>
  <div
    class="flex w-[13ch] flex-col items-end gap-0.5 pr-1 text-xs tabular-nums text-base-content/65"
    :class="{ invisible: idle }"
    :aria-hidden="idle"
  >
    <span>↓{{ formatSpeed(speed.down) }}</span>
    <span>↑{{ formatSpeed(speed.up) }}</span>
  </div>
</template>
