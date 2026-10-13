#!/usr/bin/env bash
# wbBridge 前端部署构建（当前机器可执行）：安装依赖 -> 生产构建 -> 门禁测试 -> 校验 dist/。
# 构建前会把旧 dist/ 备份到 deploy/backup/dist-<时间戳>，供 rollback.sh 回滚。
# 用法：bash deploy/build-frontend.sh        # FORCE_INSTALL=1 可强制重装依赖
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

BACKUP_DIR="$ROOT/deploy/backup"
STAMP="$(date +%Y%m%d-%H%M%S)"

command -v node >/dev/null 2>&1 || { echo "✗ 缺少 node（建议 24+）"; exit 1; }
command -v npm  >/dev/null 2>&1 || { echo "✗ 缺少 npm"; exit 1; }

# 1) 依赖
if [ ! -d node_modules ] || [ "${FORCE_INSTALL:-0}" = "1" ]; then
  echo "==> 安装依赖（npm ci，失败回退 npm install）"
  npm ci || npm install
else
  echo "==> node_modules 已存在，跳过安装（FORCE_INSTALL=1 可强制重装）"
fi

# 2) 备份现有 dist/（vite build 会 emptyOutDir 清空）
if [ -d dist ]; then
  mkdir -p "$BACKUP_DIR"
  cp -a dist "$BACKUP_DIR/dist-$STAMP"
  echo "==> 已备份旧 dist -> deploy/backup/dist-$STAMP"
fi

# 3) 前端生产构建
echo "==> 前端生产构建（vite build -> dist/）"
npm run vite:build

# 4) 门禁测试（绿集：83 例；不含缺陷护栏 review.regression.test.js）
echo "==> 前端核心门禁测试（应 83 例全绿）"
node --test src/core/text.test.js src/core/bm25.test.js src/core/kbIndex.test.js \
  src/core/assistant.test.js src/core/assistant.flow.test.js \
  src/core/tools.test.js src/core/prefs.test.js src/core/ops.test.js

# 5) 缺陷护栏（修复前预期 6 失败，不计入门禁，但发布前必须清零）
echo "==> 缺陷复现护栏（当前预期 6 fail，发布前必须清零）"
node --test src/core/review.regression.test.js || echo "⚠ review.regression.test.js 未通过（修复前预期失败，见测试报告 §三/§四）"

# 6) 校验产物
echo "==> 校验产物"
[ -f dist/index.html ] || { echo "✗ 缺少 dist/index.html"; exit 1; }
[ -d dist/assets ]   || { echo "✗ 缺少 dist/assets"; exit 1; }
echo "✔ dist 产物就绪："
ls -la dist | sed -n '1,20p'
