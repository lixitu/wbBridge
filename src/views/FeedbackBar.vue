<script setup>
// 操作反馈条：一条同时承载三类状态，避免「等待 / 成功 / 失败」各写一套提示。
//   pending 进行中 —— spinner + 中性底色，不提供关闭按钮（防止误判操作已结束）
//   error   失败   —— role=alert + assertive，必须显示原因原文
//   成功           —— role=status + polite
defineProps({
  text: { type: String, required: true },
  error: { type: Boolean, default: false },
  pending: { type: Boolean, default: false },
})
defineEmits(['dismiss'])
</script>

<template>
  <div
    class="feedback"
    :class="{ error, pending }"
    :role="error ? 'alert' : 'status'"
    :aria-live="error ? 'assertive' : 'polite'"
  >
    <span class="feedback-mark" aria-hidden="true">
      <span v-if="pending" class="spinner sm" />
      <svg v-else-if="error" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
        <circle cx="12" cy="12" r="8.4" />
        <path d="M12 7.6v5.6M12 16.4v.4" />
      </svg>
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M6.5 12.6l3.4 3.4 7.6-7.6" />
      </svg>
    </span>
    <span class="feedback-text">{{ text }}</span>
    <button
      v-if="!pending"
      type="button"
      class="feedback-close"
      title="关闭提示"
      aria-label="关闭提示"
      @click="$emit('dismiss')"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true">
        <path d="M7 7l10 10M17 7L7 17" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.feedback {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  font-size: 13px;
  line-height: 1.6;
  background: var(--green-bg);
  color: var(--green);
  padding: 10px 12px;
  border-radius: var(--radius-m);
  border-left: 3px solid currentColor;
  animation: feedback-in var(--dur-2) var(--ease-enter);
}
.feedback.error { color: var(--orange); background: var(--orange-bg); }
.feedback.pending { color: var(--muted-strong); background: var(--row); border-left-color: color-mix(in srgb, var(--muted) 55%, transparent); }
.feedback-mark { display: inline-flex; align-items: center; height: 20px; flex-shrink: 0; }
.feedback-mark svg { width: 15px; height: 15px; }
.feedback-text { flex: 1; min-width: 0; }
.feedback-close {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  background: transparent;
  border-color: transparent;
  border-radius: var(--radius-s);
  color: inherit;
  opacity: .7;
  transition: opacity var(--dur-1) var(--ease-standard), background var(--dur-1) var(--ease-standard);
}
.feedback-close svg { width: 13px; height: 13px; }
.feedback-close:hover:not(:disabled) {
  opacity: 1;
  background: color-mix(in srgb, currentColor 14%, transparent);
  border-color: transparent;
}
@keyframes feedback-in { from { opacity: 0; transform: translateY(-3px); } to { opacity: 1; transform: none; } }
</style>
