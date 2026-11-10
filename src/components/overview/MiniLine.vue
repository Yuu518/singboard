<script setup lang="ts">
import { computed } from 'vue'
import { sparkPath, type SparkPoint } from '@/utils/sparkPath'

const WIDTH = 200
const HEIGHT = 30

const props = defineProps<{
  data: readonly SparkPoint[]
  floor: number
  live: boolean
}>()

const geometry = computed(() => sparkPath(props.data, { width: WIDTH, base: HEIGHT, extent: HEIGHT - 4, direction: 1, floor: props.floor }))
</script>

<template>
  <div class="mini-line" aria-hidden="true" data-mini-line>
    <svg v-if="live && geometry.line" :viewBox="`0 0 ${WIDTH} ${HEIGHT}`" preserveAspectRatio="none">
      <path :d="geometry.area" class="mini-line-area" />
      <path :d="geometry.line" class="mini-line-stroke" vector-effect="non-scaling-stroke" />
    </svg>
  </div>
</template>

<style scoped>
.mini-line {
  height: 30px;
}

.mini-line svg {
  display: block;
  width: 100%;
  height: 100%;
}

.mini-line-area {
  fill: rgb(var(--chart-ink-rgb) / 0.05);
}

.mini-line-stroke {
  fill: none;
  stroke: rgb(var(--chart-ink-rgb) / 0.38);
  stroke-width: 1.25;
  stroke-linejoin: round;
}
</style>
