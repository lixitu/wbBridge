# 版本发布日志 · v1.0

> 本文件按主版本组织：v1.0.x 的全部迭代日志集中于此（最新在前）。
> 命名规则：`RELEASE-v{主版本}.md`；次版本迭代追加到文件顶部新分节。
> 版本纪律以根目录 `AGENTS.md`「当前基准版本」章节为准，本文件不另立规则。
> 整理规则：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本保留编号与合并指向，不再重复正文。
> 当前最新版本：**v1.0.0**。

---

## v1.0.0 · 📝 待发布

> **状态**: 📝 待发布（核心已用 Rust 重写并静态链接进完整入库的 Tauri 壳，面板为 Vue 3 + Vite 构建；但迁移后**未产出过安装包、桌面 GUI 未实机启动、`release.yml` 未在 CI 运行**，签名链路亦未实际执行，尚不具备"已发布"条件）
> **发布日期**: 待定（记为 2026-09-30 基线快照日；2026-10-01 因 Node → Rust 迁移对本节的仓库形态与测试基线记录做更正）
> **上一版本**: 无（v1.0 首个分节）
> **版本范围**: 项目首个基线版本——Tauri 托盘壳 + **Rust 核心（crate `wbbridge-core`，path 依赖静态链接）** 的桥接工具，面向 WorkBuddy 提供隔离托管的 OpenCode 免费模型服务；Node.js 仅用于 Vite 构建面板与两个版本号脚本，**不存在 sidecar / pkg / `src-tauri/binaries/`**
> **版本号说明**: 本次仅新增 `docs/version/`（本文档 + `README.md`），属纯文档更新，按版本纪律**不推进版本号**，仍按约定建分节留痕；2026-10-01 的核心迁移更正同样**不推进版本号**（产品版本保持 1.0.0）

> ⚠ **2026-10-01 迁移更正说明**：本节下方「一、版本号基线核验」「二、代码与资产快照」「三、尚未入库/未实现项」最初按 Node/sidecar 形态记录，现已按迁移后的真实仓库状态更正；原记录中的 **97 通过 / 0 失败**（`node --test`）属于**已归档的 JS 核心**（连同其测试移到仓库外 `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`），不再是当前基线。当前基线：`cargo test`（`src-tauri/core/`）**197 通过 / 0 失败**（2026-10-01 同日两轮——迁移复验轮 **195**（lib 177），全量代码审查修复轮 **197**（lib 179），详见 `docs/validation.md`）。

---

### 一、版本号基线核验（实读，2026-10-01 更正）

| 项 | 值 | 来源 |
|---|---|---|
| 版本单一来源 | **1.0.0** | 根目录 `package.json` 的 `version`（`name = wb-bridge`，`private: true`，`type: module`，`engines.node >= 24`；Node 只服务面板构建与版本脚本） |
| 壳工程同步落点 | **1.0.0** | `src-tauri/tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `[package] version`（壳 `name = wbbridge`，`edition = 2021`，`rust-version = 1.77`，`tauri = "2"`） |
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
- **`src-tauri/`**：`Cargo.toml`（`name = wbbridge`、`version = 1.0.0`、`tauri = "2"`）、`tauri.conf.json`、`build.rs`、`src/{main.rs,lib.rs}`、`capabilities/`、`icons/`、`updater-signing.env.example`。`lib.rs` 负责：在专用 tokio 运行时上 `orchestration::run(StartOptions { data_dir, port, handle_signals: false })` 装配核心、`pick_port()`（默认 41980，占用则回退系统分配端口）、`app.path().app_data_dir()` 注入数据目录、`admin_call(port, key, "/admin/…")` + `ADMIN_ROUTES`/`admin_route()` 驱动动作、轮询 `status.json` 并 emit `core-status` / `core-failed`、`orchestration::set_exit_hook` 保证核心不能反向终止壳；暴露 Tauri 命令 `core_action` / `restart_core` / `core_running` / `data_dir_path`
- **`src/`（面板，Vue 3 + Vite）**：`index.html`、`main.js`、`App.vue`、`views/*.vue`、`components/ModelRow.vue`、`styles/{variables,base}.css`、`public/`；与壳的唯一边界是 **`src/core/bridge.js`**（导出 `action(name, value)` / `onState(cb)` / `onDismiss(cb)` / `activityText`，包装 Tauri `invoke` + `listen`）与 `src/core/activity.js`。⚠ `src/core/` 现在是**前端内核**目录，与已归档的 Node 后端无关；构建产物在根 `dist/`（`vite.config.js`：`root: src`、`outDir: ../dist`、dev 端口 41990）
- **`scripts/`**：仅 `bump-version.mjs`、`check-version.mjs`（版本号的设定与 5 处一致性校验）；`vite.config.js` 在根目录
- **测试基线（本次实读执行）**：`src-tauri/core/` 下 `cargo test` → **197 通过 / 0 失败**（lib 单测 **179** + `tests/js_parity.rs` **11** + `tests/red_lines.rs` **7**）；`cargo clippy --all-targets`（核心）与 `cargo clippy --no-deps`（src-tauri）均 **0 warning**。`tests/js_parity.rs` 现与冻结在 `src-tauri/core/tests/fixtures/*.json` 的 JS 真相快照对拍（**271 例 / 11 个 fixture 模块**），**不再需要 Node**；重新录制需恢复归档 JS 源码并 `WB_PARITY_RECORD=1 cargo test --test js_parity`
- **旧 Node 基线的归属**：迁移前的 **97 通过 / 0 失败**（`node --test`）属于**已归档的 JS 核心**，源码与测试整体移出仓库到 `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`（内含 `core/src/*.js` 16 个模块、`core/test/*.test.js` 12 个测试文件——其中 `contract.test.js` 断言的「三处动作表一致」门禁没有随迁移保留，见 `docs/contract.md`；另含用于重新录制对拍快照的 `core-rs-tests-js/`）；仓库工作区内已不存在 Node 版 `src/core/` 后端与 `src/ui/`，只剩同名不同物的前端内核 `src/core/bridge.js` / `activity.js`
- **文档与品牌资产**：`AGENTS.md`（项目规范，含「当前基准版本」章节）、`README.md`、`docs/{contract,validation}.md`、`docs/version/`、`docs/research/upstream-architecture.md`（上游调研）；原品牌资产目录 **`assets/brand/`**（自 `docs/brand/` 搬来，含 `logo*.svg`、`tray*`、`render.mjs`、`build-icons.mjs` 及其独立 `package.json`）已于 **2026-10-01 按方案 C 彻底删除**，相关引用（`package.json` 的 `icons` 脚本、`eslint.config.js` ignore、`.gitignore` 两行规则、`AGENTS.md`/`README.md` 等文档说明）已同步清理；`src-tauri/icons/` 内的全平台图标产物保留，构建与运行不受影响
- **仓库状态（`git` 只读核验，2026-10-01）**：分支 `dev`，远端 `origin = https://github.com/trexwb/wbBridge.git`（另有 `main`），HEAD = `d0cdf0b docs(README): 更新文档包含完整项目说明和使用指南`（其上为 `8c30d2f Initial commit`）。索引里仍是**迁移前布局**：`src/core/**`（30 个 Node 核心文件）与 `src/ui/**`（6 个旧静态面板文件）在 HEAD 中被跟踪、在工作区已删除；`src-tauri/`（`Cargo.toml`/`Cargo.lock`/`build.rs`/`tauri.conf.json`/`src/{main,lib}.rs`/`capabilities/`/`icons/`）、根 `package.json`、`README.md`、`AGENTS.md`、`.github/workflows/release.yml`、`.gitignore`、`docs/`、`scripts/build-sidecar.mjs` 已入库。**尚未 `git add` 的新文件**：`src-tauri/core/`（Rust 核心整体）、`src/{index.html,main.js,App.vue,views/,components/,core/,styles/,public/}`（Vue 面板；`src/core/` 现仅存前端内核 `bridge.js` / `activity.js`，与 HEAD 里被删的旧 `src/core/*.js` 同名不同物）、`vite.config.js`、`eslint.config.js`、`.editorconfig`；**已删除待提交**：`scripts/build-sidecar.mjs`、`docs/brand/*`；**已修改待提交**：`src-tauri/*`、根 `package.json`、`.gitignore`、`release.yml`、`AGENTS.md`、`README.md`、`docs/`。全部迁移改动截至本节撰写时均未提交（提交由用户决定）

### 三、尚未验证 / 未完成项（不得伪装成已验证）

1. **桌面 GUI 从未实机启动**：迁移后只验证过编译、clippy 与「独立核心二进制」的一次冒烟（一次性临时数据目录内完成运行时下载、隔离 OpenCode 启动、`/agent` 校验、发现 8 个免费模型并探测、干净关停）；托盘、面板交互、壳侧重启与优雅退出链路未经真实运行验证；
2. **迁移后未产出过安装包**：`src-tauri/target/release/bundle/` 不存在，`.app` / `.dmg` / Windows / Linux 包待重新构建；
3. **CI 未实跑**：`.github/workflows/release.yml` 已按 Rust 形态重写，仅做过本地 YAML 结构校验；
4. **签名与更新链路未执行**：`.env.example` / `src-tauri/updater-signing.env.example` 已就位（`tauri.conf.json` 的 `plugins` 目前为空对象，未配置 updater），`npm run tauri -- signer generate …` 与签名构建均未实际跑过，**无对外分发安装包**；
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
| 测试基线 | **197 通过 / 0 失败**（lib 179 + `js_parity` 11 + `red_lines` 7） | `src-tauri/core/` 下 `cargo test`（根 `npm test` 转发同一命令） |
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
