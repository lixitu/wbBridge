#!/usr/bin/env bash
# wbBridge 部署后验证：前端产物 / 门禁测试 / 桌面安装包 / 核心健康接口（可选）。
# 可选：BUDDY_PORT=<端口> API_KEY=<key> bash deploy/verify.sh 追加核心 HTTP 健康检查。
# 用法：bash deploy/verify.sh
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

PASS=0; FAIL=0
check() {
  if [ "$1" = "0" ]; then echo "✔ $2"; PASS=$((PASS+1)); else echo "✘ $2"; FAIL=$((FAIL+1)); fi
}

echo "== wbBridge 部署验证 =="

# 1) 前端产物
[ -f dist/index.html ] && [ -d dist/assets ]
check $? "前端产物 dist/ 完整（index.html + assets/）"

# 2) 门禁测试（83 例绿集）
node --test src/core/text.test.js src/core/bm25.test.js src/core/kbIndex.test.js \
  src/core/assistant.test.js src/core/assistant.flow.test.js \
  src/core/tools.test.js src/core/prefs.test.js src/core/ops.test.js >/dev/null 2>&1
check $? "前端核心门禁测试 83 例通过"

# 3) 桌面安装包
BUNDLES="$(find src-tauri/target -path '*/release/bundle/*' -type f \
  \( -name '*.exe' -o -name '*.msi' -o -name '*.dmg' -o -name '*.AppImage' -o -name '*.deb' \) 2>/dev/null)"
if [ -n "$BUNDLES" ]; then
  check 0 "桌面安装包已产出"
  echo "$BUNDLES" | sort
else
  check 1 "桌面安装包（当前未构建 / 本机无 Rust，属预期）"
fi

# 4) 核心健康检查（可选）
if [ -n "${BUDDY_PORT:-}" ] && [ -n "${API_KEY:-}" ]; then
  curl -fsS "http://127.0.0.1:${BUDDY_PORT}/health" -H "Authorization: Bearer ${API_KEY}" >/dev/null 2>&1
  check $? "核心 /health（127.0.0.1:${BUDDY_PORT}）"
  curl -fsS "http://127.0.0.1:${BUDDY_PORT}/v1/models" -H "Authorization: Bearer ${API_KEY}" >/dev/null 2>&1
  check $? "核心 /v1/models（Bearer 鉴权通过）"
else
  echo "· 跳过核心健康检查（未设 BUDDY_PORT/API_KEY）"
fi

echo
echo "== 结果：${PASS} 通过 / ${FAIL} 失败 =="
exit "$FAIL"
