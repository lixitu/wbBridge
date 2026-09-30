// 构建 WB Bridge 核心 sidecar：@yao-pkg/pkg 直接打包（原生支持 ESM top-level await，
// esbuild cjs 中间产物反而会因 TLA 失败，故不预打包）。核心的 tar/undici 依赖由 pkg 静态分析收入。
// 产物命名遵循 Tauri externalBin 约定：<name>-<target-triple>[.exe]，输出到 src-tauri/binaries/。
// 用法：node scripts/build-sidecar.mjs [--targets a,b,...|all]（默认构建本机平台架构）
import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const coreDir = path.join(root, 'src', 'core');
const outDir = path.join(root, 'src-tauri', 'binaries');
const pkgBin = path.join(coreDir, 'node_modules', '.bin', process.platform === 'win32' ? 'pkg.cmd' : 'pkg');

const ALL = {
  'aarch64-apple-darwin': 'node22-macos-arm64',
  'x86_64-apple-darwin': 'node22-macos-x64',
  'x86_64-pc-windows-msvc': 'node22-win-x64',
  'aarch64-pc-windows-msvc': 'node22-win-arm64',
  'x86_64-unknown-linux-gnu': 'node22-linux-x64',
  'aarch64-unknown-linux-gnu': 'node22-linux-arm64',
};
const hostTriple = { darwin: 'aarch64-apple-darwin', win32: 'x86_64-pc-windows-msvc', linux: 'x86_64-unknown-linux-gnu' }[process.platform];

const argTargets = process.argv.find(a => a.startsWith('--targets'));
let targets = argTargets ? argTargets.split('=')[1].split(',') : [hostTriple];
if (argTargets && argTargets.includes('all')) targets = Object.keys(ALL);

for (const triple of targets) {
  const pkgTarget = ALL[triple];
  if (!pkgTarget) throw new Error(`未知目标三元组：${triple}`);
  const ext = triple.includes('windows') ? '.exe' : '';
  const output = path.join(outDir, `wbbridge-core-${triple}${ext}`);
  console.log(`→ pkg ${pkgTarget} → ${path.basename(output)}`);
  execFileSync(pkgBin, ['src/main.js',
    '--output', output, '--targets', pkgTarget, '--compress', 'GZip'], { cwd: coreDir, stdio: 'inherit', shell: process.platform === 'win32' });
  await fs.chmod(output, 0o755).catch(() => {});
}
console.log('sidecar 构建完成：', targets.join(', '));
