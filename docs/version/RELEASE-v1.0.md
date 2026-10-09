# 版本发布日志 · v1.0

> 本文件按主版本组织：v1.0.x 的全部迭代日志集中于此（最新在前）。
> 命名规则：`RELEASE-v{主版本}.md`；次版本迭代追加到文件顶部新分节。
> 版本纪律以根目录 `AGENTS.md`「当前基准版本」章节为准，本文件不另立规则。
> 整理规则：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本保留编号与合并指向，不再重复正文。
> 当前最新版本：**v1.0.5**。

---

## v1.0.5

> **状态**:。五处落点已一致为 `1.0.5`（手动改写 `package.json` / `tauri.conf.json` / `Cargo.toml` / `package-lock.json` + `AGENTS.md` 两行），但**尚未构建任何安装包、尚未打标签**，且版本落点提交仍未执行——按铁律先提交、再在那一个提交上打 `v1.0.5`。
> **日期**: 2026-10-09
> **上一版本**: v1.0.4（五处落点已一致、尚未打标签）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.0.5.md`](RELEASE-NOTES-v1.0.5.md)
> **本版主题**: 探测回归修复 + 单模型重新检测——升级后全部模型不可用的根因修复，并新增面板单模型重新检测功能
> **版本推进理由**: 与 v1.0.4 **不同类、不同根因**——v1.0.3 引入的 `probe_meta()` 让 `chat_only_attempt` 带上 `probe: true`，关闭了 `backend.rs` 两处转写闸门，导致升级后全部模型探测不可用；本版回退该标记恢复转写兜底。同时修复 `start_probes_admin` 二次解包 bug，并新增面板单模型重新检测按钮。**维护者明确要求推进版本号**，末位 +1。

### 一、版本号落点（手动改写 + `npm run version:check` 待验）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.0.5** | 版本单一来源 |
| `src-tauri/tauri.conf.json` → `version` | **1.0.5** | 打包产物版本与更新清单 `version` 来源 |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.5** | 壳工程同步落点（`src-tauri/Cargo.lock` 由 cargo 自动同步，须一并提交） |
| `AGENTS.md`「当前基准版本」两行 | **1.0.5** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.0.5` | `vite.config.js` 构建期注入（自带 `v` 前缀），非独立落点 |

### 二、本版内容

#### 1. 回退 `chat_only_attempt` 转写闸门（根因修复）

- **根因**：v1.0.3 全量代码复审时，将 `chat_only_attempt` 的 meta 从 `json!({})` 改为 `probe_meta()`（`{ probe: true }`）。这关闭了 `backend.rs:1575`（第二次重试时的 `translate` 转写）和 `backend.rs:1619`（无 tool_calls 且 handoff miss 时的 `rescue` 转写）两处闸门。主探测失败后降级到 chat-only 路径，升级前靠辅助模型转写兜底通过，升级后转写被禁用 → 全部模型不可用。
- **修复**：[orchestration.rs:1197](file:///Users/wbtrex/website/localServer/node/trexwb/git/wbBridge/src-tauri/core/src/orchestration.rs#L1197) 的 meta 回退为 `json!({})`，恢复转写闸门开放。主探测路径 `probe_single_model` 仍带 `probe: true` 不变。

#### 2. 修复 `start_probes_admin` 二次解包 bug

- **根因**：`POST /admin/probe` 服务端从请求体提取 `body.get("model")` 后传给 `start_probes_admin(model)`，但该函数又对字符串值调 `value.get("model")`，返回 `None` → 单模型探测请求退化为全量探测。
- **修复**：[orchestration.rs:1939](file:///Users/wbtrex/website/localServer/node/trexwb/git/wbBridge/src-tauri/core/src/orchestration.rs#L1939) 直接透传 `start_probes(model, false, false)`。

#### 3. 面板单模型重新检测按钮

- `ModelRow.vue`：外层 `<button>` 改为 `<div role="option" tabindex="0">`（避免嵌套 button 无效 HTML），不可用模型行内新增「重新检测」按钮，仅在 `result.ok === false` 且无检测进行时显示。
- `ModelList.vue`：透传 `probe-running` prop 和 `@reprobe` 事件。
- `App.vue`：新增 `reprobeModel(modelId)` 函数，调用 `run('probe', { model: modelId })` → `POST /admin/probe { model: id }`。

### 三、验证

- ✅ `vite:build` 编译通过
- ✅ `cargo build`（壳）编译通过
- 🔴 **未验证**：单模型重新检测的端到端 GUI 效果、回退后真实模型探测通过率须实机点一遍

---

## v1.0.4

> **状态**:。五处落点已一致为 `1.0.4`（`npm run version:check` 实测「全部 5 处版本号一致（1.0.4）」），但**尚未构建任何安装包、尚未打标签**，且版本落点提交仍未执行——按铁律先提交、再在那一个提交上打 `v1.0.4`。
> **日期**: 2026-10-08
> **上一版本**: v1.0.3（五处落点已一致、尚未打标签；v1.0.2 为最近一个已 Publish 的 Release，23 个资产）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.0.4.md`](RELEASE-NOTES-v1.0.4.md)
> **本版主题**: 模型发布多插件写入——自动检测 WorkBuddy 与 CodeBuddy，向所有检测到的目标分发写入；单目标失败不影响其他目标，未安装目标跳过并说明原因
> **版本推进理由**: 与 v1.0.3 **不同类、不同根因**——新增第二个写入目标（检测、定位、分发、聚合均为此前不存在的功能），且**维护者明确要求推进版本号**（`npm run version:set -- 1.0.4`），末位 +1。

### 一、版本号落点（`npm run version:set -- 1.0.4` + `npm run version:check` 实测）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.0.4** | 版本单一来源 |
| `src-tauri/tauri.conf.json` → `version` | **1.0.4** | 打包产物版本（安装包名随之为 `WB Bridge_1.0.4_*`）与更新清单 `version` 来源 |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.4** | 壳工程同步落点（`src-tauri/Cargo.lock` 由 cargo 自动同步，须一并提交） |
| `AGENTS.md`「当前基准版本」两行 | **1.0.4** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.0.4` | `vite.config.js` 构建期注入（自带 `v` 前缀），非独立落点 |

### 二、本版内容（代码范围 `v1.0.3..HEAD`，含未提交的工作区改动）

- **新增 `src-tauri/core/src/targets.rs`**：写入目标唯一定义（`Target::ALL`：WorkBuddy / CodeBuddy）＋ 逐目标检测（定位成功即「已安装」，不猜进程/注册表）＋ 分发结果聚合 `aggregate_sync`（count 之和、顶层 error 仅在全部定位目标失败时出现）；6 项单测。
- **新增 `src-tauri/core/src/codebuddy_config.rs`**：与 `workbuddy_config.rs` 逐行对照的 CodeBuddy 定位（`BUDDY_CODEBUDDY_MODELS_FILE` > `CODEBUDDY_CONFIG_DIR` > `CODEBUDDY_DATA_FOLDER_NAME` > 默认 `~/.codebuddy/`），复用同一校验器，失效绝不静默回退；4 项单测。
- **`orchestration.rs`**：App 持有双目标路径（`workbuddy_models_file` / `codebuddy_models_file`，对称命名）；bootstrap 双目标定位并新增顶层 `codeBuddyModelsFile`；`sync_published` 改为对每个已定位目标各写一次（同一份发布集、同一套 `sync_models` 幂等合并），`sync.targets.{workBuddy,codeBuddy}` 逐目标上报。
- **面板**：集成视图逐目标展示状态 + 「CodeBuddy 配置」只读行；底部摘要行措辞不绑定单一目标；旧核心（无 `targets`）自动回退单行展示。
- **文档**：`docs/contract.md` 新增「写入目标与检测规则」；`docs/version/RELEASE-NOTES-v1.0.4.md` 为 GitHub Release 正文。

### 三、验证（2026-10-08 本轮真实执行）

核心 `cargo test` **237 通过 / 0 失败**（lib 215 + `js_parity` 11 + `red_lines` 11，本版新增 10 项）、`git diff --stat src-tauri/core/tests/fixtures` 为空（对拍逐字节等价）、核心 `cargo clippy --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**、JS 三组 **8 / 9 / 13**、`npx eslint .` 0 problem、`npm run vite:build` ✓、`npm run version:check` **5 处一致（1.0.4）**。

### 四、未验证（不得伪装）

- ❌ GUI 实机启动与真实双插件环境下的写入效果（托盘、面板逐目标展示属「必须实机点一遍」）。
- ❌ CodeBuddy 默认目录 `~/.codebuddy/models.json` 是比照 WorkBuddy 约定的假设（仓库此前无 CodeBuddy 线索）；实际不同时改 `codebuddy_config.rs::DEFAULT_DATA_FOLDER` 或用 `BUDDY_CODEBUDDY_MODELS_FILE` 指定。
- ❌ v1.0.4 安装包（本机与 CI）尚未构建；一次真实升级闭环仍未验证。
- ⚠ 工作区既有状态（非本版改动）：`docs/version/RELEASE-NOTES-v1.0.3.md` 处于已删除未提交状态，由维护者定夺。

---

## v1.0.3

> **状态**:。五处落点已一致为 `1.0.3`（`npm run version:check` 实测「全部 5 处版本号一致（1.0.3）」），但**尚未构建任何安装包、尚未打标签**，且版本落点提交仍未执行——按铁律先提交、再在那一个提交上打 `v1.0.3`。
> **日期**: 2026-10-03
> **上一版本**: v1.0.2（标签 `0e4a535`，Release 已 Publish、23 个资产）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.0.3.md`](RELEASE-NOTES-v1.0.3.md)
> **本版主题**: 多平台接入 Stage 1（平台注册表 + 用户自持 Key 通道）＋ Stage 2（模型命名空间参数化）＋ 四项安全 / 数据红线修复 ＋ `latest.json` 下载 url 缺陷修复与 CI 资产对账闸门
> **版本推进理由**: 与 v1.0.2 **不同类、不同根因**——新增三条管理动作与凭据落盘通道；修复的四个根因（一次性子进程携带宿主环境、探测路径的转写开关、`status.json` 非对象形状在 `panic = "abort"` 下整进程退出、并发 `modelResults` 互相吞写）此前都不存在；且**维护者明确要求推进版本号**（`npm run version:set -- 1.0.3`），末位 +1。

### 一、版本号落点（`npm run version:set -- 1.0.3` + `npm run version:check` 实测）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.0.3** | 版本单一来源 |
| `src-tauri/tauri.conf.json` → `version` | **1.0.3** | 打包产物版本（安装包名随之为 `WB Bridge_1.0.3_*`）与更新清单 `version` 来源 |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.3** | 壳工程同步落点（`src-tauri/Cargo.lock` 内 `wbbridge` 由 cargo 自动同为 1.0.3，须一并提交） |
| `AGENTS.md`「当前基准版本」两行 | **1.0.3** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.0.3` | `vite.config.js` 构建期注入（自带 `v` 前缀，故本版把「已是最新版本」一行的重复 `v` 去掉），非独立落点 |

⚠ 副作用如实记录：`bump-version.mjs` 用 `JSON.stringify(…, null, 2)` 重写 `tauri.conf.json`，把原先单行的 `bundle.targets` 数组展开成多行——**纯格式差异、语义不变**，本次随版本落点一起提交。

### 二、本版内容（代码范围 `v1.0.2..HEAD`，两个提交）

- `43b1e19`：Stage 1（`providers.rs` 四平台注册表 + `providers.json` 凭据通道 + 三条 `provider-*` 动作，核心 `ACTION_ROUTES` 与壳 `ADMIN_ROUTES` 同步 5 → 8）、Stage 2（`join_namespace` / `split_namespace` / `free_models_in` / `model_target`，展示 ID 前缀只跟注册表 `label`）、`gen-latest-json.mjs` 的空格→点修复与 `release.yml` 的「校验清单 url 指向的资产在 Release 上真的存在」闸门、`docs/contract.md` 三条动作与凭据口径、同日规划与设计文档、`docs/qa/smoke-checklist.md`。
- `9d0b5ef`：四项安全 / 数据红线修复（`runtime.rs::allowed_environment` 收口 `--version` 探测的子进程环境；`orchestration.rs::probe_meta()` 补探测路径的转写开关；`restored_model_results` 挡非对象形状；`apply_patch` 锁内逐键合并 `modelResults`）＋ 对应 5 项新测试（lib 4 + `red_lines` 1）。
- 面板一行文案：`src/views/AboutView.vue` 的「已是最新版本」去掉重复 `v`。

### 三、验证（2026-10-03 本轮真实执行）

核心 `cargo test` **227 通过 / 0 失败**（lib 205 + `js_parity` 11 + `red_lines` 11）、核心 `cargo clippy --all-targets` 与壳 `cargo clippy --no-deps --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**、`npm run test:prefs` **8** / `test:manifest` **9** / `test:updater-key` **13**、`npx eslint .` **0 problem**、`npm run vite:build` ✓、`npm run version:check` **5 处一致（1.0.3）**、`git diff --stat src-tauri/core/tests/fixtures` 为空（Stage 2 等价移植的判据）。

### 四、未验证（不得伪装）

- ❌ GUI 实机启动；v1.0.3 的任何安装包（本机与 CI 都还没构建过）。
- ⚠ Stage 1 端到端：`providers.json` 里的 Key **无消费者**（Stage 3）、面板无入口（Stage 5），真实 Key 从未输入、`0600` 只在 unix 断言。
- ❌ 一次真实升级闭环；本版新增的那道 CI 对账步骤**本身未在 CI 跑过**（本机只跑过同段逻辑：线上坏清单 6/6 exit 1、修正后 6/6 OK exit 0）。

---

## v1.0.2

> **状态**:（📝→✅ 的定夺属维护者；事实部分已翻篇：标签 `v1.0.2` 已打在 `0e4a535`，GitHub Release `v1.0.2` **已 Publish、23 个资产**（六平台安装包 + 各自 `.sig` + `latest.json`）。`release.yml` 2026-10-03 首跑止步于 updater 签名步骤（私钥变量取到空值，已修接线），**同日末轮已跑通完整一轮**（run `37092915120`、作业 8/8 全绿，六平台产物与产物名、`.sig` 全部实测到）；🔴 但那一份 `latest.json` 的六条 `url` 全部 404——GitHub 上传时把**资产名里的空格规范化成 `.`**，而按空格名（含 `%20`）拼出的下载路径不存在，`gen-latest-json.mjs` 与 CI 对账闸门已修，线上那份待重传。仍**未验证**：GUI 实机启动、一次真实升级闭环、六平台的实机安装。本工作区内的后续更正尚未提交）
> **日期**: 2026-10-02
> **上一版本**: v1.0.1（远端标签 `v1.0.1` → `679a2cb`，含版本推进提交 `46c7c56`；**本地标签仍指 `f046208`**，同步动作属维护者）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.0.2.md`](RELEASE-NOTES-v1.0.2.md)
> **本版主题**: 自动更新链路接入（updater + process 插件、签名产物、`latest.json` 单一写者）+ 发布链路自证（标签↔版本闸门、产物名自检）+ 面板偏好持久化；2026-10-03 追加一轮渲染与轮询降耗（不推进版本号）。
> **版本推进理由**: 本版新增内容与 v1.0.1 **不同类、不同根因**——把「升级只能靠手动重装」变成「应用内检查 → 下载 → 重启生效」，并给发布链路加上防标签指错的门禁与偏好持久化。**推进动作由维护者本人执行**（工作区 5 处落点已改为 `1.0.2`，`npm run version:check` 通过），Agent 未擅自推进。

### 一、版本号落点（`npm run version:set -- 1.0.2` + `npm run version:check` 实测）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.0.2** | 版本单一来源；本版另加 `test:prefs`、`test:manifest` 脚本 |
| `src-tauri/tauri.conf.json` → `version` | **1.0.2** | 打包产物版本（安装包名随之为 `WB Bridge_1.0.2_*`）与 `plugins.updater` 清单的 `version` 来源 |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.2** | 壳工程同步落点（`src-tauri/Cargo.lock` 内 `wbbridge` 同为 1.0.2） |
| `AGENTS.md`「当前基准版本」两行 | **1.0.2** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.0.2` | `vite.config.js` 构建期从 `package.json` 注入（「关于与更新」展示），非独立落点 |
| `src-tauri/core/Cargo.toml` → `version` | `0.1.0`（**未改**） | crate 内部版本，与产品版本有意解耦 |
| `orchestration.rs` 写入 `status.json` 的 `version` | `0.2.0`（**未改**） | 历史沿革值；`schemaVersion` 仍为 `1`（本版未新增/改名 `status.json` 顶层字段） |

`npm run version:check` 实测输出：**全部 5 处版本号一致（1.0.2）**。

### 二、本版内容（详见 `RELEASE-NOTES-v1.0.2.md` 与 `docs/validation.md` 同日条目）

- **自动更新接线（壳）**：`src-tauri/Cargo.toml` 新增 `tauri-plugin-updater` + `tauri-plugin-process`（官方插件、同一维护方），`lib.rs` 的 builder 链注册两者；`tauri.conf.json` 加 `bundle.createUpdaterArtifacts: true` 与 `plugins.updater`（内嵌 `pubkey`，key ID `126D4E208E0F17BA`；端点 `https://github.com/trexwb/wbBridge/releases/latest/download/latest.json`）；`capabilities/default.json` 只加 `updater:default` + `process:allow-restart`（**刻意不用 `process:default`**：它含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链硬杀进程）。
- **重启路径不再冻窗口**：`RunEvent::ExitRequested` 由事件循环线程上的无界 `graceful_stop` 改为 **`stop_core_bounded`**（复用 `STOP_BUDGET = 8s`）——`process.relaunch()` 也走这条分支，无界停止会让「重启」看起来像卡死。⚠ 该项**无单测覆盖**（需真实 `AppHandle`），效果属必须实机看的一类。
- **面板更新区**：`src/core/update.js` 状态机（`idle/checking/available/downloading/ready/uptodate/error`，冷启动 5s 后静默检查、静默失败不打扰）+ `src/core/bridge.js` 的 `checkUpdate/downloadUpdate/relaunchApp`（调官方插件命令，联网在 Rust 侧，CSP 无需放宽，**未新增自有 IPC 命令/事件**）；`AboutView.vue` 只在真有新版本时出现更新条，进度百分比只在上游给出 `contentLength` 时显示。
- **`latest.json` 单一写者**：`scripts/gen-latest-json.mjs`（平台键取自产物**目录名**的 target triple，默认六平台缺一即退出 1）+ CI 末尾 `update-manifest` 作业；三个 build 作业的 `tauri-action` 全部 `includeUpdaterJson: false`（6 个并发作业各写一次会互相覆盖）。macOS 更新包由 CI 补架构后缀（tauri 原名 `WB Bridge.app.tar.gz` 不带版本与架构，两个 mac runner 会同名互相覆盖）。✅ 同日 CI 首跑实测：macOS 补后缀确实生效（`WB Bridge_aarch64.app.tar.gz(.sig)` / `WB Bridge_x86_64.app.tar.gz(.sig)`，无同名覆盖）；Windows 为 `WB Bridge_1.0.2_{x64,arm64}-setup.exe(.sig)`（另有 `.msi(.sig)`，不参与更新）；**Linux 的更新包是裸 `.AppImage`**（`WB Bridge_1.0.2_{amd64,arm64}.AppImage(.sig)`，bundler 没有额外产出 `.AppImage.tar.gz`；另有 `.deb(.sig)`，不参与更新）；六条 `.sig` 的签名者 key ID 逐字节解出全部 = `2B11F78BEA8A43F`。
- **发布链路自证（A9）**：CI `test` 作业新增**标签↔版本闸门**（`GITHUB_REF_NAME` 去前导 `v` 必须等于 `tauri.conf.json` 的 `version`，否则 exit 1；`workflow_dispatch` 跳过）；三个 build 作业上传前打印 bundle 文件名与源码版本；`update-manifest` 汇总六平台安装包并把「产物版本 = 标签版本」自检三分类写进 run summary（**advisory warning 而非硬失败**；非 macOS 产物名已于同日首跑实测到）。⚠ 这套自证**盖不住**真正的坑：首跑六平台齐全、CI 全绿，`latest.json` 的六条 `url` 却全部 404（见「状态」行的 🔴 说明）。同日据此补了一道硬闸门——`update-manifest` 新增「校验清单 url 指向的资产在 Release 上真的存在」，用 API 把六条 url 末段与实际资产名逐条对账、不符 `exit 1`；⚠ 该步骤本身尚未在 CI 上实跑过。
- **`latest.json` 生成器入库行为基线（A6-7b）**：新增 `scripts/gen-latest-json.test.mjs`（`node --test`，**9 通过**，命令 `npm run test:manifest`），用 `mkdtemp` 临时产物目录跑**真脚本**，钉住「平台键只由 artifact 目录名的 target triple 决定」以及四类必须失败的路径（缺平台 / 缺 `.sig` / mac 资产名漏架构后缀 / 同一平台多个候选目录或互不相干的已签名包），并覆盖 Linux 双层打包时优先取 `.tar.gz` 的降级口径（实测：bundler 不双层打包，首跑取的是裸 `.AppImage`）。此前这些结论只在一次性手写夹具里验证过、**未入库**，回归无从保护；现在这类错误的暴露点从「用户装上才发现平台缺席」提前到 CI。测试**不联网**、不读 `src-tauri/target/` 下的真实产物。
- **面板偏好持久化（A8 + A6-7）**：新增 `src/core/prefs.js`——`localStorage` 键前缀 `wb.`、**写入前键白名单校验** + 值字段投影 + 序列化后 2KB 上限、存储不可用/抛错一律静默降级为默认值；`App.vue` 持久化当前视图，`update.js` 持久化「启动后自动检查更新」开关与上次**成功**检查时间戳（12 小时节流），`AboutView.vue` 提供该开关。凭据类字段（`api-key`、`OPENCODE_SERVER_PASSWORD`、models 文件路径）在设计上不可能进入 `localStorage`，由 `src/core/prefs.test.js`（`node --test`，**8 通过**）钉住，命令 `npm run test:prefs`。
- **文档（A5 / A7 / A10）**：`.env.example` 写清三个签名环境变量的真实分工（`tauri build|bundle` 只读 `TAURI_SIGNING_PRIVATE_KEY`，值可为私钥全文或**绝对路径**）；`docs/wiki/版本与发布.md` 新增「商业签名与公证（A7 预留位，尚未接入）」「失败路径与回滚」「标签纪律」三节并扩充发布前检查清单；`docs/wiki/常见问题与故障排查.md` 补更新失败/回滚/偏好开关/Gatekeeper 四条；`docs/wiki/已知限制与未验证项.md` 按实测状态更正未验证项与已知限制表。

### 三、基线与验证边界（本版真实执行）

- 核心 `cargo test` → **207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9）；核心 `cargo clippy --all-targets` → **0 warning**（`touch src/lib.rs` 强制重检后仍为 0）。
- 壳 `cargo test --lib` → **9 通过 / 0 失败**；壳 `cargo clippy --no-deps --all-targets` → **0 warning**（版本改写触发重新检查，无告警）。
- `npm run test:prefs` → **8 通过 / 0 失败**；`npm run test:manifest` → **9 通过 / 0 失败**；`npm run test:updater-key` → **9 通过 / 0 失败**（2026-10-03 签名接线轮新增，见下方第六节）；`npx eslint .` → **0 problem**；`npm run vite:build` → ✓ built；`npm run version:check` → 5 处一致（1.0.2）；`release.yml` YAML 解析通过。三组 JS 测试都已接入 CI 的 `test` 作业。
- 本机签名构建（发生在 `1.0.1` 源码版本上，产物名因此带 `1.0.1`）：`bundle/dmg/WB Bridge_1.0.1_aarch64.dmg`（3,720,766 B）+ `bundle/macos/WB Bridge.app.tar.gz`（3,595,514 B）与配对 `.sig`，key ID 与配置公钥一致；dmg 只读挂载核对、`codesign --verify --deep --strict` 通过（adhoc + hardened runtime，**未公证**）。
- ❌ **未验证**：GUI 实机启动与面板交互、v1.0.2 六个安装包的实机安装、`latest.json` 发布后被真实客户端下载并安装（线上那份仍是 URL 缺陷的那份）、一次完整的「检查 → 下载 → 安装 → 重启 → 首启」与失败回滚、`relaunch` 后带新核心的重启、`localStorage` 在真实 WebView 内的行为、同日新增那道 CI 资产对账步骤本身。
- ✅ 已由同日（2026-10-03）末轮的 CI 完整一轮实测到、不再属于未验证项：`release.yml` **跑完整一轮**（tag `v1.0.2`、head `0e4a535`、run `37092915120`、作业 8/8 全绿）、Windows / Linux 的产物名与 `.sig`、GitHub 仓库级 **Variables 已配好且可用**（签名能跑完即证据；本机仍无 `gh`、未联网核验其值本身）。详见 `docs/validation.md`「CI 完整一轮实跑核对与 `latest.json` 的 URL 缺陷」。

### 四、兼容与升级口径

- **对外契约未变**：HTTP 路由、错误码、`status.json` 字段（本版无新增/改名）、自有 IPC 命令名与事件名全部保持 v1.0.1 原样；`STATUS_SCHEMA_VERSION` 两侧仍为 `1`。
- **新增的是插件权限**（`updater:default`、`process:allow-restart`）与两个官方插件依赖，不影响既有命令。
- **v1.0.1 及更早的用户收不到自动更新**：那一版没有接线更新器（产物无 `.sig`、面板无更新区），必须**手动下载 v1.0.2 安装包覆盖安装一次**；此后 v1.0.2 → v1.0.3 才可能走应用内更新。
- 数据目录与 WorkBuddy 写入规则不变；`api-key` 沿用，无需重装后重配。

### 五、2026-10-03 追加（不推进版本号）：渲染与轮询降耗 + 同日代码复审修正

- **壳**（`src-tauri/src/lib.rs`）：`watch_status` 原先每 500ms 都 `read_to_string` 整份 `status.json`（实测 8.6 KB）再逐字节比对；改为先用 `(mtime, 长度)` 前置过滤，两者都没变即跳过，只有变了才读内容，`stamp` 只在成功读到内容后记录（避免读取失败的那一轮被误当稳态）。
- **核心**（`src-tauri/core/Cargo.toml` + `src-tauri/Cargo.lock` + `src-tauri/core/Cargo.lock`）：`reqwest` 拆除 `brotli` feature；同处留注 `base64 = "0.22"` 不动。
- **面板**（`src/core/bridge.js`、`src/views/ModelList.vue`、`src/App.vue`）：`core-status` / `core-activity` / `core-failed` 改为状态操作入队 + `requestAnimationFrame` 每帧最多 flush 一次；无结果的行与详情共用 `EMPTY_RESULT` 稳定引用，「请求中」判断由逐行 `some()` 改为一次成 `Set`。
- **不推进版本号**：降耗与同日复审修正都属同一未发布版本（v1.0.2）内的打磨与再修复，未引入新功能、无新根因修复，版本保持 **1.0.2**（`npm run version:check` 复验 5 处一致）。

**同日代码复审（两个独立 code-reviewer 并行复审上述改动）的结论与修正**：

- 🔴 **修 1（会冻结面板状态）**：原先 `meta.modified().unwrap_or(UNIX_EPOCH)` 让 mtime 不可得时 stamp 退化成 `(EPOCH, 长度)`，此后**长度不变的改写被永久跳过**、面板不再收到任何推送。改为 `meta.modified().ok().map(...)` + 新增 `status_read_needed()`：**拿不到 mtime 就不启用快路径**，一律回落读内容。
- 🔴 **修 2（吞掉既有保障）**：恢复运行分支原先只 `last.clear()`，没作废 stamp；而该分支注释的整条理由就是「重启后 status.json 可能与故障前逐字节相同，不重置就永远不会再推送」——粗粒度 mtime 文件系统上这条保障被快路径抵消。现补 `last_stamp = None`。
- 🟠 **修 3（队列无上限）**：替换型事件（`core-status` / `core-failed`，其 payload 整体替换状态）入队前先 `pendingOps.length = 0`。这与逐条折叠**严格等价**（后面的替换本就丢弃前面所有 op 的产出），却让队列天然封顶——否则窗口不可见、rAF 停摆期间队列每 ~0.5s 堆两个持有整份 payload 的闭包。
- 🟠 **修 4（注释与事实不符）**：「合并语义与逐个执行**完全**等价」不成立——订阅者侧等价，但 `lastState` 要到帧末才更新，事件到达与 flush 之间的**同步读取点**（`action()` 里的 `lastState.modelsFile`、`onState()` 的首次回放）会读到旧一帧。按最小改动只更正注释、不改逻辑（影响是极小概率多弹一次文件选择框，下一帧自愈）。
- 🟡 **修 5（依赖注释的事实修正）**：`base64` 注释的「依赖树里的两份」口径**只对核心 workspace 成立**（实读 `src-tauri/core/Cargo.lock`：0.22.1 ← reqwest、0.23.1 ← hyper-util；壳 workspace 是**三份**，另有一份 0.21.7 ← `swift-rs`），注释已限定范围。「npm registry 回 gzip」本轮**独立复测**（`curl -H 'Accept-Encoding: gzip, br'`）：两白名单源的元数据均 `content-encoding: gzip`，而 tarball（npmjs 直连、npmmirror 的 CDN 目标）均为 `application/octet-stream` 且**不带 content-encoding**——即 brotli 在真实链路上从不被用到，去掉它不影响 sha512 校验语义。另补口径边界：这是**核心 crate 自身**依赖树的收益，壳产物里 brotli 仍会经 `tauri-codegen` 引入，安装包总体积不会因此等量减少。
- ✅ **复审确认无恙的项**（不再重复怀疑）：`service_down` 分支 emit `core-failed` 后直接 `continue`，故障期间不发 `core-status`，故 FIFO 折叠不会出现「status 洗掉 error」；`EMPTY_RESULT` 共享单例安全（`ModelRow.vue`、`ModelDetails.vue` 对 `props.result` 全为只读取值）；`props.activity || []` **不是冗余**（壳在缺键时会发 `activity: null`，prop 默认值拦不住 null）；两份 Cargo.lock 无多删漏删（核心恰好移除 brotli/brotli-decompressor/alloc-stdlib/alloc-no-stdlib，壳只删 async-compression 的一行引用）；未新增 IPC 命令或事件名、未写 localStorage、未引入外部请求、未推进版本号。
- ⚠️ **仍无单测的项**：新补的 `status_read_needed_only_skips_when_mtime_is_known` 只覆盖快路径**判定**；`watch_status` 整条循环仍需真实 `AppHandle`，面板帧合并也没有 JS 单测（JS 侧本轮仍只有 `test:prefs` 8 + `test:manifest` 9 两套；同日稍后的签名接线轮另加 `test:updater-key` 9，见下方第六节）。
- **验证（2026-10-03 复审后复跑）**：核心 `cargo test` **207 通过 / 0 失败**、壳 `cargo test --lib` **9 通过 / 0 失败**（8 → 9，新增上述快路径判定）、核心与壳 `cargo clippy` **0 warning**、`npm run test:prefs` **8 通过**、`npm run test:manifest` **9 通过**、`npx eslint .` **0 problem**、`npm run vite:build` ✓ built（`dist/assets/index-*.js` 100.87 kB / gzip 37.95 kB）、`npm run version:check` **5 处一致（1.0.2）**。细节见 `docs/validation.md` 2026-10-03 条目与 [`RELEASE-NOTES-v1.0.2.md`](RELEASE-NOTES-v1.0.2.md)。
- ⚠️ 渲染 / 轮询收益仍需实机确认：GUI 从未启动，rAF 在 WKWebView 中最小化 / `hide()` 下的真实停摆行为未实测。

### 六、2026-10-03 追加（不推进版本号）：updater 签名接线与 CI 首轮失败定位

- **起因**：`release.yml` 首次实跑在签名那步报 `failed to decode secret key: incorrect updater private key password: Missing comment in secret key`。本机用 **14 组形态**复现把 tauri 的解码报错逐条钉到成因，结论：**这句就是「私钥变量取到空值」**，不是口令错（当时私钥只配在 GitHub Secrets，而 build 作业没声明 `environment:`，环境级 Secret 对作业不可见 → 展开成空串）。且因 `createUpdaterArtifacts: true` 时 tauri 要跑完整套 Rust 编译才在打包那步解私钥，一次配置错误要烧十几分钟才暴露。
- **三项用户决定**：① **就用 9-30 那把旧钥** `~/.tauri/wbBridge-updater.key`（`2B11F78BEA8A43F`，口令非空）；② CI 变量**改用仓库级 Variables** 对齐参考项目 fastenerTradeWorkbench（⚠ 明文值、日志不打码，代价已写进 workflow 顶部注释与两处 wiki，随时可 `vars.`→`secrets.` 换回）；③ **要本地包装脚本**。
- **由此更正一条既有错误记录**：`AGENTS.md` 与 `docs/wiki/版本与发布.md` 此前称 9-30 钥「密码未知、与现 pubkey 不配对、已弃用」——那是**整串比较 pubkey** 造成的误判。~~`plugins.updater.pubkey` 解开后内嵌**两条**公钥（`126D4E208E0F17BA` / `2B11F78BEA8A43F`，minisign 支持多条＝轮换白名单），本机实测该钥「形态 OK → 配对 OK → **试签 OK**」，**`tauri.conf.json` 无需改动**。~~ ⚠ **本条的「两条公钥＝轮换白名单、无需改配置」结论已于同日（2026-10-03 第二轮）被源码推翻**：`verify_signature` → `PublicKey::decode` 只认 pubkey 的**第一条**，当时配置里其实只有 `126D4E208E0F17BA`（属已消失的 20261001 钥），签名用的却是 `2B11F78BEA8A43F`，于是构建告警 does-not-match、`.sig` 会被客户端判无效；修复＝`pubkey` 改为**仅 `2B11F78BEA8A43F` 一条**，**需要改 `tauri.conf.json`**。详见下方「七」。⚠ 另一把 `wbBridge-updater-20261001.key` 本日发现**已从 `~/.tauri/` 消失**（Agent 全程只读，未删未改；找回与否属用户决策）。
- **新增签名自检脚本** `scripts/with-updater-key.mjs`（**本地手跑工具，不接入 build 与 CI**，2026-10-03 用户决定）：取值（**进程环境 > `.env.local` > `.env`**，逐项独立回落、逐项打印来源，`~/…` 展开，显式空口令压过文件）→ ① 形态（单行 base64、解出首行须是 `untrusted comment`）② 同目录 `.pub` 的 key ID 必须在配置白名单内（不在即**失败关闭**）③ **真跑一次 `tauri signer sign`**（`mkdtemp` 临时目录、签完即删，钥与口令只经环境传子进程、不进 argv、不打印）→ 全过才 exec `tauri build`；`--check-only` 只前置不构建。`npm run build` 保持 `vite:build && tauri build`（前置脚本为本地手跑工具，不接入 build 与 CI，2026-10-03 用户决定）。
- **CI 改动**：三个 build 作业的签名变量 `secrets.` → `vars.`（`GITHUB_TOKEN` 不变）；test 作业那一步扩为三组 `node --test`；顶部注释整段重写（含 Variables 的明文代价、口令非空、两条 key ID 白名单、空值→`Missing comment` 的映射）。⚠ 曾给三个 build 作业在 `npm ci` 之后各加一步「前置校验 updater 私钥」跑 `--check-only`，**同日按用户决定撤除**——CI 不再校验私钥，脚本留作本地自检。
- **新测试** `npm run test:updater-key`（`node --test`，**9 通过 / 0 失败**）：钉住取值优先级 / 显式空口令 / `~` 展开 / 四类非法形态 / key ID 解析 / 配对失败关闭 / 四条报错→成因映射 / **CLI 相对路径下确实执行**且空值与路径型私钥在试签前退出 1（**报错不回显密钥**）/ `release.yml` 与 `package.json` 的接线。**不联网、不调 tauri CLI、不碰真实私钥**。
- **写脚本过程中查出并修掉的自身缺陷**（全部补了单测）：入口守卫 `file://${argv[1]}` 在**相对调用**下永不成立（→ `--help` 静默无输出、退出 0，等于前置没跑）；`.env` 与 `.env.local` **优先级写反**；不展开 `~` 导致本地 `.env.local` 的路径形式被误判「形态不对」；私钥与口令**来源混搭**时只报「口令不对」无法归因（→ 逐项打印来源）；key ID 断言过严（配置里那条实为 **15 位**十六进制，minisign 不补前导零）。
- **本轮真实执行**：`test:updater-key` **9 通过**、`test:prefs` **8**、`test:manifest` **9**、核心 `cargo test` **207 通过 / 0 失败**（Rust 未改动，复跑确认未破坏）、壳 `cargo test --lib` **9**、`npx eslint .` **0 problem**、`npm run vite:build` ✓ built、`npm run version:check` **5 处一致（1.0.2）**、`release.yml` YAML 解析通过，且三个 build 作业步骤里**都不含**私钥前置步骤（撤除后复验）。**前置本身用真 minisign 跑过**：一次性新钥 inline ⇒「配对跳过 + 告警 → 试签 OK → 退出 0」；同钥路径形式 ⇒「key ID 不在白名单 → 退出 1」；真实 9-30 钥（回落仓库 `.env.local`）⇒「配对 OK → 试签 OK → 退出 0」。
- ❌ **仍未验证**：改完的 `release.yml` **没有在 CI 跑第二轮**（`vars.` 切换、配好变量后整条 `tauri build` 能否签出可被客户端验签的 `.sig`；私钥配置错误在 CI 里仍要等到打包那一步才暴露，因为已不设前置）；「试签 OK」只证明**这把钥 + 这个口令可用**，不等于产物签名已被验证。「待用户操作」清单（配 GitHub 仓库级 Variables 的私钥全文与非空口令、是否 `chmod 600 .env.local`、是否找回 20261001 钥、提交与打标签）见 `docs/validation.md` 同日条目。〔**同日末轮更正**：`release.yml` 的完整一轮已跑通（tag `v1.0.2`、head `0e4a535`、run `37092915120`、作业 8/8 全绿），Variables 与 CI 侧签名链路已被证明可用，标签与提交也已就位；真实升级闭环与 GUI 实机启动仍未验证。〕

### 七、2026-10-03 追加（不推进版本号）：签名注入链路重写 + 单条公钥 + hdiutil dmg

> 同日第二轮，**重写并部分更正**上方「六」的签名链路。「六」按「历史只增不改」保留原样，其被推翻处（两条公钥＝轮换白名单）已在该节就地标注作废。

- **`scripts/with-updater-key.mjs` 从「本地手跑的签名自检工具」重写为 `npm run tauri:build` 的签名注入包装器**：`npm run tauri:build` = `node scripts/with-updater-key.mjs tauri build`、`npm run build` = `vite:build && tauri:build && make:dmg`，**已接入 build 与 CI**。取值优先级扩为 **进程环境 > 仓库 `.env.local` > `.env` > `~/.tauri/wbBridge.env` > `~/.tauri/wbBridge-updater.env` > 兜底 `~/.tauri/wbBridge-updater.key`**（逐项打印来源、`~` 由脚本展开、显式空口令压过文件）；把 `.key` 路径读成内联全文并 trim 注入 `TAURI_SIGNING_PRIVATE_KEY`（删互斥的 `_PATH`）、明文钥显式置空口令，再 exec 目标命令。**`--check-only` 与「形态→配对→试签」前置全部撤除**，不判形态、不试签、配对不符只告警不阻断；早前的 `resolveKey`/`checkKeyShape`/`configuredKeyIds`/`readConfiguredKeyIds`/`checkPairing`/`classifySignerError`/`dryRunSign` 导出删除。
- 🔴 **关键更正（推翻「六」）**：`plugins.updater.pubkey` 并非「两条公钥＝轮换白名单」——实读 `tauri-plugin-updater` 2.13 `verify_signature()` → `minisign-verify` 0.2.5 `PublicKey::decode()`，**只有第一条生效**，后面的公钥框被静默丢弃。当时配置里其实只有 `126D4E208E0F17BA`（已消失的 20261001 钥），签名用的是 `2B11F78BEA8A43F`，于是告警 does-not-match、`.sig` 客户端验签必失败。修复＝`pubkey` 现**只嵌 `2B11F78BEA8A43F`**，**改了 `tauri.conf.json`**。规则：换钥必须同一次发布同步换 pubkey，老客户端只认内嵌那条。本次替换**不锁死任何已发布版本**（v1.0.0 / v1.0.1 都早于 updater 接线，`126D4E208E0F17BA` 从未进入交付的二进制；v1.0.2 是第一个带更新通道的产物）。
- **macOS `.dmg` 改由 hdiutil**：`bundle.targets` 从 `"all"` 改显式 `["app","nsis","msi","appimage","deb"]`；`npm run make:dmg`（`scripts/make-dmg.sh`，`hdiutil create -format UDZO`，架构取 `TARGET_TRIPLE` 前缀、非 mac 跳过）→ `bundle/dmg/<productName>_<version>_<arch>.dmg`。原因：Tauri 的 create-dmg 末尾用 AppleScript，无 GUI 的 CI runner 会失败。
- **CI**：撤除 `tauri-apps/tauri-action`；build 作业跑 `npm run tauri:build -- --target <triple>`（macOS 另加 `make:dmg` + 补架构后缀改名）；Release 由 `softprops/action-gh-release` 建、`tag_name` 现读 `tauri.conf.json`；签名变量取仓库级 **Variables**（明文值，工作流绝不 echo，`vars.`→`secrets.` 即可换回），**不设私钥前置步骤**。
- **测试基线**：`npm run test:updater-key` 由 9 → **13** 用例（钉住注入语义与接线断言），三组 JS 套件合计 **30**（prefs 8 + manifest 9 + updater-key 13），Rust 基线不变（核心 207、壳 9）；CI test 步骤改名「运行面板偏好、更新清单与签名注入单测」。
- **本轮真实执行（2026-10-03）**：零环境变量下 `npm run tauri:build -- --bundles app` **退出 0**（从 `.env.local` 取私钥路径 + 口令，打印「已注入内联签名私钥（来自 `wbBridge-updater.key`）→ 公钥配对 OK（`2B11F78BEA8A43F`）→ 加密态私钥 + 已提供口令」），产 `bundle/macos/WB Bridge.app.tar.gz`（3,596,811 B）+ `.sig`（428 B，逐字节解出的签名者 key ID = `2B11F78BEA8A43F` = 配置唯一那条）；`npm run make:dmg` 产 `bundle/dmg/WB Bridge_1.0.2_aarch64.dmg`（约 3.9 MB，只读挂载含 `WB Bridge.app` + `Applications`，`codesign --verify --deep --strict` 通过、adhoc / 无 TeamID）；`test:updater-key` **13**、`test:prefs` **8**、`test:manifest` **9**、核心 `cargo test` **207**、壳 `cargo test --lib` **9**、`eslint` **0 problem**、`vite:build` ✓、`version:check` **5 处一致（1.0.2）**、`release.yml` YAML 解析通过、`bash -n scripts/make-dmg.sh` 干净。告警甄别：`Warn skipping app notarization…`＝预期（ad-hoc、无公证凭据），pubkey does-not-match **已消失**，macOS 27 的 hdiutil `deprecated…use diskutil image` 只是警告。
- ❌ **仍未验证**：~~完整 CI 一轮（六平台 + `latest.json` + 一次真实升级）、Windows/Linux 产物名与签名~~〔同日末轮的 CI 完整一轮已把前三项实测到，见本节顶部的「状态」行与「三、基线与验证边界」〕、一次真实升级闭环、发布后 `latest.json` 的下载安装（线上那份仍是 URL 缺陷的那份）、`relaunch()` 后带更新核心的重启、六平台的实机安装、GUI 实机启动、同日新增那道 CI 资产对账步骤本身。测试基线不变（核心 `cargo test` **207**、壳 `cargo test --lib` **9**、JS **8 + 9 + 13 = 30**）。版本保持 **1.0.2**（本轮为签名/发布链路与文档重写，未引入新功能根因，不推进版本号）。

---

## v1.0.1

> **状态**:（改动已在工作区，**尚未提交、尚未打标签**；GUI 未实机启动、`release.yml` 未在 CI 跑过、无安装包产出）
> **日期**: 2026-10-01（同日第八轮）
> **上一版本**: v1.0.0（该分节从未发布，无安装包；本次推进即代表 v1.0.0 基线快照的全部内容并入 1.0.1）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.0.1.md`](RELEASE-NOTES-v1.0.1.md)
> **版本推进理由**: 本版包含与 v1.0.0 **不同类、不同根因**的新内容——供应链来源校验、鉴权兜底、壳生命周期、并发/panic 修复、面板 4 个只读视图与 `usage` 统计；且**用户在本次明确要求推进版本号**（"更新版本到 v1.0.1"）。两条同时满足 `AGENTS.md` 的版本号规则，故末位 +1：`1.0.0 → 1.0.1`。

### 一、版本号落点（`npm run version:set -- 1.0.1` + `npm run version:check` 实测）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.0.1** | 版本单一来源 |
| `src-tauri/tauri.conf.json` → `version` | **1.0.1** | 打包产物版本；安装包名随之为 `WB Bridge_1.0.1_*` |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.1** | 壳工程同步落点 |
| `AGENTS.md`「当前基准版本」两行 | **1.0.1** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.0.1` | `vite.config.js` 构建期从 `package.json` 注入（`src/views/AboutView.vue` 展示），非独立落点 |
| `src-tauri/core/Cargo.toml` → `version` | `0.1.0`（**未改**） | crate 内部版本，与产品版本有意解耦 |
| `orchestration.rs` 写入 `status.json` 的 `version` | `0.2.0`（**未改**） | 历史沿革值；`schemaVersion` 仍为 `1`（本版仅加法式新增顶层 `usage`） |

`npm run version:check` 实测输出：**全部 5 处版本号一致（1.0.1）**。

### 二、本版内容（详见 `RELEASE-NOTES-v1.0.1.md` 与 `docs/validation.md` 同日各轮条目）

- **安全/供应链**：tarball 来源必须属于白名单 registry（`Url::origin()` 比对，堵死「元数据可信但 tarball 站外」的活洞）；空白 `api-key` 一律重新生成并强制 `0600`，`authorized()` 拒绝空键；4 处 `let _ = set_permissions(..)` 改为传播错误；红线清单从手写 `/admin/*` 改为派生自 `server::ACTION_ROUTES`，壳侧新增 `shell_action_routes_match_the_core_contract`。
- **并发/生命周期**：`core_action` / `restart_core` 改 async + `spawn_blocking`（面板不再被动作冻结）；启动失败记入 `startup_error`、故障期不再推送上一份 `status.json` 残影、`core-failed` 去重后每 ~4s 重播、恢复时作废内容缓存；`drop_pending` 取代 `pending.remove(0)`；`start_probes` 用 `swap` 原子占用；`refresh()` 链位登记先于任何 `await`。
- **面板**：侧栏 5 入口全部实现（新增运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新 4 个只读视图，新增只读命令 `read_log`）；`status.json` 顶层 `usage`；导入不再无条件弹文件选择框；详情改为右侧常驻分栏、窗口 1120×720、侧栏 208px、交互反馈与 token 规范固化。
- **行为变更（同日第九轮）**：**关闭窗口 = 退出应用**，macOS / Windows / Linux 一致。`on_window_event` 的 `CloseRequested` 由「`prevent_close` + 隐藏到托盘驻留」改为「`prevent_close` + 隐藏 + 后台线程 `quit_app`」，与托盘「退出」走同一条优雅关停链路；`show_main` 在 `quitting` 置位后不再唤回窗口。托盘与其四项菜单保留。⚠️ 未做 GUI 实机验证。
- **文档**：新增 `docs/wiki/` 11 页；`AGENTS.md` / `README.md` / `docs/contract.md` / `docs/version/*` / `docs/wiki/*` 的测试基线统一为 207，并修正侧栏入口状态、符号名清单与过期路径引用。

### 三、基线与验证边界（真实执行）

- 核心 `cargo test` → **207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9）；核心 `cargo clippy --all-targets` → **0 warning**。
- 壳 `cargo test --lib` → **8 通过 / 0 失败**（日志尾部 4 项 + 核心路由契约 1 项 + 退出链路 3 项）；壳 `cargo clippy --no-deps --all-targets` → **0 warning**。
- `npx eslint .` → **0 problems**；`npm run vite:build` → 成功；`npm run version:check` → 5 处一致（1.0.1）。
- ❌ **未验证**：GUI 实机启动、托盘与面板交互、`release.yml` 在 CI 跑通、签名与打包产物、自动更新（未接线）。同日历史基线：迁移复验轮 195 → 审查修复轮 197（v1.0.0 标签口径）→ 面板与只读视图轮 202 → 安全与并发修复轮 207（本版）。

---

## v1.0.0

> **状态**:（核心已用 Rust 重写并静态链接进完整入库的 Tauri 壳，面板为 Vue 3 + Vite 构建；但迁移后**未产出过安装包、桌面 GUI 未实机启动、`release.yml` 未在 CI 运行**，签名链路亦未实际执行，尚不具备"已发布"条件）
> **发布日期**: 待定（记为 2026-09-30 基线快照日；2026-10-01 因 Node → Rust 迁移对本节的仓库形态与测试基线记录做更正，同日面板布局改造亦已并入本节）
> **上一版本**: 无（v1.0 首个分节）
> **版本范围**: 项目首个基线版本——Tauri 托盘壳 + **Rust 核心（crate `wbbridge-core`，path 依赖静态链接）** 的桥接工具，面向 WorkBuddy 提供隔离托管的 OpenCode 免费模型服务；Node.js 仅用于 Vite 构建面板与两个版本号脚本，**不存在 sidecar / pkg / `src-tauri/binaries/`**
> **版本号说明**: 本次仅新增 `docs/version/`（本文档 + `README.md`），属纯文档更新，按版本纪律**不推进版本号**，仍按约定建分节留痕；2026-10-01 的核心迁移更正同样**不推进版本号**（产品版本保持 1.0.0）

> ⚠ **2026-10-01 迁移更正说明**：本节下方「一、版本号基线核验」「二、代码与资产快照」「三、尚未入库/未实现项」最初按 Node/sidecar 形态记录，现已按迁移后的真实仓库状态更正；原记录中的 **97 通过 / 0 失败**（`node --test`）属于**已归档的 JS 核心**（连同其测试移到仓库外 `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`），不再是当前基线。当前基线：`cargo test`（`src-tauri/core/`）**207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9），壳侧另有 `cargo test --lib`（`src-tauri/`）**5 通过**（该分节口径；同日第九轮「关窗即退出」补 3 项壳侧单测后为 **8 通过**，见顶部 v1.0.1 分节；2026-10-03 代码复审修正轮补快路径判定后为 **9 通过**，见顶部 v1.0.2 分节）。同日历史：迁移复验轮 **195**（lib 177）→ 全量代码审查修复轮 **197**（lib 179）→ 面板与只读视图轮 **202**（lib 184）→ 安全与并发修复轮 **207**（本轮，详见 `docs/validation.md`）。

> ⚠ **2026-10-01 面板布局改造说明**：同日面板（`src/`，Vue 3）完成一次布局改造，本节「二、代码与资产快照」的 `src/` 一条已按改造后状态记录：详情面板改为**右侧常驻分栏**（`--details-w: clamp(300px, 45%, 360px)`，可收起，`Esc` / 详情头部按钮 / 窗口失焦三条等价路径，无遮罩层、不覆盖列表）、**删除 `<900px` 上下堆叠降级**；默认窗口由 980×680 调整为 **1120×720**（最小 `860×560` 不变）；侧栏宽 `--sidebar-w` 由 224px 调整为 **208px** 并改为分组导航（模型 / 运行 / 集成 / 其他 + 运行设置），其中 4 个入口当时仅为禁用态 + 「规划中」标签（同日第四轮已把 5 个入口全部实现，见下方快照与 `docs/validation.md`）；`styles/variables.css` 新增 `--muted-strong`。属同一未发布版本的界面调整，**不推进版本号**；验证证据见 `docs/validation.md`「面板布局改造」（**仅在浏览器引擎内经 CDP 实测，GUI 仍未实机启动**）。

---

### 一、版本号基线核验（实读，2026-10-01 更正）

| 项 | 值 | 来源 |
|---|---|---|
| 版本单一来源 | **1.0.0**（v1.0.0 分节当时口径；现已推进至 **1.0.1**，见顶部 v1.0.1 分节）| 根目录 `package.json` 的 `version`（`name = wb-bridge`，`private: true`，`type: module`，`engines.node >= 24`；Node 只服务面板构建与版本脚本） |
| 壳工程同步落点 | **1.0.0**（现已推进至 **1.0.1**） | `src-tauri/tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `[package] version`（壳 `name = wbbridge`，`edition = 2021`，`rust-version = 1.77`，`tauri = "2"`） |
| 核心 crate 内部版本 | `0.1.0` | `src-tauri/core/Cargo.toml`（crate `wbbridge-core`，`rust-version = 1.75`，`publish = false`）；**与产品版本有意解耦**，不是版本落点、不参与 `version:check`、不随产品版本递增 |
| 状态内置版本 | `0.2.0` | `src-tauri/core/src/orchestration.rs` 写入 `status.json` 的 `"version": "0.2.0"`（同处 `STATUS_SCHEMA_VERSION = 1` 描述快照结构），沿自上游参考实现（参考 https://github.com/louchi1984-coder/ow-bridge），历史沿革值，界面上可见 |
| 上游调研基线 | `0.2.5` | `docs/research/upstream-architecture.md`（该文档同时记录上游 `package.json` 0.2.5 与 `src/main.js` 内嵌 0.2.0 不一致的事实） |
| 运行时要求 | Rust stable（壳 `rust-version = 1.77` / 核心 `1.75`）；Node `>= 24`（仅面板构建与版本脚本）；OpenCode 运行时版本由 registry 最新版决定（不固定） | `src-tauri/Cargo.toml`、`src-tauri/core/Cargo.toml`、根 `package.json` 的 `engines.node`、`src-tauri/core/src/runtime.rs` |
| 版本一致性核验 | 5 处落点 | `node scripts/check-version.mjs`（`npm run version:check`）：`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml` / `AGENTS.md` 基准表两行；迁移后本次复验通过 |
| `src-tauri/tauri.conf.json` | **已入库** | `frontendDist: ../dist`、`devUrl: http://localhost:41990`、`identifier: app.wbbridge.desktop`、`bundle.externalBin: []`（无 sidecar） |
| CI workflow | **已入库，未在 CI 实跑** | `.github/workflows/release.yml` 已按 Rust 形态重写，仅做过本地 YAML 结构校验 |

### 二、代码与资产快照（实读核验，2026-10-01 迁移后更正）

- **`src-tauri/core/src/`（Rust 核心，crate `wbbridge-core`，lib `wbbridge_core`，16 个模块 + 独立入口）**：`orchestration.rs`（原 `main.js` 的等价编排/生命周期，含 `STATUS_SCHEMA_VERSION = 1` 与写入 `status.json` 的 `"version": "0.2.0"`，导出 `run(StartOptions)` 与 `set_exit_hook`）、`main.rs`（独立可执行入口 `wbbridge-core`，`--help` / `--version`）、`lib.rs`（crate 根，导出 `Env`、`BridgeError`、`STAGE = "stage-4-embedded-no-node"`、`PACKAGE_NAME`、`VERSION`）、`server.rs`（HTTP 路由与鉴权）、`protocol.rs`、`backend.rs`、`runtime.rs`、`probe.rs`、`repair.rs`、`handoff.rs`、`sync.rs`、`workbuddy_config.rs`、`system_proxy.rs`、`reasoning.rs`、`model_status.rs`、`platform.rs`、`atomic.rs`、`json.rs`
- **构建形态**：**无 sidecar、无 `@yao-pkg/pkg`、核心链路无 esbuild**。核心作为 path 依赖静态链接进壳；`src-tauri/tauri.conf.json` 的 `bundle.externalBin` 为 `[]`，`src-tauri/binaries/` 目录已删除，`src-tauri/core/Cargo.toml` 只额外产出一个用于独立运行/冒烟的 `[[bin]] wbbridge-core`
- **`src-tauri/`**：`Cargo.toml`（`name = wbbridge`、`version = 1.0.0`、`tauri = "2"`）、`tauri.conf.json`、`build.rs`、`src/{main.rs,lib.rs}`、`capabilities/`、`icons/`（签名环境变量模板只有一个落点：仓库根 `.env.example`；`src-tauri/updater-signing.env.example` 与之逐字节相同、已删除）。`lib.rs` 负责：在专用 tokio 运行时上 `orchestration::run(StartOptions { data_dir, port, handle_signals: false })` 装配核心、`pick_port()`（默认 41980，占用则回退系统分配端口）、`app.path().app_data_dir()` 注入数据目录、`admin_call(port, key, "/admin/…")` + `ADMIN_ROUTES`/`admin_route()` 驱动动作、轮询 `status.json` 并 emit `core-status` / `core-activity` / `core-failed`、`orchestration::set_exit_hook` 保证核心不能反向终止壳；暴露 Tauri 命令 `core_action` / `restart_core` / `core_running` / `data_dir_path` / `read_log`
- **`src/`（面板，Vue 3 + Vite）**：`index.html`、`main.js`、`App.vue`、`views/{SideBar,ModelList,ModelDetails,ServiceStatus,MetricsBar,FeedbackBar,LogsView,UsageView,IntegrationView,AboutView}.vue`（`SideBar.vue` = 分组导航 + 运行设置；`ModelDetails.vue` = 详情右栏；后 4 个 = 只读视图）、`components/ModelRow.vue`、`styles/{variables,base}.css`、`public/`；与壳的唯一边界是 **`src/core/bridge.js`**（导出 `action(name, value)` / `onState(cb)` / `onDismiss(cb)` / `readLog()` / `dataDir()` / `activityText`，包装 Tauri `invoke` + `listen`）与 `src/core/activity.js`。⚠ `src/core/` 现在是**前端内核**目录，与已归档的 Node 后端无关；构建产物在根 `dist/`（`vite.config.js`：`root: src`、`outDir: ../dist`、dev 端口 41990）。**布局（2026-10-01 改造后）**：外层为「左侧栏（`--sidebar-w: 208px`）+ 右侧主区」，主区默认单列，选中模型时 `.content.is-split` 变为 `minmax(0, 1fr) var(--details-w)` 两栏（`--details-w: clamp(300px, 45%, 360px)`）；详情为**右侧常驻分栏**（无遮罩、不 `position: fixed/absolute`、**任何宽度都不降级为上下堆叠**），`Esc` / 详情头部「收起详情」按钮 / 窗口失焦均可收起；侧栏 5 个入口全部为已实现视图（模型与服务 / 运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新），后 4 个是只读视图，不再有 `state: 'planned'` 禁用占位与「规划中」标签。壳窗口默认 **1120 × 720**、最小 **860 × 560**（`src-tauri/tauri.conf.json`）
- **`scripts/`**：仅 `bump-version.mjs`、`check-version.mjs`（版本号的设定与 5 处一致性校验）；`vite.config.js` 在根目录
- **测试基线（本次实读执行）**：`src-tauri/core/` 下 `cargo test` → **207 通过 / 0 失败**（lib 单测 **187** + `tests/js_parity.rs` **11** + `tests/red_lines.rs` **9**）；`src-tauri/` 下 `cargo test --lib` → **5 通过 / 0 失败**；`cargo clippy --all-targets`（核心）与 `cargo clippy --no-deps --all-targets`（src-tauri）均 **0 warning**。`tests/js_parity.rs` 现与冻结在 `src-tauri/core/tests/fixtures/*.json` 的 JS 真相快照对拍（**271 例 / 11 个 fixture 模块**），**不再需要 Node**；重新录制需恢复归档 JS 源码并 `WB_PARITY_RECORD=1 cargo test --test js_parity`
- **旧 Node 基线的归属**：迁移前的 **97 通过 / 0 失败**（`node --test`）属于**已归档的 JS 核心**，源码与测试整体移出仓库到 `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`（内含 `core/src/*.js` 16 个模块、`core/test/*.test.js` 12 个测试文件——其中 `contract.test.js` 断言的「三处动作表一致」门禁没有随迁移保留，见 `docs/contract.md`；另含用于重新录制对拍快照的 `core-rs-tests-js/`）；仓库工作区内已不存在 Node 版 `src/core/` 后端与 `src/ui/`，只剩同名不同物的前端内核 `src/core/bridge.js` / `activity.js`
- **文档与品牌资产**：`AGENTS.md`（项目规范，含「当前基准版本」章节）、`README.md`、`docs/{contract,validation}.md`、`docs/version/`、`docs/research/upstream-architecture.md`（上游调研）；原品牌资产目录 **`assets/brand/`**（自 `docs/brand/` 搬来，含 `logo*.svg`、`tray*`、`render.mjs`、`build-icons.mjs` 及其独立 `package.json`）已于 **2026-10-01 按方案 C 彻底删除**，相关引用（`package.json` 的 `icons` 脚本、`eslint.config.js` ignore、`.gitignore` 两行规则、`AGENTS.md`/`README.md` 等文档说明）已同步清理；`src-tauri/icons/` 内的全平台图标产物保留，构建与运行不受影响
- **仓库状态（`git` 只读核验，2026-10-01）**：分支 `dev`，远端 `origin = https://github.com/trexwb/wbBridge.git`（另有 `main`），HEAD = `d0cdf0b docs(README): 更新文档包含完整项目说明和使用指南`（其上为 `8c30d2f Initial commit`）。索引里仍是**迁移前布局**：`src/core/**`（30 个 Node 核心文件）与 `src/ui/**`（6 个旧静态面板文件）在 HEAD 中被跟踪、在工作区已删除；`src-tauri/`（`Cargo.toml`/`Cargo.lock`/`build.rs`/`tauri.conf.json`/`src/{main,lib}.rs`/`capabilities/`/`icons/`）、根 `package.json`、`README.md`、`AGENTS.md`、`.github/workflows/release.yml`、`.gitignore`、`docs/`、`scripts/build-sidecar.mjs` 已入库。**尚未 `git add` 的新文件**：`src-tauri/core/`（Rust 核心整体）、`src/{index.html,main.js,App.vue,views/,components/,core/,styles/,public/}`（Vue 面板；`src/core/` 现仅存前端内核 `bridge.js` / `activity.js`，与 HEAD 里被删的旧 `src/core/*.js` 同名不同物）、`vite.config.js`、`eslint.config.js`、`.editorconfig`；**已删除待提交**：`scripts/build-sidecar.mjs`、`docs/brand/*`；**已修改待提交**：`src-tauri/*`、根 `package.json`、`.gitignore`、`release.yml`、`AGENTS.md`、`README.md`、`docs/`。全部迁移改动截至本节撰写时均未提交（提交由用户决定）

### 三、尚未验证 / 未完成项（不得伪装成已验证）

1. **桌面 GUI 从未实机启动**：迁移后只验证过编译、clippy 与「独立核心二进制」的一次冒烟（一次性临时数据目录内完成运行时下载、隔离 OpenCode 启动、`/agent` 校验、发现 8 个免费模型并探测、干净关停）；托盘、面板交互、壳侧重启与优雅退出链路未经真实运行验证；2026-10-01 的面板布局改造仅在浏览器引擎内经 CDP 实测（1120×720 / 860×560 两档无横向溢出、键盘可达、收起路径生效、深浅色对比度达标），**仍非 Tauri GUI 实机**；
2. **迁移后未产出过安装包**：`src-tauri/target/release/bundle/` 不存在，`.app` / `.dmg` / Windows / Linux 包待重新构建；
3. **CI 未实跑**：`.github/workflows/release.yml` 已按 Rust 形态重写，仅做过本地 YAML 结构校验；
4. **签名与更新链路未执行**：`.env.example` 已就位（`src-tauri/updater-signing.env.example` 已删除，模板只留根目录一份）（`tauri.conf.json` 的 `plugins` 目前为空对象，未配置 updater），`npm run tauri -- signer generate …` 与签名构建均未实际跑过，**无对外分发安装包**；
5. **`cargo fmt --check` 全仓不干净**，未作为门禁；
6. 因此本版本**不具备"已发布"条件**，状态标记为。

### 四、与 `AGENTS.md` 记录的差异

#### 四·1 迁移前（2026-09-30 复核同步，Node/sidecar 时代，历史记录保留原样）

| 项 | `AGENTS.md`「当前基准版本」记录 | 当时实读核验 | 处理建议 |
|---|---|---|---|
| 测试基线 | 97 通过 / 0 失败（11 个测试文件） | **97 通过 / 0 失败**（11 个测试文件，`core/` 下 `npm test`，node v24.21.0） | ✅ 已同步至 `AGENTS.md` |
| 已构建 sidecar | 4 平台（darwin ×2、windows ×2；脚本另支持 linux ×2） | `src-tauri/binaries/` 现含 **6 个产物**（darwin/linux/windows 各 ×2，合计约 401MB） | ✅ 已同步至 `AGENTS.md` |
| 版本落点 | 表格未列 `src-tauri/Cargo.toml` | `Cargo.toml` 已带 `version = "1.0.0"` | ✅ 已同步至 `AGENTS.md`（基准表已补充壳工程落点） |

> 说明：以上三项是迁移前的复核结论；**97 项 Node 测试基线与 6 个 sidecar 产物均已随迁移失效**（前者归属归档在仓库外的 JS 核心，后者目录已删除）。

#### 四·2 迁移后（2026-10-01 实读，待由用户同步进 `AGENTS.md`）

| 项 | 当前实读值 | 来源 |
|---|---|---|
| 测试基线 | **207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9）；壳 `cargo test --lib` 5 通过 | `src-tauri/core/` 下 `cargo test`（根 `npm test` 转发同一命令） |
| Lint 门禁 | 核心 `cargo clippy --all-targets` **0 warning**；src-tauri `cargo clippy --no-deps` **0 warning**；`cargo fmt --check` 全仓不干净（非门禁） | 本次实跑 |
| 版本单一来源 | 根目录 `package.json`（**1.0.0**），`scripts/check-version.mjs` 校验 5 处落点 | 根 `package.json` / `scripts/check-version.mjs` |
| 核心形态 | Rust crate `wbbridge-core`（`src-tauri/core/`，内部版本 **0.1.0**，与产品版本有意解耦）静态链接进壳；无 sidecar / pkg / `src-tauri/binaries/` | `src-tauri/core/Cargo.toml`、`src-tauri/{Cargo.toml,tauri.conf.json,src/lib.rs}` |
| 已构建 sidecar | **不适用**：`src-tauri/binaries/` 已删除，`bundle.externalBin = []` | `src-tauri/tauri.conf.json` |
| Node 定位 | 仅 Vite 面板构建 + 两个 `.mjs` 版本脚本；`engines.node >= 24` | 根 `package.json`、`vite.config.js`、`scripts/` |

### 五、版本纪律沿用声明

- 自本版本起，wbBridge 的版本迭代日志统一收录于 `docs/version/`，规则见 [README.md](README.md)；
- 版本号**仅在**「不同类新功能 / 不同根因新修复 + 用户明确允许」时末位 +1；
- 同一问题多轮往返、同日同模块追加修复、仅文档更新、纯文案/措辞打磨等场景**禁止**推进版本号，但仍需在本文件顶部追加分节留痕（注明日期与"不推进版本号"）；
- 历史分节**只增不改**。
