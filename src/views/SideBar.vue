<script setup>
// 侧栏：品牌、导航占位、系统代理开关。
import logo from '../public/logo.svg'

const version = __APP_VERSION__

defineProps({
  proxyOn: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
})
defineEmits(['toggle-proxy'])
</script>

<template>
  <aside>
    <img class="brand-mark" :src="logo" alt="WB Bridge" aria-hidden="true">
    <h1>WB Bridge</h1>
    <p class="tagline">让 WorkBuddy 连接 OpenCode 免费模型</p>
    <div class="section-current">▦ &nbsp; 模型与服务</div>
    <div class="sidebar-bottom">
      <label class="toggle-label">
        使用系统代理
        <input
          type="checkbox"
          role="switch"
          :checked="proxyOn"
          :disabled="disabled"
          @change="$emit('toggle-proxy', $event.target.checked)"
        >
      </label>
      <p class="creator">基于 Tauri + Vue · {{ version }}<br>模型名单与免费额度由上游决定</p>
    </div>
  </aside>
</template>

<style scoped>
aside {
  width: 224px;
  flex-shrink: 0;
  padding: 28px 22px;
  background: var(--panel);
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
}
.brand-mark { width: 44px; height: 44px; margin-bottom: 18px; filter: drop-shadow(0 2px 6px rgb(79 70 229 / .25)); }
h1 { font-size: 21px; margin: 0 0 6px; letter-spacing: -.6px; }
.tagline { font-size: 12px; line-height: 1.8; color: var(--muted); margin: 0; }
.section-current { margin-top: 26px; padding: 12px 11px; background: var(--green-bg); border-radius: var(--radius-m); color: var(--green); font-weight: 600; font-size: 13px; }
.sidebar-bottom { margin-top: auto; }
.toggle-label { display: flex; align-items: center; justify-content: space-between; gap: 5px; font-size: 13px; }
input[role=switch] { appearance: none; width: 36px; height: 22px; border-radius: 20px; background: #aab2ae; cursor: pointer; position: relative; flex-shrink: 0; margin: 0; transition: background .15s; }
input[role=switch]:after { content: ""; position: absolute; top: 3px; left: 3px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .18s; box-shadow: 0 1px 2px rgb(0 0 0 / .2); }
input[role=switch]:checked { background: var(--green); }
input[role=switch]:checked:after { transform: translateX(14px); }
input:disabled { opacity: .45; cursor: default; }
.creator { font-size: 11px; color: var(--muted); line-height: 1.9; margin: 22px 0 0; }
</style>
