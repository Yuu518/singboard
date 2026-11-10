<script setup lang="ts">
import { computed, useId } from 'vue'
import { sparkPath, type SparkPoint } from '@/utils/sparkPath'

const WIDTH = 600
const HEIGHT = 100
const MID = 62
const FLOOR = 32 * 1024

const props = defineProps<{
  down: readonly SparkPoint[]
  up: readonly SparkPoint[]
  live: boolean
}>()

const id = useId()
const down = computed(() => sparkPath(props.down, { width: WIDTH, base: MID, extent: MID - 4, direction: 1, floor: FLOOR }))
const up = computed(() => sparkPath(props.up, { width: WIDTH, base: MID, extent: HEIGHT - MID - 3, direction: -1, floor: FLOOR }))
const drawn = computed(() => props.live && !!down.value.line)
const head = computed(() => {
  const last = down.value.last
  return drawn.value && last ? { top: `${(last.y / HEIGHT) * 100}%` } : null
})
</script>

<template>
  <div class="traffic-wave" aria-hidden="true" data-traffic-wave>
    <svg v-if="drawn" :viewBox="`0 0 ${WIDTH} ${HEIGHT}`" preserveAspectRatio="none" class="traffic-wave-svg">
      <defs>
        <linearGradient :id="`${id}-down`" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" class="stop-strong" />
          <stop offset="1" class="stop-faint" />
        </linearGradient>
        <linearGradient :id="`${id}-up`" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" class="stop-none" />
          <stop offset="1" class="stop-soft" />
        </linearGradient>
      </defs>
      <path :d="down.area" :fill="`url(#${id}-down)`" />
      <path v-if="up.area" :d="up.area" :fill="`url(#${id}-up)`" />
      <line x1="0" :x2="WIDTH" :y1="MID" :y2="MID" class="traffic-wave-axis" vector-effect="non-scaling-stroke" />
      <path :d="down.line" class="traffic-wave-down" vector-effect="non-scaling-stroke" />
      <path v-if="up.line" :d="up.line" class="traffic-wave-up" vector-effect="non-scaling-stroke" />
    </svg>
    <svg v-else :viewBox="`0 0 ${WIDTH} ${HEIGHT}`" preserveAspectRatio="none" class="traffic-wave-svg">
      <line x1="0" :x2="WIDTH" :y1="MID" :y2="MID" class="traffic-wave-idle" vector-effect="non-scaling-stroke" />
    </svg>
    <span v-if="head" class="traffic-wave-head" :style="head"></span>
  </div>
</template>

<style scoped>
.traffic-wave {
  position: relative;
  height: 92px;
}

.traffic-wave-svg {
  display: block;
  width: 100%;
  height: 100%;
  overflow: visible;
  -webkit-mask-image: linear-gradient(to right, transparent, #000 12%);
  mask-image: linear-gradient(to right, transparent, #000 12%);
}

.stop-strong {
  stop-color: rgb(var(--chart-ink-rgb));
  stop-opacity: 0.16;
}

.stop-faint {
  stop-color: rgb(var(--chart-ink-rgb));
  stop-opacity: 0.03;
}

.stop-none {
  stop-color: rgb(var(--chart-ink-rgb));
  stop-opacity: 0.02;
}

.stop-soft {
  stop-color: rgb(var(--chart-ink-rgb));
  stop-opacity: 0.08;
}

.traffic-wave-axis {
  stroke: rgb(var(--chart-ink-rgb) / 0.12);
  stroke-width: 1;
}

.traffic-wave-down {
  fill: none;
  stroke: rgb(var(--chart-ink-rgb) / 0.55);
  stroke-width: 1.5;
  stroke-linejoin: round;
}

.traffic-wave-up {
  fill: none;
  stroke: rgb(var(--chart-ink-rgb) / 0.3);
  stroke-width: 1.25;
  stroke-linejoin: round;
}

.traffic-wave-idle {
  stroke: rgb(var(--chart-ink-rgb) / 0.18);
  stroke-width: 1.25;
  stroke-dasharray: 3 5;
}

.traffic-wave-head {
  position: absolute;
  right: -3px;
  width: 6px;
  height: 6px;
  margin-top: -3px;
  border-radius: 9999px;
  background: rgb(var(--chart-ink-rgb) / 0.75);
}

@container overview (min-width: 640px) {
  .traffic-wave {
    height: 104px;
  }
}
</style>
