<script setup>
// 操作反馈条：导入结果或错误信息，出现即带淡入。
defineProps({
  text: { type: String, required: true },
  error: { type: Boolean, default: false },
})
</script>

<template>
  <div id="feedback" role="status" aria-live="polite" :class="{ error }">{{ text }}</div>
</template>

<style scoped>
#feedback {
  font-size: 13px;
  background: var(--green-bg);
  color: var(--green);
  padding: 11px 14px;
  border-radius: var(--radius-m);
  animation: fade-in .18s ease-out;
}
#feedback.error { color: var(--orange); background: var(--orange-bg); }
@keyframes fade-in { from { opacity: 0; transform: translateY(-2px); } to { opacity: 1; transform: none; } }
</style>
