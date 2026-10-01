<script setup>
// 侧栏：品牌、分组导航（含规划中入口）、运行设置。
import logo from '../public/logo.svg'

const version = __APP_VERSION__

defineProps({
  proxyOn: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
})
defineEmits(['toggle-proxy'])

// 导航分组：当前只有「模型与服务」是已实现视图（激活态）；
// 其余为规划中的未来入口，一律以禁用态 + 「规划中」标签呈现，不做可点击却无响应的假入口。
const groups = [
  {
    title: '模型',
    items: [{ id: 'models', icon: '▦', label: '模型与服务', state: 'current' }],
  },
  {
    title: '运行',
    items: [
      { id: 'logs', icon: '▤', label: '运行日志', state: 'planned' },
      { id: 'usage', icon: '◔', label: '用量与额度', state: 'planned' },
    ],
  },
  {
    title: '集成',
    items: [{ id: 'workbuddy', icon: '⇄', label: 'WorkBuddy 集成', state: 'planned' }],
  },
  {
    title: '其他',
    items: [{ id: 'about', icon: 'ⓘ', label: '关于与更新', state: 'planned' }],
  },
]
</script>

<template>
  <aside>
    <img class="brand-mark" :src="logo" alt="WB Bridge" aria-hidden="true">
    <h1>WB Bridge</h1>
    <p class="tagline">让 WorkBuddy 连接 OpenCode 免费模型</p>

    <nav class="nav" aria-label="主导航（当前仅「模型与服务」可用，其余为规划中）">
      <section v-for="group in groups" :key="group.title" class="nav-group">
        <h2 class="nav-group-title">{{ group.title }}</h2>
        <ul class="nav-list">
          <li v-for="item in group.items" :key="item.id">
            <button
              type="button"
              class="nav-item"
              :class="item.state"
              :disabled="true"
              :aria-current="item.state === 'current' ? 'page' : undefined"
              :title="item.state === 'planned' ? `${item.label}（规划中，尚未实现）` : `${item.label}（当前视图）`"
            >
              <span class="nav-icon" aria-hidden="true">{{ item.icon }}</span>
              <span class="nav-label">{{ item.label }}</span>
              <span v-if="item.state === 'planned'" class="nav-tag">规划中</span>
            </button>
          </li>
        </ul>
      </section>
    </nav>

    <div class="sidebar-bottom">
      <section class="nav-group">
        <h2 class="nav-group-title">运行设置</h2>
        <label class="toggle-label">
          <span class="toggle-text"><span class="nav-icon" aria-hidden="true">⇌</span>使用系统代理</span>
          <input
            type="checkbox"
            role="switch"
            :checked="proxyOn"
            :aria-checked="proxyOn"
            :disabled="disabled"
            @change="$emit('toggle-proxy', $event.target.checked)"
          >
        </label>
      </section>
      <p class="creator">基于 Tauri + Vue · {{ version }}<br>模型名单与免费额度由上游决定</p>
    </div>
  </aside>
</template>

<style scoped>
aside {
  width: var(--sidebar-w);
  flex-shrink: 0;
  padding: 24px 16px 20px;
  background: var(--panel);
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.brand-mark { width: 40px; height: 40px; margin-bottom: 14px; filter: drop-shadow(0 2px 6px color-mix(in srgb, var(--indigo) 25%, transparent)); }
h1 { font-size: 20px; margin: 0 0 6px; letter-spacing: -.6px; }
.tagline { font-size: 12px; line-height: 1.7; color: var(--muted-strong); margin: 0; }

.nav { margin-top: 20px; overflow: auto; min-height: 0; }
.nav-group + .nav-group { margin-top: 16px; }
.nav-group-title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: .08em;
  color: var(--muted-strong);
  margin: 0 0 6px;
  padding: 0 9px;
}
.nav-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
.nav-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 9px;
  font-size: 13px;
  text-align: left;
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-s);
  color: var(--muted-strong);
}
/* 导航项均为知情禁用态（当前视图 / 规划中）：不做 hover 抬升，避免假入口 */
.nav-item:hover:not(:disabled) { background: transparent; }
.nav-icon { width: 15px; flex-shrink: 0; text-align: center; font-size: 13px; line-height: 1; }
.nav-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.nav-tag {
  flex-shrink: 0;
  font-size: 10px;
  line-height: 1.5;
  padding: 0 5px;
  border-radius: 5px;
  border: 1px solid var(--line);
  background: var(--bg);
  color: var(--muted-strong);
}
.nav-item.current {
  background: var(--green-bg);
  color: var(--green);
  font-weight: 600;
  /* 当前视图是激活态，必须清晰可辨，不随禁用态一起变淡 */
  opacity: 1;
  cursor: default;
}
/* 规划中入口：禁用态由「规划中」标签 + 禁点光标 + 无 hover 表达，
   标签文字本身保持 AA 对比度（不整体降透明度） */
.nav-item.planned { opacity: 1; cursor: not-allowed; }
.nav-item.planned .nav-icon { opacity: .62; }

.sidebar-bottom { margin-top: auto; padding-top: 16px; }
.toggle-label { display: flex; align-items: center; justify-content: space-between; gap: 6px; font-size: 13px; padding: 2px 9px; }
.toggle-text { display: flex; align-items: center; gap: 8px; min-width: 0; }
input[role=switch] { appearance: none; width: 34px; height: 20px; border-radius: 20px; background: var(--switch-off); cursor: pointer; position: relative; flex-shrink: 0; margin: 0; transition: background .15s; }
input[role=switch]:after { content: ""; position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: var(--switch-knob); transition: transform .18s; box-shadow: 0 1px 2px rgb(0 0 0 / .2); }
input[role=switch]:checked { background: var(--green); }
input[role=switch]:checked:after { transform: translateX(14px); }
input:disabled { opacity: .45; cursor: default; }
.creator { font-size: 11px; color: var(--muted-strong); line-height: 1.8; margin: 16px 0 0; padding: 0 9px; }
</style>
