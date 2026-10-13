#!/usr/bin/env bash
# wbBridge 完整桌面应用构建（目标环境必需 Rust 1.90+ 与平台系统依赖）。
# 当前机器若无 cargo/rustc 会直接中止（见 preflight.sh 判定）。
# 签名私钥经 npm run tauri:build（= scripts/with-updater-key.mjs）注入，见仓库 .env.example。
# 用法：bash deploy/build-desktop.sh        # FORCE_INSTALL=1 强制重装依赖
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

command -v node  >/dev/null 2>&1 || { echo "✗ 缺少 node（>=24）"; exit 1; }
command -v npm   >/dev/null 2>&1 || { echo "✗ 缺少 npm"; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "✗ 缺少 cargo（Rust 1.90+）；桌面打包必须在含 Rust 的目标环境执行"; exit 1; }
command -v rustc >/dev/null 2>&1 || { echo "✗ 缺少 rustc"; exit 1; }

echo "==> 1/5 安装依赖"
if [ ! -d node_modules ] || [ "${FORCE_INSTALL:-0}" = "1" ]; then
  npm ci || npm install
else
  echo "node_modules 已存在，跳过（FORCE_INSTALL=1 强制重装）"
fi

echo "==> 2/5 前端生产构建"
npm run vite:build

echo "==> 3/5 前端门禁测试（83 例全绿）"
node --test src/core/text.test.js src/core/bm25.test.js src/core/kbIndex.test.js \
  src/core/assistant.test.js src/core/assistant.flow.test.js \
  src/core/tools.test.js src/core/prefs.test.js src/core/ops.test.js

echo "==> 4/5 桌面打包（tauri build，经签名注入包装器）"
# 缺签名私钥时包装器只告警，打包器会在生成 updater 产物阶段报错（详见 .env.example）
npm run tauri:build

echo "==> 5/5 平台安装包收尾（macOS 出 .dmg；Windows/Linux 由 make:dmg 空跳过）"
npm run make:dmg

echo "==> 产物位置"
find src-tauri/target -path '*/release/bundle/*' -type f \
  \( -name '*.exe' -o -name '*.msi' -o -name '*.dmg' -o -name '*.AppImage' \
     -o -name '*.deb' -o -name '*.app.tar.gz' -o -name '*.sig' \) 2>/dev/null | sort
