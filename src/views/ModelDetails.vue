<script setup>
// 选中模型的详情面板：能力、上下文与最近一次调用的观测。
import { computed } from 'vue'

const props = defineProps({
  model: { type: Object, required: true },
  result: { type: Object, default: () => ({}) },
})

const lines = computed(() => {
  const r = props.result || {}
  const out = []
  if (r.chatOnly) out.push({ text: '已自动关闭工具调用；导入后仅支持普通对话。' })
  if (Number.isInteger(r.nativeAttempts) && r.nativeAttempts > 0)
    out.push({ text: `最近一次调用拦截了 ${r.nativeAttempts} 次本地执行尝试，动作必须由 WorkBuddy 执行。`, error: true })
  if (Number.isInteger(r.calls) && r.calls > 0) out.push({ text: `最近一次调用返回了 ${r.calls} 个动作。` })
  if (r.handoff) out.push({ text: `最近一次调用把被拦下的本地动作转交成外部 ${r.handoff} 调用。` })
  const repairs = Object.entries(r.repaired ?? {}).filter(([, v]) => v?.ok)
  if (repairs.length)
    out.push({ text: `这一轮由格式兜底救回：${repairs.map(([shape, v]) => `${shape === 'action' ? '动作转写' : '信封重排'}（${String(v.model).replace('opencode/', '')}）`).join('、')}。` })
  out.push({ text: `图片输入：${props.model.images ? '支持' : '不支持'}` })
  out.push({ text: `上下文：${props.model.context ?? '未声明'} · 输入上限：${props.model.input ?? '未单独声明'} · 输出上限：${props.model.output ?? '未声明'}` })
  const variants = Object.keys(props.model.variants || {})
  out.push({ text: props.model.reasoning
    ? `推理：支持 · ${variants.length ? '可选档位：' + variants.join(' / ') : '使用默认模式'}`
    : '推理：OpenCode 未声明支持' })
  if (r.error) out.push({ text: r.error, error: true })
  out.push({ text: '最近更新：' + (r.time ? new Date(r.time).toLocaleString() : '尚未检测') })
  return out
})
</script>

<template>
  <section class="details">
    <strong>{{ model.id }}</strong>
    <p v-for="(line, i) in lines" :key="i" :class="{ 'error-text': line.error }">{{ line.text }}</p>
  </section>
</template>

<style scoped>
.details {
  padding: 14px 15px;
  background: var(--row);
  border-radius: var(--radius-m);
  font-size: 12px;
  color: var(--muted);
  line-height: 1.8;
  max-height: 190px;
  overflow: auto;
  user-select: text;
  box-shadow: var(--shadow);
}
strong {
  display: block;
  font-family: ui-monospace, monospace;
  color: var(--text);
  overflow-wrap: anywhere;
  margin-bottom: 5px;
}
p { margin: 0; }
.error-text { color: var(--orange); overflow-wrap: anywhere; }
</style>
