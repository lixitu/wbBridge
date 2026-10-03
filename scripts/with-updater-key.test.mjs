// scripts/with-updater-key.test.mjs
// `node --test` 基线：钉住 `npm run tauri:build` 包装器（注入器语义，不是前置校验）的
// 取值优先级、内联归一、口令处理、只告警的公钥配对，以及 package.json / release.yml /
// tauri.conf.json 三处接线。不联网、不调 tauri CLI、不读 ~/.tauri、不碰任何真实私钥。
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  DEFAULT_KEY_PATH,
  KEY_CONTENT_NAME,
  KEY_PASSWORD_NAME,
  KEY_PATH_NAME,
  buildCommand,
  expandHome,
  firstConfiguredKeyId,
  injectKey,
  isInlineKeyShape,
  keyIdFromPubFile,
  loadEnvFiles,
  parseEnvFile,
} from './with-updater-key.mjs';

// 合成一把「形态合法」的假私钥：单行 base64，解出来首行是 untrusted comment。
// 纯本地字符串，不是任何真实密钥材料；encrypted 变体只在解码文本里带上 minisign 的
// 「encrypted」字样，用来驱动口令分支。
const fakeKey = (kind = 'plain') =>
  Buffer.from(
    `untrusted comment: minisign ${kind === 'encrypted' ? 'encrypted ' : ''}secret key\nRwTHIsIsNotARealKey`,
    'utf8'
  ).toString('base64');

const pubFile = (id) =>
  Buffer.from(`untrusted comment: minisign public key: ${id}\nRWQ/pKi+bNotARealPubKey`, 'utf8').toString('base64');

const withTempDir = (fn) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wbbridge-updater-key-test-'));
  try {
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
};

// 静默 logger：把 log/warn 收进数组，既断言「说了什么」也断言「没说什么」（密钥值）。
const silent = () => {
  const lines = [];
  return {
    lines,
    log: (m) => lines.push(`log:${m}`),
    warn: (m) => lines.push(`warn:${m}`),
    error: (m) => lines.push(`error:${m}`),
  };
};

// 注入点：envFiles 指向临时目录里的文件、defaultKeyPath 指向临时目录里的假钥。
// 不传这两项就会读到仓库真实 .env.local 与 ~/.tauri —— 单测严禁碰它们。
const run = (env, dir, extra = {}) => {
  const logger = silent();
  const out = injectKey({
    env,
    cwd: dir,
    defaultKeyPath: extra.defaultKeyPath ?? path.join(dir, 'none.key'),
    envFiles: extra.envFiles ?? [path.join(dir, '.env.local'), path.join(dir, '.env')],
    logger,
  });
  return { ...out, logger };
};

test('parseEnvFile 只取键值，容忍 export、引号、注释与 CRLF', () => {
  const parsed = parseEnvFile(
    [
      '# 注释',
      '',
      `export ${KEY_CONTENT_NAME}=plainvalue`,
      `${KEY_PASSWORD_NAME}="quoted value"`,
      `${KEY_PATH_NAME}='/tmp/a b.key'`,
      'alias tauri=npx tauri',
      'IGNORED LINE',
    ].join('\r\n')
  );
  assert.equal(parsed[KEY_CONTENT_NAME], 'plainvalue');
  assert.equal(parsed[KEY_PASSWORD_NAME], 'quoted value');
  assert.equal(parsed[KEY_PATH_NAME], '/tmp/a b.key');
  assert.equal(parsed.IGNORED, undefined);
});

test('loadEnvFiles：.env.local 压过 .env，进程环境压过文件，显式空值不被文件补上', () => {
  withTempDir((dir) => {
    const local = path.join(dir, '.env.local');
    const dot = path.join(dir, '.env');
    fs.writeFileSync(local, `${KEY_CONTENT_NAME}=from-local\n${KEY_PASSWORD_NAME}=local-pw\n`);
    fs.writeFileSync(dot, `${KEY_CONTENT_NAME}=from-dotenv\n${KEY_PASSWORD_NAME}=dot-pw\n${KEY_PATH_NAME}=~/x.key\n`);

    const fromFiles = {};
    const from = loadEnvFiles(fromFiles, [local, dot]);
    assert.equal(fromFiles[KEY_CONTENT_NAME], 'from-local', '.env.local 必须盖过 .env');
    assert.equal(fromFiles[KEY_PATH_NAME], '~/x.key', '只在两份仓库文件里都没有的键才从 .env 补');
    assert.equal(from[KEY_CONTENT_NAME], '.env.local', '来源要逐项可追溯：钥与口令来自不同文件是最难归因的坑');
    assert.equal(from[KEY_PASSWORD_NAME], '.env.local');

    const preset = { [KEY_CONTENT_NAME]: 'from-process' };
    loadEnvFiles(preset, [local, dot]);
    assert.equal(preset[KEY_CONTENT_NAME], 'from-process', '已设置的变量不得被文件覆盖');

    // 显式空串 = 「这把钥确实没有口令」，压过文件里的口令；否则本机 .env 里另一把钥的口令
    // 会悄悄盖掉调用方给的空口令，而 tauri 只报「Wrong password」。
    const blank = { [KEY_PASSWORD_NAME]: '' };
    loadEnvFiles(blank, [local, dot]);
    assert.equal(blank[KEY_PASSWORD_NAME], '');

    const missing = {};
    loadEnvFiles(missing, [path.join(dir, 'no-such.env')]);
    assert.deepEqual(missing, {}, '文件不存在时静默跳过，不抛错');
  });
});

test('expandHome 展开 ~/ 与 ~，私钥全文原样返回', () => {
  assert.equal(expandHome('~/a.key'), path.join(os.homedir(), 'a.key'));
  assert.equal(expandHome('~'), os.homedir());
  assert.equal(expandHome('/abs/a.key'), '/abs/a.key');
  assert.equal(expandHome(fakeKey()), fakeKey(), '私钥全文原样返回，不得被路径逻辑改写');
  assert.equal(expandHome(undefined), undefined);
});

test('isInlineKeyShape 区分「私钥全文」与「.key 文件路径」（内联位两种都可能出现）', () => {
  assert.equal(isInlineKeyShape(fakeKey()), true);
  assert.equal(isInlineKeyShape(fakeKey('encrypted')), true);
  assert.equal(isInlineKeyShape(`${fakeKey()}\n`), true, '尾部换行不影响判定，但读取时会 trim 掉');
  assert.equal(isInlineKeyShape('/Users/x/.tauri/wbBridge-updater.key'), false, '含 . 与 _ 的字符串不是 base64');
  assert.equal(isInlineKeyShape('/tmp/mykey'), false, '路径的字符可以全在 base64 表里，靠长度（4 的倍数）与解码首行区分');
  assert.equal(isInlineKeyShape('~/wbBridge-updater.key'), false);
  assert.equal(isInlineKeyShape(''), false);
  assert.equal(isInlineKeyShape(null), false);
  // 公钥框首行同样是 untrusted comment：这里只判形态，不判私钥/公钥（配对另有 keyIdFromPubFile）。
  assert.equal(isInlineKeyShape(Buffer.from('untrusted comment: minisign public key: DEADBEEF\nRWQ/x', 'utf8').toString('base64')), true);
});

test('injectKey：内联私钥原样沿用，明文钥补齐空口令，绝不打印密钥', () => {
  withTempDir((dir) => {
    const inline = fakeKey();
    const env = { [KEY_CONTENT_NAME]: inline };
    const r = run(env, dir);
    assert.equal(r.injected, true);
    assert.equal(env[KEY_CONTENT_NAME], inline, 'CI 传的就是内联全文，不得再被文件覆盖');
    assert.equal(env[KEY_PASSWORD_NAME], '', '明文私钥必须显式置空口令：否则无 TTY 时 CLI 交互式询问并以 Device not configured (os error 6) 失败');
    assert.deepEqual(r.actions, ['沿用已有的内联私钥', '明文私钥 → 显式置空口令']);
    assert.equal(r.logger.lines.join('\n').includes(inline), false, '输出里不得出现私钥');
  });
});

test('injectKey：显式空口令不被文件里的口令覆盖，非空口令原样保留', () => {
  withTempDir((dir) => {
    fs.writeFileSync(path.join(dir, '.env.local'), `${KEY_PASSWORD_NAME}=另一个口令\n`);
    const blank = { [KEY_CONTENT_NAME]: fakeKey('encrypted'), [KEY_PASSWORD_NAME]: '' };
    const r1 = run(blank, dir);
    assert.equal(blank[KEY_PASSWORD_NAME], '', '加密态钥 + 显式空口令 → 只告警，不改值');
    assert.ok(r1.logger.lines.some((l) => l.startsWith('warn:') && /口令/.test(l)));
    assert.equal(r1.logger.lines.join('\n').includes('另一个口令'), false, '口令值不得出现在输出里');

    const set = { [KEY_CONTENT_NAME]: fakeKey('encrypted'), [KEY_PASSWORD_NAME]: 'process-pw' };
    const r2 = run(set, dir);
    assert.equal(set[KEY_PASSWORD_NAME], 'process-pw');
    assert.deepEqual(r2.actions.at(-1), '加密态私钥 + 已提供口令');
  });
});

test('injectKey：加密态私钥且没有任何口令时只告警，不阻断构建', () => {
  withTempDir((dir) => {
    const encrypted = fakeKey('encrypted');
    const env = { [KEY_CONTENT_NAME]: encrypted };
    const r = run(env, dir);
    assert.equal(r.injected, true, '前置校验已撤掉：是否在位由维护者自行确认，报错交回 tauri');
    assert.equal(KEY_PASSWORD_NAME in env, false, '加密态钥不能自作主张置空口令（那会把「缺口令」伪装成「口令是空串」）');
    assert.ok(r.logger.lines.some((l) => /未提供口令，签名将失败/.test(l)));
    assert.ok(r.actions.includes('加密态私钥 + 无口令（告警）'));
  });
});

test('injectKey：内联位放的是 .key 路径时读成全文并 trim（仓库 .env.local 就是 ~/ 形式）', () => {
  withTempDir((dir) => {
    const keyPath = path.join(dir, 'wbBridge-updater.key');
    // 结尾换行是本机实测过的坑：不 trim 时 tauri 报 Invalid symbol 10。
    fs.writeFileSync(keyPath, `${fakeKey()}\n`);

    const viaContentPath = { [KEY_CONTENT_NAME]: keyPath };
    const r1 = run(viaContentPath, dir);
    assert.equal(r1.injected, true);
    assert.equal(viaContentPath[KEY_CONTENT_NAME], fakeKey(), '路径必须读成内联全文并去掉结尾换行');
    assert.ok(r1.actions.includes('内联位放的是文件路径 → 已读成全文（trim）'));

    // ~ 不展开就永远命中失败（Node 与 tauri 都不展开），这里把家目录指到临时目录。
    const saved = { HOME: process.env.HOME, USERPROFILE: process.env.USERPROFILE };
    process.env.HOME = dir;
    process.env.USERPROFILE = dir;
    try {
      const tilde = { [KEY_PATH_NAME]: '~/wbBridge-updater.key' };
      const r2 = run(tilde, dir);
      assert.equal(r2.injected, true, '~/… 必须先展开再读文件');
      assert.equal(tilde[KEY_CONTENT_NAME], fakeKey());
      assert.equal(KEY_PATH_NAME in tilde, false, '内联值与 _PATH 互斥，注入后要删掉 _PATH');
      assert.ok(r2.actions.includes('已删除 TAURI_SIGNING_PRIVATE_KEY_PATH（与内联值互斥）'));

      const missingPtr = { [KEY_PATH_NAME]: '~/no-such.key' };
      const r3 = run(missingPtr, dir);
      assert.equal(r3.injected, false, '指针指向的文件不存在时明确失败，而不是静默交出空私钥');
      assert.ok(r3.logger.lines.some((l) => /指向的文件不存在/.test(l)));
    } finally {
      if (saved.HOME === undefined) delete process.env.HOME;
      else process.env.HOME = saved.HOME;
      if (saved.USERPROFILE === undefined) delete process.env.USERPROFILE;
      else process.env.USERPROFILE = saved.USERPROFILE;
    }
  });
});

test('injectKey：兜底路径命中时注入，哪都没有时返回 injected:false 且不回显坏值', () => {
  withTempDir((dir) => {
    const fallback = path.join(dir, 'home', '.tauri', 'wbBridge-updater.key');
    fs.mkdirSync(path.dirname(fallback), { recursive: true });
    fs.writeFileSync(fallback, fakeKey('encrypted'));
    const env = {};
    const r = run(env, dir, { defaultKeyPath: fallback });
    assert.equal(r.injected, true);
    assert.equal(env[KEY_CONTENT_NAME], fakeKey('encrypted'), '兜底 ~/.tauri/wbBridge-updater.key 要真的被用上');
    assert.ok(r.actions.includes('私钥已归一成内联值'));

    const none = run({}, dir);
    assert.equal(none.injected, false);
    assert.ok(none.logger.lines.some((l) => /未找到可用私钥/.test(l)));

    const junk = { [KEY_CONTENT_NAME]: '/no/such/wbBridge-updater.key' };
    const r2 = run(junk, dir);
    assert.equal(r2.injected, false);
    assert.equal(junk[KEY_CONTENT_NAME], '/no/such/wbBridge-updater.key', '既不是内联形态也不是存在的文件时原样透传，由 tauri 给报错');
    assert.ok(r2.logger.lines.some((l) => /值不打印/.test(l)), '警告只能说结论，不能把可疑值摊进日志');
  });
});

test('公钥配对只告警、且只认配置里的第一条（多拼不构成轮换白名单）', () => {
  const CONFIG_ID = '2B11F78BEA8A43F';
  const SECOND_ID = '126D4E208E0F17BA';
  withTempDir((dir) => {
    fs.mkdirSync(path.join(dir, 'src-tauri'), { recursive: true });
    // 两条公钥拼进同一个 pubkey（此前误记为「轮换白名单」）：客户端只读第一条，这里必须同样只取第一条。
    const both = Buffer.from(
      [
        `untrusted comment: minisign public key: ${CONFIG_ID}`,
        'RWQ/pKi+eB+xFirst',
        `untrusted comment: minisign public key: ${SECOND_ID}`,
        'RWQ1xSecondNotARealKey',
      ].join('\n'),
      'utf8'
    ).toString('base64');
    fs.writeFileSync(
      path.join(dir, 'src-tauri', 'tauri.conf.json'),
      JSON.stringify({ plugins: { updater: { pubkey: both } } })
    );
    assert.equal(firstConfiguredKeyId(dir), CONFIG_ID);
    assert.equal(firstConfiguredKeyId(path.join(dir, 'nope')), null, '读不到配置时返回 null，不抛错');

    const keyPath = path.join(dir, 'wbBridge-updater.key');
    fs.writeFileSync(keyPath, fakeKey());
    fs.writeFileSync(`${keyPath}.pub`, pubFile(CONFIG_ID));
    assert.equal(keyIdFromPubFile(`${keyPath}.pub`), CONFIG_ID, 'tauri 的 .pub 是「两行公钥框的单行 base64」');
    fs.writeFileSync(`${keyPath}.pub`, `untrusted comment: minisign public key: ${SECOND_ID}\nRWQ1x`);
    assert.equal(keyIdFromPubFile(`${keyPath}.pub`), SECOND_ID, '明文两行形态同样要能解析');
    assert.equal(keyIdFromPubFile(path.join(dir, 'nope.pub')), null);

    // 相符 → 记 OK
    fs.writeFileSync(`${keyPath}.pub`, pubFile(CONFIG_ID));
    const ok = run({ [KEY_PATH_NAME]: keyPath }, dir, { defaultKeyPath: keyPath });
    assert.ok(ok.actions.some((a) => a.startsWith('公钥配对 OK')), ok.actions.join(' / '));

    // 只对得上第二条 → 判为不配对（客户端根本不会读第二条），但**不阻断**
    fs.writeFileSync(`${keyPath}.pub`, pubFile(SECOND_ID));
    const bad = run({ [KEY_PATH_NAME]: keyPath }, dir, { defaultKeyPath: keyPath });
    assert.equal(bad.injected, true, '配对不符只告警，决定权交回构建（与参考项目一致）');
    assert.ok(bad.actions.some((a) => a.startsWith('公钥不配对')), bad.actions.join(' / '));
    assert.ok(
      bad.logger.lines.some((l) => l.includes(SECOND_ID) && l.includes(CONFIG_ID) && /客户端只用第一条验签/.test(l)),
      bad.logger.lines.join('\n')
    );

    // 缺 .pub → 只说「未核对」，不硬失败（CI 传内联全文就是这种情况）
    fs.rmSync(`${keyPath}.pub`);
    const noPub = run({ [KEY_CONTENT_NAME]: fakeKey() }, dir);
    assert.ok(!noPub.actions.some((a) => /公钥/.test(a)));
  });
});

test('buildCommand：tauri 换成仓库内 CLI，缺则 npx；flag 开头退回默认 build；其它命令原样', () => {
  withTempDir((dir) => {
    const noBin = buildCommand(['tauri', 'build', '--bundles', 'app'], dir);
    assert.deepEqual(noBin, { bin: 'npx', args: ['tauri', 'build', '--bundles', 'app'] });

    const local = path.join(dir, 'node_modules', '.bin');
    fs.mkdirSync(local, { recursive: true });
    fs.writeFileSync(path.join(local, 'tauri'), '#!/bin/sh\n');
    const viaLocal = buildCommand(['tauri', 'build', '--target', 'aarch64-apple-darwin'], dir);
    assert.equal(viaLocal.bin, path.join(local, 'tauri'), '必须用 node_modules/.bin/tauri，不再多套一层 npx');
    assert.deepEqual(viaLocal.args, ['build', '--target', 'aarch64-apple-darwin']);

    assert.deepEqual(buildCommand([], dir).args, ['build'], '省略命令时默认 tauri build');
    assert.deepEqual(buildCommand(['--bundles', 'app'], dir).args, ['build', '--bundles', 'app'], '首个 token 是 flag 时按旧写法当 build 参数');
    const other = buildCommand(['bash', 'scripts/make-dmg.sh'], dir);
    assert.deepEqual(other, { bin: 'bash', args: ['scripts/make-dmg.sh'] }, '非 tauri 命令原样执行');
  });
});

test('CLI：相对路径调用确实执行，--help 不碰私钥也不构建', () => {
  const root = path.resolve(import.meta.dirname, '..');
  // 本地用的就是**相对**路径调用；入口守卫一旦拼错，--help 会安静地不输出任何东西并退出 0。
  const help = spawnSync(process.execPath, [path.join('scripts', 'with-updater-key.mjs'), '--help'], {
    cwd: root,
    encoding: 'utf8',
  });
  assert.equal(help.status, 0, `--help 应退出 0，实际 ${help.status}：${help.stderr}`);
  assert.match(help.stdout, /用法：node scripts\/with-updater-key\.mjs/);
  assert.match(help.stdout, new RegExp(KEY_PASSWORD_NAME), '用法要列出私钥与口令的变量名（只给名字，不给值）');
  assert.match(help.stdout, /不校验私钥形态、不试签/, '契约是注入器，不是前置校验');
  assert.equal(help.stdout.includes(fakeKey()), false);
});

test('接线：tauri:build 经包装器、build 链 make:dmg、CI 读 vars 并调用 npm run tauri:build', () => {
  const root = path.resolve(import.meta.dirname, '..');
  const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  assert.equal(pkg.scripts['tauri:build'], 'node scripts/with-updater-key.mjs tauri build', '打包入口必须经包装器（否则 tauri 不读 _PATH，本地会缺钥）');
  assert.match(pkg.scripts.build, /vite:build && npm run tauri:build && npm run make:dmg/, 'build = 前端 + 签名构建 + hdiutil dmg');
  assert.equal(pkg.scripts['make:dmg'], 'bash scripts/make-dmg.sh');
  assert.ok(pkg.scripts['test:updater-key'], '本测试套件要有 npm 入口');
  assert.equal(fs.existsSync(path.join(root, 'scripts', 'make-dmg.sh')), true);

  const yml = fs.readFileSync(path.join(root, '.github', 'workflows', 'release.yml'), 'utf8');
  assert.equal(yml.includes('secrets.TAURI_SIGNING_PRIVATE_KEY'), false, '签名变量已从 Secrets 迁到 Variables');
  // 只数**真正注入环境的行**（注释里也提到 vars./secrets.，不能靠总出现次数判定）。
  const keyEnv = (yml.match(/^\s*TAURI_SIGNING_PRIVATE_KEY: \$\{\{ vars\./gm) ?? []).length;
  const pwEnv = (yml.match(/^\s*TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \$\{\{ vars\./gm) ?? []).length;
  assert.equal(keyEnv, 3, '三个 build 作业都要把 vars 私钥注入构建环境');
  assert.equal(pwEnv, 3, '三个 build 作业都要把 vars 口令注入构建环境');
  assert.equal((yml.match(/run: npm run tauri:build/g) ?? []).length, 3, '三个 build 作业都走包装器，不再用 tauri-action');
  assert.equal(yml.includes('tauri-apps/tauri-action'), false, 'tauri-action 已撤（它自己会写清单，六个并发作业互相覆盖）');
  assert.equal((yml.match(/run: node scripts\/with-updater-key\.mjs/g) ?? []).length, 0, 'CI 不得再跑私钥前置自检（2026-10-03 用户决定）');
  assert.equal(yml.includes('npm run make:dmg'), true, 'mac 的 dmg 由 hdiutil 步骤生成');

  // 配置侧：pubkey 只能有**一条**公钥（客户端 verify_signature 只读第一条），targets 不含 dmg。
  const conf = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const blocks = Buffer.from(conf.plugins.updater.pubkey, 'base64')
    .toString('utf8')
    .split('\n')
    .filter((l) => l.startsWith('untrusted comment'));
  assert.equal(blocks.length, 1, `plugins.updater.pubkey 应只内嵌一条公钥，实际 ${blocks.length} 条`);
  assert.equal(firstConfiguredKeyId(root), blocks[0].split('key:').pop().trim());
  assert.equal(conf.bundle.targets.includes('dmg'), false, 'dmg 由 scripts/make-dmg.sh 生成：Tauri 的 create-dmg 走 AppleScript，CI 无 GUI 会失败');
  assert.equal(conf.bundle.createUpdaterArtifacts, true);
  assert.equal(DEFAULT_KEY_PATH, path.join(os.homedir(), '.tauri', 'wbBridge-updater.key'), '本地兜底路径要与维护者的实际私钥位置一致');
});
