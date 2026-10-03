<script setup>
// 关于与更新：版本与运行位置信息只读；更新区是唯一动作区。
// 状态机在 src/core/update.js（冷启动静默检查由 App.vue 触发），本视图只渲染与触发，
// 百分比一律来自 updater 事件的真实字节数，拿不到 contentLength 就不显示数字。
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { dataDir } from '../core/bridge.js'
import { check, dismiss, install, restart, setAutoCheck, subscribe } from '../core/update.js'
import FeedbackBar from './FeedbackBar.vue'

defineProps({
  state: { type: Object, default: () => ({}) },
})

const version = __APP_VERSION__
const dir = ref('')
const dirError = ref(null)
const update = ref({
  status: 'idle', version: '', notes: '', received: 0, total: 0, error: '', checkedAt: 0, autoCheck: true,
})
let unsubscribe = null

const busy = computed(() => ['checking', 'downloading'].includes(update.value.status))
// 可见的更新条：available / downloading / ready 三态共用一条，避免状态切换时布局跳动。
const hasBar = computed(() => ['available', 'downloading', 'ready'].includes(update.value.status))
const percent = computed(() => {
  const { received, total } = update.value
  if (!total) return null
  return Math.min(100, Math.round((received / total) * 100))
})
const progressText = computed(() => {
  const text = `已下载 ${formatBytes(update.value.received)}`
  if (!update.value.total) return `${text}（总大小未知）`
  return `${percent.value}% · ${text} / ${formatBytes(update.value.total)}`
})

function formatBytes(bytes) {
  const value = Number(bytes) || 0
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(2)} MB`
}

// 更新条上的唯一主按钮：还没装就下载安装，装好了就重启生效。
function primary() {
  return update.value.status === 'ready' ? restart() : install()
}

onMounted(async () => {
  unsubscribe = subscribe(next => { update.value = next })
  const response = await dataDir()
  if (response.ok) dir.value = String(response.result || '')
  else dirError.value = response.error
})

onUnmounted(() => {
  if (unsubscribe) unsubscribe()
})
</script>

<template>
  <section class="view" aria-label="关于与更新">
    <header>
      <div>
        <h2>关于与更新</h2>
        <p class="subtitle">版本信息与运行位置。</p>
      </div>
    </header>

    <dl class="facts">
      <div>
        <dt>面板版本</dt>
        <dd>{{ version }}</dd>
      </div>
      <div>
        <dt>核心状态版本</dt>
        <dd>{{ state.version || '未启动' }}</dd>
      </div>
      <div>
        <dt>OpenCode 版本</dt>
        <dd>{{ state.opencodeVersion || '未就绪' }}</dd>
      </div>
      <div>
        <dt>状态 schema</dt>
        <dd>{{ state.schemaVersion ?? '未知' }}</dd>
      </div>
      <div>
        <dt>本机接口地址</dt>
        <dd class="mono">{{ state.endpoint || '未启动' }}</dd>
      </div>
      <div>
        <dt>数据目录</dt>
        <dd class="mono">
          <template v-if="dirError"><span class="error-text">{{ dirError }}</span></template>
          <template v-else>{{ dir || '读取中…' }}</template>
        </dd>
      </div>
    </dl>

    <div class="panel update" :class="`is-${update.status}`">
      <h3>更新</h3>
      <p class="lede">
        启动后在后台静默检查一次；只有真的发现新版本才会在下面出现更新条。
        下载与安装一律由你点击触发，安装完成后需重启应用生效。
      </p>

      <!-- 自动检查开关是界面偏好（存 prefs，跨重启生效）；关掉只停掉冷启动的静默检查，
           下面的「检查更新」按钮始终可用。 -->
      <label class="auto-check">
        <span>启动后自动检查更新</span>
        <input
          id="auto-check"
          type="checkbox"
          role="switch"
          :checked="update.autoCheck"
          :aria-checked="String(update.autoCheck)"
          @change="setAutoCheck($event.target.checked)"
        >
      </label>

      <div v-if="hasBar" class="update-bar">
        <span class="pulse" aria-hidden="true" />
        <div class="update-main">
          <strong>{{ update.status === 'ready' ? `v${update.version} 已下载完成` : `发现新版本 v${update.version}` }}</strong>
          <p v-if="update.notes" class="notes">{{ update.notes }}</p>
        </div>
        <button
          v-if="update.status !== 'downloading'"
          type="button"
          class="primary"
          :disabled="busy"
          @click="primary"
        >{{ update.status === 'ready' ? '重启应用' : '下载并安装' }}</button>
        <button v-else type="button" class="primary" disabled aria-busy="true">
          <span class="spinner sm" />下载中
        </button>
      </div>

      <div
        v-if="update.status === 'downloading'"
        class="progress is-active"
        role="progressbar"
        :aria-valuenow="percent"
        :aria-valuetext="progressText"
        aria-valuemin="0"
        aria-valuemax="100"
      >
        <i :style="{ width: `${percent ?? 0}%` }" />
      </div>
      <p v-if="update.status === 'downloading'" class="progress-text">{{ progressText }}</p>

      <div class="update-actions">
        <button type="button" class="ghost" :disabled="busy" @click="check()">
          <span v-if="update.status === 'checking'" class="spinner sm" />检查更新
        </button>
        <span v-if="update.checkedAt" class="checked-at">
          上次检查：{{ new Date(update.checkedAt).toLocaleString() }}
        </span>
      </div>

      <FeedbackBar
        v-if="update.status === 'error'"
        :text="`更新失败：${update.error}`"
        error
        @dismiss="dismiss()"
      />
      <p v-else-if="update.status === 'uptodate'" class="uptodate">已是最新版本（{{ version }}）。</p>
    </div>

    <p class="note">
      · 模型名单与免费额度由上游 OpenCode 决定，本应用不控制、不缓存额度。<br>
      · 数据目录中保存 api-key、status.json 与运行日志；密钥不会出现在日志或状态里。
    </p>
  </section>
</template>

<style scoped>
header { display: flex; justify-content: space-between; gap: var(--sp-3); align-items: center; }
.subtitle { margin: 0; color: var(--muted-strong); font-size: var(--fs-sm); }
.facts {
  margin: 0;
  display: flex;
  flex-direction: column;
  padding: var(--sp-4);
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
}
/* 行间极淡分隔线，与「WorkBuddy 集成」的键值区保持同一套阅读节奏 */
.facts div { display: flex; gap: var(--sp-3); align-items: baseline; padding: 7px 0; }
.facts div + div { border-top: 1px solid color-mix(in srgb, var(--line) 70%, transparent); }
.facts div:first-child { padding-top: 0; }
.facts div:last-child { padding-bottom: 0; }
.facts dt { flex-shrink: 0; width: 112px; font-size: var(--fs-xs); color: var(--muted-strong); }
.facts dd { margin: 0; font-size: var(--fs-sm); color: var(--text); overflow-wrap: anywhere; min-width: 0; }
.mono { font-family: ui-monospace, monospace; }
.panel { padding: var(--sp-4); background: var(--row); border: 1px solid var(--line); border-radius: var(--radius-m); }
.panel h3 { display: flex; align-items: center; gap: 8px; margin: 0 0 var(--sp-2); font-size: var(--fs-base); font-weight: 600; }
.panel h3::before { content: ""; width: 3px; height: 13px; border-radius: 2px; background: var(--green); }
.panel p { margin: 0; font-size: var(--fs-sm); color: var(--muted-strong); line-height: var(--lh-loose); }
.error-text { color: var(--orange); }
.note { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); line-height: var(--lh-loose); }

/* ── 更新区 ───────────────────────────────────────────────────────
   更新条随 v-if 挂载而入场（与主区 rise-in 同一时长与缓动，不引 <Transition>）；
   prefers-reduced-motion 下位移与淡入被全站降级规则取消，绿点脉冲同样只放一次。 */
.update-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  margin-top: var(--sp-3);
  padding: var(--sp-3);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  background: var(--surface-raised);
  box-shadow: var(--shadow-s);
  animation: update-rise var(--dur-3) var(--ease-enter) both;
}
.pulse {
  flex-shrink: 0;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--green);
}
.is-available .pulse { animation: update-ping var(--dur-3) var(--ease-emphasis) 1; }
.update-main { flex: 1; min-width: 0; }
.update-main strong { font-size: var(--fs-sm); font-weight: 600; }
.notes { margin: 4px 0 0; font-size: var(--fs-xs); color: var(--muted); line-height: var(--lh-loose); }
.progress { margin-top: var(--sp-3); }
.progress-text { margin: 6px 0 0; font-size: var(--fs-xs); color: var(--muted-strong); font-variant-numeric: tabular-nums; }
.update-actions { display: flex; align-items: center; gap: var(--sp-3); margin-top: var(--sp-3); }
/* 自动检查开关：与侧栏「使用系统代理」同一套开关语言（同样的轨道尺寸、同一批 token）。 */
.auto-check {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-3);
  margin-top: var(--sp-3);
  padding: var(--sp-2) var(--sp-3);
  border: 1px solid var(--line);
  border-radius: var(--radius-s);
  font-size: var(--fs-sm);
  color: var(--muted-strong);
  cursor: pointer;
  transition: background var(--dur-2) var(--ease-standard);
}
.auto-check:hover { background: var(--nav-hover-bg); }
.auto-check input[role=switch] {
  appearance: none;
  width: 34px;
  height: 20px;
  border-radius: 20px;
  background: var(--switch-off);
  cursor: pointer;
  position: relative;
  flex-shrink: 0;
  margin: 0;
  transition: background var(--dur-2) var(--ease-standard);
}
.auto-check input[role=switch]:after {
  content: "";
  position: absolute;
  top: 3px;
  left: 3px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--switch-knob);
  box-shadow: var(--shadow-s);
  transition: transform var(--dur-2) var(--ease-emphasis);
}
.auto-check input[role=switch]:checked { background: var(--green); }
.auto-check input[role=switch]:checked:after { transform: translateX(14px); }
.auto-check input[role=switch]:focus-visible { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
.checked-at { font-size: var(--fs-xs); color: var(--muted-strong); }
.uptodate { margin-top: var(--sp-3); }
@keyframes update-rise {
  from { opacity: 0; transform: translateY(6px); }
  to { opacity: 1; transform: none; }
}
@keyframes update-ping {
  from { box-shadow: 0 0 0 0 color-mix(in srgb, var(--green) 55%, transparent); }
  to { box-shadow: 0 0 0 8px transparent; }
}
</style>
