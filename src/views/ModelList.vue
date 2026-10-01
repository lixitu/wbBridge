<script setup>
// 模型列表：可用在前、不可用在后、检测中其次；行内徽章与耗时口径与原版一致。
import { computed } from 'vue'
import ModelRow from '../components/ModelRow.vue'

const props = defineProps({
  models: { type: Array, default: () => [] },
  results: { type: Object, default: () => ({}) },
  available: { type: Array, default: () => [] },
  probe: { type: Object, default: null },
  activity: { type: Array, default: () => [] },
})

const selected = defineModel('selected', { type: Object, default: null })

const waiting = (id) => props.probe?.running && props.probe?.pending?.includes(id)
const rank = (model) => {
  if (waiting(model.id)) return 1
  if (props.available.includes(model.id)) return 0
  return props.results[model.id]?.ok === false ? 2 : 1
}

const sorted = computed(() => [...props.models].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name)))
</script>

<template>
  <section class="models" aria-label="模型列表">
    <ModelRow
      v-for="model in sorted"
      :key="model.id"
      :model="model"
      :result="results[model.id] || {}"
      :waiting="waiting(model.id)"
      :probing="probe?.current === model.id"
      :in-request="!!activity?.some(a => a.model === model.id)"
      :available="available.includes(model.id)"
      :selected="selected?.id === model.id"
      @toggle="selected = selected?.id === model.id ? null : model"
    />
    <p v-if="!models.length" class="empty">正在安装或扫描模型，完成后将在这里显示。</p>
  </section>
</template>

<style scoped>
.models {
  overflow: auto;
  flex: 1;
  min-height: 110px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 2px;
}
.empty { margin: auto; color: var(--muted); font-size: 13px; }
</style>
