<script setup>
// 根组件：布局壳 + 全局状态持有者。
// 状态经 src/core/bridge.js 订阅（与 Electron 版 window.buddy 契约一致），
// 下发给视图组件；跨组件动作也统一走 bridge.action()。
import { onMounted, onUnmounted, ref } from 'vue'
import { action, onState, onDismiss } from './core/bridge.js'
import SideBar from './views/SideBar.vue'
import ModelList from './views/ModelList.vue'
import ModelDetails from './views/ModelDetails.vue'
import ServiceStatus from './views/ServiceStatus.vue'
import MetricsBar from './views/MetricsBar.vue'
import FeedbackBar from './views/FeedbackBar.vue'

const state = ref({})
const selected = ref(null)
const busyAction = ref(false)
const feedback = ref(null) // { text, error }
let lastModels = []

function apply(next) {
  state.value = next || {}
}

async function run(name, value) {
  if (busyAction.value) return { ok: false, error: '已有操作进行中' }
  busyAction.value = true
  feedback.value = null
  try {
    const response = await action(name, value)
    if (!response.ok) throw new Error(response.error)
    if (name === 'import') {
      const r = response.result
      if (r.canceled) feedback.value = { text: '已取消导入，配置未更改。' }
      else if (r.changed === false) feedback.value = { text: `配置已是最新，共 ${r.count} 个模型，无需重复写入。` }
      else feedback.value = { text: `导入完成，已将 ${r.count} 个可用模型导入 WorkBuddy。` }
    }
    return response
  } catch (error) {
    feedback.value = { text: error.message, error: true }
    return { ok: false }
  } finally {
    busyAction.value = false
  }
}

function dismiss() {
  selected.value = null
}

onMounted(() => {
  onState(apply)
  onDismiss(dismiss)
})
</script>

<template>
  <div class="shell">
    <SideBar
      :proxy-on="state.useSystemProxy === true"
      :disabled="busyAction || state.probe?.running || !['ready', 'error'].includes(state.phase)"
      @toggle-proxy="run('system-proxy', $event)"
    />
    <main>
      <header>
        <div>
          <h2>免费模型</h2>
          <p class="subtitle">自动发现，保留每一个模型的状态。</p>
        </div>
        <div class="actions">
          <button id="refresh" :disabled="busyAction || !!state.probe?.running || state.phase !== 'ready'" @click="run('refresh')">
            <span v-if="busyAction === 'refresh'" class="spinner" />读取免费模型
          </button>
          <button id="probe" :disabled="busyAction || !!state.probe?.running || state.phase !== 'ready'" @click="run('probe')">
            <span v-if="state.probe?.running" class="spinner" />检测全部
          </button>
        </div>
      </header>

      <ServiceStatus
        :activity="state.activity"
        :message="state.message"
        :phase="state.phase"
        :busy="busyAction"
        @restart="run('restart')"
      />

      <MetricsBar
        :models="state.models || []"
        :available="state.availableModels || []"
        :probe="state.probe"
      >
        <button id="import" class="primary" :disabled="busyAction || !!state.probe?.running || state.phase !== 'ready'" @click="run('import')">
          <span v-if="busyAction" class="spinner" />导入 WorkBuddy
        </button>
      </MetricsBar>

      <FeedbackBar v-if="feedback" :error="feedback.error" :text="feedback.text" />

      <ModelList
        v-model:selected="selected"
        :models="state.models || []"
        :results="state.modelResults || {}"
        :available="state.availableModels || []"
        :probe="state.probe"
        :activity="state.activity"
      />

      <ModelDetails v-if="selected" :model="selected" :result="(state.modelResults || {})[selected.id] || {}" />
      <section v-else-if="(state.models || []).length" hidden />

      <footer>
        <p id="sync">{{ state.sync?.error || (state.sync?.time ? `已导入 ${state.sync.count ?? 0} 个模型 · 再次检测后需点击导入 WorkBuddy 更新` : '首次读取和检测完成后自动导入 WorkBuddy') }}</p>
        <p class="note">启动后自动发送简短请求检测，会使用少量免费额度，不代表工具流程已验证。耗时为完整请求用时，非首字延迟。不可用模型仅在本窗口保留，不供 WorkBuddy 使用；剩余额度暂不可查询。</p>
      </footer>
    </main>
  </div>
</template>

<style>
.shell { display: flex; height: 100vh; }
main { padding: 28px 30px; flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 16px; }
header { display: flex; justify-content: space-between; gap: 12px; align-items: center; }
h2 { font-size: 27px; letter-spacing: -.8px; margin: 0 0 6px; font-weight: 650; }
.subtitle { margin: 0; color: var(--muted); font-size: 13px; }
.actions { display: flex; gap: 8px; }
footer { font-size: 11px; color: var(--muted); line-height: 1.7; }
footer p { margin: 0; }
.note { margin-top: 6px !important; }
</style>
