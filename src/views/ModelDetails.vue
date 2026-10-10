<script setup>
// 选中模型的详情面板：常驻右侧分栏（非浮层），头部固定、正文区独立滚动。
import { computed } from 'vue'

const props = defineProps({
  model: { type: Object, required: true },
  result: { type: Object, default: () => ({}) },
})

defineEmits(['collapse'])

const lines = computed(() => {
  const r = props.result || {}
  const out = []
  if (r.chatOnly) out.push({ text: '已自动关闭工具调用；导入后仅支持普通对话。' })
  if (Number.isInteger(r.nativeAttempts) && r.nativeAttempts > 0)
    out.push({ text: `最近一次调用拦截了 ${r.nativeAttempts} 次本地执行尝试，动作必须由客户端（WorkBuddy / CodeBuddy）执行。`, error: true })
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
  <section class="details" aria-label="模型详情">
    <div class="details-head">
      <div class="details-title">
        <span class="details-kicker">模型详情</span>
        <strong>{{ model.id }}</strong>
      </div>
      <button
        type="button"
        class="collapse"
        title="收起详情（Esc）"
        aria-label="收起详情"
        @click="$emit('collapse')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M9.5 6l6 6-6 6" />
        </svg>收起
      </button>
    </div>
    <div class="details-body">
      <p v-for="(line, i) in lines" :key="i" :class="{ 'error-text': line.error }">{{ line.text }}</p>
    </div>
  </section>
</template>

<style scoped>
.details {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
  padding: 12px 14px 14px;
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  font-size: 12px;
  color: var(--muted-strong);
  line-height: 1.8;
  user-select: text;
  box-shadow: var(--shadow);
  animation: details-in var(--dur-2) var(--ease-enter) both;
}
/* 详情是右侧常驻分栏而非浮层：选中模型时从右轻推 + 淡入，
   把「面板是新出现的」这件事讲清楚，而不是让右列凭空刷新。 */
@keyframes details-in {
  from { opacity: 0; transform: translateX(8px); }
  to { opacity: 1; transform: none; }
}
.details-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--line);
  margin-bottom: 10px;
}
.details-title { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.details-kicker {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: .1em;
  color: var(--muted-strong);
}
strong {
  font-family: ui-monospace, monospace;
  font-size: 12.5px;
  color: var(--text);
  overflow-wrap: anywhere;
  min-width: 0;
}
.collapse {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 4px 9px;
  font-size: 12px;
  color: var(--muted-strong);
  background: transparent;
  border-color: var(--line);
  border-radius: 999px;
}
.collapse svg { width: 12px; height: 12px; }
.collapse:hover:not(:disabled) {
  background: var(--green-bg);
  border-color: color-mix(in srgb, var(--green) 40%, var(--line));
  color: var(--green);
}
.details-body { overflow: auto; min-height: 0; padding-right: 2px; }
p { margin: 0; }
p + p { margin-top: 7px; }
.error-text { color: var(--orange); font-weight: 500; overflow-wrap: anywhere; }
</style>
