<script setup>
// 服务状态行：阶段灯 + 活动文案（托盘/面板共用同一套诚实文案）。
import { computed } from 'vue'
import { activityText } from '../core/activity.js'

const props = defineProps({
  activity: { type: Array, default: () => [] },
  message: { type: String, default: '' },
  phase: { type: String, default: 'starting' },
  busy: { type: Boolean, default: false },
})
defineEmits(['restart'])

const text = computed(() => activityText(props.activity?.[0]) || props.message || '正在启动隔离模型服务')
const isError = computed(() => props.phase === 'error')
</script>

<template>
  <div class="service-status" :class="{ error: isError }" role="status" aria-live="polite">
    <span class="dot" :class="{ error: isError }" />
    <span class="status-text">{{ text }}</span>
    <!-- 重启期间按钮转 spinner 防连点；文案保持「重试」，动作本身没变 -->
    <button
      v-if="isError"
      :disabled="busy"
      :aria-busy="String(busy)"
      aria-label="重试核心服务"
      @click="$emit('restart')"
    >
      <span v-if="busy" class="spinner" />重试
    </button>
  </div>
</template>

<style scoped>
.service-status {
  padding: 12px 14px;
  border-radius: var(--radius-m);
  background: var(--green-bg);
  display: flex;
  align-items: center;
  gap: 9px;
  min-height: 42px;
  font-size: 13px;
  box-shadow: inset 0 0 0 1px var(--status-ring);
  transition: background var(--dur-2) var(--ease-standard), box-shadow var(--dur-2) var(--ease-standard);
}
/* 出错时整条状态条一起转橙，不留「绿灯背景 + 橙灯」的矛盾表达 */
.service-status.error {
  background: var(--orange-bg);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--orange) 28%, transparent);
}
/* 状态灯带同色光晕：运行时缓慢呼吸，出错后停住转橙 —— 静止画面也能分辨状态 */
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  background: var(--green);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--green) 18%, transparent);
  animation: pulse 2.4s ease-in-out infinite;
  transition: background var(--dur-2) var(--ease-standard), box-shadow var(--dur-2) var(--ease-standard);
}
.dot.error {
  background: var(--orange);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--orange) 20%, transparent);
  animation: none;
}
@keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: .55; } }
.status-text { flex: 1; min-width: 0; }
button { margin-left: auto; flex-shrink: 0; }
</style>
