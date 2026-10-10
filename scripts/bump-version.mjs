#!/usr/bin/env node
/* ═══════════════════════════════════════════════════════════════════
   WB Bridge — 版本号统一更新脚本
   ───────────────────────────────────────────────────────────────────
   用法：
     node scripts/bump-version.mjs 1.0.1        # 更新全部版本号
     node scripts/bump-version.mjs 1.0.1 --dry-run  # 只预览不写入
   覆盖位置（7 处）：
     package.json（根级，版本单一来源）/
     src-tauri/tauri.conf.json / src-tauri/Cargo.toml /
     src-tauri/core/Cargo.toml（核心 crate 版本，自 2026-10-10 起随产品版本同步——用户指令） /
     AGENTS.md（当前基准版本表三行）
   注意：status.json 的 0.2.0 为历史沿革值，刻意不改；版本落点变更后两份 Cargo.lock
   （src-tauri/ 与 src-tauri/core/ 各自独立 workspace）由 cargo 自动同步，须一并提交。
   ═══════════════════════════════════════════════════════════════════ */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');

const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const versionArg = args.find((a) => !a.startsWith('--'));

if (!versionArg || !/^v?\d+\.\d+\.\d+$/.test(versionArg)) {
  console.error('用法: node scripts/bump-version.mjs <x.y.z | vx.y.z> [--dry-run]');
  process.exit(1);
}
const V = versionArg.replace(/^v/, '');

const results = [];

function patchJson(file, apply, label) {
  const p = path.join(ROOT, file);
  if (!fs.existsSync(p)) {
    results.push(`⚠️  ${file}: 不存在，跳过`);
    return;
  }
  const d = JSON.parse(fs.readFileSync(p, 'utf8'));
  const before = JSON.stringify(d);
  apply(d);
  const after = JSON.stringify(d);
  if (before === after) {
    results.push(`⚠️  ${file}: 未匹配到版本号（${label}），跳过`);
    return;
  }
  if (!dryRun) fs.writeFileSync(p, JSON.stringify(d, null, 2) + '\n');
  results.push(`${dryRun ? '🔍' : '✅'} ${file}: ${label}`);
}

function patchText(file, regex, replace, label) {
  const p = path.join(ROOT, file);
  const raw = fs.readFileSync(p, 'utf8');
  const out = raw.replace(regex, replace);
  if (out === raw) {
    results.push(`⚠️  ${file}: 未匹配到版本号（${label}），跳过`);
    return;
  }
  if (!dryRun) fs.writeFileSync(p, out);
  results.push(`${dryRun ? '🔍' : '✅'} ${file}: ${label}`);
}

/* ── JSON 类 ────────────────────────────────────────────────────── */
patchJson('package.json', (d) => { d.version = V; }, `version -> ${V}（版本单一来源）`);
patchJson('src-tauri/tauri.conf.json', (d) => { d.version = V; }, `version -> ${V}（打包产物版本）`);

/* ── 文本类 ─────────────────────────────────────────────────────── */
patchText('src-tauri/Cargo.toml', /^version = "\d+\.\d+\.\d+"$/m, `version = "${V}"`, `version -> ${V}`);

// 核心 crate 版本随产品版本同步（2026-10-10 用户指令）。行首锚定只命中 [package] 的
// version 行：依赖的 version 都有缩进，不会误伤。
patchText('src-tauri/core/Cargo.toml', /^version = "\d+\.\d+\.\d+"$/m, `version = "${V}"`, `version -> ${V}（核心 crate，随产品版本同步）`);

patchText('AGENTS.md',
  /\| 版本单一来源 \| \*\*\d+\.\d+\.\d+\*\*/g,
  `| 版本单一来源 | **${V}**`,
  `版本单一来源 -> ${V}`);

patchText('AGENTS.md',
  /\| 壳工程同步落点 \| \*\*\d+\.\d+\.\d+\*\*/g,
  `| 壳工程同步落点 | **${V}**`,
  `壳工程同步落点 -> ${V}`);

patchText('AGENTS.md',
  /\| 核心 crate 版本 \| \*\*\d+\.\d+\.\d+\*\*/g,
  `| 核心 crate 版本 | **${V}**`,
  `核心 crate 版本 -> ${V}`);

console.log(results.join('\n'));
console.log(dryRun
  ? '\n[dry-run] 未写入任何文件'
  : `\n完成：全部版本号已统一为 ${V}`);
