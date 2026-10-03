/* ═══════════════════════════════════════════════════════════════════
   WB Bridge — 生成 Tauri updater 更新清单 latest.json
   ───────────────────────────────────────────────────────────────────
   用法（CI 内）：
     node scripts/gen-latest-json.mjs --dir artifacts --out latest.json
       [--expect darwin-aarch64,...] [--tag vX.Y.Z]
   规则：
     • 版本号取自 src-tauri/tauri.conf.json（单一真源，不重复登记）
     • 平台键取自 artifact **目录名**里的 target triple（macos|windows|linux + aarch64|x86_64）。
       不能靠文件名：macOS 的 updater 包实测叫「WB Bridge.app.tar.gz」，既不带版本也不带架构，
       两个 mac 架构的文件名完全相同 —— 架构后缀由 CI 的 macos 作业在上传前补齐。
     • 每个目录内只允许出现一个带配对 .sig 的 updater 安装包，多个即拒绝（宁缺毋假）
     • 没有同名 .sig 的安装包一律不进清单；默认要求六平台齐全，缺一即退出 1，
       让「某平台静默缺席」在 CI 里失败，而不是等用户装上旧版本才发现
     • url 指向 GitHub Release 资产；Draft 未 Publish 前下载 404 属预期
   ═══════════════════════════════════════════════════════════════════ */
import { readFileSync, existsSync, readdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const arg = (name, fallback) => {
  const index = process.argv.indexOf(`--${name}`)
  return index > -1 ? process.argv[index + 1] : fallback
}

const conf = JSON.parse(readFileSync(path.join(ROOT, 'src-tauri/tauri.conf.json'), 'utf8'))
const version = conf.version
const repo = 'trexwb/wbBridge'
const tag = arg('tag') || `v${version}`
const base = `https://github.com/${repo}/releases/download/${tag}`
const dir = arg('dir', 'artifacts')
const out = arg('out', 'latest.json')

// 六平台：与 .github/workflows/release.yml 的 matrix 一一对应。
const EXPECTED = (arg('expect')
  || 'windows-x86_64,windows-aarch64,darwin-aarch64,darwin-x86_64,linux-x86_64,linux-aarch64')
  .split(',').map(s => s.trim()).filter(Boolean)

const OS_FROM_DIR = { macos: 'darwin', windows: 'windows', linux: 'linux' }
const ARCH_TOKENS = ['aarch64', 'x86_64']

// 各平台的 updater 安装包后缀（MSI / .deb / .dmg 都不参与自动更新）。
function isUpdaterArtifact(name, os) {
  const lower = name.toLowerCase()
  if (os === 'darwin') return lower.endsWith('.app.tar.gz')
  if (os === 'linux') return lower.endsWith('.appimage.tar.gz') || lower.endsWith('.appimage')
  return lower.endsWith('-setup.exe')
}

function walkFiles(current, collected = []) {
  for (const entry of readdirSync(current, { withFileTypes: true })) {
    const full = path.join(current, entry.name)
    if (entry.isDirectory()) walkFiles(full, collected)
    // entry.isFile() 不跟随符号链接：.app 目录里的 Versions/Current 之类不会被当成安装包。
    else if (entry.isFile()) collected.push(full)
  }
  return collected
}

function platformFromDirName(name) {
  const parts = name.split('-')
  const os = OS_FROM_DIR[parts[0]]
  if (!os) return null
  const arch = parts.find(part => ARCH_TOKENS.includes(part))
  return arch ? `${os}-${arch}` : null
}

if (!existsSync(dir)) {
  console.error(`[gen-latest-json] 产物目录不存在：${dir}`)
  process.exit(1)
}

const platforms = {}
for (const entry of readdirSync(dir, { withFileTypes: true })) {
  if (!entry.isDirectory()) continue
  const key = platformFromDirName(entry.name)
  if (!key) {
    console.error(`[gen-latest-json] 忽略无法判定平台的目录：${entry.name}（目录名需形如 macos-aarch64-…-bundles）`)
    continue
  }
  if (platforms[key]) {
    console.error(`[gen-latest-json] ${key} 对应多个产物目录（${platforms[key].dir} 与 ${entry.name}），拒绝猜测`)
    process.exit(1)
  }
  const files = walkFiles(path.join(dir, entry.name))
  const hits = files.filter(name => {
    if (name.endsWith('.sig')) return false
    const os = key.split('-')[0]
    return isUpdaterArtifact(path.basename(name), os) && files.includes(`${name}.sig`)
  })
  if (!hits.length) {
    const installers = files.filter(name => isUpdaterArtifact(path.basename(name), key.split('-')[0]))
    console.error(`[gen-latest-json] ${key}：${installers.length
      ? `找到 ${installers.map(p => path.basename(p)).join(', ')} 但都缺同名 .sig（该平台没签名？）`
      : '未找到 updater 安装包（.app.tar.gz / -setup.exe / .AppImage.tar.gz）'}`)
    continue
  }
  if (hits.length > 1) {
    // 唯一允许的歧义：Linux 同时产出裸 `.AppImage` 与 `.AppImage.tar.gz`。
    // 官方文档写的是裸 AppImage + .sig，v2 的 bundler 又可能额外打一层 tar —— 这里无法在本机验证
    // （Linux 产物只能在 CI 出），所以两者都签名时优先取 .tar.gz 并显式告警，让人在 Draft 上复核。
    const stripped = name => path.basename(name).replace(/\.tar\.gz$/i, '').toLowerCase()
    if (new Set(hits.map(stripped)).size > 1) {
      console.error(`[gen-latest-json] ${key}：一个目录里出现 ${hits.length} 个互不相干的已签名 updater 包，拒绝猜测`)
      process.exit(1)
    }
  }
  const installer = hits.find(p => /\.tar\.gz$/i.test(p)) ?? hits[0]
  if (hits.length > 1) {
    console.error(`[gen-latest-json] ${key}：${hits.map(p => path.basename(p)).join(' 与 ')} 都带 .sig（同一安装包的两层打包），取 ${path.basename(installer)}`)
  }
  const name = path.basename(installer)
  // GitHub 在上传时把 Release 资产名里的**空格规范化成 `.`**（v1.0.2 实测：磁盘上的
  // `WB Bridge_1.0.2_x64-setup.exe` 在 API 里是 `WB.Bridge_1.0.2_x64-setup.exe`），
  // 而 `/releases/download/<tag>/<空格名>` 一律 404。只 encodeURIComponent 会拼出打不开的 URL，
  // 客户端到下载那一步才发现（清单本身验不出这个问题）。
  platforms[key] = { name, url: `${base}/${encodeURIComponent(name.replace(/ /g, '.'))}`, signature: readFileSync(`${installer}.sig`, 'utf8').trim() }
  if (key.startsWith('darwin') && !/_(aarch64|x86_64)\.app\.tar\.gz$/i.test(name)) {
    console.error(`[gen-latest-json] ${key}：资产名 ${name} 没有架构后缀，两个 mac 架构会在 Release 上同名互覆盖（CI 的重命名步骤没生效？）`)
    process.exit(1)
  }
}

const missing = EXPECTED.filter(key => !platforms[key])
if (missing.length) {
  console.error(`[gen-latest-json] 缺少 ${missing.length} 个平台的已签名产物：${missing.join(', ')}`)
  console.error('  常见原因：该平台的构建作业没产出 .sig（Secrets 未配置），或产物 glob 没收到 updater 包。')
  process.exit(1)
}

const manifest = {
  version,
  pub_date: new Date().toISOString(),
  platforms: Object.fromEntries(
    Object.entries(platforms).map(([key, value]) => [key, { signature: value.signature, url: value.url }]),
  ),
}
writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`)
const keys = Object.keys(manifest.platforms)
console.log(`[gen-latest-json] ${out} 已生成（v${version}，${keys.length} 个平台：${keys.join(' / ')}）`)
for (const key of keys) console.log(`  ${key} → ${platforms[key].name}`)
