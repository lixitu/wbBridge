#!/usr/bin/env bash
# wbBridge 回滚：前端 dist/ 从备份恢复，或从指定 git 提交/标签重建。
# 用法：
#   bash deploy/rollback.sh list                    列出可用备份
#   bash deploy/rollback.sh dist <备份目录名>         回滚 dist/（如 dist-20260101-120000）
#   bash deploy/rollback.sh dist-latest              回滚到最近一次 dist 备份
#   bash deploy/rollback.sh git-ref <ref>            从指定 git 提交/标签重建前端 dist/
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/rollback.sh"
cd "$ROOT" || exit 1

BACKUP_DIR="$ROOT/deploy/backup"
MODE="${1:-list}"

case "$MODE" in
  list)
    echo "可用备份（$BACKUP_DIR/）："
    if [ -d "$BACKUP_DIR" ]; then ls -1 "$BACKUP_DIR"; else echo "（无）"; fi
    echo
    echo "用法："
    echo "  bash deploy/rollback.sh list                  列出备份"
    echo "  bash deploy/rollback.sh dist <备份目录名>       回滚前端 dist/"
    echo "  bash deploy/rollback.sh dist-latest            回滚到最近一次 dist 备份"
    echo "  bash deploy/rollback.sh git-ref <ref>          从指定 git ref 重建前端 dist/"
    ;;

  dist-latest)
    [ -d "$BACKUP_DIR" ] || { echo "✗ 无备份目录 $BACKUP_DIR"; exit 1; }
    LATEST="$(ls -1t "$BACKUP_DIR" | grep '^dist-' | head -1 || true)"
    [ -n "$LATEST" ] || { echo "✗ 没有 dist-* 备份"; exit 1; }
    bash "$SCRIPT" dist "$LATEST"
    ;;

  dist)
    SRC="${2:-}"
    [ -n "$SRC" ] || { echo "✗ 需要备份目录名（如 dist-20260101-120000）"; exit 1; }
    [ -d "$BACKUP_DIR/$SRC" ] || { echo "✗ 备份不存在：$BACKUP_DIR/$SRC"; exit 1; }
    echo "==> 回滚 dist/ <- $BACKUP_DIR/$SRC"
    rm -rf dist
    cp -a "$BACKUP_DIR/$SRC" dist
    if [ -f dist/index.html ]; then
      echo "✔ 前端产物已回滚"
    else
      echo "✗ 回滚产物缺少 index.html"; exit 1
    fi
    ;;

  git-ref)
    REF="${2:-}"
    [ -n "$REF" ] || { echo "✗ 需要 git ref（如 HEAD~1 或 v0.9.0）"; exit 1; }
    echo "==> 从 $REF 回滚前端源码并重建 dist/"
    # 只回滚前端源码与构建配置，不触碰 src-tauri/（Rust 壳）
    git checkout "$REF" -- src vite.config.js
    if [ ! -d node_modules ]; then npm ci || npm install; fi
    npm run vite:build
    [ -f dist/index.html ] && echo "✔ 已按 $REF 重建前端 dist/"
    echo "提示：如需一并回滚 Rust 壳/核心，执行 git checkout $REF -- src-tauri 后重跑 bash deploy/build-desktop.sh"
    ;;

  *)
    echo "✗ 未知模式：$MODE"; exit 1
    ;;
esac
