<script setup>
// 运行日志：只读展示数据目录下核心日志的尾部。
// 面板不读文件也不拼路径：壳侧 read_log 自己定位数据目录下的日志文件，只回尾部内容。
import { onMounted, ref } from 'vue'
import { readLog } from '../core/bridge.js'

const log = ref(null) // { text, truncated, bytes }
const error = ref(null)
const loading = ref(false)

// 只做单位换算，不改变原始数值口径（bytes 是日志文件总字节数）。
function formatBytes(bytes) {
  const value = Number(bytes) || 0
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(2)} MB`
}

async function load() {
  loading.value = true
  error.value = null
  const response = await readLog()
  if (response.ok) log.value = response.result || {}
  else error.value = response.error
  loading.value = false
}

onMounted(load)
</script>

<template>
  <section class="view" aria-label="运行日志">
    <header>
      <div>
        <h2>运行日志</h2>
        <p class="subtitle">核心服务的运行记录，只读展示。</p>
      </div>
      <button id="reload-log" :disabled="loading" @click="load">
        <span v-if="loading" class="spinner" />刷新
      </button>
    </header>

    <p v-if="error" class="notice error-text">读取运行日志失败：{{ error }}</p>
    <template v-else>
      <p v-if="log?.text" class="meta">
        日志文件共 {{ formatBytes(log.bytes) }}，{{ log.truncated ? '内容超出上限，仅显示末尾一段' : '已显示全部内容' }}。
      </p>
      <pre v-if="log?.text" class="log">{{ log.text }}</pre>
      <p v-else-if="log" class="notice">
        日志为空：核心尚未写入任何内容（日志文件由核心启动时创建）。
      </p>
    </template>

    <p class="note">日志文件与 status.json 同在应用数据目录；面板不会修改日志，也不跟随任意路径。</p>
  </section>
</template>

<style scoped>
header { display: flex; justify-content: space-between; gap: var(--sp-3); align-items: center; }
.subtitle { margin: 0; color: var(--muted-strong); font-size: var(--fs-sm); }
.meta { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); }
.log {
  margin: 0;
  padding: var(--sp-3);
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  font-family: ui-monospace, monospace;
  font-size: var(--fs-xs);
  line-height: var(--lh-base);
  color: var(--text);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}
.notice { margin: 0; font-size: var(--fs-sm); color: var(--muted-strong); }
.error-text { color: var(--orange); overflow-wrap: anywhere; }
.note { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); line-height: var(--lh-loose); }
</style>
