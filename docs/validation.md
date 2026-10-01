# WB Bridge v1.0.1 验证记录

> 阅读顺序：最新记录在前。自 **2026-10-01** 起核心已从 Node.js sidecar 迁移为 Rust 库（静态链接进壳），
> 该日期之后的条目描述 Rust 形态；下方的 2026-09-30 条目属于**迁移前的 Node/sidecar 时代**，作为历史
> 保留原样（其 97 项测试、`src/core/`、`src-tauri/binaries/` 等结论已不再对应当前仓库）。

日期：2026-10-01（本机 macOS，Apple Silicon；Rust 核心 + Vue 面板）

## updater 接线、真实签名构建与更新清单（2026-10-01，同日第十轮）

### 落地的改动

- **Rust 侧**：`src-tauri/Cargo.toml` 新增 `tauri-plugin-updater = "2"`、`tauri-plugin-process = "2"`（Cargo.lock +298 行 / **24 个新传递 crate**；理由：官方 Tauri 插件、与既有 tauri 同一维护方，是应用内更新的唯一正规路径）。`lib.rs` builder 链注册两者。
- **配置**：`tauri.conf.json` 加 `bundle.createUpdaterArtifacts: true` 与 `plugins.updater`（内联 `pubkey`，端点 `https://github.com/trexwb/wbBridge/releases/latest/download/latest.json`）。
- **权限**：`capabilities/default.json` 加 `updater:default` + **`process:allow-restart`**（不是 `process:default`：后者含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链直接杀进程）。
- **前端**：`src/core/bridge.js` 加 `checkUpdate/downloadUpdate/relaunchApp`（`Update` 句柄只留在模块内，视图拿可序列化快照）；新增 `src/core/update.js` 状态机（`idle|checking|available|downloading|ready|uptodate|error`，冷启动 5s 后静默检查、静默失败不打扰）；`App.vue` 挂载时启动静默检查；`AboutView.vue` 换成真实更新区；`base.css` 新增确定型进度条 `.progress`（`prefers-reduced-motion` 允许清单同步补 `.progress.is-active > i::after`）。
- **脚本 / CI**：新增 `scripts/gen-latest-json.mjs`（`npm run gen:latest`）；`release.yml` 三个构建作业补 `.sig` / `.app.tar.gz` / `.AppImage.tar.gz` glob 与签名环境变量、`includeUpdaterJson: false`、**macOS 补架构后缀**步骤，末尾新增 `update-manifest` 作业单点写 `latest.json`。
- **依赖判断更正**：**不需要**任何 `@tauri-apps/plugin-*` npm 包 —— `app.withGlobalTauri` 会把插件 API 注入 `window.__TAURI__.updater` / `.process`（读 `@tauri-apps/cli` 内的 `api-iife.js` 证实）。面板因此仍无外部请求、CSP 不变。

### 实测证据（本轮真实执行）

| 步骤 | 结果 |
|---|---|
| `cargo test`（`src-tauri/core/`） | ✅ **207 通过 / 0 失败**（lib 187 + js_parity 11 + red_lines 9） |
| `cargo test --lib`（`src-tauri/`） | ✅ **8 通过 / 0 失败** |
| `cargo clippy --all-targets`（核心）/ `--no-deps --all-targets`（壳） | ✅ **0 warning**（两者） |
| `npx eslint .` | ✅ 0 problems（`.vue` 不在 `eslint.config.js` 匹配范围内，仍是既有缺口） |
| `npm run vite:build` | ✅ 通过 |
| `node scripts/gen-latest-json.mjs` 合成产物测试 | ✅ 正例 4 项：六平台齐全、linux 裸 `.AppImage` 与 `.tar.gz` 并存（取 `.tar.gz` 并告警）、只有裸 `.AppImage`、`--expect` 收窄 + `--tag` 覆盖；负例 5 项全部退出 1：mac 资产缺架构后缀、某平台缺 `.sig`、同目录互不相干的两个已签名包、平台目录缺失、产物目录不存在 |
| 真实签名构建 `tauri build --bundles app` | ✅ 产出 `WB Bridge.app.tar.gz`（3,595,514 B）+ `WB Bridge.app.tar.gz.sig`（428 B）；解析签名包体得 key ID **`126D4E208E0F17BA`**，与 `tauri.conf.json` 内嵌公钥一致；trusted comment 含 `version:1.0.1` |
| `tauri build --bundles dmg` | ✅ 产出 `WB Bridge_1.0.1_aarch64.dmg`（3,720,766 B / 3.55 MiB）；只读挂载后核对：`WB Bridge.app` + `Applications` 软链，Mach-O **arm64**、`CFBundleShortVersionString=1.0.1`、id `app.wbbridge.desktop`；`codesign` 显示 `flags=0x10002(adhoc,runtime)`、`Signature=adhoc`、无 TeamID；`codesign --verify --deep --strict` = valid on disk + satisfies DR；核对后已 `hdiutil detach` |

### 本轮推翻了四条此前写进文档的结论

1. **`latest.json` 不能交给 tauri-action**：官方文档那句 "Tauri Action generates a static JSON file" 在**单平台**成立；本工作流是 6 个并发作业往同一个 tag 上传，各写一次清单会互相覆盖并静默漏平台。因此改为 `includeUpdaterJson: false` + 末尾单一 `update-manifest` 作业。
2. **平台键不能靠产物文件名推**：macOS 的 updater 包实测就叫 `WB Bridge.app.tar.gz` —— 既无版本也无架构，两个 mac runner 的名字完全相同（会互覆盖 Release 资产）。故键名改由 artifact **目录名**（含 target triple）推导，并在 mac 作业补一道改名。
3. **签名变量的分工**：`tauri build|bundle` 只读 `TAURI_SIGNING_PRIVATE_KEY`（值可为私钥全文**或私钥文件绝对路径**，本机用路径形式实测通过）；`TAURI_SIGNING_PRIVATE_KEY_PATH` 只对 `tauri signer sign` 生效（等价 `-f`），`signer sign -k` 要的是**私钥字符串**，误传路径报 `failed to decode base64 secret key: Invalid symbol 46`。
4. **前端 npm 插件包并非必需**（见上「依赖判断更正」）。

### 密钥事实与遗留风险（不得对外宣称已闭环）

- 历史文件 `~/.tauri/wbBridge-updater.key`（key ID `2B11F78BEA8A43F`）**不可用**：带密码且 `~/.tauri/wbBridge.env` 里那个口令解不开，且与现配置公钥不配对。按用户决定新生成 wbBridge 专用钥 `~/.tauri/wbBridge-updater-20261001.key`（`0600`、无密码），`pubkey` 已同步换成新公钥；旧文件**未删除、未改写**。
- ⚠ 本机权限隐患（`ls -l` 实测，2026-10-01）：两把私钥本体都是 `0600`（含旧钥），**真正 0644 的是配套的凭据文件**——`~/.tauri/wbBridge.env` 与仓库本地 `.env`（各含非空 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，只核对过变量名与是否有值，未读取内容）。仓库侧安全：`.gitignore:71` 忽略 `.env`、`git ls-files` 只有 `.env.example`，两者均未入库。收紧这两个文件的权限属**本机操作、留给用户决定**，Agent 不改 `~/.tauri` 与仓库外的凭据文件。
- 任何打 tag 的 CI 运行之前，必须先在 GitHub **Secrets** 写入 `TAURI_SIGNING_PRIVATE_KEY`（私钥全文）与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（当前为空）；缺任一项，`createUpdaterArtifacts` 会让三个平台的构建作业**全部失败**（这是有意的 fail-closed，不静默产出无签名包）。
- **仍未验证**：Windows / Linux 产物的真实文件名与签名、`latest.json` 上传后客户端能否真的完成一次「检查 → 下载 → 安装 → 重启」、`relaunch()` 与 `service.pid` 单实例锁 / 托盘的时序，以及 **GUI 实机启动**（本轮只做挂载与静态核对，未运行 `.app`——真跑会写用户真实的 `~/.workbuddy/models.json`）。更新链路的端到端验证只能从 **v1.0.2 → v1.0.3** 起做。

## 关窗即退出应用（2026-10-01，同日第九轮）

用户诉求：**无论哪个系统的桌面端，关闭窗口就要把后台服务一起停掉**，不允许出现「窗口关了、核心还在跑，
用户得自己去托盘再关一次」的驻留态。改动只在壳（`src-tauri/src/lib.rs`）与文档，**未触碰核心、IPC 命令、
事件名、路由、错误码与 `status.json` 字段**；版本号沿用用户上一轮批准的 **1.0.1**（同一未发布版本内的行为调整）。

### 改了什么

| 位置 | 改动 | 原因 |
|---|---|---|
| `on_window_event`（`CloseRequested`） | 由「`prevent_close` + `window.hide()`（驻留托盘）」改为「`prevent_close` + `window.hide()` + 后台线程 `quit_app(&app)`」 | 关窗即走与托盘「退出」**同一条**优雅关停链路（`stop_core_bounded` → `APP.exits` → `app.exit(0)`），不新增第二套退出实现 |
| 同上，事件循环 | 关停放到 `thread::spawn`，不在事件回调里就地执行 | 关停含 `/admin/shutdown` + `STOP_BUDGET = 8s` 等待，就地执行会**冻住事件循环**；先 `hide()` 再后台跑，用户立刻看到窗口消失 |
| `show_main` | `quitting` 置位后直接返回 | 否则关停的数秒窗口里，托盘菜单「打开控制面板」或 macOS Dock 点击会把窗口重新唤回，形成「关不掉也留着」的僵尸窗口 |
| 托盘 | **保留未删**（`build_tray` 与四项菜单照旧） | 最小改动；驻留能力不再依赖它，用户仍可用托盘开关代理与主动退出 |

行为后果（全平台一致）：macOS 红按钮 / `Cmd+W`、Windows/Linux 标题栏关闭 = **终止应用**，不再有驻留态；
想让服务继续运行只能**最小化**窗口。`RunEvent::ExitRequested` 仍就地 `graceful_stop`（有意保留：WorkBuddy 配置
清理必须在进程结束前跑完），与关窗触发的退出经 `quitting.swap(false)` 互相去重，因此两次触发只会停一次。

### 新增测试（壳侧 `cargo test --lib` 5 → 8）

- `repeated_quit_requests_stop_only_once`：连续两次 `graceful_stop` 只有第一次生效（`quitting` 置位、核心槽取空后不再改动）。
- `a_quit_requested_shutdown_is_not_reported_as_failure`：`core_stopped` + `stopping_by_request` 期间 `core_exit` / `service_down`
  都必须返回 `None`；对照断言「主动停止标志复位后同一状态必须被认成崩溃」，避免实现退化为永远返回 `None` 而空过。
- `service_down_reports_only_real_failures`：核心在跑 → 不报；崩溃 → `Some((含「核心服务已退出」, by_exit = true))`；
  关窗收尾期 → 不报；启动失败 → `Some((startup_error 原文, by_exit = false))`，不得与「已退出」口径混用。
- 另补 `idle_state()` 构造 11 个字段的空壳状态，供上述三项复用。

### 本轮实测（数字来自真实执行）

- `cargo test --lib`（`src-tauri/`）→ **8 通过 / 0 失败**（改前 5 通过）；`cargo clippy --no-deps --all-targets` → **0 warning**。
- `cargo test`（`src-tauri/core/`）→ **207 通过 / 0 失败**（核心未改，基线不变）；核心 `cargo clippy --all-targets` → **0 warning**。
- **反向验证（变异测试）**：把 `core_exit` 中的 `|| state.stopping_by_request.load(Ordering::SeqCst)` 守卫删掉后，
  `a_quit_requested_shutdown_is_not_reported_as_failure` 与 `service_down_reports_only_real_failures` **两条同时转红**
  （`test result: FAILED. 6 passed; 2 failed`，位置 `src-tauri/src/lib.rs:799` 与 `:829`）；守卫已按原文恢复并复跑至全绿。
  结论：这组断言不是永真式，确实钉住了「主动关停不得谎报故障」这条口径。

### 未验证与遗留（如实标注）

- ❌ **GUI 仍未实机启动**（`npm run tauri:dev` 与打包 `.app` 都没跑过）。本轮证据只有 clippy 0 warning + 壳侧 8 项单测，
  属于「编译通过 + 单元测试」级别。
- ❌ **必须实机点一遍**才能确认的三件事：① 点关闭后进程与 `127.0.0.1:<port>` 是否真的在 ≤8s 内一起消失；
  ② 托盘驻留取消后 macOS 红按钮 / `Cmd+W` / Dock 图标行为；③ 关窗到进程退出这几秒里窗口是否保持隐藏、
  面板是否弹出假的「核心服务已退出 + 重试」。
- ⚠️ macOS 的「应用仍在 Dock、窗口关掉后重新点击应唤回」这一惯用行为被本改动**主动放弃**（用户明确要求关窗即停）。
  `RunEvent::Reopen → show_main` 代码路径仍在，只在退出流程结束后失效。
- ⚠️ 工作区仍有上一轮遗留：改动**未提交、未打 `v1.0.1` 标签**（提交由用户决定）；`docs/version/RELEASE-NOTES-v1.0.0.md`
  处于已删除状态（`D`），恢复还是提交由用户决定。

## 版本推进 1.0.0 → 1.0.1 与 GitHub 发布说明（2026-10-01，同日第八轮）

范围：**用户明确要求**推进版本号（"更新版本到 v1.0.1"），并把同日各轮已落地的改动整理成 GitHub Release 正文。
除版本落点与文档外**未改任何运行时代码**；IPC 命令、事件、路由、错误码、`status.json` 既有字段全部未变。
改动文件：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`AGENTS.md`（两行基准表）、
`README.md`、`docs/validation.md`、`docs/version/{README.md,RELEASE-v1.0.md,RELEASE-NOTES-v1.0.1.md}`、
`docs/wiki/{版本与发布,Home,快速开始,已知限制与未验证项,_Footer}.md`。

### 版本号落点（`npm run version:set -- 1.0.1` 实跑输出）

| 落点 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.0.1** | 版本单一来源（脚本改写） |
| `src-tauri/tauri.conf.json` → `version` | **1.0.1** | 安装包名随之为 `WB Bridge_1.0.1_*` |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.1** | 壳工程同步落点 |
| `AGENTS.md` 基准表两行 | **1.0.1** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.0.1` | `vite.config.js` 构建期注入，已在产物 `dist/assets/index-*.js` 中回读到 `v1.0.1` |
| `src-tauri/core/Cargo.toml` | `0.1.0`（**未改**） | crate 内部版本，有意与产品版本解耦 |
| `status.json` 内置 `version` | `0.2.0`（**未改**） | 历史沿革值；`schemaVersion` 仍为 `1`（本版仅加法式新增顶层 `usage`） |

`npm run version:check` → **全部 5 处版本号一致（1.0.1）**。

### 推进理由（对齐 `AGENTS.md` 版本号规则）

规则要求「新增与现有问题**不同类、不同根因**的功能/修复 **且** 用户明确允许」两者同时成立才可末位 +1：
本版含供应链来源校验、鉴权兜底、壳生命周期、并发/panic、文件权限传播 5 类不同根因的修复，以及侧栏 4 个只读视图 + `usage` 统计的新增功能；
用户本轮明确要求推进 → 允许 `1.0.0 → 1.0.1`。**注意**：v1.0.0 分节从未发布（无安装包、未实机冒烟），其全部内容并入本版。

### 发布说明落点

- 新增 `docs/version/RELEASE-NOTES-v1.0.1.md`：**可直接复制为 GitHub Release body**，含发布状态如实标注（GUI/CI/安装包/自动更新均未验证）、安全修复逐条根因、并发与生命周期修复、面板新入口、基线对照表、六平台安装包命名、已知限制、升级与兼容说明。
- `docs/version/RELEASE-v1.0.md` 顶部新增 **v1.0.1 分节**（最新在前），`当前最新版本` 改为 v1.0.1；v1.0.0 分节保留为历史口径。
- `docs/version/README.md`：索引新增 GitHub Release 正文文件的命名规则与条目，落点表同步为 1.0.1。

### 本轮实测（数字来自真实执行）

- 核心 `cargo test` → **207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9）；`cargo clippy --all-targets` → **0 warning**。
- 壳 `cargo test --lib` → **5 通过 / 0 失败**；`cargo clippy --no-deps --all-targets` → **0 warning**。
- `npx eslint .` → **0 problems**；`npm run vite:build` → 成功（CSS 25.85 kB / JS 93.99 kB，产物含 `v1.0.1`）；`node -v` → v24.19.0。
- 影响面提示：**版本号是打包元数据的一部分**，`tauri.conf.json` 变更后需重新构建安装包才生效；面板「关于与更新」显示 `v1.0.1`，而 `status.json` 的 `version` 仍为 `0.2.0`（历史沿革值，两者不同源，界面上会同时出现）。

### 未验证与遗留（如实标注）

- ❌ **本版仍未发布**：改动全部在工作区，**尚未 `git commit`、尚未打 `v1.0.1` 标签**（提交与推送由用户决定）。`release.yml` 由 tag 触发且从仓库读取 `package.json`，因此**必须先提交版本号改动再打标签**，否则 CI 仍会构建出 1.0.0 产物。
- ❌ GUI 未实机启动、`release.yml` 未在 CI 跑通、无安装包产出、自动更新未接线 —— 发布说明里逐条标注为未验证。
- ⚠️ 既有 tag `v1.0.0`（`f046208`）**不是** `dev` HEAD 的祖先；`git diff v1.0.0 --name-status` 为 39 文件 / +3554 / −231，发布说明即以此范围整理。
- ⚠️ `docs/version/RELEASE-NOTES-v1.0.0.md` 当前在工作区处于**已删除**状态（`git status` 显示 ` D`，该文件存在于 HEAD 而不在 v1.0.0 标签内）。本轮**未替用户决定**是恢复还是提交该删除；发布说明正文已合并到 `RELEASE-NOTES-v1.0.1.md`。

## 安全与并发修复（2026-10-01，同日第七轮：审计后的逐项落地）

范围：审计出的 3 个 P0 与 4 个 P1 逐项修复。**对外契约未变**（路由、错误码、`status.json` 字段、IPC 命令名与事件名全部保持原样），
版本号仍为 **1.0.0**（本轮属同一批安全修复的多轮往返，按 `AGENTS.md` 版本号规则不推进）。
改动文件：`src-tauri/core/src/runtime.rs`、`src-tauri/core/src/orchestration.rs`、`src-tauri/core/src/server.rs`、
`src-tauri/core/tests/red_lines.rs`、`src-tauri/src/lib.rs`、`AGENTS.md`、`README.md`、`docs/contract.md`、`docs/wiki/*`、
`docs/version/*`；`.github/workflows/release.yml` **只改了顶部一行注释里的测试数量**（179/7 → 187/9），jobs/steps 与 Node 版本均未动。

### 改了什么

- **P0 供应链（`runtime.rs::valid_metadata`）**：原实现只校验 tarball 的**路径前缀** `/包名/-/` 与「registry 字符串自身的 origin」，
  从未把 `metadata.tarball` 的 origin 与白名单比对，而下载循环恰恰先取 `metadata.tarball`。被篡改的元数据因此可以把 tarball
  指向任意主机，`sha512` 形同虚设（哈希与 tarball 同源）。现在按 `Url::origin()` 逐字节比对白名单 registry，并补三条对抗用例
  （路径形状合规但站外的 URL、`registry.npmjs.org@evil.example` 的 userinfo 伪装、镜像源 URL 必须仍通过）。
- **P0 鉴权（`orchestration.rs::resolve_api_key` + `server.rs::authorized`）**：先前存在但内容为空白的 `api-key` 文件会被 `trim()` 后沿用，
  期望头退化成 `"Bearer "`，任何本机进程都能匹配。现在空白内容一律重新生成 32 字节 hex，覆盖写入时**强制** `0600`
  （`OpenOptions::mode` 只在新建时生效，故额外 `set_permissions`，失败即上报、不静默降级），并在 `opencode.log` 记录「已重新生成」但不写值；
  `authorized` 另加「空密钥一律拒绝」的兜底。
- **P0 生命周期（`src-tauri/src/lib.rs`）**：`setup` 里的 `core-failed` 必然早于面板注册监听（事件丢失），随后 `watch_status` 无条件把磁盘上
  上一轮的 `status.json` 当 `core-status` 推出去 ⇒ 面板显示「就绪」却没有核心在跑、重试入口消失。现在壳把启动失败原因记进
  `startup_error`，`watch_status` 在故障态**不再推送残影状态**，改为按原因去重并每 ~4s 重播 `core-failed`（晚挂载的监听器仍能收到），
  恢复运行时清空去重标记**并作废内容缓存**（重启后的 status.json 可能与故障前逐字节相同，不重置就永远不再推送）。
- **P1 阻塞 IPC**：`core_action` / `restart_core` 改为 async 命令，阻塞段（最长 15s 的 key 轮询 + 60s 回环 HTTP + 最坏十余秒的停止等待）
  下沉 `tauri::async_runtime::spawn_blocking`，不再占住主线程；前端 `invoke` 契约不变。
- **P1 panic / 并发**：`run_probe_batch` 的三处 `pending.remove(0)` 改为按 id 移除的 `drop_pending`（`pending` 只收录带字符串 id 的模型，
  按位置弹出会在提前耗尽时 panic，`probing` 永久为真、后续 refresh/import 全被「请等待检测完成」挡死）；`start_probes` 的占用声明由
  `load` + `store` 改为 `swap`（两个并发 `/admin/probe` 可双双通过检查、各起一批探测）；`refresh()` 把「代理解析 + 清空为 reading」
  移到链位登记之后（登记前的 await 让两个并发刷新都判定「无人刷新」，隔离运行时被重启两次、模型结果互相覆盖）。
- **P1 静默 chmod**：`runtime.rs` 3 处（安装后的二进制 0755、复制候选的临时文件 0755、隔离 XDG 目录 0700）与 `orchestration.rs`
  数据目录 0700 一处，`let _ = set_permissions(...)` 改为传播错误——隔离目录里放着 `OPENCODE_SERVER_PASSWORD` 与配置副本，
  chmod 失败必须终止启动而不是留下一个组/其他用户可读的「隔离」目录。
- **测试与文档**：红线守卫 **7 → 9**（`runtime_downloads_never_leave_the_registry_allow_list` 断言「一次都没向白名单外发过请求」，
  `an_empty_api_key_authorizes_nothing` 逐路由断言空 key 下 `"Bearer "` 也拿不到放行）；新增 tar 单成员解包、空白密钥再生、
  `drop_pending` 三个核心单测；新增壳侧 `shell_action_routes_match_the_core_contract`（`ADMIN_ROUTES` ↔ 核心 `ACTION_ROUTES` 逐项一致），
  并把 `tests/red_lines.rs::routes()` 里手写的 5 条 `POST /admin/*` 改为**直接从 `server::ACTION_ROUTES` 派生**（此前管理路由共有
  三份手写副本：`red_lines.rs`、`server.rs`、`src-tauri/src/lib.rs`；现在只剩核心那张表 + 一处壳侧映射，且由契约测试互相对齐，
  新增动作不会再被红线清单漏掉）；据此把 `lib.rs` 里「三处集合是否一致由契约测试断言」这句**并不存在的保证**改为事实描述，
  同步 `server.rs` 模块注释中指向已归档 `core/src/server.js` 的过期表述，并同步 `docs/contract.md`、
  `docs/wiki/架构设计.md`、`docs/wiki/已知限制与未验证项.md`；基线数字在 `AGENTS.md`（4 处）、`README.md`、`docs/wiki/{Home,版本与发布,开发指南,已知限制与未验证项,架构设计}` 统一为 207。
- **文档口径纠偏（除数字外，均已实读代码核对）**：`docs/version/RELEASE-v1.0.md` 与 `RELEASE-NOTES-v1.0.0.md` 仍把侧栏写成「只有模型与服务可用、4 个入口是「规划中」禁用占位」，
  与同日第四轮之后的真实面板不符（`grep -rn "规划中" src/` 已无命中，`src/views/` 含 `LogsView/UsageView/IntegrationView/AboutView`）——已改为「5 个入口均已实现、后 4 个只读」，
  并把 `views/` 文件清单与 `bridge.js` 导出（补 `readLog()` / `dataDir()`）补齐；`docs/version/README.md` 里同一句遗留说明同步改写。保留未改的一条是**仍然成立**的：
  浅色主题下 `App.vue:262` 副标题、`App.vue:264` 页脚、`ModelRow.vue:110` 耗时行仍用 `--muted`，对比度低于 4.5:1。

### 本轮实测（数字来自真实执行）

- `cargo test`（`src-tauri/core/`）→ **207 通过 / 0 失败**：lib **187** + `js_parity` **11** + `red_lines` **9**。
- `cargo clippy --all-targets`（`src-tauri/core/`）→ **0 warning**。
- `cargo test --lib`（`src-tauri/`）→ **5 通过 / 0 失败**；`cargo clippy --no-deps --all-targets`（`src-tauri/`）→ **0 warning**。
- `npx eslint .` → 0 problems；`npm run version:check` → 5 处一致（1.0.0）；`node -v` → v24.19.0。
- **反向验证**（临时改动、已回滚并复测全绿）：关掉 tarball origin 比对后，新红线确实失败且下载器第一个请求就是
  `https://evil.example.com/...` 的 tarball；删掉 `if key.is_empty()` 后 `an_empty_api_key_authorizes_nothing` 在 `GET /health` +
  `Authorization: Bearer ` 上失败。两处洞都是活的，不是理论风险。

### 未验证与遗留（如实标注，不得当作已完成）

- ❌ **GUI 未实测**：`npm run tauri:dev` 与打包 .app 均未启动过。上述壳侧改动（async 命令不再冻结面板、`core-failed` 重播、
  恢复时重推状态）的证据只有「编译通过 + 核心独立运行行为 + `cargo test --lib`」，没有任何实机交互验证。
- ❌ `release.yml` 仍未在 CI 跑通；其 `node-version: 22` 与根 `package.json` 的 `engines.node >= 24` **仍不一致**（本轮未动，属配置改动）。
- ⚠️ 无确定性单测的项：`refresh()` 并发去重（依赖 await 交错，难以稳定构造）、`service.pid` 单实例、探测路径禁用转写、
  转写排除刚失败模型、格式类失败不撤发布、仅回环绑定、目录 0700 的失败分支（正常文件系统上 chmod 不会失败，构造不出稳定用例）。
- ⚠️ `AGENTS.md` 的两处绝对路径在本机**不存在**，且本轮未改：`🔴 文件操作根目录 = /Users/wbtrex/website/localServer/node/trexwb/git/wbBridge`
  （实际工作副本是 `/Users/wbtrex/AI助手/node/trexwb/wbBridge`），以及仓库外 JS 归档
  `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`（`js_parity` 因此目前无法用 `WB_PARITY_RECORD=1` 重录）。
  这两条是用户写的规则，是否改写由用户决定。

## 侧栏 4 入口实现（2026-10-01，同日第四轮）

范围：核心 `usage` 计数 + 壳只读命令 `read_log` + 面板 4 个新视图；既有管理动作、路由、红线均未变。
改动文件：`src-tauri/core/src/orchestration.rs`、`src-tauri/src/lib.rs`、`src/App.vue`、`src/views/SideBar.vue`、`src/views/LogsView.vue`、`src/views/UsageView.vue`、`src/views/IntegrationView.vue`、`src/views/AboutView.vue`、`src/core/bridge.js`、`AGENTS.md`、`README.md`。

### 改了什么

- **核心**：`status.json` 新增顶层 `usage`（`since` + `total{requests,ok,failed}` + 逐模型 `lastMs/avgMs`）；只累计 `source == "request"` 的真实客户端请求，**探测不计**，**客户端取消（连接断开）整条不计入**（`requests` 也不 +1）；启动时从上一份 `status.json` 读回，与 `modelResults` 同法跨重启延续。
- **壳**：新增**只读**命令 `read_log`（无参数，读数据目录 `opencode.log` 的尾部，尾部截断 256KB / 最多 1200 行，返回 `{ text, truncated, bytes }`；不读 `.previous`、不写任何文件、不接受路径入参），并登记进 `generate_handler`（合计五命令）。
- **面板**：侧栏 **5 个入口全部可点**（去掉「规划中」禁用占位），新增运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新 4 个**只读**视图；面板内唯一写动作仍是既有的 `import`。「关于与更新」**未接入自动更新**。
- **版本口径**：`STATUS_SCHEMA_VERSION` 两侧保持 `1`（兼容加法式字段不递增，见 `AGENTS.md` 与两处常量注释）。

### 已验证（本轮实读执行）

- `cargo test`（`src-tauri/core/`）→ **202 通过 / 0 失败**：lib 单测 **184** + `js_parity` **11** + `red_lines` **7**。
- `cargo clippy --all-targets`（核心）→ **0 warning**。
- `npx eslint .` → **0 problems**；`npm run vite:build` → 成功（`dist/` 已更新）。
- ⚠️ 新增视图与 `read_log` / `usage` 仍**未在 Tauri GUI 中实机点击验证**（同「已知限制」限制 5）。

## 面板 UI 风格优化与交互反馈补全（2026-10-01，同日第六轮）

范围：只动面板样式与交互反馈（`src/styles/`、`src/App.vue`、`src/views/*`），**不改任何 IPC 契约、命令表与核心逻辑**。
改动文件：`src/styles/variables.css`、`src/styles/base.css`、`src/App.vue`、`src/views/` 下 `SideBar.vue`、`FeedbackBar.vue`、`ModelList.vue`、`ModelDetails.vue`、`ServiceStatus.vue`、`MetricsBar.vue`、`LogsView.vue`、`UsageView.vue`、`IntegrationView.vue`、`AboutView.vue`，以及 `src/components/ModelRow.vue`、`AGENTS.md`、本文件；**未触碰 `src-tauri/`（Rust 侧零改动）**。

### 改了什么

- **token 层补齐交互与动效**：`variables.css` 亮/暗两套各补 `--dur-*`、`--ease-*`、`--focus-ring`/`--focus-offset`、`--on-primary`、`--surface-raised`、`--shadow-s/m`、`--nav-hover-bg`/`--nav-active-bg`/`--nav-active-ring`；组件内仍不写死颜色与时长。
- **全局层（`base.css`）**：`button` 统一 hover（只改背景/边框，不做位移，避免密集列表抖动）、`active` 按压、`focus-visible` 焦点环、`disabled` 降透明度；新增 `.ghost` 静默按钮、`.spinner.sm`、`.skeleton`、`.busy-bar`（不确定进度条）、滚动条瘦身、`.visually-hidden`。`prefers-reduced-motion` 下位移/淡入一律取消，**但 spinner / 骨架 / 进度条保留循环**——它们是「正在加载」的唯一信息载体，停掉会被误读为卡死。
- **侧栏美化**：字符图标（▦▤◔⇄ⓘ）全部换成内联 SVG（不产生外部请求，符合 `CSP default-src 'self'`）；当前视图指示轨由 0 展开成 18px 主色竖条，分组标题加短竖标，logo 加描边与投影，代理开关补 hover 态并在禁用时抑制可点暗示。
- **等待 / 加载反馈**：`FeedbackBar` 增加 pending 态（spinner + 中性底色，**不提供关闭按钮**，防误判操作已结束）；`LogsView` 首次读取给错落骨架屏，错误态改为告警条 + 重试按钮（重试期间按钮转 spinner）；`ModelList` 空态给虚线框 + spinner + `role=status`；`ServiceStatus` 重试期间按钮转 spinner，出错时整条状态条随灯一起转橙。
- **真实进度，不编造**：`probe` 为长任务，按钮上显示「已完成/总数」（`App.vue::probeProgress`，由壳推送的 `probe.pending` 队列长度与当前模型数推算，只反映这一帧快照，缺任一项即不显示），另在状态条上方补一条不确定进度条；**其余动作前端拿不到真实进度，一律只给文字、不写数字**（沿用既有「不编造进度」纪律）。
- **弹层与视图动效**：详情仍为右侧常驻分栏（无遮罩、不 `fixed`/`absolute`、任何宽度不降级），入场为从右轻推淡入；主区视图切换淡入上浮；反馈条自上淡入。
- **数据语义表达**：`MetricsBar` / `UsageView` 指标条改为「一格一数 + 细竖线分组」，可用/成功走绿、待检测与无值走灰、失败与 0% 成功率走橙（颜色仅作辅助，文案独立表意）；用量表表头吸顶、行 hover 高亮、空态虚线框；`IntegrationView` / `AboutView` 键值区补分隔线，`核心阶段` 按状态着色，不可导入时按钮 title 说明原因（可视原因仍在「核心阶段」一栏）。

### 已验证（本轮实跑）

- `npx eslint src` → **0 problems**（exit 0）；`npm run vite:build` → **成功**（39 模块，`dist/` 更新，css 25.85 kB）。
- `cargo test`（`src-tauri/core/`）→ **205 通过 / 0 失败**（lib 单测 **185** + `js_parity` **11** + `red_lines` **9**）。
- 本轮为纯前端改动、未触碰 Rust 代码，**未跑** `cargo clippy` 与桌面打包（`npm run build`）。

### 未验证 / 已知遗留（如实标注）

- **仍未在 Tauri GUI 实机点击验证**：改动的动效、进度条、骨架屏、焦点环只经 eslint + vite 构建 + 源码复核，未在真实 WKWebView 中点击复验（「已知限制」限制 5 本轮不解除）。
- 焦点环与键盘可达性沿用第三轮结论（侧栏导航与模型行可 Tab 到达）；本轮新增的 `aria-busy` 与状态条 `role=status` 未做屏幕阅读器实测。
- 浅色主题下 3 处次要文字对比度低于 4.5:1 的既有问题（见第三轮条目）本轮**未处理**。
- **文档基线数字漂移（本轮发现，未修正）**：本轮实测核心测试为 **205 通过 / 0 失败**（lib 185 + `js_parity` 11 + `red_lines` 9），而 `AGENTS.md`（命令表 / 迁移说明 / 测试规范 / 一览表 共 4 处）与 `README.md`（`npm test` 说明）仍写 **202**（lib 184 + 11 + 7）。差异来自本轮之前某次新增的 lib 单测与 2 条红线断言；本轮未触碰 Rust 代码，故**未擅自改动这些口径**，是否同步由用户决定。

## 契约与文档同步（2026-10-01，同日第五轮）

范围：仅文档与代码注释，**无任何行为改动**（不改常量值、不改命令表）。

- **R1 `docs/contract.md`**：壳命令由「两个」更正为三个，补 `read_log` 行（无参数、只读日志尾部、返回 `{ text, truncated, bytes }`、不吃路径入参、不写文件），并注明 `core-status` 轻量快照**仍含顶层 `usage`**、新增只读命令须同步 `generate_handler`（`capabilities/default.json` 无需改动）。
- **R2 文档口径统一为「5 入口均已实现」**：使用指南（新增 5 视图对照表、主区标题改称「模型与服务视图主区」、动作表补 `read_log`）、开发指南（测试基线 202 / lib 184、侧栏规范改写并补只读边界）、FAQ（「规划中入口」条目改写为入口说明 + 新增「运行日志为空」条目）、已知限制与未验证项（基线 202、限制 5 改为「4 个新视图未经 GUI 实机验证」、红线条目改写、未验证项 7 补证）、架构设计（IPC 表补 `read_log`、`core-status` 说明补 `usage`、`status.json` 行补 `usage` 与 `schemaVersion` 口径、测试数 202）、版本与发布 / Home / research 的基线数字同步为 202。
- **R3 `schemaVersion` 升级口径**：`AGENTS.md` 写明「向后兼容的加法式顶层字段不递增，本次保持 1；删除 / 改名或改变既有字段语义才两侧同步 +1」，并在 `src-tauri/src/lib.rs` 与 `src-tauri/core/src/orchestration.rs` 的版本常量注释中写入同一口径（仅注释）。
- **本轮复验**：`cargo test`（`src-tauri/core/`）→ **202 通过 / 0 失败**；`npx eslint .` → 0 problems；`npm run vite:build` → 成功。

## 面板布局改造（2026-10-01，同日第三轮）

范围：只动面板（`src/`）与壳的窗口配置（`src-tauri/tauri.conf.json`），不涉及核心逻辑、路由、错误码与 IPC 契约。
改动文件：`src/App.vue`、`src/views/SideBar.vue`、`src/views/ModelDetails.vue`、`src/styles/variables.css`、`src-tauri/tauri.conf.json`。

### 改了什么

- **详情面板从「点开模型行就地展开」改为右侧常驻分栏**：`.content.is-split` 变为
  `grid-template-columns: minmax(0, 1fr) var(--details-w)`（`--details-w: clamp(300px, 45%, 360px)`），
  无遮罩层、不 `position: fixed/absolute`、不覆盖列表；收起路径有三条且等价：`Esc`（`App.vue::onKeydown`）、
  详情头部「收起详情」按钮（`aria-label="收起详情"`）、窗口失焦（`onDismiss`）。
- **删除 `<900px` 的上下堆叠降级**：`src/App.vue` 不再有任何宽度媒体查询，`.content.is-split` 恒为两栏，
  注释写明「任何窗口宽度都不降级为上下堆叠」；全仓 `src/` 现存媒体查询只剩 `prefers-color-scheme`
  （`variables.css`）与 `prefers-reduced-motion`（`base.css`）。
- **默认窗口 980×680 → 1120×720**，最小尺寸保持 `860 × 560`（`src-tauri/tauri.conf.json` → `app.windows[0]`）。
- **侧栏宽 `--sidebar-w` 224px → 208px**；侧栏改为**分组导航**（模型 / 运行 / 集成 / 其他 + 运行设置），
  当前只有「模型与服务」为激活视图，运行日志、用量与额度、WorkBuddy 集成、关于与更新 4 个入口为
  `state: 'planned'` 禁用态 + 「规划中」标签（不做可点击却无响应的假入口）。**该状态已于同日第四轮变更——5 入口全部实现，见上方最新条目。**
- **新增 `--muted-strong` token**（亮/暗色成对），用于副标题、页脚说明、耗时行等小字号说明文字；
  普通次要文字仍用 `--muted`。

### 已验证（本轮实跑）

- `npx eslint .` → 0 problems；`npm run vite:build` → exit 0（面板产物 `dist/` 更新）。
- 面板在浏览器引擎内经 **CDP** 实测（**非 Tauri GUI 实机**）：
  - 默认 1120×720 与最小 860×560 两档下，主区与其内列表均无横向溢出，详情始终与列表并排；
  - 收起详情三条路径（`Esc` / 「收起详情」按钮 / 失焦）均生效，收起后列表占满主区宽度；
  - 键盘可达：侧栏导航与模型行可 Tab 到达，禁用项不进入 Tab 序；
  - 深色主题下 `--muted-strong` / `--muted` 字色对比度均达标。
- 改造轮曾出现并当场修复两处回归：`App.vue` 漏导入 `onUnmounted` 导致面板白屏；侧栏禁用态文字被压暗。
- 改造前的备份：会话中间产物目录 `temp/backup-20261001-ui/`（patch + 文件快照），另有安全网提交 `32b2841`。

### 未验证 / 已知遗留（如实标注）

- **Tauri GUI 仍未实机启动**：以上实测均在浏览器引擎内完成，托盘与真实 WebView 渲染未复验，本仓库的
  GUI 验证边界不变（见 `AGENTS.md`「当前状态与验证边界」）。
- 浅色主题下仍有 3 处次要文字（副标题、页脚说明、耗时行）使用 `--muted`，对比度约 4.01 / 4.01 / 3.73，
  **低于 4.5:1**；此为改造前既有问题，本轮未处理。
- 本轮为纯面板/窗口配置改动，**未跑** `cargo test` 与 `cargo clippy`（未触碰 Rust 代码），也未重跑
  `npm run build` 桌面打包；核心测试基线仍为上一轮的 197 通过 / 0 失败。
- 后续「文档与代码一致性核对」轮次只同步文档描述，未重跑任何测试与构建。

## 全量代码审查与壳↔面板接缝修复（2026-10-01，同日第二轮）

### 已验证（本轮实跑数字）

- `cargo test`（`src-tauri/core/`）→ **197 通过 / 0 失败**：lib 单测 **179** + `js_parity` **11** + `red_lines` **7**。
  新增的 2 个 lib 单测在 `src-tauri/core/src/orchestration.rs::tests`：
  `concurrent_chain_registration_never_forks`（8 线程并发登记串行链，断言只有一个无前驱、
  predecessor 不重复——即写链不得分叉）与 `reused_slot_clone_shares_the_recorded_outcome`
  （复用进行中 refresh 时，后到者持有的 Slot 克隆必须读得到首个调用者的失败）。
- `cargo clippy --all-targets`（核心）→ **0 warning**；`cargo clippy --no-deps`（壳）→ **0 warning**。
- `npx eslint .` → 0 problems；`npm run version:check` → 5 处一致（**1.0.0**，本轮未推进）。
- `npm run build` → **exit 0，无构建错误**：vite 31 模块（`dist/` 78.76 kB js + 8.40 kB css）→
  cargo release → `bundle/macos/WB Bridge.app`（6.42 MiB）+ `bundle/dmg/WB Bridge_1.0.0_aarch64.dmg`
  （3.34 MiB），ad-hoc 签名、跳过公证（无 APPLE_ID/…）。

### 本轮修的缺陷（均为行为修复，逐条读过源码与归档 JS）

- **面板丢状态**：壳把 `activity` / `modelResults` 拆成 `core-activity` 发出，但 `src/core/bridge.js`
  从未监听它，`core-status` 收到的又是剥掉这两字段的轻量快照 → 逐模型明细与活动文案恒为空。
  现在 `bridge.js` 缓存最近一次完整状态并把两路合并后下发，`onState` 订阅时直接回放缓存
  （原先那段 `getCurrent…emit('wb-bridge/replay-request')` 调的是 Tauri 2 里不存在的 API、且全仓
  无监听者，已删）。事件名与 payload 形状未改。
- **系统代理开关必然失败**：面板发裸布尔，核心按 `docs/contract.md` 读 `{enabled}` →
  「代理开关必须是布尔值」。改为 `run('system-proxy', { enabled: $event })`（面板侧对齐契约，
  核心路由与字段名不动）。
- **refresh 的 spinner 永不显示**：`busyAction` 是布尔却被与字符串 `'refresh'` 比较。改为持有动作名，
  并把传给 Boolean prop 的 `:disabled` / `:busy` 统一 `!!busyAction`，避免 Vue 的 prop 类型告警。
- **托盘吞错**：`src-tauri/src/lib.rs` 两处 `let _ = admin_call(...)` 改为失败时 `eprintln!` 记录原因
  （GUI 打包后 stderr 不进终端，可观测性有限——面板侧可见的托盘错误提示需要新的展示位，未做）。
- **refresh 复用把失败说成成功**：后到者只 `wait_for(done)` 就返回 `Ok({count})`；JS 里大家 await 同一个
  promise、rejection 会传播给每个等待者。`Slot` 增设 `outcome`（Arc 共享，克隆可见），任务先写结果
  再置 done，复用路径原样返回该结果。
- **串行写链分叉**：`persist_status` / `chain_sync` 原本「读 predecessor」与「写回链头」分两次加锁，
  两线程可读到同一个 predecessor 并各自只等它 → 两条分支并发执行、后登记者覆盖先登记者的 Slot、
  `drain()` 等不到被覆盖的那条写。抽出 `reserve_chain_slot()` 在单次持锁内完成读+登记，
  status/sync/refresh 三条链统一走它。
- **文档口径纠正**：探测超时原写「整批共享 60s」，实际（与归档 JS 一致）是**每模型一份 deadline、
  重试共用**；已改 `AGENTS.md`（3 处）、`probe.rs` 模块注释与常量文档、`red_lines.rs` 断言文案。
  壳注释里指向已归档 `src/core/src/server.js` 的路径、`server.rs` 的「方案B 阶段二/三」过期术语同步更正。

### 未验证 / 未做（如实标注）

- **GUI 仍未实机启动**：上面的接缝修复全部靠源码与契约推导 + 编译/测试/打包通过，托盘、面板交互、
  代理开关的实际点击链路未经真实运行验证。
- **CI 仍未实跑**：`release.yml` 只是静态结构正确；本轮拆掉了它声称但不存在的自动更新产物
  （`**/*.sig` glob、`TAURI_SIGNING_*` 环境变量、`.tar.gz`），因为 `tauri.conf.json` 既无
  `bundle.createUpdaterArtifacts` 也无 `plugins.updater` 依赖。**自动更新功能本身仍未接线**。
- 仍存而未修的已知项：`pick_port()` 先 bind 再 drop 的端口抢占窗口；`[profile.release] panic = "abort"`
  下任何 panic 直接带走整个应用（无日志）；`.vue` 不在 `npm run lint` 覆盖范围（`eslint.config.js` 只匹配
  `**/*.{js,mjs}`）；CI 的 `node-version: 22` 与 `engines.node >= 24` 不一致。

---

## Node → Rust 核心迁移复验（2026-10-01）

### 已验证

- **测试全绿**：`src-tauri/core/` 下 `cargo test` → **195 通过 / 0 失败**（该轮基线；同日第二轮修复后为
  **197**，见上方条目），构成：lib 单测 **177** +
  `tests/js_parity.rs` **11** + `tests/red_lines.rs` **7**（根目录 `npm test` 转发到同一 `cargo test` 命令）。
  - `js_parity.rs` 不再需要 Node：期望值是迁移前用真实 JS 模块录制、冻结在 `src-tauri/core/tests/fixtures/*.json`
    的真相快照（**271 例 / 11 个 fixture 模块**：atomic 10、handoff 34、json 11、model_status 22、platform 10、
    protocol 59、reasoning 10、repair 41、sync 17、system_proxy 38、workbuddy_config 19），对拍只比较 Rust 实现与快照；重新录制需把归档的 `core/` 与
    `tests/js/` 放回原位再跑 `WB_PARITY_RECORD=1 cargo test --test js_parity`。
  - `red_lines.rs` 覆盖：全路由（含 `/health`）必须 Bearer 鉴权、任意非空 `Origin` → 403、
    体积/并发上限不放松、原生工具只允许 ask/deny、隔离配置 autoupdate=false 且两个 agent 存在、
    子进程 env 只透传白名单、同步只动 `OWNER = "buddy-bridge-v1"` 名下的条目。
- **Lint 干净**：`src-tauri/core/` 下 `cargo clippy --all-targets` → 0 warning；`src-tauri/` 下
  `cargo clippy --no-deps` → 0 warning（本次复验实测）。
- **结构迁移落位**：`src-tauri/tauri.conf.json` 的 `bundle.externalBin` 为 `[]`，`src-tauri/binaries/`
  与 Node `core/`（旧 `src/core/`）目录均已从仓库移除；核心以 path 依赖 crate `wbbridge-core`
  （lib `wbbridge_core`）静态链接进壳，编排入口 `src-tauri/core/src/orchestration.rs::run(StartOptions)`。
  壳侧 `src-tauri/src/lib.rs` 负责：专用 tokio 运行时装配核心、`pick_port()`（默认 41980，占用时回退
  系统分配端口）、`app_data_dir()` 注入数据目录、`admin_call()` 走回环 HTTP 驱动 `/admin/*`、轮询
  `status.json` 并 emit `core-status` / `core-failed`、`orchestration::set_exit_hook` 保证核心不能反向
  终止壳；面板 `src/` 为 Vue 3 + Vite（`frontendDist: ../dist`，dev 端口 41990）。
- **版本一致性**：根 `package.json` 为版本单一来源（**1.0.0**）；`node scripts/check-version.mjs` 校验
  5 处落点（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` / `AGENTS.md` 两行）全部
  通过。`src-tauri/core/Cargo.toml` 的内部 crate 版本 **0.1.0** 与产品版本**有意解耦**，不参与同步。
- **独立核心冒烟运行**（一次性临时数据目录，非 GUI）：运行时按需下载成功 → 隔离 OpenCode 起来并通过
  `/agent` 校验（`buddy-bridge` / `buddy-chat` 存在）→ 发现 8 个免费模型 → 逐模型探测 → 干净关停。
  数据目录约定（`api-key` / `status.json` / `service.pid` / `opencode.log` / `runtime/` / 隔离 XDG 目录）
  由 `src-tauri/core/src/platform.rs`、`orchestration.rs` 与相应单测覆盖，本次冒烟未逐项复核其权限位。

### 未验证（如实标注，不得当作已验证）

- **迁移改动尚未提交**：`src-tauri/core/`（Rust 核心整体）、Vue 面板新文件（`src/App.vue` 等）、
  `docs/contract.md`、`vite.config.js`、`eslint.config.js`、`scripts/{bump,check}-version.mjs` 都还是未跟踪状态；
  旧 JS 核心（HEAD 里的 `src/core/*.js`）、`src/ui/`、`scripts/build-sidecar.mjs`、`docs/brand/` 的删除也仅在工作区生效
  （`src-tauri/binaries/` 本就未被跟踪，随工作区删除即消失）；提交与否由用户决定。
- **GUI 桌面应用从未实机启动过**：迁移后只做过编译 + 上述独立核心冒烟；托盘、面板交互、壳侧
  `restart_core` / 优雅退出链路均未经过真实运行验证。
- **迁移后未产出过安装包**：`.app` / `.dmg` / Windows / Linux 包待 `npm run build` 与 CI 实跑后复验。
- **重写后的 `.github/workflows/release.yml` 从未在 CI 运行**：仅本地做过 YAML 结构校验。
- **`cargo fmt --check` 全仓不干净**，未纳入门禁；当前风格门禁只有 clippy 0 warning。

### 环境遗留结论

- 2026-09-30 记录的「本机 DNS 把 `opencode.ai` 解析到证书 SAN 不含该域名的地址，导致模型探测
  `ERR_TLS_CERT_ALTNAME_INVALID`」仍是同一环境问题：Rust 核心保持 TLS 校验，不改宽松。

---

日期：2026-09-30（Node/sidecar 时代基线，本机 macOS，Apple Silicon）

## 目录重构后复验（15:08）

- 用户将 core/ui 移入 `src/` 并新增版本一致性脚本后，`npm run build`（sidecar + tauri build）与单独 `tauri build` 各复验一轮，均产出 .app + .dmg（25.02 MiB）。
- `npm run version:check`：5 处版本号一致（1.0.0）；根 `npm test`：97/97。
- 期间用户报告过一次 `bundle_dmg.sh` 失败，重构后未复现；判定为上一次中断构建残留的瞬时 hdiutil 问题。复发时的处置：确认无 `/Volumes/WB Bridge` 残挂载后删除 `src-tauri/target/release/bundle/dmg/` 重新构建。

## 自动化测试

- `core/` 测试套件：**97/97 通过**（`npm test`，node --test）。
  - 自上游参考实现（https://github.com/louchi1984-coder/ow-bridge）迁移的测试 95 项（协议校验、导入与退出清理、目录能力映射、图片转发、推理档位、系统代理解析、活动文案等），其中 2 项读取 UI 源码的契约测试已指向新 `views/` 路径。
  - 新增 2 项：`POST /admin/shutdown` 先响应后触发一次优雅退出（含鉴权拒绝）；`BUDDY_PARENT_PID` 看门狗在壳进程消失后自行优雅退出并落 `stopped` 终态。
- Rust 壳 `cargo check` 通过（macOS aarch64，tauri 2.12 依赖树）。

## macOS 本机构建

- `npx tauri build` 成功：
  - `src-tauri/target/release/bundle/macos/WB Bridge.app`（64.17 MiB）
  - `src-tauri/target/release/bundle/dmg/WB Bridge_1.0.0_aarch64.dmg`（25.02 MiB）
- ad-hoc 签名（identity "-"），跳过公证（无 Apple 凭据）。
- sidecar `wbbridge-core` 随 .app 打包并随包签名。

## 实机冒烟（`npx tauri dev`，debug 构建）

| 步骤 | 结果 |
|---|---|
| 壳启动并拉起 sidecar | ✅ 进程链：wbbridge → wbbridge-core → opencode serve |
| 数据目录落位 `~/Library/Application Support/app.wbbridge.desktop/` | ✅ api-key / settings / status.json / opencode.log / runtime |
| OpenCode 运行时按需下载 | ✅ 官方 npm 源下载 1.18.33（约 1 分钟） |
| /health 健康检查（Bearer api-key） | ✅ phase=ready，endpoint=127.0.0.1:41980/v1 |
| 免费模型发现 | ✅ 发现 8 个免费模型 |
| 模型探测 | ⚠️ 全部返回 TLS 证书主机名不匹配（ERR_TLS_CERT_ALTNAME_INVALID）——**本机网络环境问题，见下** |
| 强杀壳进程 → 看门狗 | ✅ 核心在 3 秒内自行优雅退出，status.json phase=stopped，service.pid 清理，无孤儿进程 |
| 配置清理 | ✅ 退出路径执行（本机无 WorkBuddy models.json，同步为 skipped，未产生误写） |

## 本机网络环境说明（冒烟中的模型探测失败）

- 系统解析器（dscacheutil）对 `opencode.ai` 持续返回 `141.193.154.70`，该地址的 TLS 证书 SAN 仅为 IP 自身，不含 `opencode.ai`，导致 Node fetch（保持 TLS 校验）报 `ERR_TLS_CERT_ALTNAME_INVALID`。
- 运营商 DNS 与路由器 DNS 直接查询均返回正确的 Cloudflare 段地址（172.65.90.20–23，证书 SAN 含 opencode.ai），且本机 `/etc/hosts` 无相关条目；本机装有系统管理描述文件（ManagedSettings profile）。
- 结论：为该 Mac 的系统级 DNS 处理（过滤器/描述文件）所致的**环境问题**，与 WB Bridge 及上游参考实现的代码无关；同一环境下上游行为一致。TLS 校验失败而拒绝连接恰说明安全行为正常。换用正常 DNS 的网络（或在无过滤器的环境）即可通过探测。

## 交付边界（如实区分）

- 已验证：macOS ARM64 包的构建、启动、模型发现、生命周期与退出清理；全部核心测试。
- 未验证（本机无法执行）：Windows x64/ARM64 安装包、Linux x64/ARM64 AppImage/deb 的实机运行——由 GitHub Actions 构建产出后需在实际系统上冒烟。
- 未处理：Apple 公证与 Windows 发布者签名（与原版一致，发布说明中已注明放行方式）。
