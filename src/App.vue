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
import LogsView from './views/LogsView.vue'
import UsageView from './views/UsageView.vue'
import IntegrationView from './views/IntegrationView.vue'
import AboutView from './views/AboutView.vue'

const state = ref({})
const selected = ref(null)
// 当前视图：与 SideBar 的 item.id 一一对应（'models' | 'logs' | 'usage' | 'workbuddy' | 'about'）。
// 视图状态由根组件持有，侧栏只派发切换事件，避免两处各存一份选中态。
const view = ref('models')
// 进行中的动作名（null = 空闲）：模板按动作名点亮对应按钮的 spinner。
const busyAction = ref(null)
const feedback = ref(null) // { text, error }

function apply(next) {
  state.value = next || {}
}

async function run(name, value) {
  if (busyAction.value) return { ok: false, error: '已有操作进行中' }
  busyAction.value = name
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
    busyAction.value = null
  }
}

// 收起详情 = 清空选中：详情列随之消失，模型列表立刻占满可用宽度。
// Esc、详情面板右上角的「收起」按钮、Tauri 失焦事件，三者走同一条路径。
function dismiss() {
  selected.value = null
}

function onKeydown(event) {
  if (event.key !== 'Escape' || !selected.value) return
  event.preventDefault()
  dismiss()
}

onMounted(() => {
  onState(apply)
  onDismiss(dismiss)
  window.addEventListener('keydown', onKeydown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div class="shell">
    <SideBar
      :proxy-on="state.useSystemProxy === true"
      :disabled="!!busyAction || state.probe?.running || !['ready', 'error'].includes(state.phase)"
      :current="view"
      @select="view = $event"
      @toggle-proxy="run('system-proxy', { enabled: $event })"
    />
    <main>
      <div v-if="view === 'models'" class="top">
        <header>
          <div>
            <h2>免费模型</h2>
            <p class="subtitle">自动发现，保留每一个模型的状态。</p>
          </div>
          <div class="actions">
            <button id="refresh" :disabled="!!busyAction || !!state.probe?.running || state.phase !== 'ready'" @click="run('refresh')">
              <span v-if="busyAction === 'refresh'" class="spinner" />读取免费模型
            </button>
            <button id="probe" :disabled="!!busyAction || !!state.probe?.running || state.phase !== 'ready'" @click="run('probe')">
              <span v-if="state.probe?.running" class="spinner" />检测全部
            </button>
          </div>
        </header>

        <ServiceStatus
          :activity="state.activity"
          :message="state.message"
          :phase="state.phase"
          :busy="!!busyAction"
          @restart="run('restart')"
        />

        <MetricsBar
          :models="state.models || []"
          :available="state.availableModels || []"
          :probe="state.probe"
        >
          <button id="import" class="primary" :disabled="!!busyAction || !!state.probe?.running || state.phase !== 'ready'" @click="run('import')">
            <span v-if="busyAction" class="spinner" />导入 WorkBuddy
          </button>
        </MetricsBar>

        <FeedbackBar v-if="feedback" :error="feedback.error" :text="feedback.text" />
      </div>

      <!-- 主从两栏：详情是右侧常驻列（不是浮层/遮罩），任何窗口宽度都不降级为上下堆叠 -->
      <div v-if="view === 'models'" class="content" :class="{ 'is-split': !!selected }">
        <ModelList
          v-model:selected="selected"
          :models="state.models || []"
          :results="state.modelResults || {}"
          :available="state.availableModels || []"
          :probe="state.probe"
          :activity="state.activity"
        />

        <ModelDetails
          v-if="selected"
          :model="selected"
          :result="(state.modelResults || {})[selected.id] || {}"
          @collapse="dismiss"
        />
      </div>

      <footer v-if="view === 'models'">
        <p id="sync">{{ state.sync?.error || (state.sync?.time ? `已导入 ${state.sync.count ?? 0} 个模型 · 再次检测后需点击导入 WorkBuddy 更新` : '首次读取和检测完成后自动导入 WorkBuddy') }}</p>
        <p class="note">启动后自动发送简短请求检测，会使用少量免费额度，不代表工具流程已验证。耗时为完整请求用时，非首字延迟。不可用模型仅在本窗口保留，不供 WorkBuddy 使用；剩余额度暂不可查询。</p>
      </footer>

      <!-- 非「模型与服务」的视图：各自填满主区并独立滚动，不改变上面两栏布局的任何约束 -->
      <LogsView v-if="view === 'logs'" />
      <UsageView v-if="view === 'usage'" :usage="state.usage" />
      <!-- 该视图内的「导入 WorkBuddy」也走同一个 run()，反馈必须在本视图可见（只换位置，不复制状态） -->
      <FeedbackBar v-if="view === 'workbuddy' && feedback" :error="feedback.error" :text="feedback.text" />
      <IntegrationView
        v-if="view === 'workbuddy'"
        :state="state"
        :busy="!!busyAction"
        @import="run('import')"
      />
      <AboutView v-if="view === 'about'" :state="state" />
    </main>
  </div>
</template>

<style>
.shell { display: flex; height: 100dvh; overflow: hidden; }
main {
  padding: var(--sp-6) var(--sp-7);
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}
.top { flex-shrink: 0; display: flex; flex-direction: column; gap: var(--sp-4); }
/* 非模型视图（运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新）：填满主区并自行滚动，
   仍不引入任何宽度断点；.shell 的左右两段结构不受影响。 */
main > .view {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
  width: 100%;
  max-width: var(--content-max);
}
.content {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);
  gap: var(--sp-4);
  width: 100%;
  max-width: var(--content-max);
}
/* 选中模型时切主从两栏：列表占左列（自适应、自身滚动），详情为右列 300–360px。
   两列各自独立高度，详情不占列表的纵向空间；任何窗口宽度都不降级为上下堆叠。 */
.content.is-split { grid-template-columns: minmax(0, 1fr) var(--details-w); }
header { display: flex; justify-content: space-between; gap: 12px; align-items: center; }
h2 { font-size: 27px; letter-spacing: -.8px; margin: 0 0 6px; font-weight: 650; }
.subtitle { margin: 0; color: var(--muted); font-size: 13px; }
.actions { display: flex; gap: 8px; }
footer { font-size: 11px; color: var(--muted); line-height: 1.7; flex-shrink: 0; }
footer p { margin: 0; }
.note { margin-top: 6px !important; }
</style>
