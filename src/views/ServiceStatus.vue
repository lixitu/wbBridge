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
  <div class="service-status">
    <span class="dot" :class="{ error: isError }" />
    <span>{{ text }}</span>
    <button v-if="isError" :disabled="busy" @click="$emit('restart')">重试</button>
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
  box-shadow: inset 0 0 0 1px rgb(38 110 92 / .06);
}
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--green); flex-shrink: 0; transition: background .2s; animation: pulse 2.4s ease-in-out infinite; }
.dot.error { background: var(--orange); animation: none; }
@keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: .55; } }
button { margin-left: auto; }
</style>
