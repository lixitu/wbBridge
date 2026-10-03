/* ═══════════════════════════════════════════════════════════════════
   WB Bridge — 构建签名环境注入（tauri:build 包装器）
   ───────────────────────────────────────────────────────────────────
   写法与规则对齐参考项目 fastenerTradeWorkbench / lockPass 的
   scripts/with-updater-key.mjs：本脚本**不做**私钥前置校验（不判形态、不试签、
   不因为配对不符而挡住构建），只把 updater 私钥与口令汇齐、归一成 `tauri build`
   认得的内联形式，然后执行目标命令。

   背景：bundle.createUpdaterArtifacts=true 且 tauri.conf.json 内嵌公钥，打包时 Tauri
   必须有 updater 私钥，否则报「A public key has been found, but no private key」。
   CI 经 GitHub **Repository Variables** 注入；本地没手动 export 时这里自动补齐。

   取值规则（前者命中即止）：
     1) 进程环境 / env 文件里已设置 TAURI_SIGNING_PRIVATE_KEY：
        单行 base64 → 原样沿用；放的是 .key 文件路径（含 ~/…，tauri 不展开 ~）→ 读成内联全文
     2) 未设置 → 按 TAURI_SIGNING_PRIVATE_KEY_PATH（.env.local / 手动 export）
        与 ~/.tauri/wbBridge-updater.key 的顺序找到私钥文件，读成内联值
     3) 都没有 → 原样透传并打印指引（缺钥报错交回 Tauri 自己给）
   env 文件顺序：仓库 .env.local > 仓库 .env > ~/.tauri/wbBridge.env > ~/.tauri/wbBridge-updater.env，
   且**只在变量未设置时**写入（显式空串也算已设置，用于表达「这把钥确实没有口令」）。

   ⚠ 关键事实（本机实测）：`tauri build` 的打包签名只认 TAURI_SIGNING_PRIVATE_KEY
   （值可以是私钥全文，也可以是指向私钥文件的路径）；TAURI_SIGNING_PRIVATE_KEY_PATH
   只对 `tauri signer` 子命令生效，打包器完全不读 —— 所以这里一律归一成内联值，
   并删掉 _PATH（CLI 的 --private-key 与 --private-key-path 互斥，同时给会被拒）。

   纪律：私钥与口令**绝不打印**，只输出来源文件名与公开的 key ID。
   ═══════════════════════════════════════════════════════════════════ */
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

export const KEY_CONTENT_NAME = 'TAURI_SIGNING_PRIVATE_KEY';
export const KEY_PATH_NAME = 'TAURI_SIGNING_PRIVATE_KEY_PATH';
export const KEY_PASSWORD_NAME = 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const DEFAULT_KEY_PATH = path.join(os.homedir(), '.tauri', 'wbBridge-updater.key');

// 取值优先级由 loadEnvFiles 的入参顺序决定（先命中者生效）：仓库 .env.local > 仓库 .env >
// ~/.tauri/wbBridge.env > ~/.tauri/wbBridge-updater.env。后两个是维护者本机既有的私钥习惯
// 位置（与参考项目同款），存在才读。进程环境始终压过文件。
export const ENV_FILES = [
  path.join(ROOT, '.env.local'),
  path.join(ROOT, '.env'),
  path.join(os.homedir(), '.tauri', 'wbBridge.env'),
  path.join(os.homedir(), '.tauri', 'wbBridge-updater.env'),
];

// 极简 dotenv（支持 export 前缀 / 引号值 / 注释 / alias 行忽略）。
export function parseEnvFile(text) {
  const out = {};
  for (const rawLine of String(text).split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#') || line.startsWith('alias ')) continue;
    const m = /^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)=(.*)$/.exec(line);
    if (!m) continue;
    let value = m[2];
    const quote = value[0];
    if ((quote === '"' || quote === "'") && value.length >= 2 && value.endsWith(quote)) {
      value = value.slice(1, -1);
    }
    out[m[1]] = value;
  }
  return out;
}

// `~` 只有 shell 会展开，Node/tauri 都不展开：不处理会让 .env.local 里的
// `~/.tauri/…key` 永远命中失败，静默跳到下游只剩「打包器缺私钥」那句报错。
export function expandHome(value) {
  if (typeof value !== 'string' || !value) return value;
  if (value === '~') return os.homedir();
  if (value.startsWith('~/')) return path.join(os.homedir(), value.slice(2));
  return value;
}

// 只在变量「未设置」时写入（显式空串也算已设置）；同时记录每项的来源文件名，
// 因为「钥来自环境变量、口令来自 .env.local」这种混搭出问题时只报口令不对，归因极难。
// 口令值本身绝不返回、绝不打印。
export function loadEnvFiles(env = process.env, files = ENV_FILES) {
  const from = {};
  for (const file of files) {
    if (!fs.existsSync(file)) continue;
    for (const [k, v] of Object.entries(parseEnvFile(fs.readFileSync(file, 'utf8')))) {
      if (k in env) continue;
      env[k] = v;
      if (k === KEY_CONTENT_NAME || k === KEY_PATH_NAME || k === KEY_PASSWORD_NAME) {
        from[k] = path.basename(file);
      }
    }
  }
  return from;
}

// 客户端验签只认 pubkey 里**第一条**公钥：tauri-plugin-updater 的 verify_signature 走
// minisign-verify 的 PublicKey::decode，它只读解码文本的前两行，后面的公钥被静默丢弃。
// 所以「把两把公钥拼进同一个 pubkey」并不等于轮换白名单（2026-10-03 实跑踩实）。
export function firstConfiguredKeyId(cwd = ROOT) {
  let pubkey;
  try {
    const conf = JSON.parse(fs.readFileSync(path.join(cwd, 'src-tauri', 'tauri.conf.json'), 'utf8'));
    pubkey = conf?.plugins?.updater?.pubkey;
  } catch {
    return null;
  }
  if (!pubkey) return null;
  const m = Buffer.from(String(pubkey).trim(), 'base64').toString('utf8')
    .match(/minisign public key:\s*([0-9A-Fa-f]+)/);
  return m ? m[1].toUpperCase() : null;
}

// .pub 是公开值。两种落盘形态都要支持：两行明文公钥框，或该文本的单行 base64（tauri 的 .pub 就是后者）。
export function keyIdFromPubFile(pubPath) {
  let text;
  try {
    text = fs.readFileSync(pubPath, 'utf8').trim();
  } catch {
    return null;
  }
  for (const candidate of [text, Buffer.from(text, 'base64').toString('utf8')]) {
    const m = candidate.match(/minisign public key:\s*([0-9A-Fa-f]+)/);
    if (m) return m[1].toUpperCase();
  }
  return null;
}

// 内联位（TAURI_SIGNING_PRIVATE_KEY）里放的可能是私钥全文，也可能是 .key 文件路径
// （`tauri build` 两种都接受），仓库 `.env.local` 走的就是 `~/…key` 这条。两者靠形态区分：
// minisign 私钥框是「单行 base64，且解出来的首行以 untrusted comment 开头」；不满足就按路径处理。
// 只看字符集会误判——`/tmp/mykey` 的字符全在 base64 表里，所以必须再比长度与解码首行。
export function isInlineKeyShape(value) {
  const text = String(value ?? '').trim();
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(text) || text.length % 4 !== 0) return false;
  return Buffer.from(text, 'base64').toString('utf8').split('\n', 1)[0].startsWith('untrusted comment');
}

// 读私钥文件成内联全文。**必须 trim**：2026-10-03 本机实测过，值尾部带换行时 tauri 报
// 「failed to decode base64 secret key: Invalid symbol 10」，而 .key 文件常带结尾换行。
function readKeyFile(file) {
  return fs.readFileSync(file, 'utf8').trim();
}

/*
 * 汇齐私钥与口令、归一成内联值。只改 env，返回实际发生的事（供打印与单测断言）；
 * 私钥与口令的值绝不进返回值。
 */
export function injectKey({
  env = process.env,
  cwd = ROOT,
  defaultKeyPath = DEFAULT_KEY_PATH,
  envFiles = ENV_FILES,
  logger = console,
} = {}) {
  const actions = [];
  for (const [k, file] of Object.entries(loadEnvFiles(env, envFiles))) actions.push(`${k} 取自 ${file}`);

  let keyFile = null;
  const inline = env[KEY_CONTENT_NAME];
  if (inline) {
    const asPath = expandHome(inline);
    if (isInlineKeyShape(inline)) {
      actions.push('沿用已有的内联私钥');
      // 内联全文没有可核对的 .pub；_PATH 另指真实文件时仍可离线核对配对
      if (env[KEY_PATH_NAME]) {
        const cand = expandHome(env[KEY_PATH_NAME]);
        if (fs.existsSync(cand)) keyFile = cand;
      }
    } else if (fs.existsSync(asPath)) {
      env[KEY_CONTENT_NAME] = readKeyFile(asPath);
      keyFile = asPath;
      logger.log(`[with-updater-key] 已把 ${KEY_CONTENT_NAME} 里的文件路径读成内联私钥（${path.basename(asPath)}）`);
      actions.push('内联位放的是文件路径 → 已读成全文（trim）');
    } else {
      logger.warn(
        `[with-updater-key] 警告：${KEY_CONTENT_NAME} 既不是单行 base64 私钥，也不是存在的文件（值不打印）。` +
        '原样交给 tauri，由打包器给出缺钥/解码报错。'
      );
      return { injected: false, actions };
    }
  } else {
    const ptr = env[KEY_PATH_NAME] ? expandHome(env[KEY_PATH_NAME]) : null;
    let injected = false;
    for (const cand of [ptr, defaultKeyPath].filter(Boolean)) {
      if (!fs.existsSync(cand)) {
        // 显式暴露「指针存在但文件缺失」，否则下游只见打包器缺钥报错
        if (ptr && cand === ptr) logger.warn(`[with-updater-key] 警告：${KEY_PATH_NAME} 指向的文件不存在：${cand}`);
        continue;
      }
      try {
        env[KEY_CONTENT_NAME] = readKeyFile(cand);
        keyFile = cand;
        logger.log(`[with-updater-key] 已注入内联签名私钥（来自 ${path.basename(cand)}）`);
        injected = true;
        break;
      } catch (e) {
        logger.warn(`[with-updater-key] 读取私钥失败：${e && e.message}`);
      }
    }
    if (!injected) {
      logger.warn(
        `[with-updater-key] 未找到可用私钥。tauri build 只认 ${KEY_CONTENT_NAME}` +
        `（私钥全文或文件路径），不读 ${KEY_PATH_NAME}；请用 npm run tauri:build 走本包装器，` +
        '或自行 export 该变量。'
      );
      return { injected: false, actions };
    }
    actions.push('私钥已归一成内联值');
  }


  // 内联与路径互斥：同时给会被 CLI 拒绝
  if (env[KEY_PATH_NAME]) {
    delete env[KEY_PATH_NAME];
    actions.push(`已删除 ${KEY_PATH_NAME}（与内联值互斥）`);
  }

  // 口令处理：加密态钥缺口令 → 只告警（决定权交回构建）；明文钥必须**显式**置空口令，
  // 否则无 TTY 环境下 CLI 会尝试交互式询问并以「Device not configured (os error 6)」失败。
  const decodedHead = (() => {
    const first = String(env[KEY_CONTENT_NAME] || '').split(/\r?\n/)[0].replace('untrusted comment: ', '');
    return Buffer.from(first, 'base64').toString('utf8');
  })();
  if (decodedHead.includes('encrypted')) {
    if (!env[KEY_PASSWORD_NAME]) {
      logger.warn(`[with-updater-key] 警告：私钥为加密态但未提供口令，签名将失败（检查 .env.local 的 ${KEY_PASSWORD_NAME}）`);
      actions.push('加密态私钥 + 无口令（告警）');
    } else {
      actions.push('加密态私钥 + 已提供口令');
    }
  } else if (!(KEY_PASSWORD_NAME in env)) {
    env[KEY_PASSWORD_NAME] = '';
    actions.push('明文私钥 → 显式置空口令');
  }

  // 公钥配对核对：只在能离线拿到同目录 .pub 时做（CI 传的是内联全文，没有 .pub 可核对）。
  // 只告警、不阻断 —— 与参考项目一致。
  if (keyFile) {
    const id = keyIdFromPubFile(`${keyFile}.pub`);
    const configured = firstConfiguredKeyId(cwd);
    if (!id) {
      logger.warn('[with-updater-key] 警告：无法从同目录 .pub 解析 key ID，未核对公钥配对');
    } else if (!configured) {
      logger.warn('[with-updater-key] 警告：tauri.conf.json 未配 plugins.updater.pubkey，未核对公钥配对');
    } else if (id !== configured) {
      logger.warn(
        `[with-updater-key] 警告：公钥与配置不符 —— 这把钥的公钥是 ${id}，而 plugins.updater.pubkey` +
        ` 里生效的第一条是 ${configured}。客户端只用第一条验签，这样签出的 .sig 会被判无效` +
        '（tauri build 同样会报 does not match the public key from plugins > updater > pubkey）。'
      );
      actions.push(`公钥不配对（私钥 ${id} ≠ 配置第一条 ${configured}）`);
    } else {
      logger.log(`[with-updater-key] 公钥配对 OK：${id} 与配置 pubkey 生效的那条一致`);
      actions.push(`公钥配对 OK（${id}）`);
    }
  }
  return { injected: true, actions };
}

function tauriCli(cwd) {
  const local = path.join(cwd, 'node_modules', '.bin', 'tauri');
  return fs.existsSync(local) ? { bin: local, prefix: [] } : { bin: 'npx', prefix: ['tauri'] };
}

// 命令契约对齐参考项目：`… with-updater-key.mjs tauri build [参数…]`。
// `tauri` 换成仓库内的 CLI（node_modules/.bin/tauri，缺则 npx），其余 token 原样透传。
// 省略命令、或首个 token 就是 flag 时退回默认 `tauri build`，flag 当 build 参数。
export function buildCommand(tokens = [], cwd = process.cwd()) {
  const cli = tauriCli(cwd);
  const head = tokens[0];
  if (!head || head.startsWith('-')) return { bin: cli.bin, args: [...cli.prefix, 'build', ...tokens] };
  if (head === 'tauri' || /(^|[\\/])tauri(\.cmd|\.exe)?$/.test(head)) {
    return { bin: cli.bin, args: [...cli.prefix, ...tokens.slice(1)] };
  }
  return { bin: head, args: tokens.slice(1) };
}

export function main(argv = process.argv.slice(2), { env = process.env, cwd = process.cwd(), logger = console } = {}) {
  if (argv.includes('--help') || argv.includes('-h')) {
    logger.log(`用法：node scripts/with-updater-key.mjs [命令…]  （= npm run tauri:build 的默认入口）

  npm run tauri:build
  node scripts/with-updater-key.mjs tauri build --bundles app
  node scripts/with-updater-key.mjs tauri build --target aarch64-apple-darwin

汇齐 updater 私钥与口令、归一成 tauri build 认得的内联形式，然后执行目标命令
（省略命令时默认 \`tauri build\`）。取值顺序（前者命中即止）：
  1. 进程环境的 ${KEY_CONTENT_NAME}（私钥全文或 .key 路径；CI 用 Repository Variables 注入）
  2. 进程环境的 ${KEY_PATH_NAME}（只对 signer 生效，这里会读成全文）
  3. 仓库 .env.local / .env 与 ~/.tauri/wbBridge.env、~/.tauri/wbBridge-updater.env 里的同名键
  4. 兜底 ~/.tauri/wbBridge-updater.key
口令取 ${KEY_PASSWORD_NAME}；明文私钥会被显式置成空口令（否则无 TTY 时 CLI 会交互式询问并失败）。
本脚本不校验私钥形态、不试签、不因配对不符挡住构建，只汇齐并注入。
私钥与口令全程不打印，只输出来源文件名与公开的 key ID。`);
    return 0;
  }

  for (const line of injectKey({ env, cwd }).actions) logger.log(`[with-updater-key] ${line}`);

  const command = buildCommand(argv, cwd);
  const result = spawnSync(command.bin, command.args, {
    stdio: 'inherit',
    env,
    cwd,
    shell: process.platform === 'win32',
  });
  if (result.error) {
    logger.error(`[with-updater-key] 无法执行 ${command.bin}：${result.error.message}`);
    return 1;
  }
  return result.status ?? 1;
}

// 入口守卫：`node scripts/with-updater-key.mjs` 传的是**相对**路径，直接拼
// `file://${argv[1]}` 永远不相等，CLI 会静默不执行。
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  process.exitCode = main();
}
