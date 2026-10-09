<script setup>
// 平台视图：多平台免费模型的 Key 管理入口（保存 / 清除 / 状态刷新）。
// 凭据纪律：Key 只经 set-provider-key 的请求体进入核心，面板**不保存、不回显、不做掩码**；
// provider-status 也只回「是否已配置」。Key 保存后需重启核心（隔离配置在启动时注入）才会生效。
import { computed, onMounted, ref } from 'vue'
import { action, openExternal } from '../core/bridge.js'

const props = defineProps({
  busy: { type: Boolean, default: false },
})

// 静态平台清单（与核心 providers.rs 注册表的 id/label 一致，不含 Key）：
// 四张卡片**不依赖核心在线**就渲染 —— 卡片列表若完全由 provider-status 的请求结果驱动，
// 核心未就绪/未运行时表单整页消失，用户没有地方填 Key。请求只负责刷新「已配置」徽章。
const PLATFORMS = [
  { id: 'modelscope', label: 'ModelScope' },
  { id: 'siliconflow-cn', label: 'SiliconFlow' },
  { id: 'tencent-tokenhub', label: '腾讯混元 TokenHub' },
  { id: 'zhipuai', label: '智谱' },
]

// 每家平台的申请引导：官方入口（已核实）+ 三步指引。链接经壳的 open_external 用系统浏览器打开
// （WebView 里 target=_blank 默认静默失败），面板内不内嵌网页、不发起任何外部请求。
const PLATFORM_GUIDES = {
  modelscope: {
    url: 'https://www.modelscope.cn/my/access/token',
    steps: [
      '打开下方官方申请页，注册或登录魔搭社区（支持支付宝 / GitHub 登录）。',
      '进入「访问令牌」管理页，点击「创建令牌」（ms- 开头，注册即可用，无需付费）。',
      '复制令牌，粘贴到上方输入框点「保存」。',
    ],
  },
  'siliconflow-cn': {
    url: 'https://cloud.siliconflow.cn/account/ak',
    steps: [
      '打开下方官方申请页，注册或登录硅基流动（新账号含免费额度）。',
      '进入「API 密钥」页，点击「新建 API 密钥」（sk- 开头）。',
      '复制密钥，粘贴到上方输入框点「保存」。',
    ],
  },
  'tencent-tokenhub': {
    url: 'https://console.cloud.tencent.com/tokenhub/apikey',
    steps: [
      '打开下方官方申请页，登录腾讯云并开通 TokenHub 服务。',
      '进入「API Key 管理」页，点击「创建 API Key」（混元 Hy3 等模型按量计费，以平台标价为准）。',
      '复制 Key，粘贴到上方输入框点「保存」。',
    ],
  },
  zhipuai: {
    url: 'https://open.bigmodel.cn/usercenter/apikeys',
    steps: [
      '打开下方官方申请页，注册或登录智谱开放平台。',
      '进入「API keys」页，点击「创建 API Key」（glm-4.5-flash 等 flash 系列免费）。',
      '复制 Key，粘贴到上方输入框点「保存」。',
    ],
  },
}

// 正在打开的申请页（按钮转 spinner）；打开失败在按钮旁给出提示而不是吞掉。
const opening = ref(null)
const openError = ref(null)

async function openGuide(provider) {
  const url = PLATFORM_GUIDES[provider.id]?.url
  if (!url) return
  opening.value = provider.id
  openError.value = null
  const response = await openExternal(url)
  opening.value = null
  if (!response.ok) openError.value = response.error
}

// 卡片初始即渲染（configured=false 待刷新）；drafts 按同一顺序初始化，
// 模板直接按 id 取，不在渲染期间造对象。
const providers = ref(PLATFORMS.map(p => ({ ...p, configured: false })))
const error = ref(null)
const loading = ref(false)
const drafts = ref(Object.fromEntries(PLATFORMS.map(p => [p.id, { key: '', feedback: null }])))
// 明文切换：input 的动态 type 由它驱动；切换只影响本平台输入框，与保存动作无关。
const revealing = ref({})
const saving = ref({})
const clearing = ref({})
// 正在应用（refresh 重读中）的平台：按钮置忙，防连续保存叠加重启。
const applying = ref({})
const applyingBusy = computed(() => Object.values(applying.value).some(Boolean))

// 兜底手动应用：等价于「模型与服务」页的「读取免费模型」；成功与否都弹回反馈区。
async function applyAll() {
  applying.value.__all = true
  const response = await action('refresh')
  applying.value.__all = false
  if (response.ok) {
    error.value = null
  } else {
    error.value = response.error
  }
}

// 用响应快照按 id 合并「已配置」徽章；快照缺失/平台集合不一致时以本地清单为准，
// 卡片数量与形态永远不会因核心返回异常而缩水或消失。
function mergeStatuses(snapshot) {
  const byId = new Map((snapshot?.providers || []).map(item => [item.id, item]))
  providers.value = providers.value.map(provider => {
    const known = byId.get(provider.id)
    return known ? { ...provider, configured: known.configured === true } : provider
  })
  return (snapshot?.providers || []).some(item => item.configured === true)
}

// 错误提示的克制原则：核心没跑 + 用户还没配过任何 Key 时，连接失败是**预期状态**
// （provider-status 只在核心进程内读盘，核心未启动自然 refused），弹一条 os error 61
// 只会吓到第一次打开平台页的用户。此时徽章保持「未配置」即可；一旦有平台配置过
// （状态真实与否开始影响用户决策）或用户点了刷新/保存，失败才值得显示。
async function load({ silent = false } = {}) {
  loading.value = true
  if (!silent) error.value = null
  const response = await action('provider-status')
  if (response.ok) {
    error.value = null
    mergeStatuses(response.result)
  } else if (silent) {
    // 静默探测：失败不打扰；若快照拿不到就维持全部「未配置」，等下一次显式刷新。
    error.value = null
  } else {
    error.value = response.error
  }
  loading.value = false
}

async function save(provider) {
  const draft = drafts.value[provider.id]
  const key = (draft.key || '').trim()
  if (!key) {
    draft.feedback = { error: true, text: '请先粘贴平台 Key' }
    return
  }
  saving.value[provider.id] = true
  draft.feedback = null
  const response = await action('set-provider-key', { provider: provider.id, apiKey: key })
  saving.value[provider.id] = false
  if (!response.ok) {
    draft.feedback = { error: true, text: `保存失败：${response.error}` }
    return
  }
  // ── 到这里 Key 已确定落盘（providers.json 已写入）────────────────────────
  // 徽章立即翻转：响应快照优先，快照缺失时本地翻——用户此刻最需要的就是「保存成功了」。
  mergeStatuses(response.result)
  if (!response.result?.providers) {
    providers.value = providers.value.map(p => p.id === provider.id ? { ...p, configured: true } : p)
  }
  draft.key = ''
  draft.feedback = { error: false, text: '已保存 ✓ 正在应用到模型列表…' }
  // 热生效：/admin/refresh 只重启隔离的 OpenCode 子进程（不动应用与 HTTP 服务），
  // start_backend 重新读 providers.json 注入新 Key 并重新发现，探测随后自动跑。
  // 失败不回滚已保存的 Key：提示给出重试入口。
  applying.value[provider.id] = true
  const refreshResponse = await action('refresh')
  applying.value[provider.id] = false
  if (refreshResponse.ok) {
    draft.feedback = { error: false, text: '已保存并应用 ✓ 切到「模型与服务」查看新模型（探测进行中）。' }
  } else {
    draft.feedback = { error: true, text: `Key 已保存 ✓，但应用失败：${refreshResponse.error}。可点下方「读取免费模型（重新应用）」重试。` }
  }
}

async function clear(provider) {
  clearing.value[provider.id] = true
  const draft = drafts.value[provider.id]
  const response = await action('clear-provider-key', { provider: provider.id })
  clearing.value[provider.id] = false
  if (!response.ok) {
    draft.feedback = { error: true, text: `清除失败：${response.error}` }
    return
  }
  mergeStatuses(response.result)
  if (!response.result?.providers) {
    providers.value = providers.value.map(p => p.id === provider.id ? { ...p, configured: false } : p)
  }
  draft.feedback = { error: false, text: '已清除 ✓ 正在应用到模型列表…' }
  // 热生效：与保存同路径——refresh 重启子进程时该平台声明段不再注入。
  applying.value[provider.id] = true
  const refreshResponse = await action('refresh')
  applying.value[provider.id] = false
  if (refreshResponse.ok) {
    draft.feedback = { error: false, text: '已清除并应用 ✓ 该平台的模型已从「模型与服务」中移除。' }
  } else {
    draft.feedback = { error: true, text: `Key 已清除 ✓，但应用失败：${refreshResponse.error}。可点下方「读取免费模型（重新应用）」重试。` }
  }
}

onMounted(() => {
  // 首次进入页面用静默探测：核心未运行时（最典型 = 全新安装还没配过 Key）不弹连接失败。
  // 之后任何显式动作（刷新按钮 / 保存 / 清除后的自动应用失败）都走非静默路径，照常报错。
  load({ silent: true })
})
</script>

<template>
  <section class="view" aria-label="平台">
    <header>
      <div>
        <h2>平台</h2>
        <p class="subtitle">配置自有平台 Key，免费模型随注册表扩展。Key 只保存在本机数据目录，不回显。</p>
      </div>
      <button id="reload-providers" :disabled="loading" @click="load()">
        <span v-if="loading" class="spinner" />刷新
      </button>
    </header>

    <div v-if="error" class="notice error" role="alert">
      <svg class="notice-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
        <circle cx="12" cy="12" r="8.4" />
        <path d="M12 7.6v5.6M12 16.4v.4" />
      </svg>
      <span class="notice-text">状态刷新失败（{{ error }}）：仍可填写并保存，待核心运行后点「刷新」同步状态。</span>
      <button type="button" class="ghost" :disabled="loading" @click="load()">
        <span v-if="loading" class="spinner" />重试
      </button>
    </div>

    <p v-if="openError" class="notice error" role="alert">
      <span class="notice-text">打开浏览器失败（{{ openError }}）。单击对应卡片中的网址即可全选复制，再到浏览器打开。</span>
    </p>

    <ul class="providers">
      <li v-for="provider in providers" :key="provider.id" class="provider">
        <div class="provider-head">
          <span class="provider-label">{{ provider.label }}<span class="provider-id">（{{ provider.id }}）</span></span>
          <span class="badge" :class="provider.configured ? 'ok' : 'off'">
            {{ provider.configured ? '已配置' : '未配置' }}
          </span>
        </div>

        <div class="guide">
          <p class="guide-title">申请 Key（三步）</p>
          <ol class="guide-steps">
            <li v-for="(step, i) in PLATFORM_GUIDES[provider.id]?.steps || []" :key="i">{{ step }}</li>
          </ol>
          <div class="guide-actions">
            <button type="button" class="guide-open" :disabled="opening === provider.id" @click="openGuide(provider)">
              <span v-if="opening === provider.id" class="spinner" />{{ opening === provider.id ? '正在打开…' : '打开官方申请页 ↗' }}
            </button>
            <!-- 单击全选整段网址（user-select: all 选中 DOM 全文，即使视觉上被省略号截断），
                 作为「打开浏览器失败」时的手动复制兜底 -->
            <span class="guide-url" :title="PLATFORM_GUIDES[provider.id]?.url">{{ PLATFORM_GUIDES[provider.id]?.url }}</span>
          </div>
        </div>

        <form class="key-form" @submit.prevent="save(provider)">
          <div class="key-input-wrap">
            <input
              v-model="drafts[provider.id].key"
              :type="revealing[provider.id] ? 'text' : 'password'"
              autocomplete="off"
              spellcheck="false"
              :placeholder="provider.configured ? '输入新 Key 以替换（保存后不回显）' : '粘贴平台 Key'"
              :aria-label="`${provider.label} 的平台 Key`"
              :disabled="busy || saving[provider.id]"
            >
            <button
              type="button"
              class="reveal-btn"
              :aria-pressed="!!revealing[provider.id]"
              :aria-label="revealing[provider.id] ? `隐藏 ${provider.label} 的 Key 明文` : `显示 ${provider.label} 的 Key 明文`"
              :title="revealing[provider.id] ? '隐藏明文' : '显示明文'"
              :disabled="busy || saving[provider.id]"
              @click="revealing[provider.id] = !revealing[provider.id]"
            >
              <!-- 眼睛图标：闭眼 = 当前隐藏（点击显示）；睁眼 = 当前明文（点击隐藏）。
                   内联 SVG 继承 currentColor，CSP 无外部资源 -->
              <svg v-if="!revealing[provider.id]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z" />
                <circle cx="12" cy="12" r="3.2" />
              </svg>
              <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z" />
                <circle cx="12" cy="12" r="3.2" />
                <path d="M4 4l16 16" />
              </svg>
            </button>
          </div>
          <button type="submit" class="primary" :disabled="busy || saving[provider.id] || applying[provider.id] || !drafts[provider.id].key.trim()">
            <span v-if="saving[provider.id]" class="spinner" />保存
          </button>
          <button
            v-if="provider.configured"
            type="button"
            class="ghost"
            :disabled="busy || clearing[provider.id]"
            @click="clear(provider)"
          >
            <span v-if="clearing[provider.id]" class="spinner" />清除
          </button>
        </form>

        <p
          v-if="drafts[provider.id].feedback"
          class="feedback"
          :class="{ error: drafts[provider.id].feedback.error }"
          role="status"
        >
          {{ drafts[provider.id].feedback.text }}
        </p>
      </li>
    </ul>

    <div class="apply-note">
      <p>Key 保存或清除后<strong>自动应用</strong>：面板会重读免费模型（只重启隔离的模型子进程，不动应用窗口），新模型出现在「模型与服务」列表。若自动应用失败，可点下方按钮重试。</p>
      <button type="button" :disabled="busy || !!applyingBusy" @click="applyAll">
        <span v-if="busy || applyingBusy" class="spinner" />读取免费模型（重新应用）
      </button>
    </div>

    <p class="note">额度与可用性由各平台决定，本应用不缓存额度；探测与对话都会消耗平台侧额度（免费模型的额度为 0 标价，仍受平台限额约束）。模型发现只覆盖注册表内的平台；上游新增免费模型需随本应用更新出现。</p>
  </section>
</template>

<style scoped>
header { display: flex; justify-content: space-between; gap: var(--sp-3); align-items: center; }
.subtitle { margin: 0; color: var(--muted-strong); font-size: var(--fs-sm); }
.providers { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-3); }
.provider {
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  background: var(--row);
  padding: var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.provider-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.provider-label { font-size: 15px; font-weight: 600; }
/* 标题内的注册表 id：弱化为括注，不再单独占一行 */
.provider-id { font-size: 11px; font-weight: 400; color: var(--muted); font-family: ui-monospace, monospace; }
.badge {
  font-size: 11px; font-weight: 500; white-space: nowrap;
  padding: 4px 9px; border-radius: 999px;
}
.badge.ok { background: var(--green-bg); color: var(--green); }
.badge.off { background: var(--panel); color: var(--muted-strong); }
.guide {
  border: 1px dashed var(--line);
  border-radius: var(--radius-s);
  padding: 10px 12px;
  background: color-mix(in srgb, var(--panel) 60%, transparent);
}
.guide-title { margin: 0 0 6px; font-size: 12px; font-weight: 600; color: var(--muted-strong); }
.guide-steps { margin: 0; padding-left: 18px; font-size: 12px; line-height: 1.8; color: var(--muted-strong); }
.guide-actions { display: flex; align-items: center; gap: 10px; margin-top: 8px; }
.guide-open {
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 500;
  padding: 5px 12px;
  border-radius: 999px;
  border: 1px solid color-mix(in srgb, var(--green) 45%, transparent);
  background: var(--green-bg);
  color: var(--green);
  cursor: pointer;
  transition: opacity var(--dur-1) var(--ease-standard);
}
.guide-open:hover:not(:disabled) { opacity: 0.78; }
.guide-open:disabled { cursor: not-allowed; opacity: 0.5; }
.guide-open:focus-visible { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
/* 网址与按钮同行：占满剩余宽度、溢出省略；user-select: all 让单击即全选全文（复制兜底） */
.guide-url {
  margin: 0; flex: 1; min-width: 0;
  font-size: 11px; color: var(--muted); font-family: ui-monospace, monospace;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  user-select: all; cursor: text;
}
.key-form { display: flex; gap: 8px; flex-wrap: wrap; }
.key-input-wrap { position: relative; flex: 1; min-width: 200px; display: flex; }
.key-form input {
  flex: 1; min-width: 0; width: 100%;
  font-size: 13px; padding: 8px 44px 8px 10px; /* 右侧留出眼睛按钮位 */
  border: 1px solid var(--line); border-radius: var(--radius-s);
  background: var(--panel); color: var(--text);
}
.key-form input:focus-visible { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
.key-form input:disabled { opacity: .5; cursor: not-allowed; }
.reveal-btn {
  position: absolute; right: 3px; top: 50%; transform: translateY(-50%);
  width: 34px; height: 34px;
  display: flex; align-items: center; justify-content: center;
  border: none; background: transparent; border-radius: var(--radius-s);
  color: var(--muted-strong); cursor: pointer;
  transition: color var(--dur-1) var(--ease-standard), background var(--dur-1) var(--ease-standard);
}
.reveal-btn:hover:not(:disabled) { color: var(--text); background: var(--nav-hover-bg); }
.reveal-btn:focus-visible { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
.reveal-btn:disabled { opacity: .4; cursor: not-allowed; }
.reveal-btn svg { width: 22px; height: 22px; }
.feedback { margin: 0; font-size: 12px; color: var(--green); }
.feedback.error { color: var(--badge-unavailable-fg); }
.apply-note {
  border: 1px solid var(--line); border-radius: var(--radius-m);
  background: var(--panel); padding: var(--sp-3) var(--sp-4);
  display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap;
}
.apply-note p { margin: 0; font-size: 12px; line-height: 1.7; color: var(--muted-strong); }
.note { margin: 0; font-size: 11px; color: var(--muted); line-height: 1.7; }
.notice { display: flex; align-items: center; gap: 8px; }
.notice-icon { width: 16px; height: 16px; flex-shrink: 0; }
.notice-text { font-size: 13px; }
</style>
