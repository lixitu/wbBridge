<script setup>
// 指标卡：已发现 / 可用 / 待检测（与原版排序口径一致）。
import { computed } from 'vue'

const props = defineProps({
  models: { type: Array, default: () => [] },
  available: { type: Array, default: () => [] },
  probe: { type: Object, default: null },
})

const pending = computed(() => props.models.filter((m) => {
  const waiting = props.probe?.running && props.probe?.pending?.includes(m.id)
  if (waiting) return true
  return !props.available.includes(m.id)
}).length)
</script>

<template>
  <section class="toolbar">
    <div class="metrics">
      <div><b>{{ models.length }}</b><span>已发现</span></div>
      <div><b>{{ available.length }}</b><span>可用</span></div>
      <div><b>{{ pending }}</b><span>待检测</span></div>
    </div>
    <slot />
  </section>
</template>

<style scoped>
.toolbar { display: flex; align-items: center; justify-content: space-between; }
.metrics { display: flex; gap: 30px; }
.metrics div { display: flex; flex-direction: column; gap: 4px; }
.metrics b { font-size: 25px; font-weight: 600; font-variant-numeric: tabular-nums; }
.metrics span { font-size: 12px; color: var(--muted); }
</style>
