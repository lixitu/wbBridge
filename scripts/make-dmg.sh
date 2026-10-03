#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════
# WB Bridge — 可靠的 macOS .dmg 生成（不依赖 osascript / Finder / node）
#
# 写法移植自参考项目 lockPass / fastenerTradeWorkbench 的 scripts/make-dmg.sh。
# Tauri 自带的 create-dmg 末尾要用 AppleScript 美化窗口，在无 GUI / 无 Finder
# 自动化授权的环境（CI、远程、部分本地终端）会失败，所以 tauri.conf.json 的
# bundle.targets 已不含 dmg，改由本脚本用 hdiutil 直接打包：
# 含 <产品名>.app + Applications 快捷方式，产物命名与 Tauri 一致
# （<productName>_<version>_<arch>.dmg）。
#
# 用法：
#   npm run tauri:build && npm run make:dmg              # 本机默认（release/）
#   TARGET_TRIPLE=aarch64-apple-darwin npm run make:dmg  # CI 交叉目标
# ═══════════════════════════════════════════════════════════════════
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONF="$ROOT/src-tauri/tauri.conf.json"

# 非 macOS 直接跳过：npm run build 在 Linux/Windows 上也要能一路跑完
if [ "$(uname -s)" != "Darwin" ]; then
  echo ">>> 非 macOS（$(uname -s)），跳过 .dmg 生成"
  exit 0
fi

# 用 grep/sed 提取，不依赖 node（CI 里 node 由 setup-node 提供，但子 shell 环境不宜赌它）
PRODUCT=$(grep -m1 '"productName"' "$CONF" | sed -E 's/.*:[[:space:]]*"([^"]+)".*/\1/')
VERSION=$(grep -m1 '"version"' "$CONF" | sed -E 's/.*:[[:space:]]*"([^"]+)".*/\1/')

# 架构优先取交叉目标的 target triple（CI 上 arm runner 交叉构建 x86_64 时 uname 会骗人），
# 否则按构建机实际架构
if [ -n "${TARGET_TRIPLE:-}" ]; then
  ARCH="${TARGET_TRIPLE%%-*}"
  BUNDLE_ROOT="$ROOT/src-tauri/target/$TARGET_TRIPLE/release/bundle"
else
  RAW_ARCH="$(uname -m)"
  case "$RAW_ARCH" in
    arm64) ARCH="aarch64" ;;
    x86_64) ARCH="x86_64" ;;
    *) ARCH="$RAW_ARCH" ;;
  esac
  BUNDLE_ROOT="$ROOT/src-tauri/target/release/bundle"
fi

MACOSDIR="$BUNDLE_ROOT/macos"
DMGDIR="$BUNDLE_ROOT/dmg"
APP="$MACOSDIR/${PRODUCT}.app"

if [ ! -d "$APP" ]; then
  echo "❌ 找不到 $APP"
  echo "   请先运行: npm run tauri:build（交叉目标时带 --target \$TARGET_TRIPLE）"
  exit 1
fi

mkdir -p "$DMGDIR"
OUT="$DMGDIR/${PRODUCT}_${VERSION}_${ARCH}.dmg"

STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

echo ">>> 生成 dmg: $OUT"
hdiutil create -volname "$PRODUCT" -srcfolder "$STAGE" -ov -format UDZO "$OUT"
rm -rf "$STAGE"

echo "✅ 完成: $OUT ($(du -h "$OUT" | cut -f1))"
