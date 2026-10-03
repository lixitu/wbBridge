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

// 行级 props 引用稳定：无结果的行共享同一个空对象（不随父级重渲染换引用），
// ModelRow 只在自身数据真正变化时更新，不被 usage 等无关字段的状态推送波及。
const EMPTY_RESULT = {}
const resultOf = (id) => props.results[id] || EMPTY_RESULT
// 请求中集合一次成 Set：逐行 some() 是 行数 × 活动数，且每趟渲染都重做。
const activeModels = computed(() => new Set((props.activity || []).map(a => a.model)))

const waiting = (id) => props.probe?.running && props.probe?.pending?.includes(id)
const rank = (model) => {
  if (waiting(model.id)) return 1
  if (props.available.includes(model.id)) return 0
  return props.results[model.id]?.ok === false ? 2 : 1
}

const sorted = computed(() => [...props.models].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name)))

// 键盘导航：↑ / ↓ 在列表内移动选中项（选中即展示详情；Enter / Space 沿用按钮原生行为）。
function move(event, step) {
  const rows = Array.from(event.currentTarget.querySelectorAll('[role="option"]'))
  if (!rows.length) return
  event.preventDefault()
  const current = rows.indexOf(document.activeElement)
  const from = current < 0 ? (step > 0 ? -1 : rows.length) : current
  const next = Math.min(rows.length - 1, Math.max(0, from + step))
  rows[next].focus()
  const model = sorted.value[next]
  if (model) selected.value = model
}
</script>

<template>
  <section
    class="models"
    role="listbox"
    aria-label="模型列表"
    @keydown.down="move($event, 1)"
    @keydown.up="move($event, -1)"
  >
    <ModelRow
      v-for="model in sorted"
      :key="model.id"
      :model="model"
      :result="resultOf(model.id)"
      :waiting="waiting(model.id)"
      :probing="probe?.current === model.id"
      :in-request="activeModels.has(model.id)"
      :available="available.includes(model.id)"
      :selected="selected?.id === model.id"
      @toggle="selected = selected?.id === model.id ? null : model"
    />
    <!-- 空列表：模型还没扫出来。配 spinner 明确「正在进行」，避免被读成「就是没有模型」。 -->
    <p v-if="!models.length" class="empty" role="status">
      <span class="spinner" aria-hidden="true" />正在安装或扫描模型，完成后将在这里显示。
    </p>
  </section>
</template>

<style scoped>
.models {
  overflow: auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 2px 4px 2px 2px;
}
/* 空态：虚线框居中 + spinner，读起来是「还在进行」而不是「就是没有」 */
.empty {
  margin: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border: 1px dashed var(--line);
  border-radius: var(--radius-m);
  color: var(--muted);
  font-size: 13px;
  line-height: 1.6;
}
.empty .spinner { margin-right: 0; color: var(--muted-strong); }
</style>
