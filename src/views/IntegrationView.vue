<script setup>
// WorkBuddy 集成：只读展示核心与 WorkBuddy 配置的连接状况。
// 唯一动作是复用既有的「导入 WorkBuddy」（action('import')），面板不直接改 models.json。
import { computed } from 'vue'

const props = defineProps({
  state: { type: Object, default: () => ({}) },
  busy: { type: Boolean, default: false },
})
defineEmits(['import'])

const sync = computed(() => props.state.sync || null)
const available = computed(() => props.state.availableModels || [])
const canImport = computed(
  () => !props.busy && !props.state.probe?.running && props.state.phase === 'ready',
)
</script>

<template>
  <section class="view" aria-label="WorkBuddy 集成">
    <header>
      <div>
        <h2>WorkBuddy 集成</h2>
        <p class="subtitle">本服务如何接入 WorkBuddy，以及当前发布结果。</p>
      </div>
      <!-- 灰按钮旁边补一条 title：可视的原因在下方「核心阶段」一栏，这里只做就近提示 -->
      <button
        id="integration-import"
        class="primary"
        :disabled="!canImport"
        :aria-busy="String(busy)"
        :title="canImport ? '把本服务的可用模型写入 WorkBuddy 配置' : '核心未就绪或正在检测，暂时无法导入'"
        @click="$emit('import')"
      >
        <span v-if="busy" class="spinner" />导入 WorkBuddy
      </button>
    </header>

    <dl class="facts">
      <div>
        <dt>本机接口地址</dt>
        <dd class="mono">{{ state.endpoint || '未启动' }}</dd>
      </div>
      <div>
        <dt>核心阶段</dt>
        <dd class="phase" :class="{ ok: state.phase === 'ready', err: state.phase === 'error' }">{{ state.phase || '未知' }}</dd>
      </div>
      <div>
        <dt>WorkBuddy 配置</dt>
        <dd class="mono">{{ state.modelsFile || '尚未定位到 models.json（可点右上「导入 WorkBuddy」手动选择）' }}</dd>
      </div>
      <div>
        <dt>最近一次发布</dt>
        <dd>
          <template v-if="sync?.error"><span class="error-text">{{ sync.error }}</span></template>
          <template v-else-if="sync?.time">
            已写入 {{ sync.count ?? 0 }} 个可用模型 · {{ new Date(sync.time).toLocaleString() }}
          </template>
          <template v-else>尚未发布：首次读取与检测完成后自动写入。</template>
        </dd>
      </div>
      <div>
        <dt>当前可用模型</dt>
        <dd>{{ available.length }} 个</dd>
      </div>
    </dl>

    <div v-if="available.length" class="chip-list">
      <span v-for="id in available" :key="id" class="chip mono">{{ id }}</span>
    </div>

    <p class="note">
      写入范围：只维护本应用（buddy-bridge-v1）名下的条目，WorkBuddy 中手动添加的模型原样保留；
      检测为不可用的模型只在本窗口展示，不会写入配置。再次检测后需要重新点「导入 WorkBuddy」才会更新。
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
/* 行间极淡分隔线：键值对读起来像表格，而不是一叠段落 */
.facts div { display: flex; gap: var(--sp-3); align-items: baseline; padding: 7px 0; }
.facts div + div { border-top: 1px solid color-mix(in srgb, var(--line) 70%, transparent); }
.facts div:first-child { padding-top: 0; }
.facts div:last-child { padding-bottom: 0; }
.facts dt { flex-shrink: 0; width: 112px; font-size: var(--fs-xs); color: var(--muted-strong); }
.facts dd { margin: 0; font-size: var(--fs-sm); color: var(--text); overflow-wrap: anywhere; min-width: 0; }
.facts dd.phase.ok { color: var(--green); }
.facts dd.phase.err { color: var(--orange); }
.chip-list { display: flex; flex-wrap: wrap; gap: var(--sp-2); }
.chip {
  font-size: var(--fs-xs);
  padding: 3px 9px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: var(--bg);
  color: var(--muted-strong);
  transition: border-color var(--dur-1) var(--ease-standard), color var(--dur-1) var(--ease-standard);
}
.chip:hover { border-color: color-mix(in srgb, var(--green) 45%, var(--line)); color: var(--text); }
.mono { font-family: ui-monospace, monospace; }
.error-text { color: var(--orange); }
.note { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); line-height: var(--lh-loose); }
</style>
