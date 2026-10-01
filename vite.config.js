import { readFileSync } from 'node:fs'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// ═══════════════════════════════════════════════════════════════════
// WB Bridge 前端构建（Vue3 SFC + Vite）
// ───────────────────────────────────────────────────────────────────
// 版本号单一来源：package.json（构建期注入 __APP_VERSION__）。
// 目录约定与 fastenerTradeWorkbench 对齐：
//   src/index.html  src/main.js  src/App.vue
//   src/views/      页面级组件
//   src/components/ 复用组件
//   src/core/       前端业务内核（与 Tauri IPC、状态轮询的唯一边界）
//   src/styles/     全局样式（variables / base）
//   src/public/     原样拷贝的静态资源
// 与参考工程的差异（有意为之）：本工程使用标准 Vue SFC 与 ESM 产物，
// 便于直接接入 npm 插件库；不使用 eval 注入经典脚本方案，
// 因此 CSP 不需要 unsafe-eval。
// ═══════════════════════════════════════════════════════════════════

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf-8'))

export default defineConfig({
  root: 'src',
  base: './',
  plugins: [vue()],
  define: {
    __APP_VERSION__: JSON.stringify('v' + pkg.version),
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    assetsInlineLimit: 4096,
    chunkSizeWarningLimit: 1500,
  },
  server: {
    port: 41990,
    strictPort: true,
  },
})
