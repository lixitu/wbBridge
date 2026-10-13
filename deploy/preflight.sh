#!/usr/bin/env bash
# wbBridge 部署预检：检查构建/运行依赖，判定当前机器可执行的部署形态。
# 用法：bash deploy/preflight.sh
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

have() { command -v "$1" >/dev/null 2>&1; }
RC=0

echo "== wbBridge 部署预检 =="
echo "机器: $(uname -a 2>/dev/null || echo unknown)"
echo

if have node; then
  echo "[node]  $(node -v)"
else
  echo "[node]  ✗ 未安装（前端构建/测试必需）"; RC=1
fi

if have npm; then
  echo "[npm]   $(npm -v)"
else
  echo "[npm]   ✗ 未安装"; RC=1
fi

if have cargo; then
  echo "[cargo] $(cargo --version)"
else
  echo "[cargo] ✗ 未安装（桌面打包 / 核心构建必需）"
fi

if have rustc; then
  echo "[rustc] $(rustc --version)"
else
  echo "[rustc] ✗ 未安装"
fi

case "$(uname -s)" in
  Darwin)             echo "[os]    macOS（可产出 .dmg）" ;;
  Linux)              echo "[os]    Linux（需 webkit2gtk 等系统依赖）" ;;
  MINGW*|MSYS*|CYGWIN*) echo "[os]    Windows（Git Bash）" ;;
  *)                  echo "[os]    $(uname -s)" ;;
esac

echo
echo "== 结论 =="
if have node && have npm; then
  echo "✔ 可执行：npm install/ci、vite:build、node --test、lint（前端静态产物部署）"
else
  echo "✘ 前端构建依赖缺失，请先安装 Node.js 24+ 与 npm"
  RC=1
fi
if have cargo && have rustc; then
  echo "✔ 可执行：tauri:build 桌面打包 / cargo 核心构建"
else
  echo "✘ 桌面打包与核心构建不可执行（缺 Rust 1.90+ 工具链）"
fi

exit "$RC"
