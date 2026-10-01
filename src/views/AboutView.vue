<script setup>
// 关于与更新：版本与运行位置信息，只读。
// 更新能力如实呈现：本构建未接入自动更新通道（tauri.conf.json 未配置 updater 插件），
// 升级只能重新下载安装包，面板不做联网检查，也不编造「有更新」结论。
import { onMounted, ref } from 'vue'
import { dataDir } from '../core/bridge.js'

defineProps({
  state: { type: Object, default: () => ({}) },
})

const version = __APP_VERSION__
const dir = ref('')
const dirError = ref(null)

onMounted(async () => {
  const response = await dataDir()
  if (response.ok) dir.value = String(response.result || '')
  else dirError.value = response.error
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

    <div class="panel">
      <h3>更新</h3>
      <p>
        当前构建未接入自动更新：应用不会在后台检查或下载新版本，这里也没有「检查更新」按钮。
        需要新版时请重新下载安装包覆盖安装；WorkBuddy 侧无需改动。
      </p>
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
  gap: var(--sp-3);
  padding: var(--sp-4);
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
}
.facts div { display: flex; gap: var(--sp-3); align-items: baseline; }
.facts dt { flex-shrink: 0; width: 112px; font-size: var(--fs-xs); color: var(--muted-strong); }
.facts dd { margin: 0; font-size: var(--fs-sm); color: var(--text); overflow-wrap: anywhere; min-width: 0; }
.mono { font-family: ui-monospace, monospace; }
.panel { padding: var(--sp-4); background: var(--row); border: 1px solid var(--line); border-radius: var(--radius-m); }
.panel h3 { margin: 0 0 var(--sp-2); font-size: var(--fs-base); font-weight: 600; }
.panel p { margin: 0; font-size: var(--fs-sm); color: var(--muted-strong); line-height: var(--lh-loose); }
.error-text { color: var(--orange); }
.note { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); line-height: var(--lh-loose); }
</style>
