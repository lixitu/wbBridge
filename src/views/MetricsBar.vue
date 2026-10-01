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
      <div><b class="ok">{{ available.length }}</b><span>可用</span></div>
      <div><b :class="{ idle: !pending }">{{ pending }}</b><span>待检测</span></div>
    </div>
    <slot />
  </section>
</template>

<style scoped>
.toolbar { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); }
/* 统计条：一格一数，格间用极淡竖线分组，数字按语义着色，不靠颜色单独表意 */
.metrics {
  display: flex;
  align-items: stretch;
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  overflow: hidden;
}
.metrics div { display: flex; flex-direction: column; gap: 2px; padding: 8px 16px; }
.metrics div + div { border-left: 1px solid var(--line); }
.metrics b { font-size: 22px; font-weight: 600; font-variant-numeric: tabular-nums; line-height: 1.15; }
.metrics b.ok { color: var(--green); }
.metrics b.idle { color: var(--muted); }
.metrics span { font-size: 12px; color: var(--muted); }
</style>
