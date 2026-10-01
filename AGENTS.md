---
AIGC:
    Label: "1"
    ContentProducer: 001191440300708461136T1XGW3
    ProduceID: 16825e3339a4e87ec3619b4c10842061_41146ba1bd6211f19ba1525400638852
    ReservedCode1: DvUbfRAmXidF/cgd/KJZh9Sv4+XUxrYf0/2rd+0lrc+0C1DQaxJDBnngEDJN6cREmQ5LVusQY+WxBaAA22q6pXuqPigtZ0kaS1Y4ZNQCHGe+C/9yV9AnIJ2S7Xy+CJJxg+eJrREjIvzZCLs3qKIGplKAsIpClmkcAwXNzVGgcVGVc1GwXrN8yH4DL68=
    ContentPropagator: 001191440300708461136T1XGW3
    PropagateID: 16825e3339a4e87ec3619b4c10842061_41146ba1bd6211f19ba1525400638852
    ReservedCode2: DvUbfRAmXidF/cgd/KJZh9Sv4+XUxrYf0/2rd+0lrc+0C1DQaxJDBnngEDJN6cREmQ5LVusQY+WxBaAA22q6pXuqPigtZ0kaS1Y4ZNQCHGe+C/9yV9AnIJ2S7Xy+CJJxg+eJrREjIvzZCLs3qKIGplKAsIpClmkcAwXNzVGgcVGVc1GwXrN8yH4DL68=
---

# AGENTS.md — WB Bridge 桥接服务 (wbBridge)

## ⚠ 强制规范（所有 Agent 必须遵守）

**本项目是「Tauri 桌面壳 + 同进程内嵌 Rust 核心」的桥接工具。核心（`src-tauri/core/`，crate 名 `wbbridge-core`）作为 `path` 依赖静态编进壳（`src-tauri/`）里，跑在壳专用 tokio 多线程运行时上，仍在 `127.0.0.1:41980`（端口被占则由壳另择端口）提供 OpenAI 兼容 API，把**隔离托管**的 OpenCode 免费模型发布给 WorkBuddy；控制面板 `src/`（Vue 3 SFC + Vite 构建，产物 `dist/`）由 Tauri WebView 加载，经 `invoke('core_action' | 'restart_core' | 'core_running' | 'data_dir_path')` 与 `core-status` / `core-activity` / `core-failed` 事件驱动。**

> 🔴 **已彻底移除 Node 运行环境**：不再有 Node sidecar、`@yao-pkg/pkg`、esbuild 预打包，不再有 `src-tauri/binaries/`（`externalBin` 为空数组）。Node 仅用于前端构建（Vite）与 `scripts/*.mjs`。原 JS 核心与其测试已归档到仓库**之外**：
> `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`（详见该目录 README）。
>
> 🔴 **所有 Agent（包括 file-agent、browser-agent、computer-agent 等一切主 Agent、Sub-Agent、子代理）在本项目中执行任何任务时，必须无条件遵守本 `AGENTS.md` 定义的全部规则，不得以任何理由违反。**
>
> 🔴 **文件操作根目录**：本项目所有文件操作默认以 `/Users/wbtrex/website/localServer/node/trexwb/git/wbBridge` 为根目录，**不得偏离**。
>
> 🔴 **根 `package.json` 存在且是版本单一来源**：`npm run test`（转发 cargo）、`npm run lint`、`npm run vite:build`、`npm run tauri:dev|build`、`npm run version:set|check`。Rust 侧命令一律 `--manifest-path src-tauri/core/Cargo.toml` 或进 `src-tauri/`；全部 Rust 代码在 `src-tauri/` 下（壳 = `src-tauri/src/`，核心 = `src-tauri/core/`）。**不存在** Node 版的 `src/core/` 与根级 `core/`（那是已归档的 JS 核心），严禁臆造不存在的脚本、命令与路径。
>
> 🔴 **实读优先、禁止猜测**：本文件所有结论均以仓库真实代码为准。修改任何模块前必须先 `read` 读取原文，禁止凭记忆推测函数名、常量名、路由、错误码与配置字段。
>
> 🔴 **最小改动优先**：只做针对性修复，禁止重构、禁止大范围重写、禁止"顺手优化"。
>
> 🔴 **密钥零泄漏**：不得读取、打印、回显、提交或推断 `api-key`、`.env.local`、用户 `~/.workbuddy/models.json` 中的凭据字段。
>
> 🔴 **提交由用户决定**：Agent 完成改动后不得自动执行 `git commit` / `git push`，除非用户在本轮明确要求。

## 当前状态与验证边界（必须如实转述，不得伪装成已验证）

| 项 | 状态 |
|---|---|
| `src-tauri/core` 单元测试 + 对拍 + 红线 | ✅ 已执行并通过（见「测试基线」） |
| `src-tauri/core` 独立进程冒烟（真实下载 OpenCode → 隔离启动 → `/agent` 校验 → 刷新 8 个免费模型 → 探测通过 → 鉴权 401/403 → `/v1/models` → 优雅关停） | ✅ 已在一次性数据目录实测通过 |
| `cargo clippy --all-targets`（src-tauri/core）/ `cargo clippy --no-deps`（src-tauri） | ✅ 0 warning |
| 实际启动 GUI（`npm run tauri:dev` / 打包后的 .app）并操作托盘与面板 | ❌ **未实测**。壳改动只能以「编译通过 + 核心独立运行行为」为证据，必须显式告知用户未做 GUI 验证 |
| `.github/workflows/release.yml`（cargo 化后） | ❌ **未在 CI 上跑通**，仅静态校验过 YAML 结构 |
| `cargo fmt --check` 全绿 | ❌ 未达成（本仓库不以 fmt 为准，勿在无关文件上顺手格式化） |

## 项目概述与定位

WB Bridge 是一个**跨平台托盘工具**，通过**隔离的 OpenCode 技术**为 WorkBuddy 提供免费模型服务。它做三件事：

1. **托管一个隔离的 OpenCode 运行时**：优先复用本机已有的 `opencode` 可执行文件，否则从 npm registry（官方源 → 国内镜像）下载官方 tarball，校验 `sha512-` 完整性后解包到数据目录，用**隔离环境变量 + 独立随机端口**以 `serve --pure` 方式启动，绝不污染用户 OpenCode 的配置、数据、缓存与登录态。
2. **对外暴露 OpenAI 兼容 API**：本地 `127.0.0.1:<port>` 提供 `/v1/models` 与 `/v1/chat/completions`（含 SSE 流式），强制 `Authorization: Bearer <api-key>` 鉴权，拒绝一切浏览器 Origin，最多 4 个并发请求。
3. **把可用模型同步给 WorkBuddy**：自动发现免费模型 → 逐模型探测 → 只把探测通过的模型发布给 WorkBuddy（原子写 + 增量合并其 `models.json`，只增删自己名下的条目）。

三层结构：

| 层 | 位置 | 技术 | 现状 |
|---|---|---|---|
| 核心 | `src-tauri/core/src/`（crate `wbbridge-core`，lib `wbbridge_core`） | Rust + tokio + axum + reqwest + serde_json（`preserve_order`） | 已入库，测试齐全；既可独立成进程，也嵌入壳 |
| 控制面板 | `src/`（Vue 3 SFC）+ `vite.config.js` → 产物 `dist/` | Vue 3 + Vite，无 CDN、无外部请求 | 已入库 |
| 桌面壳 | `src-tauri/`（crate `wbbridge`，lib `wbbridge_lib`） | Tauri 2（Rust），托盘 + IPC + 状态轮询 | 已入库，直接 `cargo build` 出成品（无 sidecar） |

## 常用命令（安装 / 开发 / 构建 / 测试 / 签名发布）

### 环境前置

- **Rust** stable（`src-tauri/core/Cargo.toml` 声明 `rust-version = "1.75"`）。
- **Node.js >= 24**（根 `package.json` 的 `engines`）——只为 Vite 前端构建与 `scripts/*.mjs` 服务，核心运行不依赖它。本机由 nvm 管理，**非登录 shell 的 PATH 中可能没有 `node`**，执行 npm 命令前先载入：
  ```bash
  export NVM_DIR="$HOME/.nvm"; . "$NVM_DIR/nvm.sh"
  ```
- Linux 构建需系统包：`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev`。

### 命令清单

| 目的 | 命令 | 执行目录 | 说明 |
|---|---|---|---|
| 安装前端依赖 | `npm install` | 仓库根 | 根 `package.json`（devDeps：`@tauri-apps/cli`、`vite`、`@vitejs/plugin-vue`、`eslint`、`@eslint/js`；deps：`vue`） |
| 运行全部核心测试 | `cargo test`（或 `npm run test`） | `src-tauri/core/` | 基准 **197 通过 / 0 失败**：lib 179 + `js_parity` 11 + `red_lines` 7（约 0.3s，另有 bin/doc-test 0 用例） |
| 静态检查 | `cargo clippy --all-targets` | `src-tauri/core/` | 必须保持 0 warning |
| 启动核心（独立进程，调试用） | `cargo run --manifest-path src-tauri/core/Cargo.toml --bin wbbridge-core` | 仓库根 | 监听 `127.0.0.1:41980`（`BUDDY_PORT` 覆盖），数据目录走平台默认值 |
| 开发桌面应用 | `npm run tauri:dev` | 仓库根 | `beforeDevCommand = npm run vite:dev`（`http://localhost:41990`），壳内嵌启动核心 |
| 构建前端产物 | `npm run vite:build` | 仓库根 | 输出 `dist/`（`tauri.conf.json` 的 `frontendDist` 指向 `../dist`） |
| 构建桌面应用 | `npm run build`（= `vite:build && tauri build`） | 仓库根 | 产物 `src-tauri/target/*/release/bundle/` |
| 版本一致性校验 | `npm run version:check` | 仓库根 | `scripts/check-version.mjs`：5 处版本号必须一致，否则退出 1 |
| 版本推进 | `npm run version:set -- <x.y.z>` | 仓库根 | `scripts/bump-version.mjs` 同步改写；**推进前必须满足下方「版本号规则」** |
| 健康检查 | `curl -H "Authorization: Bearer $(cat "<数据目录>/api-key")" http://127.0.0.1:41980/health` | 任意 | `/health` **同样需要 Bearer 鉴权**（无 key 返回 401） |
| 列出已发布模型 | `curl -H "Authorization: Bearer <key>" http://127.0.0.1:41980/v1/models` | 任意 | 返回客户端可见模型（id 形如 `OC · 名称`） |

> ⚠ `<数据目录>` 取决于运行方式：**壳运行时**是 Tauri 的 `app_data_dir`（macOS `~/Library/Application Support/app.wbbridge.desktop`，identifier `app.wbbridge.desktop`）；**独立进程运行时**是平台默认目录（macOS `~/Library/Application Support/Buddy Bridge`，见下）。两套目录不要混为一谈。

### 签名与发布

- **自动更新（updater）目前未接线**：`tauri.conf.json` 没有 `bundle.createUpdaterArtifacts`，也没有 `plugins.updater` / `tauri-plugin-updater` 依赖，因此产物不含 `.sig` 与 `latest.json`，下面这些签名变量现在**无人读取**。接入前不要引导用户生成私钥；接入时需同时补配置项、插件依赖、更新端点，并把 CI 的 `.sig` glob 加回来。
- 环境变量模板：`.env.example` → 复制为 `.env.local`（`.env.local` 已被 `.gitignore` 忽略，**严禁提交**）。这是唯一模板落点（`src-tauri/updater-signing.env.example` 与其逐字节相同、已删除）。
  - `TAURI_SIGNING_PRIVATE_KEY_PATH`：更新产物签名私钥路径，默认 `~/.tauri/wbBridge-updater.key`
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥密码
  - 备用方式：内联 `TAURI_SIGNING_PRIVATE_KEY`（仅当 CI 不支持 PATH 形式时启用）
- 密钥生成（`@tauri-apps/cli` 已在根 devDependencies，现可执行）：
  ```bash
  npm run tauri -- signer generate -p <密码> -w ~/.tauri/wbBridge-updater.key
  ```
- CI：`.github/workflows/release.yml`（推 `v*` tag 或手动 dispatch）——test 作业跑 `cargo test` + `check-version.mjs`，三个平台作业跑 `npm ci` + `tauri-apps/tauri-action`（带 `--target <triple>`）出 bundle。**该工作流自「移除 Node 核心」改造后尚未在 CI 实际跑通**，改动它时如实标注。

### 环境变量（核心）

| 变量 | 默认值 | 作用 |
|---|---|---|
| `BUDDY_PORT` | `41980` | HTTP 端口；非 1024–65535 的整数直接失败退出（嵌入壳时由壳显式传入 `StartOptions.port`，不读环境变量） |
| `BUDDY_DATA_DIR` | 平台数据目录（见下） | 覆盖数据目录（嵌入壳时由 `StartOptions.data_dir` 注入 app_data_dir） |
| `BUDDY_MODELS_FILE` | macOS/Linux `~/.workbuddy/models.json` | 覆盖 WorkBuddy 配置文件路径（Windows 走"已保存值 → 发现"流程） |
| `BUDDY_NO_SYNC` | 未设置 | `=1` 时跳过启动导入与后续模型同步 |
| `BUDDY_OPENCODE_PATH` | 无 | 首选 OpenCode 可执行文件路径（优先级最高） |
| `BUDDY_PARENT_PID` | 无 | 父进程 pid；父进程消失则核心自行优雅关停 |

> 嵌入壳时**禁止**用 `std::env::set_var` 改变进程级配置（不安全、与并发冲突）；必须走 `orchestration::run(StartOptions { data_dir, port, handle_signals })`。

数据目录（`src-tauri/core/src/platform.rs::data_directory`，目录名常量 `DATA_DIR_NAME = "Buddy Bridge"`）：macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`。壳运行时改用 `app_data_dir`。目录内文件：

| 文件 / 目录 | 用途 |
|---|---|
| `api-key` | 32 字节随机 hex（权限 `0600`），所有 HTTP 请求的 Bearer 令牌 |
| `service.pid` | 单实例锁；进程仍存活即视为已在运行（独立进程按退出码 2 结束） |
| `settings.json` | 持久化 `useSystemProxy`、`workBuddyModelsFile` |
| `status.json` | 服务状态快照（UI 与外部读取的唯一状态源），串行 + 原子写 |
| `opencode.log` | OpenCode 子进程 stdout/stderr；超过 5MB 轮转为 `opencode.log.previous` |
| `runtime/<version>/opencode` | 托管下载的 OpenCode 运行时 |
| `opencode/{config,data,cache,state,project}` | 隔离的 OpenCode XDG 目录（`0700`） |

## 目录结构说明

```
wbBridge/
├── AGENTS.md                     ← 本文件（所有 Agent 的规则来源）
├── README.md                     ← 面向使用者的项目说明
├── package.json                  ← 根级便利脚本 + 版本单一来源（非 npm 工作区）
├── package-lock.json             ← 入库：CI 的 npm ci 依赖它
├── vite.config.js                ← 前端构建（root: src，outDir: ../dist，dev 端口 41990）
├── eslint.config.js              ← 扁平 ESLint（只开能真报错的规则）
├── src/                          ← 控制面板（Vue 3 + Vite；root: src）
│   ├── index.html  main.js  App.vue
│   ├── core/bridge.js            ← 与壳的唯一边界：invoke + listen → onState/onDismiss/action
│   ├── core/activity.js          ← 活动文案（托盘与面板共用，禁止两套文案）
│   ├── views/                    ← SideBar.vue（分组导航 + 运行设置）、ModelList.vue、ModelDetails.vue（详情右栏）、
│   │                                ServiceStatus.vue、MetricsBar.vue、FeedbackBar.vue
│   ├── components/ModelRow.vue   styles/{variables,base}.css  public/logo.svg
├── src-tauri/                    ← 全部 Rust 代码都在这里（壳 + 核心）
│   ├── Cargo.toml                ← 壳 crate wbbridge；依赖 wbbridge-core = { path = "core" } + tokio(rt-multi-thread)
│   ├── tauri.conf.json           ← frontendDist ../dist、bundle.externalBin **空**、CSP
│   ├── capabilities/default.json ← 只声明插件侧权限（自有命令不经 capability 授权）
│   ├── build.rs
│   ├── src/{main.rs,lib.rs}      ← 托盘、IPC 命令、状态轮询、核心生命周期
│   ├── icons/                    ← 壳/托盘图标
│   └── core/                     ← Rust 核心（crate wbbridge-core，**独立 workspace**：自有 Cargo.lock 与 target）
│       ├── Cargo.toml            ← 内部 crate 版本 0.1.0（与产品版本解耦，勿"顺手对齐"）
│       ├── src/
│       │   ├── orchestration.rs  ← 原 main.js 的编排：启动时序、status/settings、探测调度、关停、单实例锁、StartOptions、set_exit_hook
│       │   ├── main.rs           ← 独立进程入口（--help / --version；信号自处理）
│       │   ├── lib.rs            ← 模块导出 + STAGE / PACKAGE_NAME / VERSION
│       │   ├── server.rs         ← axum 路由、Bearer 鉴权、Origin 拒绝、并发/体积上限、SSE + 心跳、/admin/*
│       │   ├── protocol.rs       ← OpenAI 兼容协议层：prepare / decode / completion / send_sse / BridgeError
│       │   ├── backend.rs        ← OpenCode HTTP 客户端：会话、事件流、原生工具审批、免费模型发现、转写钩子
│       │   ├── runtime.rs        ← 运行时发现/下载/sha512 校验/解包/隔离 env/启动/关停
│       │   ├── probe.rs          ← 模型探测（工具调用与纯文本两类；每模型各 60s 预算，重试共用）
│       │   ├── repair.rs         ← 信封/工具格式修复与辅助模型转写（含 CLIENT_CONVENTIONS）
│       │   ├── handoff.rs        ← 原生工具 handoff：构建客户端动作、拒绝反馈、动作校验
│       │   ├── sync.rs           ← WorkBuddy models.json 原子写 + 增量合并（OWNER 标记、锁、.bak）
│       │   ├── workbuddy_config.rs ← models.json 定位与校验（不猜测、不创建文件）
│       │   ├── system_proxy.rs   ← 系统代理解析（macOS scutil / Windows 注册表）与子进程环境
│       │   ├── reasoning.rs      ← 推理档位映射与 WorkBuddy 回退策略
│       │   ├── model_status.rs   ← 模型状态构造、请求元信息、客户端展示 ID（`OC · 名称`）
│       │   ├── platform.rs       ← 数据目录、路径/平台原语、运行时包名
│       │   ├── atomic.rs         ← 原子替换（重试 → 等待 → 换名，Windows 共享冲突友好）
│       │   └── json.rs           ← 容错 JSON 解析 + JS 语义原语（truthy/js_stringify/…）
│       └── tests/
│           ├── js_parity.rs      ← JS↔Rust 对拍：与 fixtures 里冻结的 expected 比较（默认无需 Node）
│           ├── red_lines.rs      ← 运行期红线守卫（鉴权/Origin/权限表/隔离配置/限额/环境白名单）
│           └── fixtures/*.json   ← 11 个模块共 271 个用例，每个带 expected 黄金快照
├── scripts/                      ← bump-version.mjs / check-version.mjs
├── docs/                         ← contract.md、validation.md、version/*、research/upstream-architecture.md
├── .github/workflows/release.yml ← cargo 化 CI（无 sidecar 步骤）
├── .env.example / .env.local     ← 签名环境变量（.env.local 严禁提交）
└── .gitignore                    ← 忽略 node_modules、dist/、src-tauri/target|gen、.zwork/、src-tauri/binaries(防误加回)
```

> **为什么核心是 `src-tauri/core/` 而不是并进壳的单个 crate**：核心仍是独立 crate（`wbbridge-core`，
> 且是**独立 workspace**），壳通过 `path = "core"` 依赖把它静态编进同一进程。这样核心的 197 个测试
> 不必编译 tauri/webkit 依赖图（CI 的 Linux 测试任务因此无需装 libwebkit2gtk），`wbbridge-core`
> 也能单独构建出可执行文件做进程级冒烟；同时全部 Rust 代码物理位置都在 `src-tauri/` 下。
> 合并成单 crate 会把这三点全部丢掉，故不采用。

## 核心架构与数据流

### 进程模型

```
┌──────────────── Tauri 壳（单一进程）─────────────────┐
│  WebView: dist/（Vue）↔ invoke(core_action/…) / core-status │
│  托盘菜单 · 状态轮询 status.json · 退出流程              │
│  专用 tokio Runtime ──► orchestration::run(StartOptions)│
│                          │                              │
│  回环 HTTP 127.0.0.1:<port>/admin/*（壳→核心，Bearer）  │
└───────────────────────────┬─────────────────────────────┘
                            ▼
             核心编排（同进程，函数调用 + HTTP）
        ├── HTTP 服务 127.0.0.1:<port>（Bearer 鉴权、拒绝 Origin、并发 ≤4）
        ├── status.json（串行 + 原子写）→ 壳/UI 读取
        ├── WorkBuddy models.json（原子写 + 增量合并）
        └── OpenCode 子进程（隔离 env + 随机回环端口 + serve --pure）
```

- **核心不得单方面结束进程**：嵌入壳时 `set_exit_hook` 把「退出」翻译成记录 `core_code` / `core_stopped`，真正退出只由壳的托盘/`RunEvent::ExitRequested` 驱动。
- **壳保留回环 HTTP 调用路径**（`admin_call` + `ACTION_ROUTES`），因此鉴权、Origin 拒绝、并发上限等红线与迁移前完全一致，`core_action` 契约未变。
- 核心可重启：`restart_core` 关停现有实例后按新的 `StartOptions` 重新装入（`APP` 为可重置的进程级全局）。

### 启动时序（`src-tauri/core/src/orchestration.rs`）

1. 端口校验（非 1024–65535 直接失败退出，独立进程按 1）。
2. 数据目录（`StartOptions.data_dir` > `BUDDY_DATA_DIR` > 平台默认），`create_dir_all` + `0700`。
3. **单实例锁**：读 `service.pid`，若指向存活进程且不是自己 → `ALREADY_RUNNING`（独立进程退出码 2）；否则写回自身 pid。
4. 读/建 `api-key`（缺失时生成 32 字节随机 hex，`0600`）。
5. 读 `settings.json`（容错解析）、上一份 `status.json`（延续 `modelResults`）、日志 >5MB 轮转。
6. 解析 models 文件路径（`BUDDY_MODELS_FILE` → settings 已保存值 → WorkBuddy 配置目录发现；失败返回 null，**绝不静默回退**）。
7. `find_runtime` 定位或下载 OpenCode 运行时 → `start_backend` 启动子进程 → axum HTTP 服务开始监听。
8. `/agent` 校验：确认隔离配置中的 `buddy-bridge`、`buddy-chat` 两个自定义 agent 存在，否则 `phase` 不进入 `ready`。
9. 系统代理（`useSystemProxy`）：macOS `scutil --proxy`、Windows 注册表解析注入子进程 env；**解析失败回退为关闭，不阻断启动**。
10. 启动导入（`BUDDY_NO_SYNC !== "1"`）：**先清除旧 own 条目并 `drain_sync().await`，再逐模型探测（每模型各 `PROBE_TIMEOUT_MS = 60000` 预算，重试共用同一 deadline）**，仅 `ok` 的模型进入发布集。
11. 同步发布集到 WorkBuddy `models.json`，启动状态轮询/活动计时/刷新与探测后台任务；服务已在监听时启动失败会保留进程，由壳经 `/admin/shutdown` 结束这次启动。

### 请求链路（`/v1/chat/completions`）

```
WorkBuddy
  → server.rs：Bearer 鉴权（timing-safe）→ 拒绝 Origin（403）→ 限流（>4 并发 429 busy）→ 读体（>8MB 413）
  → protocol::prepare(body, models)：模型匹配 / messages 校验 / n=1 / tool 定义唯一性 / 图片 mime 白名单（png·jpeg·webp·gif）
  → backend::complete(...)：OpenCode 会话 + 消息 + 事件流
        ├─ 原生工具调用 → native_permissions（'*': 'ask'，question/websearch/codesearch/webfetch/task/plan_enter/plan_exit/todowrite: 'deny'）
        │    → 阻塞并交回客户端（handoff），模型不得自行执行本地动作
        └─ chatOnly 模型（buddy-chat）遇原生工具 → 502 native_tool_activity
  → protocol::decode(text, prepared)：校验信封 {content, calls}、校验工具与参数
        ├─ 一次格式修正（resend_prompt：只纠正格式）
        └─ 仍失败 → repair()：由辅助模型（translator，TRANSLATOR_ORDER 中挑，**排除刚失败的模型**）转写一次；探测路径禁止转写
  → completion() / send_sse()：非流式 JSON 或 SSE（先校验后发送，含 10s 心跳注释行）
  → on_result：写入 status.json 计数与最近请求（客户端取消的请求绝不记为成功）
```

### 模型发布链路

```
models.json → free_models(providers)（免费判定：输入/输出/缓存全 0、输出支持文本、非 deprecated）
  → 逐模型探测（probe_model：工具调用类 + 纯文本类，失败可重试 1 次）
  → 仅 ok 的模型进入发布集（客户端 ID = `OC · <name>`）
  → sync_models（原子写）→ merge_models（保留非本工具条目，只清理/更新 OWNER = 'buddy-bridge-v1' 的条目）
```

### 运行时隔离链路

```
find_runtime
  ├─ 优先级：BUDDY_OPENCODE_PATH > 托管目录 runtime/<version>/ > ~/.opencode/bin/（含 Homebrew 等）> Windows npm 全局 shim
  │   （拒绝 .cmd/.bat/.ps1 启动器脚本；版本必须形如 x.y.z）
  └─ 与 registry 最新版比对：本地不落后则复用；否则下载
       registry 顺序：https://registry.npmjs.org → https://registry.npmmirror.com
       校验：metadata.name/version 合法 + dist.integrity 必须为 `sha512-` + tarball 必须来自白名单 registry
       安装：下载 → sha512 校验 → 解包只取 `package/bin/<binary>` → 原子替换 → chmod 755 → 回读版本必须一致
start_backend
  → env 白名单（ENV_ALLOW：PATH/HOME/USER/LANG/TMPDIR/SHELL/SSL_CERT_FILE/NODE_EXTRA_CA_CERTS 与 Windows 必需项）
  → XDG_* 指向数据目录 + 独立随机 OPENCODE_SERVER_PASSWORD
  → 关闭 OPENCODE_DISABLE_AUTOUPDATE / _PROJECT_CONFIG / _CLAUDE_CODE / _EXTERNAL_SKILLS
  → OPENCODE_CONFIG_CONTENT = isolated_config()（权限全 ask/deny、autoupdate:false、share:disabled、两个自定义 agent）
  → 启动 `serve --pure --hostname 127.0.0.1 --port <随机空闲端口>` → 轮询 /global/health（≤120 次 × 500ms，版本必须一致）
  → 关停：SIGTERM → 等待 ≤4s → SIGKILL
```

## 关键模块与函数清单（修改前必须确认）

| 模块 | 关键公开项 | 职责 |
|---|---|---|
| `orchestration.rs` | `run(StartOptions)`、`StartOptions{data_dir,port,handle_signals}`、`set_exit_hook`、`HELP`、`ALREADY_RUNNING`；内部 `update_and_persist`、`sync_published`、`drain_sync`、`record`、`usable_models`、`published_models`、`attach_translator`、`start_probes`、`refresh`、`read_models`、`import_models`、`shutdown`、`startup_sequence`、`bootstrap`、`watch_runtime`、`spawn_parent_watchdog`、`note_activity` | 编排与生命周期（对应旧 `main.js`） |
| `server.rs` | `Server::new(key)` 链式注入 → `build() -> (Router, ServerControl)`、`serve`、`ACTION_ROUTES`、`route_for`/`method_for`、`MAX_BODY_BYTES`、`MAX_CONCURRENT_REQUESTS`、`DEFAULT_HEARTBEAT`、`REQUEST_BODY_TIMEOUT`、`AbortController/AbortSignal`、`ResultRecord`、`Handlers`、`BoxFuture`、类型别名 `CompleteFn/ModelsFn/AdminFn/…` | HTTP 路由与鉴权：`GET /health`、`GET /v1/models`、`POST /admin/{probe,system-proxy,import,refresh,shutdown}`、`POST /v1/chat/completions` |
| `protocol.rs` | `BridgeError`（`with`/`status`/`code`）、`prepare`、`PreparedRequest`、`decode`、`completion`/`completion_with`、`send_sse`、`random_hex`/`random_uuid`、`parse_image_data_url` | OpenAI 兼容入参校验、信封解码、响应组装、SSE |
| `backend.rs` | `Backend`（`complete`、`set_translator`）、`native_permissions()`、`free_models`、`shrink_permission`、`to_bridge_error` | OpenCode HTTP 客户端、事件流、原生审批拦截、免费模型发现 |
| `runtime.rs` | `find_runtime`、`isolated_config()`、`isolated_environment`、`ENV_ALLOW`、`start_backend`/`Started`、`stop_backend`、`runtime_candidates`、`compare_versions`、`generate_password`、`RuntimeOptions`、`FetchFn/LatestFn/…` | 运行时定位/下载/校验/启动与隔离配置 |
| `probe.rs` | `PROBE_TIMEOUT_MS`、`probe_tools`、`probe_body`、`judge_probe`、`format_unsupported`、`retryable_probe_codes`、`probe_model`、`probe_failure`、`should_retry` | 模型探测协议与判定 |
| `repair.rs` | `REPAIR_SYSTEM`、`client_conventions`、`raw_material`、`tool_catalog`、`repair_body`、`extract_json`、`translator_request`、`resend_prompt`、`RepairDeps`、`repair` | 格式修复与辅助模型转写 |
| `handoff.rs` | `build_handoff`、`handoff_input`、`reject_feedback`、`validate_action`、`has_category` | 原生工具 handoff 协议 |
| `sync.rs` | `OWNER`（`'buddy-bridge-v1'`）、`LOCK_STALE_MS`、`atomic_write`、`merge_models`、`sync_models`/`sync_models_with`、`SyncOptions`、`SyncOutcome`、`SyncIo`、`SyncError` | WorkBuddy 配置写入与增量合并 |
| `workbuddy_config.rs` | `validate_models_file`、`resolve_models_file`、`resolve_models_path`、`ConfigError`、`INVALID_PATH_MESSAGE` | models.json 定位与校验 |
| `system_proxy.rs` | `parse_system_proxy`、`system_proxy_environment`、`environment_from_output`、`parse_windows_proxy`、`js_number`、`ProxyError` | 系统代理发现与子进程 env 映射 |
| `reasoning.rs` | `EFFORT_LEVELS`、`reasoning_efforts`、`work_buddy_reasoning` | 推理档位与回退策略 |
| `model_status.rs` | `model_result`/`model_result_at`、`with_request_meta`、`client_model_id`、`CATEGORY_*`、`now_iso` | 状态记录与客户端展示 ID |
| `platform.rs` | `data_directory`/`data_directory_with`、`DATA_DIR_NAME`、`runtime_package`、`host_platform`/`host_arch`、路径原语 | 平台路径与运行时包名 |
| `atomic.rs` | `replace_with_retry`/`replace_with_retry_with`、`ReplaceError`、`DELAYS`、`TRANSIENT_CODES`、`node_code_for_io` | 原子替换（Windows 共享冲突重试） |
| `json.rs` | `parse_json`、`Env`、`truthy`、`strict_eq`、`js_stringify`/`js_stringify_pretty`、`number_from_f`、`type_of` 等 | 容错 JSON + JS 语义等价原语 |
| `src-tauri/src/lib.rs` | `core_action`、`restart_core`、`core_running`、`data_dir_path`（`generate_handler` 四命令）、事件 `core-status`（轻量快照）/ `core-activity`（activity + modelResults）/ `core-failed`、`start_core`/`stop_core`/`watch_status`/`build_tray`/`admin_call` | 托盘壳：生命周期、IPC、状态轮询、退出预算 |
| `src/core/bridge.js` | `action(name, value)`、`onState(cb)`、`onDismiss(cb)` | 面板与壳的唯一边界（invoke + listen） |
| `src/core/activity.js` | `activityText(...)` | 活动文案统一（托盘与面板共用） |
| `scripts/*.mjs` | `bump-version.mjs`、`check-version.mjs` | 版本单一来源同步与校验 |

### 关键常量（改动前必须确认语义）

| 常量 | 值 | 位置 |
|---|---|---|
| 默认端口 | `41980`（`BUDDY_PORT` 覆盖，范围 1024–65535） | `orchestration.rs`；壳 `pick_port()` |
| 请求体上限 | `MAX_BODY_BYTES = 8MB` → 413 | `server.rs` |
| 并发上限 | `MAX_CONCURRENT_REQUESTS = 4`（超出 429 busy） | `server.rs` |
| 请求体读取超时 | `REQUEST_BODY_TIMEOUT = 20s` | `server.rs` |
| SSE 心跳 | `DEFAULT_HEARTBEAT = 10s` | `server.rs` |
| 探测超时 | `PROBE_TIMEOUT_MS = 60_000`（每个模型一份，该模型的重试共用同一 deadline） | `probe.rs`；`orchestration.rs::start_probes` |
| 日志轮转阈值 | `5MB` | `orchestration.rs` |
| 活动去抖 | `1000ms`（紧急文案除外） | `orchestration.rs::note_activity` |
| 同步锁过期 | `LOCK_STALE_MS = 5min` | `sync.rs` |
| 健康轮询 | `120` 次 × `500ms` | `runtime.rs` |
| 关停窗口 | SIGTERM → `4s` → SIGKILL | `runtime.rs::stop_backend` |
| 权限文本截断 | `shrink_permission(value, limit)`，调用侧固定传 `400` | `backend.rs` |
| 同步归属标记 | `OWNER = "buddy-bridge-v1"` | `sync.rs` |
| status schema | `STATUS_SCHEMA_VERSION = 1` | `orchestration.rs` |
| 状态内置版本 | `"0.2.0"`（写入 `status.json`，历史沿革值） | `orchestration.rs` 状态初值 |
| 格式类失败集合 | `REQUEST_SHAPED_FAILURES`（4 项） | `orchestration.rs` |
| 转写首选名单 | `TRANSLATOR_ORDER`（4 项，须排除刚失败模型） | `orchestration.rs` |

## 编码规范

> 以下规范适用于本项目所有代码改动，所有 Agent 新增或修改代码时必须遵守。

### 语言与依赖（Rust 核心）

- `src-tauri/core/` 是**纯 Rust、无 Node 依赖**；edition 2021，`rust-version = "1.75"`。
- 跨语言语义对齐靠 `json.rs` 的 JS 等价原语（`truthy`、`strict_eq`、`js_stringify`、`Env`）与 `serde_json` 的 `preserve_order`；**不得改用无序 map**，键序是 `models.json` 合并语义的一部分。
- 新增运行期依赖必须说明理由（体积 + 供应链可信度双重成本）；能用标准库或既有依赖完成的绝不引包。禁止把任何 Node 运行时、pkg、esbuild 重新引入核心链路。
- 异步一律 `async/await` + tokio；共享可变状态用 `Arc<Mutex<..>>` / 原子量，串行写链（`Slot` + `CHAIN_SEQ`）不得改成"看起来更快"的并发写。
- 禁止 `std::env::set_var` 配置核心实例；一切走 `StartOptions`。
- 禁止在未确认生命周期安全的前提下使用 `unwrap()`/`expect()` 于错误路径之外的 panic 风险点；`Mutex` 取值统一 `.lock().unwrap()` 仅用于「中毒即不可恢复」的既有约定写法。

### 错误与注释

- 业务错误统一 `BridgeError { message, status, code }`（`protocol.rs`），HTTP 层据此映射状态码与 `error.type`；不得吞掉错误或返回无码错误。
- 注释写**为什么**（约束、坑、平台差异、红线由来），不写"是什么"；现有源码已建立该风格。
- 平台差异集中在 `platform.rs` / `system_proxy.rs` / `runtime.rs`（`#[cfg(unix)]` / `#[cfg(windows)]`），业务代码不得散落平台判断。
- 日志只写非敏感信息；**严禁把 `api-key`、`OPENCODE_SERVER_PASSWORD`、用户凭据写入日志或 `status.json`**。

### 文件与平台

- 文件写入优先走 `atomic.rs::replace_with_retry` 或 `sync.rs::atomic_write`，禁止裸 `rename` 覆盖；Windows 需考虑 `.exe` 与不能替换运行中镜像。
- 新建目录/敏感文件显式权限（目录 `0700`、密钥文件 `0600`）。
- 路径拼接使用 `platform.rs` 提供的原语，禁止硬编码分隔符。

### 测试规范

- 核心测试全部用 Rust：`cargo test`（`src-tauri/core/`）。基线 **197 通过 / 0 失败**：
  - lib 单元测试 179（含 `src/*.rs` 内 `#[cfg(test)]`）；
  - `tests/js_parity.rs` 11（每模块一组，比较 `tests/fixtures/*.json` 冻结的 `expected`）；
  - `tests/red_lines.rs` 7（运行期红线守卫）。
- **JS↔Rust 对拍已快照化**：`tests/fixtures/*.json` 每个用例带 `expected`（迁移前由 JS 实现录制、已抹平随机 id / `created` / `ms` / sync 文案；沙箱绝对路径在比较前还原成 `$BASE` 占位符，快照因此不绑定机器与目录布局）。默认不启动 Node。需要重新录制时，把仓库外归档 `backup/wbBridge-node-20261001/` 的 `core/` 与 `core-rs-tests-js/`（归档内的目录名，放回后即 `tests/js/`）放回原位，再 `WB_PARITY_RECORD=1 cargo test --test js_parity`；**禁止**在没有 JS 真相的情况下手工编辑 `expected` 来"让测试通过"。
- **测试严禁真实联网、真实下载**：网络与运行时行为必须通过注入点（`RuntimeOptions` 的 `FetchFn`/`LatestFn`/`ProbeFn`、`SyncIo`、`atomic::replace_with_retry_with`）替换。
- 新增/修改行为必须补测试；测试名要描述被保护的行为。触碰安全/供应链/数据红线时，优先在 `tests/red_lines.rs` 补断言而不是只写文档。
- 修改 `src-tauri/core/src/` 后必须运行 `cargo test` 并报告**实际**通过/失败数量，**不得以"应该能过"代替执行**；`cargo clippy --all-targets` 必须保持 0 warning。

### UI 规范

- 面板 = Vue 3 SFC + Vite 构建产物 `dist/`，由 Tauri WebView 加载；**不得引入 CDN 或任何外部请求**，必须满足 `tauri.conf.json` 的 CSP（`default-src 'self'`、`script-src 'self'`、`connect-src ipc: http://ipc.localhost`）。
- 与壳的通信只走既有契约：`src/core/bridge.js` 的 `action(name, value)` / `onState(cb)` / `onDismiss(cb)`，底层是 `invoke('core_action' | 'restart_core' | 'core_running' | 'data_dir_path')` + 事件 `core-status`（轻量快照）/ `core-activity`（`activity` + `modelResults` 明细，面板必须与最近一次轻量快照合并后再下发，否则逐模型状态永远为空）/ `core-failed`。**不得新增或改名 IPC 命令/事件**，除非同步更新 `src-tauri/src/lib.rs` 与 `src-tauri/capabilities/default.json`。
- 面板为只读展示 + 动作触发：不得在面板内直接读写文件、直接访问网络、直接调用 OpenCode。
- 动作执行期间必须置忙（按钮禁用 + spinner + 结果反馈），失败必须显示原因。

#### 布局与窗口（2026-10-01 面板改造后的当前实现）

- **布局**：外层 `.shell` 为左右两段——左侧栏固定宽 `var(--sidebar-w)` + 右侧主区；主区默认单列，选中模型时 `.content.is-split` 变为
  `grid-template-columns: minmax(0, 1fr) var(--details-w)`（`src/App.vue`）。
- **详情面板是右侧常驻分栏，不是浮层**：无遮罩、不 `position: fixed/absolute`、不覆盖列表；**任何窗口宽度都不降级为上下堆叠**。
  禁止新增按宽度堆叠的媒体查询或 `<900px` 降级分支——`src/` 内现存的媒体查询只有 `prefers-color-scheme`（`variables.css`）与
  `prefers-reduced-motion`（`base.css`）两处。收起详情有三条等价路径：`Esc`（`App.vue::onKeydown`）、详情头部「收起详情」按钮、
  窗口失焦（`onDismiss`）；收起即清空选中，列表占满主区宽度。
- **侧栏是分组导航**：分组为「模型 / 运行 / 集成 / 其他」，底部为「运行设置」（系统代理开关 + 版本号）。当前**只有「模型与服务」
  是已实现视图**（`state: 'current'`）；运行日志、用量与额度、WorkBuddy 集成、关于与更新这 4 个入口必须保持**禁用态 + 「规划中」标签**
  （`state: 'planned'`，`src/views/SideBar.vue`）：**文档与界面都不得把它们描述/呈现为已实现功能**，也不得改成可点击却无响应的假入口。
- **设计 token**：尺寸与颜色一律取自 `src/styles/variables.css`；小字号说明文字（副标题、页脚说明、耗时行等）用 `--muted-strong`，
  普通次要文字用 `--muted`，不得在组件里写死颜色或宽度。

| 布局常量 | 值 | 位置 |
|---|---|---|
| 默认窗口 | `1120 × 720` | `src-tauri/tauri.conf.json` → `app.windows[0]` |
| 最小窗口 | `860 × 560`（`minWidth` / `minHeight`） | 同上 |
| 侧栏宽 | `--sidebar-w: 208px` | `src/styles/variables.css` |
| 详情栏宽 | `--details-w: clamp(300px, 45%, 360px)` | 同上 |
| 宽度断点 | **无**（不按窗口宽度堆叠，见上） | `src/` 内无宽度媒体查询 |

### 命名规范

| 类别 | 规则 | 示例 |
|---|---|---|
| Rust 文件/模块 | snake_case，单词表意 | `workbuddy_config.rs`、`model_status.rs` |
| Rust 函数/变量 | snake_case，动词开头表意 | `usable_models()`、`sync_published()`、`resolve_models_file()` |
| Rust 常量 | SCREAMING_SNAKE_CASE | `PROBE_TIMEOUT_MS`、`TRANSLATOR_ORDER`、`MAX_BODY_BYTES`、`OWNER` |
| Rust 类型 | CamelCase | `BridgeError`、`PreparedRequest`、`SyncOptions` |
| 对外契约 | 保持 JS 侧原名（snake_case 化） | 路由 `/v1/chat/completions`、`/admin/system-proxy`；错误码 `invalid_model_output` |
| Vue 组件文件 | PascalCase；视图放 `src/views/`，通用组件放 `src/components/` | `views/SideBar.vue`、`views/ModelDetails.vue`、`components/ModelRow.vue` |

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

`scope` 约定：`core`（`src-tauri/core/`）、`ui`（`src/` 面板与 Vite）、`tauri`（壳与 `src-tauri/icons/`）、`scripts`（版本脚本）、`docs`（文档）、`ci`（工作流）。

示例：`refactor(core): 移除 Node sidecar，核心改为 Rust 内嵌`

提交纪律：

- 提交前必须运行 `cargo test`（`src-tauri/core/`）；测试失败禁止提交。
- 一次提交只包含一类改动，禁止把源码改动与大批二进制/图标混在一起。
- **严禁提交**：`api-key`、`.env.local`、`status.json`、`settings.json`、`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/core/target/`、`src-tauri/gen/`、`.zwork/`、任何私钥文件、以及仓库外的 Node 归档目录。
- 仓库现状：分支 `dev`，远端 `origin = https://github.com/trexwb/wbBridge.git`（另有 `main`）。历史上只有 `README.md` 被跟踪，**其余文件全部处于未跟踪/修改态**；`git status` 里的 `D src/core/*`、`D docs/brand/*` 等是目录重组的结果，删除记录不等于丢数据（数据在仓库外归档）。提交前务必 `git status` 复核实际纳入内容。

## 红线与限制

### 安全红线（不得移除、不得放松、不得绕过；由 `tests/red_lines.rs` 部分钉死）

1. **鉴权**：所有路由（含 `/health`）必须校验 `Authorization: Bearer <api-key>`，比较走 `server.rs::constant_time_eq`（与 Node `timingSafeEqual` 同语义：定长逐字节异或累加，长度不等直接 false）；`api-key` 文件权限 `0600`，缺失时随机生成；裸 key / 非 Bearer 方案一律 401。
2. **拒绝浏览器来源**：请求带任何非空 `Origin` 头一律 403。WorkBuddy 只经其原生运行时访问本服务。
3. **仅监听回环地址**：`TcpListener::bind(("127.0.0.1", port))`（`orchestration.rs`），禁止改为 `0.0.0.0` 或对外开放。
4. **限流与体积**：并发 ≤4、请求体 ≤8MB，不得放宽。
5. **原生工具严禁开放给模型**：`native_permissions()` 必须保持 `'*': 'ask'`，且 `question`/`websearch`/`codesearch`/`webfetch`/`task`/`plan_enter`/`plan_exit`/`todowrite` 为 `deny`；`isolated_config()` 的 `autoupdate: false`、`share: "disabled"` 不得改动。**模型绝不允许直接执行本地动作**，原生动作只能以 handoff 交回客户端。
6. **chatOnly 模型不得使用工具**：纯文本 agent（`buddy-chat`）遇原生工具活动必须报 `native_tool_activity`，不得降级为静默执行。
7. **密钥零泄漏**：子进程环境只透传 `ENV_ALLOW` 白名单（其中不得出现任何凭据类变量名）；`OPENCODE_SERVER_PASSWORD` 只能用核心自己生成的值；密钥不得写入日志、`status.json` 或仓库。

### 供应链红线（OpenCode 运行时）

1. 只能从**白名单 registry**（`registry.npmjs.org`、`registry.npmmirror.com`）获取运行时元数据与 tarball；元数据必须满足 `name` 匹配、语义化 `version`、`dist.integrity` 以 `sha512-` 开头、tarball 来源属于白名单。
2. **下载后必须校验 sha512 完整性**，校验失败不得使用、不得"跳过校验重试"；镜像与官方字节不一致时必须报错（测试已固化该行为）。
3. 解包只提取 `package/bin/<binary>` 单个文件，禁止整包解开到运行时目录。
4. 安装后必须**回读 `--version` 与元数据版本一致**，不一致即失败；禁止把无法确认版本的候选当作运行时。
5. 拒绝 `.cmd` / `.bat` / `.ps1` 启动器脚本作为运行时二进制。
6. 不得把运行时降级为"直接调用系统 `opencode` 的共享实例"——隔离（独立 XDG 目录、独立端口、独立密码、独立配置）是本项目的核心价值。

### 数据红线

1. 写 WorkBuddy `models.json` 时：**只清理/更新 `OWNER='buddy-bridge-v1'` 名下的条目，其他条目与对象元数据必须原样保留**；手动条目 ID 冲突时不得擅自覆盖。
2. `status.json` 写入必须串行 + 原子；探测中的临时状态不得覆盖已完成状态。
3. `service.pid` 单实例锁不得删除或绕过；重复启动必须失败（独立进程 `exit(2)`）而不是抢占端口。
4. 格式类上游失败（`invalid_model_output` / `invalid_tool_call` / `native_tool_activity` / `output_truncated`）**不得撤销已发布模型**；探测路径**不得启用辅助模型转写**；转写候选**不得包含刚失败的模型本身**。
5. 客户端取消的请求不得记录为成功。

### 工程红线

1. **禁止重构**：只做针对性最小改动；不得重命名已有导出、路由、错误码、IPC 命令与事件名。
2. **禁止重新引入 Node 运行期到核心**：不得为 `src-tauri/core` 加 sidecar/pkg/esbuild 链路，也不得新建第二个核心实现。
3. **禁止在仓库内写入运行时数据**：所有运行期文件（`api-key`、`status.json`、日志、runtime、opencode 目录）只能写入平台数据目录（或壳的 `app_data_dir`）。
4. **禁止编造已验证状态**：GUI 启动、CI 跑通、签名发布这三件事在本仓库**尚未实测**，涉及它们的说明必须标注"未验证"。
5. **禁止大规模二进制入库**：`src-tauri/binaries/` 已随 sidecar 方案废弃（目录已删除，`.gitignore` 仍保留守卫条目），不得复活该约定。
6. **禁止擅自推进版本号**：见下节。
7. **Rust 代码只能放在 `src-tauri/` 下**：壳 = `src-tauri/src/`，核心 = `src-tauri/core/`（独立 workspace、壳以 `path = "core"` 依赖）。不得在仓库根重建 `core-rs/` 之类的第三个 Rust 位置，也不得把核心并进壳的单个 crate（那会让核心测试被迫编译 tauri/webkit 依赖图、失去独立二进制）。

## 当前基准版本

| 项 | 值 | 来源 |
|---|---|---|
| 版本单一来源 | **1.0.0** | 根 `package.json` 的 `version` |
| 壳工程同步落点 | **1.0.0** | `src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 的 `[package] version` |
| 核心 crate 内部版本 | **0.1.0** | `src-tauri/core/Cargo.toml`（`wbbridge-core --version` 输出，与产品版本解耦，**不得"顺手对齐"**） |
| 状态内置版本 | `0.2.0` | `src-tauri/core/src/orchestration.rs` 写入 `status.json` 的 `version`（沿自上游参考实现，界面上可见） |
| 上游调研基线 | `0.2.5` | `docs/research/upstream-architecture.md` |
| 测试基线 | **197 通过 / 0 失败**（lib 179 + js_parity 11 + red_lines 7，约 0.3s） | `src-tauri/core/` 下 `cargo test` |
| 迁移前 JS 基线 | 97 通过 / 0 失败（node v24.21.0） | 仓库外归档 `backup/wbBridge-node-20261001/core/test/` |
| 运行时基线 | OpenCode 版本由 registry 最新版决定（不固定）；核心不再需要 Node | `src-tauri/core/src/runtime.rs` |

**版本号规则（写死，所有 Agent 必须遵守）**：

- 版本号以**根 `package.json`** 为唯一来源；`src-tauri/core/Cargo.toml` 的 `0.1.0` 是 crate 内部版本，`status.json` 的 `0.2.0` 是历史沿革值，**两者都不得在无用户指令的情况下静默改动**。
- `npm run version:check` 校验 5 处一致（`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml` / `AGENTS.md` 两行）；推进版本一律用 `npm run version:set -- <x.y.z>`。
- **仅当**新增了与现有问题**不同类、不同根因**的功能/修复，且**用户明确允许**推进版本号时，才可末位 +1。
- **以下情形绝对禁止推进版本号**：
  1. 同一问题的多轮往返排查与再次修复；
  2. 用户明确要求"不修改版本号 / 回退到 X.Y.Z"；
  3. 同一自然日对同一模块/同一类 bug 的追加修复；
  4. 纯 `docs/`、`README.md`、本 `AGENTS.md` 等文档类改动；
  5. 纯文案、注释、日志措辞、去抖等不引入新逻辑分支的打磨；
  6. 移植/迁移过程中的等价实现（行为未变即版本未变）。
- 用户要求"回退到 X.Y.Z"时，所有落点必须一致改写为用户指定值，本回合内不得再以"我刚改了代码所以 +1"为由推进。

## 变更与交付约定（面向 Agent）

1. **改动前**：`read` 读取目标文件原文；涉及 `orchestration.rs` 编排、`server.rs` 路由、`protocol.rs` 校验、`runtime.rs` 下载的逻辑，必须同时阅读相关测试（含 `tests/fixtures/*.json` 的快照断言）确认既有行为边界。
2. **改动中**：一次只改一个独立区块/函数，避免大范围替换；不修改与任务无关的文件。
3. **改动后**：
   - 必须运行 `src-tauri/core/` 的 `cargo test` 与 `cargo clippy --all-targets`，并在回复中给出**真实的**通过/失败与 warning 数量；
   - 必须用 `grep` / `read` 验证改动已落盘；
   - 不得自动 `git commit` / `git push`；
   - 若改动涉及对外契约（路由、错误码、`status.json` 字段、`models.json` 写入格式、IPC 命令/事件），必须在回复中显式列出并提示用户影响面；
   - 若改动触及红线，必须同步更新 `tests/red_lines.rs` 的对应断言。
4. **遇到不确定**：宁可向用户询问，也不要凭推测修改安全、供应链、隔离相关代码。
*（内容由AI生成，仅供参考）*
