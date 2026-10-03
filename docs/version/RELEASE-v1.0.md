# 版本发布日志 · v1.0

> 本文件按主版本组织：v1.0.x 的全部迭代日志集中于此（最新在前）。
> 命名规则：`RELEASE-v{主版本}.md`；次版本迭代追加到文件顶部新分节。
> 版本纪律以根目录 `AGENTS.md`「当前基准版本」章节为准，本文件不另立规则。
> 整理规则：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本保留编号与合并指向，不再重复正文。
> 当前最新版本：**v1.0.2**。

---

## v1.0.2 · 📝 待发布

> **状态**: 📝 待发布（改动在工作区，**尚未提交、尚未打标签**；GUI 未实机启动、`release.yml` 未在 CI 跑过、v1.0.2 的安装包未构建）
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
- **`latest.json` 单一写者**：`scripts/gen-latest-json.mjs`（平台键取自产物**目录名**的 target triple，默认六平台缺一即退出 1）+ CI 末尾 `update-manifest` 作业；三个 build 作业的 `tauri-action` 全部 `includeUpdaterJson: false`（6 个并发作业各写一次会互相覆盖）。macOS 更新包由 CI 补架构后缀（tauri 原名 `WB Bridge.app.tar.gz` 不带版本与架构，两个 mac runner 会同名互相覆盖）。
- **发布链路自证（A9）**：CI `test` 作业新增**标签↔版本闸门**（`GITHUB_REF_NAME` 去前导 `v` 必须等于 `tauri.conf.json` 的 `version`，否则 exit 1；`workflow_dispatch` 跳过）；三个 build 作业上传前打印 bundle 文件名与源码版本；`update-manifest` 汇总六平台安装包并把「产物版本 = 标签版本」自检三分类写进 run summary（**advisory warning 而非硬失败**，因非 macOS 产物名仍未实测）。
- **`latest.json` 生成器入库行为基线（A6-7b）**：新增 `scripts/gen-latest-json.test.mjs`（`node --test`，**9 通过**，命令 `npm run test:manifest`），用 `mkdtemp` 临时产物目录跑**真脚本**，钉住「平台键只由 artifact 目录名的 target triple 决定」以及四类必须失败的路径（缺平台 / 缺 `.sig` / mac 资产名漏架构后缀 / 同一平台多个候选目录或互不相干的已签名包），并覆盖 Linux 双层打包时优先取 `.tar.gz` 的降级口径。此前这些结论只在一次性手写夹具里验证过、**未入库**，回归无从保护；现在这类错误的暴露点从「用户装上才发现平台缺席」提前到 CI。测试**不联网**、不读 `src-tauri/target/` 下的真实产物。
- **面板偏好持久化（A8 + A6-7）**：新增 `src/core/prefs.js`——`localStorage` 键前缀 `wb.`、**写入前键白名单校验** + 值字段投影 + 序列化后 2KB 上限、存储不可用/抛错一律静默降级为默认值；`App.vue` 持久化当前视图，`update.js` 持久化「启动后自动检查更新」开关与上次**成功**检查时间戳（12 小时节流），`AboutView.vue` 提供该开关。凭据类字段（`api-key`、`OPENCODE_SERVER_PASSWORD`、models 文件路径）在设计上不可能进入 `localStorage`，由 `src/core/prefs.test.js`（`node --test`，**8 通过**）钉住，命令 `npm run test:prefs`。
- **文档（A5 / A7 / A10）**：`.env.example` 写清三个签名环境变量的真实分工（`tauri build|bundle` 只读 `TAURI_SIGNING_PRIVATE_KEY`，值可为私钥全文或**绝对路径**）；`docs/wiki/版本与发布.md` 新增「商业签名与公证（A7 预留位，尚未接入）」「失败路径与回滚」「标签纪律」三节并扩充发布前检查清单；`docs/wiki/常见问题与故障排查.md` 补更新失败/回滚/偏好开关/Gatekeeper 四条；`docs/wiki/已知限制与未验证项.md` 按实测状态更正未验证项与已知限制表。

### 三、基线与验证边界（本版真实执行）

- 核心 `cargo test` → **207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9）；核心 `cargo clippy --all-targets` → **0 warning**（`touch src/lib.rs` 强制重检后仍为 0）。
- 壳 `cargo test --lib` → **9 通过 / 0 失败**；壳 `cargo clippy --no-deps --all-targets` → **0 warning**（版本改写触发重新检查，无告警）。
- `npm run test:prefs` → **8 通过 / 0 失败**；`npm run test:manifest` → **9 通过 / 0 失败**；`npx eslint .` → **0 problem**；`npm run vite:build` → ✓ built；`npm run version:check` → 5 处一致（1.0.2）；`release.yml` YAML 解析通过。两组 JS 测试都已接入 CI 的 `test` 作业。
- 本机签名构建（发生在 `1.0.1` 源码版本上，产物名因此带 `1.0.1`）：`bundle/dmg/WB Bridge_1.0.1_aarch64.dmg`（3,720,766 B）+ `bundle/macos/WB Bridge.app.tar.gz`（3,595,514 B）与配对 `.sig`，key ID 与配置公钥一致；dmg 只读挂载核对、`codesign --verify --deep --strict` 通过（adhoc + hardened runtime，**未公证**）。
- ❌ **未验证**：GUI 实机启动与面板交互、v1.0.2 的任何安装包、`release.yml` 在 CI 实跑、`latest.json` 发布后被真实客户端消费、一次完整的「检查 → 下载 → 安装 → 重启 → 首启」与失败回滚、`relaunch` 后带新核心的重启、`localStorage` 在真实 WebView 内的行为、Windows/Linux 产物名与签名、GitHub Secrets 是否已配（本机无 `gh`、未联网核验）。

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
- ⚠️ **仍无单测的项**：新补的 `status_read_needed_only_skips_when_mtime_is_known` 只覆盖快路径**判定**；`watch_status` 整条循环仍需真实 `AppHandle`，面板帧合并也没有 JS 单测（JS 侧仍只有 `test:prefs` 8 + `test:manifest` 9 两套）。
- **验证（2026-10-03 复审后复跑）**：核心 `cargo test` **207 通过 / 0 失败**、壳 `cargo test --lib` **9 通过 / 0 失败**（8 → 9，新增上述快路径判定）、核心与壳 `cargo clippy` **0 warning**、`npm run test:prefs` **8 通过**、`npm run test:manifest` **9 通过**、`npx eslint .` **0 problem**、`npm run vite:build` ✓ built（`dist/assets/index-*.js` 100.87 kB / gzip 37.95 kB）、`npm run version:check` **5 处一致（1.0.2）**。细节见 `docs/validation.md` 2026-10-03 条目与 [`RELEASE-NOTES-v1.0.2.md`](RELEASE-NOTES-v1.0.2.md)。
- ⚠️ 渲染 / 轮询收益仍需实机确认：GUI 从未启动，rAF 在 WKWebView 中最小化 / `hide()` 下的真实停摆行为未实测。

---

## v1.0.1 · 📝 待发布

> **状态**: 📝 待发布（改动已在工作区，**尚未提交、尚未打标签**；GUI 未实机启动、`release.yml` 未在 CI 跑过、无安装包产出）
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

## v1.0.0 · 📝 待发布

> **状态**: 📝 待发布（核心已用 Rust 重写并静态链接进完整入库的 Tauri 壳，面板为 Vue 3 + Vite 构建；但迁移后**未产出过安装包、桌面 GUI 未实机启动、`release.yml` 未在 CI 运行**，签名链路亦未实际执行，尚不具备"已发布"条件）
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
6. 因此本版本**不具备"已发布"条件**，状态标记为 📝 待发布。

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
