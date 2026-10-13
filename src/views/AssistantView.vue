<script setup>
// AI 助手主视图：承载「提问 → 检索 → 模型 → 工具确认 → 结果回显」的完整对话流。
// 本组件只做**渲染与交互**，全部编排逻辑在 src/core/assistant.js（无状态，node --test 可测）；
// 会话 transcript 由 App.vue 通过 v-model:messages 持有（状态提升，切换视图不丢），
// 本组件卸载不保留任何业务状态，只保留随生命周期走的进行中标记与检索层。
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { action, activityText, aiChat, dataDir, readLog } from '../core/bridge.js'
import { runTurn } from '../core/assistant.js'
import { confirmPrompt, findTool } from '../core/tools.js'
import { initIndex, searchKnowledge } from '../core/kbIndex.js'

const props = defineProps({
  // 会话条目：{ id, role: 'user'|'assistant'|'tool'|'notice'|'error', text?, tool?, ... }
  messages: { type: Array, default: () => [] },
  // App 下发的 core-status 快照（构建系统提示与工具上下文用）。
  state: { type: Object, default: () => ({}) },
})
const emit = defineEmits(['update:messages', 'navigate'])

const draft = ref('')
const busy = ref(false)
const busyText = ref('')
// 检索层：初始化失败 / 降级都不阻断对话（检索结果为空时模型照常回答）。
const index = ref(null)
const degradedReason = ref('')

// 待确认工具：{ tool, args, prompt, resolve }。runTurn 的 confirm 回调打开它，
// 弹窗按钮决议后写回 resolve；同一时刻最多一个（runTurn 是串行 await confirm）。
const pendingConfirm = ref(null)

let abortController = null
let localSeq = 0
const transcriptEl = ref(null)

function nextId() {
  localSeq += 1
  return `${Date.now().toString(36)}-${localSeq.toString(36)}`
}

// v-model:messages 的新数组式写回：不就地 push props，保证父级能收到更新。
// 同一 tick 内可能出现同步相邻的两次 append（parseFailed 的 notice→final、拒绝确认的
// tool-result→final），而 props.messages 在父组件重渲染前不会同步更新；若每次都以
// props.messages 为基准生成新数组，后一次会覆盖前一次（提示气泡 / 工具卡丢失）。
// 因此维护一份随 append 同步推进的本地镜像，父级回传后再以父级值为准。
let transcript = Array.isArray(props.messages) ? props.messages : []

function append(entry) {
  transcript = [...transcript, { id: nextId(), ...entry }]
  emit('update:messages', transcript)
}

// 模型池为空（尚未收到快照时不判空，避免首屏误报）：WB · auto 没有可转发目标，助手拿不到模型。
const modelPoolEmpty = computed(() => {
  const state = props.state || {}
  return (Array.isArray(state.models) && state.models.length === 0)
    || (Array.isArray(state.availableModels) && state.availableModels.length === 0)
})

// runTurn 的 history 契约是 { role, content }；视图层 transcript 用的是 { role, text }，
// 这里做一次形状转换（只保留 user/assistant 的非空对话，与 trimHistory 同一口径）。
function chatHistory() {
  return props.messages
    .filter(entry => entry && (entry.role === 'user' || entry.role === 'assistant')
      && typeof entry.text === 'string' && entry.text.trim())
    .map(entry => ({ role: entry.role, content: entry.text }))
}

// 工具执行上下文：与 tools.js 的 ctx 契约一一对应。
const ctx = {
  get state() {
    return props.state
  },
  viewLabel: 'AI 助手',
  coreAction: action,
  readLog,
  dataDir,
  navigate: id => emit('navigate', id),
  activityText,
}

// runTurn 事件流 → 渲染映射（stage 驱动 spinner 文案，其余追加为对应气泡）。
function handleEvent(event) {
  if (!event) return
  if (event.type === 'stage') {
    busyText.value = event.text || ''
  } else if (event.type === 'notice') {
    append({ role: 'notice', text: event.text })
  } else if (event.type === 'tool-pending') {
    // 弹窗本体由 confirm 回调打开；这里只把阶段文案提前到「等待确认」。
    busyText.value = `正在等待确认：${event.title || event.tool}`
  } else if (event.type === 'tool-result') {
    const tool = findTool(event.tool)
    append({
      role: 'tool',
      tool: event.tool,
      title: tool?.title || event.tool,
      ok: event.ok === true,
      denied: event.denied === true,
      summary: event.summary || '',
      error: event.error || '',
    })
  } else if (event.type === 'final') {
    append({ role: 'assistant', text: event.text })
  } else if (event.type === 'error') {
    append({ role: 'error', text: event.text })
  }
}

async function submit() {
  const text = draft.value.trim()
  if (!text || busy.value) return
  // 先取历史（不含本条提问），再追加用户气泡；runTurn 自己会把当前 query 作为最新一条 user 消息。
  const history = chatHistory()
  append({ role: 'user', text })
  draft.value = ''

  const controller = new AbortController()
  abortController = controller
  busy.value = true
  busyText.value = '正在检索应用知识…'

  try {
    const outcome = await runTurn({
      history,
      query: text,
      ctx,
      chat: aiChat,
      retrieve: query => searchKnowledge(index.value, query),
      confirm: (tool, args) => new Promise((resolve) => {
        pendingConfirm.value = { tool, args, prompt: confirmPrompt(tool, args), resolve }
      }),
      onEvent: handleEvent,
      signal: controller.signal,
    })
    // canceledResult 不发 final：由视图补一条「已停止」说明。
    if (outcome?.canceled) append({ role: 'notice', text: '已停止。' })
  } catch (error) {
    append({ role: 'error', text: `助手执行异常：${String(error?.message || error)}` })
  } finally {
    busy.value = false
    busyText.value = ''
    pendingConfirm.value = null
    abortController = null
  }
}

function resolveConfirm(approved) {
  const pending = pendingConfirm.value
  pendingConfirm.value = null
  if (pending?.resolve) pending.resolve(approved)
}

function stop() {
  // IPC 在途请求无法被 JS 硬取消（壳内 ureq 仍会跑完），这里只中止 UI 等待；
  // 若确认弹窗开着，必须先决议 false 才能让 runTurn 越过 await confirm 检查 aborted。
  abortController?.abort()
  if (pendingConfirm.value) resolveConfirm(false)
}

function clearMessages() {
  transcript = []
  emit('update:messages', [])
}

function scrollToBottom() {
  nextTick(() => {
    const el = transcriptEl.value
    if (el) el.scrollTop = el.scrollHeight
  })
}

// 父级回传后以父级数组为准（保持单一真相），同时保持滚动跟随。
watch(() => props.messages, next => {
  transcript = Array.isArray(next) ? next : []
  scrollToBottom()
})
watch(busyText, scrollToBottom)

onMounted(async () => {
  // 索引初始化不阻断输入：失败 / 降级都返回可用内存索引 + 可读 reason，检索仍能工作。
  try {
    const outcome = await initIndex({})
    index.value = outcome.index
    degradedReason.value = outcome.reason || ''
  } catch (error) {
    degradedReason.value = `知识检索初始化失败：${String(error?.message || error)}`
  }
})

onUnmounted(() => {
  abortController?.abort()
})
</script>

<template>
  <section class="view assistant" aria-label="AI 助手">
    <header>
      <div>
        <h2>AI 助手</h2>
        <p class="subtitle">基于应用知识回答，操作类请求会先向你确认再执行。</p>
      </div>
      <div class="actions">
        <button type="button" class="ghost" :disabled="!messages.length || busy" @click="clearMessages">清空会话</button>
      </div>
    </header>

    <!-- 非阻断提示：模型池空 / 检索未持久化都只提示，不禁用输入（失败由通道错误兜底）。
         两条提示相互独立——「模型池空」与「检索降级」可能同时成立，不得用 v-else-if 互相遮蔽。 -->
    <div v-if="modelPoolEmpty" class="notice warn" role="status">
      模型池为空，助手无法对话，请先到「模型与服务」读取免费模型。
    </div>
    <div v-if="degradedReason" class="notice warn" role="status">{{ degradedReason }}</div>

    <div ref="transcriptEl" class="transcript" aria-live="polite">
      <p v-if="!messages.length && !busy" class="empty">
        问我关于应用的问题，例如「现在运行正常吗」或「帮我看看最近用量」。
      </p>

      <template v-for="entry in messages" :key="entry.id">
        <!-- 用户 / 助手 / 提示 / 错误 四种文本气泡 -->
        <div v-if="entry.role === 'user' || entry.role === 'assistant' || entry.role === 'notice' || entry.role === 'error'" class="bubble-row" :class="entry.role">
          <div class="bubble" :class="entry.role">{{ entry.text }}</div>
        </div>

        <!-- 工具执行结果：真实回显成功 / 失败 / 已取消 -->
        <div v-else-if="entry.role === 'tool'" class="tool-card" :class="{ failed: !entry.ok, denied: entry.denied }">
          <div class="tool-head">
            <span class="tool-title">工具：{{ entry.title || entry.tool }}</span>
            <span class="tool-status">
              {{ entry.denied ? '已取消' : entry.ok ? '成功' : '失败' }}
            </span>
          </div>
          <p v-if="entry.summary || entry.error" class="tool-body">{{ entry.summary || entry.error }}</p>
        </div>
      </template>

      <div v-if="busy" class="bubble-row assistant">
        <div class="bubble busy-bubble" aria-live="polite">
          <span class="spinner sm" />{{ busyText || '正在思考…' }}
        </div>
      </div>
    </div>

    <form class="composer" @submit.prevent="submit">
      <textarea
        v-model="draft"
        :disabled="busy"
        rows="3"
        placeholder="输入问题，Enter 发送，Shift+Enter 换行"
        aria-label="向 AI 助手提问"
        @keydown.enter.exact.prevent="submit"
      />
      <div class="composer-row">
        <button v-if="busy" type="button" class="ghost" @click="stop">停止</button>
        <button type="submit" class="primary" :disabled="busy || !draft.trim()">
          发送
        </button>
      </div>
    </form>

    <!-- 工具确认弹窗：中 / 高风险操作执行前必弹，模型无权代决。 -->
    <div v-if="pendingConfirm" class="confirm-backdrop">
      <div class="confirm" role="dialog" aria-modal="true" aria-labelledby="assistant-confirm-title">
        <h3 id="assistant-confirm-title">{{ pendingConfirm.prompt.title }}</h3>
        <p class="confirm-detail">{{ pendingConfirm.prompt.detail }}</p>
        <p class="confirm-risk">风险等级：{{ pendingConfirm.prompt.risk }}</p>
        <div class="confirm-actions">
          <button type="button" class="ghost" @click="resolveConfirm(false)">取消</button>
          <button type="button" class="primary" @click="resolveConfirm(true)">确认执行</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
header { display: flex; justify-content: space-between; gap: var(--sp-3); align-items: center; }
.subtitle { margin: 0; color: var(--muted-strong); font-size: var(--fs-sm); }
.actions { display: flex; gap: 8px; }

/* 非阻断提示：与其它视图 .notice 同一套阅读语言，但用绿色调表示「可用但不完整」。 */
.notice.warn {
  padding: 10px 12px;
  font-size: var(--fs-sm);
  color: var(--green);
  background: var(--green-bg);
  border-left: 3px solid currentColor;
  border-radius: var(--radius-m);
}

.transcript {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  padding: var(--sp-2);
}
.empty { margin: var(--sp-5) auto; font-size: var(--fs-sm); color: var(--muted-strong); }

.bubble-row { display: flex; }
.bubble-row.user { justify-content: flex-end; }
.bubble-row.assistant { justify-content: flex-start; }
.bubble-row.notice, .bubble-row.error { justify-content: flex-start; }

.bubble {
  max-width: 76%;
  padding: 9px 12px;
  border-radius: var(--radius-m);
  font-size: var(--fs-sm);
  line-height: var(--lh-loose);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.bubble.user { background: var(--green-bg); color: var(--text); border-bottom-right-radius: var(--radius-s); }
.bubble.assistant { background: var(--row); border: 1px solid var(--line); border-bottom-left-radius: var(--radius-s); }
.bubble.notice { background: transparent; color: var(--muted-strong); font-size: var(--fs-xs); padding: 4px 6px; }
.bubble.error { background: var(--orange-bg); color: var(--orange); border-left: 3px solid currentColor; }
.busy-bubble { display: inline-flex; align-items: center; gap: 6px; color: var(--muted-strong); }

/* 工具结果卡：与「气泡」分开，明确这是真实执行回显而不是模型发言。 */
.tool-card {
  align-self: flex-start;
  max-width: 88%;
  min-width: 240px;
  padding: var(--sp-3);
  background: var(--row);
  border: 1px solid var(--line);
  border-left: 3px solid var(--green);
  border-radius: var(--radius-m);
}
.tool-card.failed { border-left-color: var(--orange); }
.tool-card.denied { border-left-color: var(--muted); }
.tool-head { display: flex; justify-content: space-between; align-items: center; gap: var(--sp-2); }
.tool-title { font-size: var(--fs-xs); color: var(--muted-strong); font-weight: 600; }
.tool-status { font-size: var(--fs-xs); color: var(--green); font-variant-numeric: tabular-nums; }
.tool-card.failed .tool-status { color: var(--orange); }
.tool-card.denied .tool-status { color: var(--muted-strong); }
.tool-body { margin: 6px 0 0; font-size: var(--fs-sm); color: var(--text); white-space: pre-wrap; overflow-wrap: anywhere; }

.composer { display: flex; flex-direction: column; gap: var(--sp-2); flex-shrink: 0; }
textarea {
  width: 100%;
  resize: vertical;
  min-height: 68px;
  padding: 10px 12px;
  font: inherit;
  font-size: var(--fs-sm);
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
}
textarea:focus-visible { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
textarea:disabled { opacity: .6; }
.composer-row { display: flex; justify-content: flex-end; gap: 8px; }

/* 确认弹窗：轻量遮罩 + 居中卡片，不引第三方对话框。 */
.confirm-backdrop {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--bg) 55%, transparent);
  z-index: 40;
}
.confirm {
  width: min(440px, calc(100vw - 48px));
  padding: var(--sp-5);
  background: var(--surface-raised);
  border: 1px solid var(--line);
  border-radius: var(--radius-l);
  box-shadow: var(--shadow-s);
}
.confirm h3 { margin: 0 0 var(--sp-2); font-size: var(--fs-base); font-weight: 650; }
.confirm-detail { margin: 0; font-size: var(--fs-sm); line-height: var(--lh-loose); white-space: pre-wrap; overflow-wrap: anywhere; color: var(--text); }
.confirm-risk { margin: var(--sp-2) 0 0; font-size: var(--fs-xs); color: var(--muted-strong); }
.confirm-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: var(--sp-4); }
</style>
