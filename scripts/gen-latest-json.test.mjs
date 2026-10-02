/* ═══════════════════════════════════════════════════════════════════
   scripts/gen-latest-json.mjs 的行为基线（npm run test:manifest）
   ───────────────────────────────────────────────────────────────────
   生成器是 latest.json 的唯一写者，它出错的表现是「客户端拿到错平台的包」或
   「某平台静默缺席」，两者都要等到用户装上才被发现，所以必须在 CI 里先失败。
   这里跑真进程（spawnSync）+ 一次性临时产物目录，不联网、不碰仓库里的真实产物；
   版本号从 tauri.conf.json 现读，测试因此不随版本推进而失效。
   ═══════════════════════════════════════════════════════════════════ */
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SCRIPT = path.join(ROOT, 'scripts', 'gen-latest-json.mjs')
const VERSION = JSON.parse(readFileSync(path.join(ROOT, 'src-tauri/tauri.conf.json'), 'utf8')).version

// 六平台的目录名 = CI 里 download-artifact 收到的 artifact 名（`${os}-${target triple}-bundles`）。
// 平台键只由目录名里的 triple 片段决定，与文件名无关 —— 下面 mac 两条刻意用不同命名佐证这点。
const SIX = {
  'macos-aarch64-apple-darwin-bundles': ['WB Bridge_1_aarch64.app.tar.gz'],
  'macos-x86_64-apple-darwin-bundles': ['WB Bridge_1_x86_64.app.tar.gz'],
  'windows-x86_64-pc-windows-msvc-bundles': ['WB Bridge_1_x64-setup.exe'],
  'windows-aarch64-pc-windows-msvc-bundles': ['WB Bridge_1_arm64-setup.exe'],
  'linux-x86_64-unknown-linux-gnu-bundles': ['WB Bridge_1_amd64.AppImage'],
  'linux-aarch64-unknown-linux-gnu-bundles': ['WB Bridge_1_arm64.AppImage'],
}

/** 造一棵产物目录树：目录名 → 文件清单；每个安装包配一个内容可辨识的 `.sig`。 */
function makeArtifacts(files, { signatures = true } = {}) {
  const base = mkdtempSync(path.join(os.tmpdir(), 'wbgen-'))
  for (const [dirName, names] of Object.entries(files)) {
    const dir = path.join(base, dirName)
    mkdirSync(dir, { recursive: true })
    for (const name of names) {
      const full = path.join(dir, name)
      writeFileSync(full, '')
      if (signatures) writeFileSync(`${full}.sig`, `sig:${name}\n`)
    }
  }
  return base
}

/** 跑生成器；expect 把「六平台齐全」的校验收窄到单平台，让各失败路径互不干扰。 */
function run(base, { out = 'latest.json', expect, tag } = {}) {
  const args = [SCRIPT, '--dir', base, '--out', path.join(base, out)]
  if (expect) args.push('--expect', expect)
  if (tag) args.push('--tag', tag)
  const result = spawnSync(process.execPath, args, { cwd: ROOT, encoding: 'utf8' })
  return {
    status: result.status,
    output: `${result.stdout ?? ''}${result.stderr ?? ''}`,
    manifest: existsSync(path.join(base, out))
      ? JSON.parse(readFileSync(path.join(base, out), 'utf8'))
      : null,
  }
}

test('六平台齐全：写出六个平台键、签名与资产名，url 指向指定 tag 且空格被编码', () => {
  const base = makeArtifacts(SIX)
  try {
    const { status, manifest, output } = run(base, { tag: 'v9.9.9' })
    assert.equal(status, 0, output)
    assert.equal(manifest.version, VERSION, '版本号必须取自 tauri.conf.json，不在脚本里重复登记')
    assert.deepEqual(
      Object.keys(manifest.platforms).sort(),
      ['darwin-aarch64', 'darwin-x86_64', 'linux-aarch64', 'linux-x86_64', 'windows-aarch64', 'windows-x86_64'],
    )
    assert.equal(manifest.platforms['darwin-aarch64'].signature, 'sig:WB Bridge_1_aarch64.app.tar.gz')
    assert.equal(
      manifest.platforms['darwin-aarch64'].url,
      'https://github.com/trexwb/wbBridge/releases/download/v9.9.9/WB%20Bridge_1_aarch64.app.tar.gz',
    )
    assert.equal(manifest.platforms['darwin-x86_64'].url.endsWith('/WB%20Bridge_1_x86_64.app.tar.gz'), true)
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('缺一个平台的已签名产物：整体失败，而不是写出缺平台的清单', () => {
  const partial = { ...SIX }
  delete partial['linux-aarch64-unknown-linux-gnu-bundles']
  const base = makeArtifacts(partial)
  try {
    const { status, manifest, output } = run(base)
    assert.equal(status, 1)
    assert.match(output, /缺少 1 个平台的已签名产物/)
    assert.match(output, /linux-aarch64/)
    assert.equal(manifest, null, '失败时不得留下 latest.json')
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('安装包缺配对 .sig（Secrets 未配置时的表现）：该平台判缺失并失败', () => {
  const base = makeArtifacts(
    { 'macos-aarch64-apple-darwin-bundles': ['WB Bridge_1_aarch64.app.tar.gz'] },
    { signatures: false },
  )
  try {
    const { status, output } = run(base, { expect: 'darwin-aarch64' })
    assert.equal(status, 1)
    assert.match(output, /都缺同名 \.sig（该平台没签名？）/)
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('mac 资产名漏了架构后缀（CI 重命名没生效）：必须失败，不能让两个架构同名互覆盖', () => {
  const base = makeArtifacts({ 'macos-aarch64-apple-darwin-bundles': ['WB Bridge.app.tar.gz'] })
  try {
    const { status, output } = run(base, { expect: 'darwin-aarch64' })
    assert.equal(status, 1)
    assert.match(output, /没有架构后缀/)
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('同一平台出现两个产物目录：拒绝猜测，直接失败', () => {
  const base = makeArtifacts({
    'macos-aarch64-apple-darwin-bundles': ['WB Bridge_1_aarch64.app.tar.gz'],
    'macos-aarch64-apple-darwin-extra': ['WB Bridge_1_aarch64.app.tar.gz'],
  })
  try {
    const { status, output } = run(base, { expect: 'darwin-aarch64' })
    assert.equal(status, 1)
    assert.match(output, /对应多个产物目录/)
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('目录名判定不了平台：忽略该目录，不影响其余平台', () => {
  const base = makeArtifacts({
    'macos-aarch64-apple-darwin-bundles': ['WB Bridge_1_aarch64.app.tar.gz'],
    'misc-things': ['readme.txt'],
  })
  try {
    const { status, output, manifest } = run(base, { expect: 'darwin-aarch64' })
    assert.equal(status, 0, output)
    assert.match(output, /忽略无法判定平台的目录：misc-things/)
    assert.deepEqual(Object.keys(manifest.platforms), ['darwin-aarch64'])
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('Linux 同时产出裸 .AppImage 与 .AppImage.tar.gz：取 tar.gz 并告警，不算歧义失败', () => {
  const base = makeArtifacts({
    'linux-x86_64-unknown-linux-gnu-bundles': [
      'WB Bridge_1_amd64.AppImage',
      'WB Bridge_1_amd64.AppImage.tar.gz',
    ],
  })
  try {
    const { status, output, manifest } = run(base, { expect: 'linux-x86_64' })
    assert.equal(status, 0, output)
    assert.match(output, /同一安装包的两层打包/)
    assert.match(manifest.platforms['linux-x86_64'].url, /\.AppImage\.tar\.gz$/)
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('一个目录里出现互不相干的两个已签名 updater 包：拒绝猜测', () => {
  const base = makeArtifacts({
    'windows-x86_64-pc-windows-msvc-bundles': [
      'WB Bridge_1_x64-setup.exe',
      'WB Bridge_1_x64-portable-setup.exe',
    ],
  })
  try {
    const { status, output } = run(base, { expect: 'windows-x86_64' })
    assert.equal(status, 1)
    assert.match(output, /互不相干的已签名 updater 包/)
  } finally {
    rmSync(base, { recursive: true, force: true })
  }
})

test('产物目录不存在：失败并说明路径，不产出空清单', () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'wbgen-'))
  const base = path.join(parent, 'no-such-dir')
  try {
    const { status, output, manifest } = run(base)
    assert.equal(status, 1)
    assert.match(output, /产物目录不存在/)
    assert.equal(manifest, null)
  } finally {
    rmSync(parent, { recursive: true, force: true })
  }
})
