# AGENTS.md — WB Bridge 桥接服务 (wbBridge)

## ⚠ 强制规范（所有 Agent 必须遵守）

**本项目是「Tauri 托盘壳 + 独立 Node sidecar 核心（`src/core/`）」的桥接工具：核心进程 `src/core/src/main.js` 由 `@yao-pkg/pkg` 打包成单文件可执行 sidecar，在本机回环地址 `127.0.0.1:41980` 上提供 OpenAI 兼容 API，把**隔离托管**的 OpenCode 免费模型发布给 WorkBuddy；控制面板 `src/ui/` 是纯静态页面，由 Tauri WebView 加载、经 `window.buddy.*` IPC 驱动；`src-tauri/` 已入库 `Cargo.toml` / `build.rs` / `tauri.conf.json` / Rust 主源码（`src/main.rs` + `src/lib.rs`）与图标、已构建的 sidecar 二进制。**

> 🔴 **所有 Agent（包括 file-agent、browser-agent、computer-agent 等一切主 Agent、Sub-Agent、子代理）在本项目中执行任何任务时，必须无条件遵守本 `AGENTS.md` 定义的全部规则，不得以任何理由违反。**
>
> 🔴 **文件操作根目录**：本项目所有文件操作默认以 `/Users/wbtrex/website/localServer/node/trexwb/git/wbBridge` 为根目录，**不得偏离**。
>
> 🔴 **仓库根目录没有 `package.json`**：不存在根级 `npm run dev` / `npm run tauri` 等脚本，npm 命令必须在 `src/core/` 下执行（或使用 `--prefix src/core`）。严禁臆造不存在的脚本、命令与路径。
>
> 🔴 **实读优先、禁止猜测**：本文件所有结论均以仓库真实代码为准。修改任何模块前必须先 `read_text` 读取原文，禁止凭记忆推测函数名、常量名、路由、错误码与配置字段。
>
> 🔴 **最小改动优先**：只做针对性修复，禁止重构、禁止大范围重写、禁止"顺手优化"。
>
> 🔴 **密钥零泄漏**：不得读取、打印、回显、提交或推断 `api-key`、`.env.local`、用户 `~/.workbuddy/models.json` 中的凭据字段。
>
> 🔴 **提交由用户决定**：Agent 完成改动后不得自动执行 `git commit` / `git push`，除非用户在本轮明确要求。

## 项目概述与定位

WB Bridge（包名 `wbbridge-core`）是一个**跨平台托盘工具**，通过**隔离的 OpenCode 技术**，为 WorkBuddy 提供免费模型服务。它做三件事：

1. **托管一个隔离的 OpenCode 运行时**：优先复用本机已有的 `opencode` 可执行文件，否则从 npm registry（官方源 → 国内镜像）下载官方 tarball，校验 `sha512-` 完整性后解包到数据目录，用**隔离环境变量 + 独立随机端口**以 `serve --pure` 方式启动，绝不污染用户 OpenCode 的配置、数据、缓存与登录态。
2. **对外暴露 OpenAI 兼容 API**：本地 `127.0.0.1:41980` 提供 `/v1/models` 与 `/v1/chat/completions`（含 SSE 流式），强制 `Authorization: Bearer <api-key>` 鉴权，拒绝一切浏览器 Origin，最多 4 个并发请求。
3. **把可用模型同步给 WorkBuddy**：自动发现免费模型 → 逐模型探测 → 只把探测通过的模型以 OpenAI 兼容条目写入 WorkBuddy 的 `models.json`（原子写 + 增量合并，只增删自己名下的条目）。

三层结构：

| 层 | 位置 | 技术 | 现状 |
|---|---|---|---|
| 核心 sidecar | `src/core/src/`（打包产物 `src-tauri/binaries/wbbridge-core-<triple>[.exe]`） | Node.js ESM，`engines.node >= 22`，运行期依赖仅 `tar` + `undici` | 已入库，测试齐全 |
| 控制面板 | `src/ui/` | 原生 HTML/CSS/JS，无框架、无外部请求（CSP `connect-src 'none'`） | 已入库 |
| 桌面壳 | `src-tauri/` | Tauri（Rust） | 已入库（`Cargo.toml` / `tauri.conf.json` / `build.rs` / `src/` / `icons/` / `binaries/`） |

## 常用命令（安装 / 开发 / 构建 / 测试 / 签名发布）

### 环境前置

- Node.js **>= 22**（`src/core/package.json` 的 `engines`）。本机实测版本：node `v24.21.0` / npm `11.19.0`。
- 本机 Node 由 nvm 管理，**非登录 shell 的 PATH 中可能没有 `node`**。执行任何 npm 命令前先载入：
  ```bash
  export NVM_DIR="$HOME/.nvm"; . "$NVM_DIR/nvm.sh"
  ```

### 命令清单

| 目的 | 命令 | 执行目录 | 说明 |
|---|---|---|---|
| 安装依赖 | `npm install` | `src/core/` | 该项目唯一需要安装依赖的包（`tar`、`undici`；devDeps `@yao-pkg/pkg`、`esbuild`） |
| 启动核心服务 | `npm start`（等价 `node src/main.js`） | `src/core/` | 监听 `127.0.0.1:41980`（可用 `BUDDY_PORT` 覆盖），写数据目录、拉起 OpenCode 子进程 |
| 运行测试 | `npm test`（等价 `node --test test/*.test.js`） | `src/core/` | 基准结果：**97 通过 / 0 失败**，耗时约 5.3s（node v24.21.0） |
| 打包 sidecar | `node scripts/build-sidecar.mjs` | 仓库根 | 默认打包宿主平台；`--targets=all` 全平台；`--targets=aarch64-apple-darwin,x86_64-pc-windows-msvc` 指定平台 |
| 生成图标资产 | `node build-icons.mjs` | `docs/brand/` | 从 `logo-v2.svg` 生成 Tauri 全平台图标到 `docs/brand/out/`；依赖 `iconutil` 与 `docs/brand` 内独立安装的 `sharp`、`png-to-ico` |
| 单图渲染 | `node render.mjs <input.svg> <output.png> <size>` | `docs/brand/` | 依赖 `sharp` |
| 健康检查 | `curl -H "Authorization: Bearer $(cat "$HOME/Library/Application Support/Buddy Bridge/api-key")" http://127.0.0.1:41980/health` | 任意 | `/health` **同样需要 Bearer 鉴权**（无 key 返回 401） |
| 列出已发布模型 | `curl -H "Authorization: Bearer <key>" http://127.0.0.1:41980/v1/models` | 任意 | 返回客户端可见模型（id 形如 `OC · 名称`） |

### 签名与发布

- 环境变量模板：复制 `.env.example` → `.env.local`（`.env.local` 已被 `.gitignore` 忽略，**严禁提交**）：
  - `TAURI_SIGNING_PRIVATE_KEY_PATH`：更新产物签名私钥路径，默认 `~/.tauri/wbBridge-updater.key`
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥密码
  - 备用方式：内联 `TAURI_SIGNING_PRIVATE_KEY`（仅当 CI 不支持 PATH 形式时启用）
- 密钥生成命令（`.env.example` 中给出）：
  ```bash
  npm run tauri -- signer generate -p <密码> -w ~/.tauri/wbBridge-updater.key
  ```
  > ⚠ 该命令依赖 **尚未完整入库** 的 Tauri 壳（`Cargo.toml` 已入库，但仍缺 `tauri.conf.json` 与根级 `@tauri-apps/cli`），**在当前仓库状态下不可执行**，仅作为约定记录。
- **当前仓库不存在完整的签名发布链路**（`.github/workflows/release.yml` 已定义 CI 构建工作流，但尚未实际运行验证）。壳源码已入库，签名需按 `.env.example` 约定配置密钥。

### sidecar 打包细节（`scripts/build-sidecar.mjs`）

- 使用 `src/core/node_modules/.bin/pkg` 直接打包 `src/core/src/main.js`，**不做 esbuild 预打包**（源码含顶层 `await`，预打包方案不可靠）。
- 目标平台映射（`ALL`）：`aarch64-apple-darwin`、`x86_64-apple-darwin`、`x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`；默认仅打包宿主对应平台。
- 传入 `--compress GZip`，Windows 目标自动加 `.exe` 后缀，产物 `chmod 755`。
- 输出目录 `src-tauri/binaries/`，文件名必须为 `wbbridge-core-<target-triple>[.exe]`（Tauri `externalBin` 命名约定）。已构建 6 个产物（darwin / linux / windows 各 ×2 架构），单文件约 55–77MB，合计约 401MB。

### 环境变量（核心 sidecar）

| 变量 | 默认值 | 作用 |
|---|---|---|
| `BUDDY_PORT` | `41980` | HTTP 端口；非 1024–65535 的整数直接抛错退出 |
| `BUDDY_DATA_DIR` | 平台数据目录（见下） | 覆盖数据目录 |
| `BUDDY_MODELS_FILE` | macOS/Linux `~/.workbuddy/models.json` | 覆盖 WorkBuddy 配置文件路径（Windows 走"已保存值 → 发现"流程） |
| `BUDDY_NO_SYNC` | 未设置 | `=1` 时跳过启动导入与后续模型同步 |
| `BUDDY_OPENCODE_PATH` | 无 | 首选 OpenCode 可执行文件路径（优先级最高） |

数据目录（`src/core/src/platform.js`）：macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`。目录内文件：

| 文件 / 目录 | 用途 |
|---|---|
| `api-key` | 32 字节随机 hex（权限 `0600`），所有 HTTP 请求的 Bearer 令牌 |
| `service.pid` | 单实例锁；进程已存活时启动直接 `exit(2)` 并打印 `WB Bridge is already running` |
| `settings.json` | 持久化 `useSystemProxy`、`workBuddyModelsFile` |
| `status.json` | 服务状态快照（UI 与外部读取的唯一状态源），串行 + 原子写 |
| `opencode.log` | OpenCode 子进程 stdout/stderr；超过 5MB 轮转为 `opencode.log.previous` |
| `runtime/<version>/opencode` | 托管下载的 OpenCode 运行时 |
| `opencode/{config,data,cache,state,project}` | 隔离的 OpenCode XDG 目录（`0700`） |

## 目录结构说明

```
wbBridge/
├── README.md                     ← 项目简述（基于 Tauri 的托盘工具，为 WorkBuddy 提供免费模型）
├── package.json                  ← 根级便利脚本（test / sidecar / dev / build / icons），非 npm 工作区
├── src/                          ← 功能代码统一归入 src/
│   ├── core/                     ← 核心 sidecar（独立 Node 包，唯一有 npm 脚本的目录）
│   │   ├── package.json          ← 包名 wbbridge-core、version 1.0.0、type=module、engines.node>=22
│   │   ├── src/                  ← 核心源码（ESM，16 个模块）
│   │   │   ├── main.js           ← sidecar 入口：顶层 await 编排启动/探测/同步/关停（无导出）
│   │   │   ├── server.js         ← HTTP 服务：路由、Bearer 鉴权、Origin 拒绝、并发上限、SSE、请求计时
│   │   │   ├── protocol.js       ← OpenAI 兼容协议层：prepare / decode / completion / sendSSE / BridgeError
│   │   │   ├── backend.js        ← OpenCode HTTP 客户端：会话、事件流、原生工具审批、免费模型发现
│   │   │   ├── runtime.js        ← 运行时发现/下载/校验/启动 + 隔离配置 isolatedConfig
│   │   │   ├── probe.js          ← 模型探测（工具调用与纯文本两类；整批共享 60s 预算）
│   │   │   ├── repair.js         ← 信封/工具格式修复与辅助模型转写（含客户端约定 CLIENT_CONVENTIONS）
│   │   │   ├── handoff.js        ← 原生工具 handoff：构建客户端动作、拒绝反馈、动作校验
│   │   │   ├── sync.js           ← WorkBuddy models.json 原子写 + 增量合并（OWNER 标记）
│   │   │   ├── workbuddy-config.js ← models.json 定位与校验（不猜测、不创建文件）
│   │   │   ├── system-proxy.js   ← 系统代理解析（macOS scutil / Windows 注册表）与子进程环境
│   │   │   ├── reasoning.js      ← 推理档位（reasoning effort）映射与 WorkBuddy 回退策略
│   │   │   ├── model-status.js   ← 模型状态构造、请求元信息、客户端展示 ID（`OC · 名称`）
│   │   │   ├── platform.js       ← 数据目录与运行时包名（按平台/架构推导）
│   │   │   ├── atomic.js         ← 原子替换（重试 → 等待 → 换名），替代裸 rename
│   │   │   └── json.js           ← 容错 JSON 解析
│   │   ├── test/                 ← node:test 测试（11 个 *.test.js：`activity` / `atomic` / `bridge` / `lifecycle` / `platform` / `repair` / `runtime` / `shutdown` / `system-proxy` / `watchdog` / `workbuddy-config`；fixtures 假运行时、runtime-loader.mjs）
│   │   └── dist/                 ← 构建产物（core.mjs、test-core；被 .gitignore 忽略）
│   └── ui/                       ← 控制面板（纯静态，由 Tauri WebView 加载）
│       ├── index.html            ← 面板结构 + CSP（default-src 'self'、script-src 'self'、connect-src 'none'）
│       ├── renderer.js           ← 渲染与动作（window.buddy.action / onState / onDismiss）
│       ├── activity.cjs          ← 活动文案共享模块（托盘与控制面板共用）
│       └── style.css             ← 面板样式
├── src-tauri/                    ← Tauri 壳
│   ├── Cargo.toml                ← 壳工程清单（name = wbbridge、version = 1.0.0、tauri = "2.9"）
│   ├── tauri.conf.json           ← Tauri 配置（frontendDist 指向 src/ui）
│   ├── build.rs                  ← tauri-build 构建脚本
│   ├── src/                      ← Rust 主源码（main.rs + lib.rs）
│   ├── icons/                    ← 壳/托盘图标（icon.icns、icon.ico、tray.png、各尺寸 PNG、logo.svg）
│   └── binaries/                 ← sidecar 产物，命名必须为 wbbridge-core-<target-triple>[.exe]
├── scripts/
│   └── build-sidecar.mjs         ← 用 @yao-pkg/pkg 打包 src/core/src/main.js → src-tauri/binaries/
├── docs/
│   ├── research/upstream-architecture.md   ← 上游参考实现（参考 https://github.com/louchi1984-coder/ow-bridge）架构规格书（移植调研基线，402 行）
│   └── brand/                    ← 品牌资产：logo.svg / logo-v2.svg / tray.* 、render.mjs、build-icons.mjs、out/
├── .github/workflows/
│   └── release.yml               ← CI 构建与发布工作流（六平台）
├── .env.example                  ← 签名环境变量模板（复制为 .env.local）
├── .gitignore                    ← 忽略 node_modules、dist/、out/、.env*、package-lock.json、.zwork/、src-tauri/target、src-tauri/gen
└── .zwork/                       ← 本地后台任务日志与产物（被 .gitignore 忽略）
```

> 注意：`src/ui/` 由 Tauri WebView 加载（`tauri.conf.json` 的 `frontendDist` 指向 `../src/ui`）；`src/ui/activity.cjs` 也被设计为托盘与面板共用的文案模块。改动 `src/ui/` 时不得擅自改动其对外契约（`window.buddy.*`）。

## 核心架构与数据流

### 进程模型

```
┌──────────────── Tauri 壳 ──────────────────────────┐
│  WebView: src/ui/index.html + renderer.js           │
│      ↕ window.buddy.action / onState / onDismiss   │
└───────────────┬────────────────────────────────────┘
                │ spawn（externalBin: wbbridge-core-<triple>）
                ▼
     src/core/src/main.js（sidecar，单实例，pid 锁）
        ├── HTTP 服务 127.0.0.1:41980（Bearer 鉴权、拒绝 Origin、并发 ≤4）
        ├── status.json（串行 + 原子写）→ 壳/UI 读取
        ├── ~/.workbuddy/models.json（原子写 + 增量合并）
        └── OpenCode 子进程（隔离 env + 随机回环端口 + serve --pure）
```

### 启动时序（`src/core/src/main.js`）

1. 取数据目录（`BUDDY_DATA_DIR` 或 `platform.dataDirectory()`），`mkdir 0700`。
2. **单实例锁**：读 `service.pid`，`process.kill(pid, 0)` 成功即视为已在运行 → 打印提示并 `exit(2)`；否则写入自身 pid（`wx` / `0600`）。
3. 读/建 `api-key`（缺失时生成 `randomBytes(32).toString('hex')`）。
4. 读 `settings.json`；解析 models 文件路径（优先级：`BUDDY_MODELS_FILE` → settings 已保存值 → WorkBuddy 配置目录发现 / Windows 走 `resolveModelsFile`）。
5. `findRuntime` 定位或下载 OpenCode 运行时 → `startBackend` 启动子进程，起 HTTP server。
6. `/agent` 校验：确认隔离配置中的 `buddy-bridge`、`buddy-chat` 两个自定义 agent 存在，否则 `phase` 不进入 `ready`。
7. 系统代理（`useSystemProxy`）：macOS 走 `scutil --proxy`、Windows 走注册表解析，注入子进程 env；**解析失败回退为关闭，不阻断启动**。
8. 启动导入（`BUDDY_NO_SYNC !== '1'` 时）：**先清除旧的 own 条目再探测（串行逐模型，整批共享 `PROBE_TIMEOUT = 60000` 预算）**，仅 `ok` 的模型进入发布集。
9. 同步发布集到 WorkBuddy `models.json`，启动后台活动定时器与事件订阅。

### 请求链路（`/v1/chat/completions`）

```
WorkBuddy
  → server.js：Bearer 鉴权 → 拒绝 Origin（403）→ 限流（>4 并发返回 429 busy）→ 读体（>8MB 返回 413）
  → protocol.prepare(body, models)：模型匹配 / messages 校验 / n=1 / tool 定义唯一性 / 图片 mime 白名单（png·jpeg·webp·gif）
  → backend.complete(...)：OpenCode 会话 + 消息 + 事件流
        ├─ 原生工具调用 → nativePermissions（'*': 'ask'，question/websearch/codesearch/webfetch/task/plan_*/todowrite: 'deny'）
        │    → 阻塞并交回客户端（handoff），模型不得自行执行本地动作
        └─ chatOnly 模型（buddy-chat）遇原生工具 → 502 native_tool_activity
  → protocol.decode(text, request)：校验信封 {content, calls}、校验工具与参数
        ├─ 一次格式修正（resendPrompt：只纠正格式）
        └─ 仍失败 → repair()：由辅助模型（translator，TRANSLATOR_ORDER 中挑）转写一次；探测路径禁止转写
  → completion() / sendSSE()：非流式 JSON 或 SSE（先校验后发送，含 10s 心跳注释行）
  → onResult：写入 status.json 计数与最近请求（客户端取消的请求绝不记为成功）
```

### 模型发布链路

```
models.json → freeModels(providers)（免费判定：输入/输出/缓存全 0、输出支持文本、非 deprecated）
  → 逐模型探测（probeModel：工具调用类 + 纯文本类，失败可重试 1 次）
  → 仅 ok 的模型进入 publishedModels（客户端 ID = `OC · <name>`）
  → syncModels(原子写) → mergeModels（保留非本工具条目，只清理/更新 OWNER='buddy-bridge-v1' 的条目）
```

### 运行时隔离链路

```
findRuntime
  ├─ 优先级：BUDDY_OPENCODE_PATH > 托管目录 runtime/<version>/ > ~/.opencode/bin/（含 Homebrew 等）> Windows npm 全局 shim
  │   （拒绝 .cmd/.bat/.ps1 启动器脚本；版本必须形如 x.y.z）
  └─ 与 registry 最新版比对：本地不落后则复用；否则下载
       registry 顺序：https://registry.npmjs.org → https://registry.npmmirror.com
       校验：metadata.name/version 合法 + dist.integrity 必须为 `sha512-` + tarball 必须来自白名单 registry
       安装：下载 → sha512 校验 → 解包仅取 `package/bin/<binary>` → 原子替换 → chmod 755 → 回读版本必须一致
startBackend
  → env 白名单（PATH/HOME/LANG/TMPDIR/SSL_CERT_FILE/... 与 Windows 必需项）+ XDG_* 指向数据目录 + 随机 OPENCODE_SERVER_PASSWORD
  → 关闭 OPENCODE_DISABLE_AUTOUPDATE / _PROJECT_CONFIG / _CLAUDE_CODE / _EXTERNAL_SKILLS
  → OPENCODE_CONFIG_CONTENT = isolatedConfig（permission 全 ask/deny、autoupdate:false、share:disabled、两个自定义 agent）
  → spawn `serve --pure --hostname 127.0.0.1 --port <随机空闲端口>` → 轮询 /global/health（≤120 次 × 500ms，版本必须一致）
  → 关停：SIGTERM → 等待 ≤4s → SIGKILL
```

## 关键模块与函数清单（修改前必须确认）

| 模块 | 关键导出 / 内部函数 | 职责 |
|---|---|---|
| `src/core/src/main.js` | `update`、`syncPublished`、`record`、`usableModels`、`publishedModels`、`attachTranslator`、`startProbes`、`refresh`、`importModels`、`shutdown` | sidecar 入口与全局编排（无导出） |
| `src/core/src/server.js` | `createServer({ key, backend, getModels, refresh, importModels, setSystemProxy, probe, status, onResult, onActivity })` | HTTP 路由与鉴权：`GET /health`、`GET /v1/models`、`POST /admin/probe`、`POST /admin/system-proxy`、`POST /admin/import`、`POST /admin/refresh`、`POST /v1/chat/completions` |
| `src/core/src/protocol.js` | `BridgeError`、`prepare`、`decode`、`completion`、`sendSSE` | OpenAI 兼容入参校验、信封解码、响应组装、SSE 发送 |
| `src/core/src/backend.js` | `Backend`、`nativePermissions`、`freeModels`、`shrinkPermission` | OpenCode HTTP 客户端、事件流、原生审批拦截、免费模型发现 |
| `src/core/src/runtime.js` | `runtimeCandidates`、`findRuntime`、`isolatedConfig`、`startBackend` | 运行时定位/下载/校验/启动与隔离配置 |
| `src/core/src/probe.js` | `PROBE_TIMEOUT`、`PROBE_TOOLS`、`probeBody`、`judgeProbe`、`formatUnsupported`、`RETRYABLE_PROBE`、`probeModel`、`probeFailure` | 模型探测协议与判定 |
| `src/core/src/repair.js` | `REPAIR_SYSTEM`、`CLIENT_CONVENTIONS`、`CONVENTION_ALIASES`、`clientConventions`、`rawMaterial`、`toolCatalog`、`repairBody`、`extractJson`、`translatorRequest`、`repair`、`resendPrompt` | 格式修复与辅助模型转写 |
| `src/core/src/handoff.js` | `buildHandoff`、`handoffInput`、`rejectFeedback`、`validateAction` | 原生工具 handoff 协议 |
| `src/core/src/sync.js` | `OWNER`（`'buddy-bridge-v1'`）、`atomicWrite`、`mergeModels`、`syncModels` | WorkBuddy 配置写入与增量合并 |
| `src/core/src/workbuddy-config.js` | `validateModelsFile`、`resolveModelsFile` | models.json 定位与校验 |
| `src/core/src/system-proxy.js` | `parseSystemProxy`、`systemProxyEnvironment`、`parseWindowsProxy` | 系统代理发现与子进程 env 映射 |
| `src/core/src/reasoning.js` | `reasoningEfforts`、`workBuddyReasoning` | 推理档位与回退策略 |
| `src/core/src/model-status.js` | `modelResult`、`withRequestMeta`、`clientModelID` | 状态记录与客户端展示 ID |
| `src/core/src/platform.js` | `dataDirectory`、`runtimePackage` | 平台路径与运行时包名 |
| `src/core/src/atomic.js` | `replaceWithRetry(temp, target, { rename, sleep, delays })` | 原子替换（Windows 共享冲突重试） |
| `src/core/src/json.js` | `parseJson` | 容错 JSON 解析 |
| `src/ui/renderer.js` | `run(name, value)`、`render()`、`renderModels()`、`renderDetails()`、`feedback(text, error)` | 面板渲染与动作：`refresh` / `probe` / `import` / `restart` / `system-proxy`，通过 `window.buddy.action` 调用壳 |
| `src/ui/activity.cjs` | `activityText(...)` | 活动文案统一（托盘与面板共用，禁止两处各写一套文案） |
| `scripts/build-sidecar.mjs` | `ALL` 目标映射 | sidecar 打包（输出 `src-tauri/binaries/`） |

### 关键常量（改动前必须确认语义）

| 常量 | 值 | 位置 |
|---|---|---|
| 默认端口 | `41980`（`BUDDY_PORT` 覆盖，范围 1024–65535） | `main.js` |
| 请求体上限 | 8MB → 413 | `server.js` |
| 并发上限 | 4（超出 429 `busy`） | `server.js` |
| HTTP 超时 | `requestTimeout 20000` / `headersTimeout 15000`（模型生成长任务不受控制类超时约束） | `server.js` |
| 探测批预算 | `PROBE_TIMEOUT = 60000`（整批共享） | `probe.js` |
| 日志轮转阈值 | 5MB | `main.js` |
| 健康轮询 | ≤120 次 × 500ms | `runtime.js` |
| 关停窗口 | SIGTERM → 4000ms → SIGKILL | `runtime.js` |
| 权限文本截断 | `shrinkPermission(value, limit = 400)` | `backend.js` |
| 同步归属标记 | `OWNER = 'buddy-bridge-v1'` | `sync.js` |
| 状态内置版本 | `version: '0.2.0'`（写入 status.json，历史沿革值） | `main.js` |

## 编码规范

> 以下规范适用于本项目所有代码改动，所有 Agent 新增或修改代码时必须遵守。

### 语言与依赖

- `src/core/` 一律使用 **ESM**（`type: module`），内置模块统一 `node:` 前缀（`node:fs/promises`、`node:crypto` 等）。
- **运行期依赖只有 `tar` 与 `undici`**。新增运行期依赖必须说明理由（sidecar 体积 + 供应链可信度双重成本）；能用 `node:` 内置模块实现的绝不引包。
- 异步优先 `async/await`；禁止 `.then()` 链式拼接业务逻辑。
- 共享可变状态（`status.json`、`models.json`）的写入必须经 `promise` 链**串行化**（现有实现：`statusWrites` / `syncWrites`），禁止并发直写。

### 错误与注释

- 业务错误统一抛 `BridgeError(message, status, code)`，HTTP 层据此映射状态码与 `error.type`；不得吞掉错误或返回无码错误。
- 注释写**为什么**（约束、坑、平台差异），不写"是什么"；现有源码已建立该风格，新增代码保持一致。例如说明为何不能用启动器脚本、为何要保留原位不覆盖等。
- 日志只写非敏感信息；**严禁把 `api-key`、`OPENCODE_SERVER_PASSWORD`、用户凭据写入日志或 `status.json`**。

### 文件与平台

- 平台差异集中在 `platform.js` / `system-proxy.js` / `runtime.js`；业务代码不得散落 `process.platform === 'win32'` 判断。
- 路径拼接使用 `path.join`（跨平台）；Windows 需考虑 `.exe` 后缀与不能替换运行中镜像的约束。
- 文件写入优先走 `atomic.js` 的 `replaceWithRetry` 或 `sync.js` 的 `atomicWrite`，禁止裸 `fs.rename` 覆盖。
- 新建目录/敏感文件时显式指定权限（目录 `0700`、密钥文件 `0600`）。

### 测试规范

- 使用 Node 内置 `node:test`（`node --test test/*.test.js`），测试文件命名 `*.test.js`。
- **测试严禁真实联网、真实下载**：网络与运行时行为必须通过注入参数（`options.fetch` / `options.latest` / `options.probe` / `options.rename` 等）或 `test/fixtures/`（`runtime.mjs`、`runtime-loader.mjs`）替换。
- 新增/修改行为必须补测试；测试名需描述被保护的行为（现有测试名即为此风格，如"共享冲突重试后才失败"）。
- 修改 `src/core/src/` 后必须运行 `npm test` 并报告实际结果，**不得以"应该能过"代替执行**。

### UI 规范

- `src/ui/` 为无构建纯静态页面：不得引入框架、打包器、CDN 或任何外部请求，必须遵守 `index.html` 的 CSP（`default-src 'self'`、`script-src 'self'`、`connect-src 'none'`）。
- 与壳的通信只走既有契约：`window.buddy.action(name, value)` / `window.buddy.onState(cb)` / `window.buddy.onDismiss(cb)`。**不得新增或改名 IPC 方法**，除非同步更新壳（`src-tauri/src/lib.rs`）。
- 面板为只读展示 + 动作触发：不得在面板内直接读写文件、直接访问网络、直接调用 OpenCode。
- 动作执行期间必须置忙（按钮禁用 + spinner + 结果反馈），失败必须显示原因（`feedback(text, true)`）。

### 命名规范

| 类别 | 规则 | 示例 |
|---|---|---|
| 文件命名 | 全小写，短横线分词 | `workbuddy-config.js`、`system-proxy.js` |
| 变量 / 函数 | 小驼峰，函数动词开头 | `usableModels()`、`syncPublished()`、`resolveModelsFile()` |
| 常量 | 全大写下划线 | `PROBE_TIMEOUT`、`TRANSLATOR_ORDER`、`REGISTRIES`、`OWNER` |
| 类 | 大驼峰 | `Backend`、`BridgeError` |
| 私有 / 内部 | 无对外导出即保持模块内定义 | `attachTranslator`、`startProbes` |

## Git 提交规范

提交格式：`type(scope): content`

| type | 说明 |
|---|---|
| `feat` | 新功能 |
| `fix` | 修复 |
| `docs` | 文档 |
| `style` | 格式调整（不影响逻辑） |
| `refactor` | 重构 |
| `perf` | 性能优化 |
| `test` | 测试 |
| `chore` | 构建 / 工具 / 依赖 |

`scope` 约定：`core`（sidecar 源码）、`ui`（控制面板）、`tauri`（壳与图标）、`scripts`（打包脚本）、`docs`（文档与品牌资产）、`ci`（工作流）。

示例：`fix(core): 修正探测批超时预算分配`

其他提交纪律：

- 提交前必须运行 `src/core/` 测试；测试失败禁止提交。
- 一次提交只包含一类改动，禁止把源码改动与大批二进制/图标混在一起。
- **严禁提交**：`api-key`、`.env.local`、`status.json`、`settings.json`、`src/core/node_modules/`、`src/core/dist/`、`.zwork/`、任何私钥文件。
- 仓库现状：分支 `dev`，远端 `origin = https://github.com/trexwb/wbBridge.git`（另有 `main`）。当前仅 `README.md` 被跟踪（`Initial commit`），其余文件均未跟踪；提交时注意 `src-tauri/binaries/`（6 个产物、合计约 401MB）**未被 `.gitignore` 忽略**，提交前必须与用户确认处理方式（改 `.gitignore` 或走 Release 分发）。

## 红线与限制

### 安全红线（不得移除、不得放松、不得绕过）

1. **鉴权**：所有路由（含 `/health`）必须校验 `Authorization: Bearer <api-key>`，并使用 `timingSafeEqual` 定时安全比较；`api-key` 文件权限 `0600`，缺失时随机生成。
2. **拒绝浏览器来源**：请求带 `Origin` 头一律 403。WorkBuddy 只经其原生运行时访问本服务。
3. **仅监听回环地址**：`server.listen(port, '127.0.0.1')`，禁止改为 `0.0.0.0` 或对外开放。
4. **限流与体积**：并发 ≤4、请求体 ≤8MB，不得放宽。
5. **原生工具严禁开放给模型**：`nativePermissions` 必须保持 `'*': 'ask'`、`question`/`websearch`/`codesearch`/`webfetch`/`task`/`plan_enter`/`plan_exit`/`todowrite` 为 `deny`；`isolatedConfig.autoupdate: false`、`share: 'disabled'` 不得改动。**模型绝不允许直接执行本地动作**，原生动作只能以 handoff 形式交回客户端。
6. **chatOnly 模型不得使用工具**：纯文本 agent（`buddy-chat`）遇原生工具活动必须报 `native_tool_activity`，不得降级为静默执行。
7. **密钥零泄漏**：子进程环境白名单之外不得透传任何环境变量（尤其其他 provider 的 API Key、OpenCode 登录态）；密钥不得写入日志、`status.json` 或仓库。

### 供应链红线（OpenCode 运行时）

1. 只能从**白名单 registry**（`registry.npmjs.org`、`registry.npmmirror.com`）获取运行时元数据与 tarball；元数据必须满足 `name` 匹配、语义化 `version`、`dist.integrity` 以 `sha512-` 开头、tarball 来源属于白名单。
2. **下载后必须校验 sha512 完整性**，校验失败不得使用、不得"跳过校验重试"。镜像与官方字节不一致时必须报错（测试已固化该行为）。
3. 解包只提取 `package/bin/<binary>` 单个文件，禁止整包解开到运行时目录。
4. 安装后必须**回读 `--version` 与元数据版本一致**，不一致即失败；禁止把无法确认版本的候选当作运行时。
5. 拒绝 `.cmd` / `.bat` / `.ps1` 启动器脚本作为运行时二进制。
6. 不得把运行时降级为"直接调用系统 `opencode` 的共享实例"——隔离（独立 XDG 目录、独立端口、独立密码、独立配置）是本项目的核心价值。

### 数据红线

1. 写 WorkBuddy `models.json` 时：**只清理/更新 `OWNER='buddy-bridge-v1'` 名下的条目，其他条目与对象元数据必须原样保留**；手动条目 ID 冲突时不得擅自覆盖。
2. `status.json` 写入必须串行 + 原子；探测中的临时状态不得覆盖已完成状态。
3. `service.pid` 单实例锁不得删除或绕过；重复启动必须 `exit(2)` 而不是抢占端口。
4. 格式类上游失败（`invalid_model_output` / `invalid_tool_call` / `native_tool_activity` / `output_truncated`）**不得撤销已发布模型**；探测路径**不得启用辅助模型转写**。
5. 客户端取消的请求不得记录为成功。

### 工程红线

1. **禁止重构**：只做针对性最小改动；不得重命名已有导出、路由、错误码或 `window.buddy.*` IPC 契约。
2. **禁止引入根级 `package.json` 或根级构建脚本**来"顺便统一命令"，除非用户明确要求。
3. **禁止在仓库内写入运行时数据**：所有运行期文件（`api-key`、`status.json`、日志、runtime、opencode 目录）只能写入平台数据目录。
4. **禁止编造不存在的实现**：本仓库 Tauri 壳源码与 CI 工作流（`.github/workflows/release.yml`）已入库，但尚未经实际构建验证。涉及这些内容的文档或说明必须如实标注当前状态，不得伪装成已验证。
5. **禁止大规模二进制入库**：`src-tauri/binaries/` 属构建产物，入库或分发方式需用户确认。
6. **禁止擅自推进版本号**：见下节。

## 当前基准版本

| 项 | 值 | 来源 |
|---|---|---|
| 版本单一来源 | **1.0.0** | `src/core/package.json` 的 `version` |
| 壳工程同步落点 | **1.0.0** | `src-tauri/Cargo.toml` 的 `[package] version` |
| 状态内置版本 | `0.2.0` | `src/core/src/main.js` 写入 `status.json` 的 `version`（沿自上游参考实现，界面上可见） |
| 上游调研基线 | `0.2.5` | `docs/research/upstream-architecture.md` |
| 测试基线 | 97 通过 / 0 失败（≈5.3s，11 个测试文件） | `src/core/` 下 `npm test`（node v24.21.0） |
| 运行时基线 | 由 registry 最新版决定（不固定版本）；Node `>= 22` | `runtime.js` / `src/core/package.json` |
| 已构建 sidecar | 6 个产物：darwin ×2、windows ×2、linux ×2（合计约 401MB） | `src-tauri/binaries/` |

**版本号规则（写死，所有 Agent 必须遵守）**：

- 版本号以 `src/core/package.json` 为唯一来源；`status.json` 中的 `0.2.0` 为历史沿革值，**不得在无用户指令的情况下静默改动**。若用户要求统一两处版本，必须同时更新本文件与 `docs/` 说明。
- **仅当**新增了与现有问题**不同类、不同根因**的功能/修复，且**用户明确允许**推进版本号时，才可末位 +1。
- **以下情形绝对禁止推进版本号**：
  1. 同一问题的多轮往返排查与再次修复；
  2. 用户明确要求"不修改版本号 / 回退到 X.Y.Z"；
  3. 同一自然日对同一模块/同一类 bug 的追加修复；
  4. 仅改 `docs/`、`README.md`、本 `AGENTS.md` 等文档类内容；
  5. 纯文案、注释、日志措辞、去抖等不引入新逻辑分支的打磨。
- 用户要求"回退到 X.Y.Z"时，`src/core/package.json`（及被提及的其他位置）必须一致改写为用户指定值，本回合内不得再以"我刚改了代码所以 +1"为由推进。

## 变更与交付约定（面向 Agent）

1. **改动前**：`read_text` 读取目标文件原文；涉及 `main.js` 编排、`server.js` 路由、`protocol.js` 校验、`runtime.js` 下载的逻辑，必须同时阅读相关测试，确认既有行为边界。
2. **改动中**：一次只改一个独立区块/函数，避免大范围替换；不修改与任务无关的文件。
3. **改动后**：
   - 必须运行 `src/core/` 的 `npm test`，并在回复中给出真实的通过/失败数量；
   - 必须用 `grep` / `read_text` 验证改动已落盘；
   - 不得自动 `git commit` / `git push`；
   - 若改动涉及对外契约（路由、错误码、`status.json` 字段、`models.json` 写入格式、`window.buddy.*`），必须在回复中显式列出并提示用户影响面。
4. **遇到不确定**：宁可向用户询问，也不要凭推测修改安全、供应链、隔离相关代码。
