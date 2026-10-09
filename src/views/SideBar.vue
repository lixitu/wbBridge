<script setup>
// 侧栏：品牌、分组导航（五个视图均已实现，点击即切换）、运行设置。
// 品牌图标是 public 目录里的静态资源，构建时原样拷到产物根，因此只按 URL 引用、不做 JS import
// （import public 资源会让 Vite 报 "Assets in public directory cannot be imported from JavaScript"）。
const logo = '/logo.svg'

const version = __APP_VERSION__

// 代理开关是受控组件：视觉状态只跟随 proxyOn 真值。原生点击会先把 DOM 打向另一侧，
// 若动作被守卫拒绝（在飞 / 冷却），App 的状态不变、props 不变，复选框就会停在假位置谎报成功。
// 因此发出事件后立即把 DOM 拨回真值；真正翻转由核心状态推送驱动（动作期间有「正在应用」横幅）。
const props = defineProps({
  proxyOn: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
  // 当前视图 id：由 App.vue 持有，侧栏只负责高亮与派发切换，不自己维护选中态。
  current: { type: String, default: 'models' },
})
const emit = defineEmits(['toggle-proxy', 'select'])

function onToggleProxy(event) {
  const target = event.target
  emit('toggle-proxy', target.checked)
  target.checked = props.proxyOn === true
}

// 导航图标：内联 SVG 描边路径（24×24 视框，继承 currentColor）。
// 用 SVG 而非字符符号（原 ▦▤◔⇄ⓘ）：字符图标随系统字体变化、基线不齐、无法统一线宽与配色。
// 全部内联，不产生任何外部资源请求（CSP default-src 'self'）。
const ICONS = {
  models: 'M4.8 4.8h5.4v5.4H4.8zM13.8 4.8h5.4v5.4h-5.4zM4.8 13.8h5.4v5.4H4.8zM13.8 13.8h5.4v5.4h-5.4z',
  providers: 'M12 3.6l7.2 4.2v8.4L12 20.4l-7.2-4.2V7.8Z M12 12.2l7-4.1M12 12.2v8M12 12.2L5 8.1',
  logs: 'M5 6.5h14M5 12h14M5 17.5h8',
  usage: 'M12 4a8 8 0 1 0 8 8h-8V4Z',
  workbuddy: 'M8 8.5h10.5l-3-3M16 15.5H5.5l3 3',
  about: 'M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16ZM12 11.2v5M12 8.2v.4',
  proxy: 'M12 4.5v7M7.6 6.8a7 7 0 1 0 8.8 0',
}

// 导航分组：六个视图均已实现，点击即切换主区；当前视图常驻高亮。
const groups = [
  {
    title: '模型',
    items: [
      { id: 'models', icon: 'models', label: '模型与服务' },
      { id: 'providers', icon: 'providers', label: '平台' },
    ],
  },
  {
    title: '运行',
    items: [
      { id: 'logs', icon: 'logs', label: '运行日志' },
      { id: 'usage', icon: 'usage', label: '用量与额度' },
    ],
  },
  {
    title: '集成',
    items: [{ id: 'workbuddy', icon: 'workbuddy', label: '插件集成' }],
  },
  {
    title: '其他',
    items: [{ id: 'about', icon: 'about', label: '关于与更新' }],
  },
]
</script>

<template>
  <aside>
    <img class="brand-mark" :src="logo" alt="WB Bridge" aria-hidden="true">
    <h1>WB Bridge</h1>
    <p class="tagline">让 WorkBuddy / CodeBuddy 连接 OpenCode 免费模型</p>

    <nav class="nav" aria-label="主导航">
      <section v-for="group in groups" :key="group.title" class="nav-group">
        <h2 class="nav-group-title">{{ group.title }}</h2>
        <ul class="nav-list">
          <li v-for="item in group.items" :key="item.id">
            <button
              type="button"
              class="nav-item"
              :class="{ current: item.id === current }"
              :aria-current="item.id === current ? 'page' : undefined"
              :title="item.id === current ? `${item.label}（当前视图）` : `切换到${item.label}`"
              @click="$emit('select', item.id)"
            >
              <svg
                class="nav-icon"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path :d="ICONS[item.icon]" />
              </svg>
              <span class="nav-label">{{ item.label }}</span>
            </button>
          </li>
        </ul>
      </section>
    </nav>

    <div class="sidebar-bottom">
      <section class="nav-group">
        <h2 class="nav-group-title">运行设置</h2>
        <label class="toggle-label">
          <span class="toggle-text">
            <svg
              class="nav-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path :d="ICONS.proxy" />
            </svg>使用系统代理
          </span>
          <input
            type="checkbox"
            role="switch"
            :checked="proxyOn"
            :aria-checked="proxyOn"
            :disabled="disabled"
            @change="onToggleProxy"
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
  padding: 24px 14px 18px;
  background: var(--panel);
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  min-height: 0;
}
/* 品牌区：logo 加描边与投影，与浅色/暗色底都能分离 */
.brand-mark {
  width: 40px;
  height: 40px;
  margin-bottom: 14px;
  border-radius: 11px;
  box-shadow: var(--shadow-s), inset 0 0 0 1px color-mix(in srgb, var(--indigo) 22%, transparent);
  filter: drop-shadow(0 3px 8px color-mix(in srgb, var(--indigo) 22%, transparent));
}
h1 { font-size: 20px; margin: 0 0 6px; letter-spacing: -.6px; }
.tagline { font-size: 12px; line-height: 1.7; color: var(--muted-strong); margin: 0; }

.nav { margin-top: 20px; overflow: auto; min-height: 0; padding-right: 2px; }
.nav-group + .nav-group { margin-top: 16px; }
/* 分组标题：左侧加一道短竖标，让「模型 / 运行 / 集成 / 其他」的分组边界一眼可见 */
.nav-group-title {
  position: relative;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: .08em;
  color: var(--muted-strong);
  margin: 0 0 6px;
  padding: 0 10px;
}
.nav-group-title::before {
  content: "";
  position: absolute;
  left: 0;
  top: 50%;
  width: 2px;
  height: 10px;
  margin-top: -5px;
  border-radius: 2px;
  background: color-mix(in srgb, var(--muted) 40%, transparent);
}
.nav-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
.nav-item {
  position: relative;
  width: 100%;
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 8px 10px;
  font-size: 13px;
  text-align: left;
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-s);
  color: var(--muted-strong);
  transition: background var(--dur-2) var(--ease-standard),
              color var(--dur-2) var(--ease-standard),
              box-shadow var(--dur-2) var(--ease-standard);
}
/* 当前视图指示轨：常态高度 0，选中时展开成 18px 主色竖条（有长度变化，不是突然出现） */
.nav-item::before {
  content: "";
  position: absolute;
  left: -1px;
  top: 50%;
  width: 3px;
  height: 0;
  border-radius: 0 3px 3px 0;
  background: var(--green);
  opacity: 0;
  transform: translateY(-50%);
  transition: height var(--dur-2) var(--ease-emphasis), opacity var(--dur-2) var(--ease-standard);
}
/* 导航项均可点击（五个视图都已实现）：hover 用主色淡底提示可点，当前视图常驻高亮。 */
.nav-item:hover:not(.current) { background: var(--nav-hover-bg); color: var(--text); }
.nav-item.current {
  background: var(--nav-active-bg);
  color: var(--green);
  font-weight: 600;
  cursor: default;
  box-shadow: inset 0 0 0 1px var(--nav-active-ring);
}
.nav-item.current::before { height: 18px; opacity: 1; }
.nav-icon { width: 16px; height: 16px; flex-shrink: 0; opacity: .8; transition: opacity var(--dur-2) var(--ease-standard); }
.nav-item:hover .nav-icon, .nav-item.current .nav-icon, .toggle-label:hover .nav-icon { opacity: 1; }
.nav-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.sidebar-bottom { margin-top: auto; padding-top: 14px; border-top: 1px solid var(--line); }
.toggle-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  font-size: 13px;
  padding: 7px 10px;
  border-radius: var(--radius-s);
  cursor: pointer;
  transition: background var(--dur-2) var(--ease-standard);
}
.toggle-label:hover { background: var(--nav-hover-bg); }
/* 开关被禁用（核心未就绪）时，整行不再给出可点暗示 */
.toggle-label:has(input:disabled) { cursor: not-allowed; }
.toggle-label:has(input:disabled):hover { background: transparent; }
.toggle-text { display: flex; align-items: center; gap: 9px; min-width: 0; color: var(--muted-strong); transition: color var(--dur-2) var(--ease-standard); }
.toggle-label:hover .toggle-text { color: var(--text); }
input[role=switch] {
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
input[role=switch]:after {
  content: "";
  position: absolute;
  top: 3px;
  left: 3px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--switch-knob);
  box-shadow: 0 1px 2px rgb(0 0 0 / .2);
  transition: transform var(--dur-2) var(--ease-emphasis);
}
input[role=switch]:checked { background: var(--green); }
input[role=switch]:checked:after { transform: translateX(14px); }
input[role=switch]:focus-visible { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
input:disabled { opacity: .45; cursor: not-allowed; }
.creator { font-size: 11px; color: var(--muted-strong); line-height: 1.8; margin: 14px 0 0; padding: 0 10px; }
</style>
