<script setup>
// 单个模型行：状态徽章（推理/图片/状态）与最近耗时。
import { computed } from 'vue'

const props = defineProps({
  model: { type: Object, required: true },
  result: { type: Object, default: () => ({}) },
  waiting: { type: Boolean, default: false },
  probing: { type: Boolean, default: false },
  inRequest: { type: Boolean, default: false },
  available: { type: Boolean, default: false },
  selected: { type: Boolean, default: false },
})
defineEmits(['toggle'])

const LABELS = { timeout: '检测超时', quota: '额度不足', rate_limit: '请求受限', access: '访问受限' }

const statusLabel = computed(() => {
  if (props.waiting) return props.probing ? '检测中' : '等待检测'
  if (props.inRequest) return '请求中'
  if (props.available) return props.result?.chatOnly ? '可用 · 仅对话' : '可用'
  return LABELS[props.result?.category] || (props.result?.ok === false ? '不可用' : '待检测')
})
const unavailable = computed(() => !props.waiting && !props.inRequest && !props.available && (props.result?.ok === false || !props.available))

const timing = computed(() => {
  const r = props.result
  if (!Number.isFinite(r?.durationMs)) return '响应耗时 —'
  const duration = r.durationMs < 1000 ? `${r.durationMs} ms` : `${(r.durationMs / 1000).toFixed(1)} 秒`
  return `最近${r.source === 'probe' ? '检测' : '调用'} · ${r.ok ? '响应' : '失败'}耗时 ${duration}`
})
</script>

<template>
  <button class="model" :class="{ selected }" :aria-expanded="String(selected)" @click="$emit('toggle')">
    <span class="model-icon" aria-hidden="true">
      <span v-if="waiting" class="spinner" />
      <span v-else>◇</span>
    </span>
    <span class="model-info">
      <span class="model-name">OC · {{ model.name }}</span>
      <span class="duration">{{ timing }}</span>
    </span>
    <span class="badges">
      <span v-if="model.reasoning" class="badge reasoning">推理</span>
      <span v-if="model.images" class="badge images">图片</span>
      <span class="badge" :class="{ unavailable: unavailable && !available, waiting: waiting || inRequest }">{{ statusLabel }}</span>
    </span>
  </button>
</template>

<style scoped>
.model {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 13px 14px;
  border: 1px solid transparent;
  background: var(--row);
  border-radius: var(--radius-m);
  text-align: left;
  min-height: 69px;
  width: 100%;
  white-space: normal;
  transition: background .15s, border-color .15s;
}
.model:hover:not(.selected) { background: color-mix(in srgb, var(--row) 82%, var(--green) 18%); }
.model.selected { background: var(--green-bg); border-color: var(--green); }
.model-icon { font-size: 24px; color: var(--green); width: 26px; flex-shrink: 0; text-align: center; }
.model-info { min-width: 0; flex: 1; display: block; }
.model-name { display: block; font-size: 14px; font-weight: 550; line-height: 1.4; }
.duration { display: block; font-size: 11px; color: var(--muted); margin-top: 6px; font-variant-numeric: tabular-nums; }
.badges { display: flex; align-items: center; gap: 5px; flex-shrink: 0; flex-wrap: wrap; justify-content: flex-end; }
.badge { font-size: 11px; white-space: nowrap; padding: 4px 8px; border-radius: 6px; background: var(--green-bg); color: var(--green); }
.reasoning { color: #815a99; background: #eee8f3; }
.images { color: #477bad; background: #e7eff9; }
.unavailable { color: var(--orange); background: var(--orange-bg); }
.waiting { color: var(--muted); background: var(--panel); }

@media (prefers-color-scheme: dark) {
  .reasoning { background: #403249; color: #d4b5e8; }
  .images { background: #293e52; color: #a4c9ec; }
  .unavailable { background: var(--orange-bg); }
}
</style>
