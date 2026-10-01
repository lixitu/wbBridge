#!/usr/bin/env node
/* ═══════════════════════════════════════════════════════════════════
   WB Bridge — 版本号一致性校验脚本
   ───────────────────────────────────────────────────────────────────
   用法：node scripts/check-version.mjs
   以根级 package.json 的 version 为基准（版本单一来源），比对其余位置的版本号，
   全部一致退出 0，任一不一致列出差异并退出 1（可挂 pre-commit / CI）。
   src-tauri/core/Cargo.toml 的 version 是 crate 内部版本、status.json 的 0.2.0 是历史沿革值，
   两者与产品版本解耦，不在校验范围内。
   ═══════════════════════════════════════════════════════════════════ */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');

const read = (f) => fs.readFileSync(path.join(ROOT, f), 'utf8');
const base = JSON.parse(read('package.json')).version;

const checks = [
  ['package.json', JSON.parse(read('package.json')).version, base],
  ['src-tauri/tauri.conf.json', JSON.parse(read('src-tauri/tauri.conf.json')).version, base],
  ['src-tauri/Cargo.toml', /^version = "(\d+\.\d+\.\d+)"$/m.exec(read('src-tauri/Cargo.toml'))?.[1], base],
  ['AGENTS.md 版本单一来源', /\| 版本单一来源 \| \*\*(\d+\.\d+\.\d+)\*\*/.exec(read('AGENTS.md'))?.[1], base],
  ['AGENTS.md 壳工程同步落点', /\| 壳工程同步落点 \| \*\*(\d+\.\d+\.\d+)\*\*/.exec(read('AGENTS.md'))?.[1], base],
];

let bad = 0;
for (const [label, actual, expect] of checks) {
  const ok = actual === expect;
  if (!ok) bad++;
  console.log(`${ok ? '✅' : '❌'} ${label}: ${actual ?? '(未找到)'} ${ok ? '' : `≠ ${expect}`}`);
}

if (bad === 0) {
  console.log(`\n全部 ${checks.length} 处版本号一致（${base}）`);
  process.exit(0);
} else {
  console.log(`\n发现 ${bad} 处不一致！请运行: npm run version:set <x.y.z>`);
  process.exit(1);
}
