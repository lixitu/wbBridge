<script setup>
// 根组件：布局壳 + 全局状态持有者。
// 状态经 src/core/bridge.js 订阅（与 Electron 版 window.buddy 契约一致），
// 下发给视图组件；跨组件动作也统一走 bridge.action()。
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { action, onState, onDismiss } from './core/bridge.js'
import { loadView, saveView } from './core/prefs.js'
import {
  ACTION_COOLDOWN_MS,
  COOLDOWN_ACTIONS,
  FEEDBACK_VISIBLE_MS,
  rejectionOf,
  successMessage,
} from './core/ops.js'
import { startSilentCheck } from './core/update.js'
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
import ProvidersView from './views/ProvidersView.vue'

const state = ref({})
const selected = ref(null)
// 详情无结果时共享同一个空对象：引用稳定，ModelDetails 不被无关状态推送触发更新。
const EMPTY_RESULT = {}
// 当前视图：与 SideBar 的 item.id 一一对应（'models' | 'providers' | 'logs' | 'usage' | 'workbuddy' | 'about'）。
// 视图状态由根组件持有，侧栏只派发切换事件，避免两处各存一份选中态。
// 初值来自 prefs（上次停留的视图）；非法或已删除的视图名会在读取时回落 'models'。
const view = ref(loadView())
// 记住视图属于界面偏好，写入即发即忘：偏好写失败（隐私模式 / 配额）绝不能影响切换本身。
watch(view, next => { saveView(next) })
// 进行中的动作名（null = 空闲）：模板按动作名点亮对应按钮的 spinner。
const busyAction = ref(null)
const feedback = ref(null) // { text, error }

// ── 操作守卫（防抖 + 节流）与成功反馈（src/core/ops.js 钉住判定与文案）───────
// 终态动作成功完成的时刻表：进入冷却窗口（ACTION_COOLDOWN_MS）后重复触发会被拒绝——
// refresh / system-proxy / restart / import 都会在响应后继续改核心状态（重启子进程、
// 重跑探测、写配置），连点只会把刚拿到的结果又冲掉。失败不写入：马上重试是正当操作。
const lastFinished = ref({})
// 成功文案自动消隐的定时器：同一时刻只有一个，新反馈覆盖旧的时先撤旧的。
let successTimer = 0

function clearSuccessTimer() {
  if (successTimer) {
    clearTimeout(successTimer)
    successTimer = 0
  }
}

// 成功反馈 + 冷却登记 + 自动消隐。文案一律来自 ops.js 的 successMessage（唯一来源，
// 视图不各自造句子）；import 与平台 Key 动作的成功反馈由视图自有机制承担，不在此列。
function noteSuccess(name, result) {
  const text = successMessage(name, result)
  if (COOLDOWN_ACTIONS.includes(name)) {
    lastFinished.value = { ...lastFinished.value, [name]: Date.now() }
  }
  if (!text) return
  showTimedFeedback(text)
}

// 展示一条 5 秒后自动消隐的成功/信息文案（import 的三类结果也走这里）。
function showTimedFeedback(text) {
  clearSuccessTimer()
  feedback.value = { text }
  successTimer = setTimeout(() => {
    successTimer = 0
    // 只清成功文案：期间若出现进行中/失败提示（FeedbackBar 的 pending / error 优先级更高），
    // 那次赋值已经把 feedback 换成了别的对象，这里再赋 null 会把新提示吞掉。
    if (feedback.value && !feedback.value.error) feedback.value = null
  }, FEEDBACK_VISIBLE_MS)
}

// 是否应该拒绝本次触发（在飞互斥 + 完成冷却）：拒绝时给出节流后的可见提示，
// 连点不再叠加重复文案（FeedbackBar 以 text 为 key，同文不重挂载）。
function rejectIfGuarded(name) {
  const reason = rejectionOf(name, { busy: !!busyAction.value, lastFinished: lastFinished.value })
  if (!reason) return false
  feedback.value = { text: reason, error: true }
  return true
}

// 单模型「重新检测」的行内挂起标记：modelId -> 'pending'（请求还在空中）| 'acked'（核心已确认开始）。
// 它只补「点击 → 核心快照接手」之间那不到 500ms 的空档；进行中的权威来源是核心状态里的
// singleProbes（orchestration.rs::note_single_probe 在响应前登记、探测结束时撤销），
// 「检测中」何时结束也由它决定，前端不猜。
const reprobeClicked = ref({})
const reprobing = computed(() => new Set([
  ...(state.value.singleProbes || []),
  ...Object.keys(reprobeClicked.value),
]))
// 还在空中的那一个（'pending'）：用于把进行中文案从「逐个检测」改成单模型措辞。
const reprobeRequest = computed(() => Object.keys(reprobeClicked.value)
  .find(id => reprobeClicked.value[id] === 'pending') || '')

watch(state, (snapshot) => {
  const running = snapshot?.singleProbes || []
  for (const [id, phase] of Object.entries(reprobeClicked.value)) {
    // 核心已经报出这一项 → 标记交还给核心；核心确认过开始、之后的快照里却不再有它 → 探测已结束。
    // 两种情况都到了撤标记的时候，留着它只会把按钮永久锁死。
    if (running.includes(id) || phase === 'acked') delete reprobeClicked.value[id]
  }
})

function apply(next) {
  state.value = next || {}
}

async function run(name, value) {
  // 已有动作在跑 / 终态动作刚完成：拒绝必须留下可见痕迹（文案由 ops.js 节流约定，
  // 同一次拒绝期间连点不叠加），模板里的 @click 会丢返回值，只 return 的话用户读到
  // 的是「按钮坏了」，而不是「前一个还没结束 / 刚完成」。成功冷却的判定在置忙之前：
  // 冷却窗口内的重复点击不该把 busyAction 占住又立刻释放，那会让按钮闪一下假忙态。
  if (rejectIfGuarded(name)) return { ok: false, error: 'guarded' }
  busyAction.value = name
  feedback.value = null
  clearSuccessTimer()
  try {
    const response = await action(name, value)
    if (!response.ok) throw new Error(response.error)
    if (name === 'import') {
      const r = response.result
      if (r.canceled) {
        // 用户主动取消：不算一次完成的动作，不进冷却（马上换个文件重选是正当操作）。
        showTimedFeedback('已取消导入，配置未更改。')
      } else if (r.changed === false) {
        showTimedFeedback(`配置已是最新，共 ${r.count} 个模型，无需重复写入。`)
      } else {
        // 逐目标如实汇报：count 是各成功目标之和，只有在拿到 targets 时才知道写进了几家。
        const targets = r.targets || {}
        const okNames = ['workBuddy', 'codeBuddy']
          .map(key => ({ key, label: key === 'workBuddy' ? 'WorkBuddy' : 'CodeBuddy' }))
          .filter(item => targets[item.key]?.status === 'ok' && targets[item.key].changed)
          .map(item => item.label)
        const scope = okNames.length ? `导入 ${okNames.join(' 与 ')}` : '导入已完成'
        showTimedFeedback(`导入完成，已将 ${r.count} 个可用模型${scope}。`)
        lastFinished.value = { ...lastFinished.value, import: Date.now() }
      }
    }
    // /admin/probe 是同步返回 202：核心拒收（正在读取模型、模型不在当前目录、批量探测已占用）
    // 时给的是**带 2xx** 的 { error: { message } } 或 { started: false, message }，bridge 不会把
    // 它们判成失败。不落到反馈条就等于「点了没反应」，所以这里补上（只认 probe，别的动作
    // 的成功载荷里本来就可能带 error，例如 sync.error）。
    if (name === 'probe') {
      const refusal = response.result?.error?.message
        || (response.result?.started === false ? response.result?.message : '')
      if (refusal) {
        feedback.value = { text: refusal, error: true }
      } else {
        // 提交成功：批量不显示「1/x」之类的假进度，只确认「已开始」；单模型（带 model）
        // 的落点由行内「检测中」徽章承担，反馈条文案由 ops.js 给出同一份。
        noteSuccess(name, response.result)
      }
    } else {
      // 其余动作（refresh / restart / system-proxy 等）：响应返回即本轮完成，统一给成功文案；
      // import 在上面已经写过逐目标文案（noteSuccess 对 import 返回空串，不会覆盖）。
      noteSuccess(name, response.result)
    }
    return response
  } catch (error) {
    feedback.value = { text: error.message, error: true }
    return { ok: false }
  } finally {
    busyAction.value = null
  }
}

// 单模型重新检测：只向指定模型发一次探测请求，不重跑全量。
// 进行中的展示不在这里，而是行内的 reprobing 标记 + 核心的 singleProbes（见上方 watch）。
async function reprobeModel(modelId) {
  // 防抖：这一行还在飞就不再发第二次。一次探测要跑几十秒并消耗真实额度，连点只会叠出
  // 多个并发探测，界面上根本分不清谁是谁；拒绝同样要有可见痕迹，不能静默吞掉。
  if (reprobing.value.has(modelId)) {
    feedback.value = { text: `（${modelId}）正在检测中，请等它结束。`, error: true }
    return
  }
  reprobeClicked.value[modelId] = 'pending'
  const response = await run('probe', { model: modelId })
  // 没跑起来（请求失败、或核心以 2xx 拒收 —— 文案已由 run() 落到反馈条）：撤掉标记，
  // 按钮立刻恢复可点，不会把这一行锁死。跑起来了则交还给核心的 singleProbes。
  if (response.ok && response.result?.started === true) {
    reprobeClicked.value[modelId] = 'acked'
    return
  }
  delete reprobeClicked.value[modelId]
}

// 检测进度：待检队列是壳推送的真实数据，已完成 = 当前模型总数 − 仍在待检的数量。
// 只反映这一帧的快照，不做外推；拿不到总数或待检数时返回空串，宁可不显示也不猜数字。
const probeProgress = computed(() => {
  const probe = state.value.probe
  const total = (state.value.models || []).length
  const pending = probe?.pending?.length
  if (!probe?.running || !total || !Number.isFinite(pending)) return ''
  return `${Math.max(0, Math.min(total, total - pending))}/${total}`
})

// 进行中提示：把「哪个动作在跑」翻成一句人话，纯 CSS spinner 之外再给一行文字交代。
// 除检测外都不写进度数字 —— 其余动作前端拿不到真实进度，写数字就是编造。
const pendingText = computed(() => {
  if (busyAction.value === 'refresh') return '正在读取免费模型，请稍候…'
  if (busyAction.value === 'import') return '正在写入 WorkBuddy / CodeBuddy 配置…'
  if (busyAction.value === 'restart') return '正在重启核心服务…'
  if (busyAction.value === 'system-proxy') return '正在应用系统代理设置…'
  // 单模型重新检测的请求还在空中：措辞必须区别于批量，否则一行「逐个检测」会把
  // 「只检这一个」说成整库重跑。请求返回后进度由那一行自己承担，这里不再占用反馈条。
  if (busyAction.value === 'probe' && reprobeRequest.value) {
    return `正在发起重新检测（${reprobeRequest.value}）…`
  }
  if (busyAction.value === 'probe' || state.value.probe?.running) {
    const progress = probeProgress.value ? `（${probeProgress.value}）` : ''
    return `正在逐个检测模型${progress}，会向每个模型发送一次简短请求（消耗少量免费额度）…`
  }
  return ''
})
// 反馈条只保留一个来源：进行中压过上一次结果，避免「还在跑」和「上次的结果」上下叠着互相打架。
const banner = computed(() => {
  if (pendingText.value) return { text: pendingText.value, pending: true, error: false }
  if (feedback.value) return { text: feedback.value.text, error: !!feedback.value.error, pending: false }
  return null
})

// 底部同步摘要行：写入目标可能只有部分在线（WorkBuddy / CodeBuddy 任一），
// 顶层 count 是各成功目标之和，所以措辞不再绑定「WorkBuddy」单家。
const syncSummary = computed(() => {
  const sync = state.value.sync
  if (sync?.error) return sync.error
  if (sync?.time) return `已导入 ${sync.count ?? 0} 个模型（写入所有检测到的插件）· 再次检测后需点击导入更新`
  return '首次读取和检测完成后自动写入所有检测到的插件'
})

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
  // 更新检查与核心状态无关，可以并行；延迟交给 update.js，避免和启动探测抢带宽。
  startSilentCheck()
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
            <button
              id="refresh"
              :disabled="!!busyAction || !!state.probe?.running || state.phase !== 'ready'"
              :aria-busy="String(busyAction === 'refresh')"
              @click="run('refresh')"
            >
              <span v-if="busyAction === 'refresh'" class="spinner" />读取免费模型
            </button>
            <!-- 检测是长任务：按钮上带真实进度（取自壳推送的待检队列），不是估算出来的
                 —— 进度是唯一的提醒来源，把它放在触发点上比只在底部飘一行文字更容易被看到 -->
            <button
              id="probe"
              :disabled="!!busyAction || !!state.probe?.running || state.phase !== 'ready'"
              :aria-busy="String(!!state.probe?.running)"
              @click="run('probe')"
            >
              <span v-if="state.probe?.running" class="spinner" />检测全部<template v-if="probeProgress"> {{ probeProgress }}</template>
            </button>
          </div>
        </header>

        <!-- 检测是长任务，按钮里的计数负责「还剩多少」，这条不确定进度条负责「还在动」：
             两者都是真实信号的呈现，进度条不谎报百分比 -->
        <div v-if="state.probe?.running" class="busy-bar" role="progressbar" aria-label="正在检测模型" />

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
            <!-- spinner 只跟自己的动作绑：任何动作在跑都点亮它，会把「正在检测」误读成「正在导入」 -->
            <span v-if="busyAction === 'import'" class="spinner" />导入 WorkBuddy / CodeBuddy
          </button>
        </MetricsBar>

        <FeedbackBar
          v-if="banner"
          :key="banner.text"
          :text="banner.text"
          :error="banner.error"
          :pending="banner.pending"
          @dismiss="feedback = null"
        />
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
          :reprobing="reprobing"
          @reprobe="reprobeModel"
        />

        <ModelDetails
          v-if="selected"
          :model="selected"
          :result="(state.modelResults || {})[selected.id] || EMPTY_RESULT"
          @collapse="dismiss"
        />
      </div>

      <footer v-if="view === 'models'">
        <p id="sync">{{ syncSummary }}</p>
        <p class="note">打开应用默认沿用上次检测的模型与结果（不再每次重跑一遍探测），点「读取免费模型」才会重新获取并逐个检测，检测会向每个模型发送一次简短请求、消耗少量免费额度。耗时为完整请求用时，非首字延迟。不可用模型仅在本窗口保留，不供任何插件使用；剩余额度暂不可查询。</p>
      </footer>

      <!-- 非「模型与服务」的视图：各自填满主区并独立滚动，不改变上面两栏布局的任何约束 -->
      <LogsView v-if="view === 'logs'" />
      <UsageView v-if="view === 'usage'" :usage="state.usage" />
      <!-- 平台视图：Key 管理与申请引导；保存/清除后自动热生效（refresh 重读），无需重启应用 -->
      <ProvidersView
        v-if="view === 'providers'"
        :busy="!!busyAction"
      />
      <!-- 非「模型与服务」的视图都要有反馈落点：侧栏的代理开关在任何视图都能按下，
           只把横幅留在 workbuddy 里，其他视图点坏了就什么都不显示（模型视图的横幅在 .top 内）。
           同一份 banner、同一条 dismiss，只换位置，不复制状态。 -->
      <FeedbackBar
        v-if="view !== 'models' && banner"
        :key="banner.text"
        :text="banner.text"
        :error="banner.error"
        :pending="banner.pending"
        @dismiss="feedback = null"
      />
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

/* ── 视图进入动效 ──────────────────────────────────────────────────
   视图靠 v-if 挂载 / 卸载，把动画写在挂载元素上即可得到「淡入 + 轻微上浮」，
   无需引入 <Transition> 去改视图链结构。位移只给 6px：面板窗口窄，
   大幅滑动会迫使视线重新定位；切换视图的动作本身已经在侧栏给了高亮反馈。 */
.top, .content { animation: rise-in var(--dur-3) var(--ease-enter) both; }
.content { animation-delay: 30ms; }
@keyframes rise-in {
  from { opacity: 0; transform: translateY(6px); }
  to { opacity: 1; transform: none; }
}
</style>
