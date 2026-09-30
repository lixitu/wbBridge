# 版本发布日志 · v1.0

> 本文件按主版本组织：v1.0.x 的全部迭代日志集中于此（最新在前）。
> 命名规则：`RELEASE-v{主版本}.md`；次版本迭代追加到文件顶部新分节。
> 版本纪律以根目录 `AGENTS.md`「当前基准版本」章节为准，本文件不另立规则。
> 整理规则：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本保留编号与合并指向，不再重复正文。
> 当前最新版本：**v1.0.0**。

---

## v1.0.0 · 📝 待发布

> **状态**: 📝 待发布（首个基线版本；核心 sidecar 与静态控制面板已入库并完成构建，`src-tauri/` 壳脚手架（`Cargo.toml` / `build.rs`）已入库但主源码、`tauri.conf.json`、CI 与签名发布链路仍未入库，尚无对外分发的安装包）
> **发布日期**: 待定（记为 2026-09-30 基线快照日）
> **上一版本**: 无（v1.0 首个分节）
> **版本范围**: 项目首个基线版本——Tauri 托盘壳 + 独立 Node sidecar 核心的双层桥接工具，面向 WorkBuddy 提供隔离托管的 OpenCode 免费模型服务
> **版本号说明**: 本次仅新增 `docs/version/`（本文档 + `README.md`），属纯文档更新，按版本纪律**不推进版本号**，仍按约定建分节留痕

---

### 一、版本号基线核验（实读，2026-09-30）

| 项 | 值 | 来源 |
|---|---|---|
| 版本单一来源 | **1.0.0** | `core/package.json` 的 `version`（`name = wbbridge-core`，`private: true`，`type: module`） |
| 壳工程同步落点 | **1.0.0** | `src-tauri/Cargo.toml` 的 `[package] version`（`name = wbbridge`，`edition = 2021`，`rust-version = 1.77`，`tauri = "2.9"`） |
| 状态内置版本 | `0.2.0` | `core/src/main.js`（第 39 行 `state.version`）写入 `status.json`，沿自上游参考实现（参考 https://github.com/louchi1984-coder/ow-bridge），历史沿革值，界面上可见 |
| 上游调研基线 | `0.2.5` | `docs/research/upstream-architecture.md`（该文档同时记录上游 `package.json` 0.2.5 与 `src/main.js` 内嵌 0.2.0 不一致的事实） |
| 运行时要求 | Node `>= 22`；OpenCode 运行时版本由 registry 最新版决定（不固定） | `core/package.json` 的 `engines.node`、`core/src/runtime.js` |
| 根目录 `package.json` | **不存在** | 根目录实读；所有 npm 命令在 `core/` 下执行 |
| `tauri.conf.json` | **不存在** | 全仓搜索（排除 `node_modules`）无命中 |
| CI workflow | **不存在** | `.github/workflows/` 为空目录，无任何文件 |

### 二、代码与资产快照（实读核验）

- **`core/src/`（16 个 ESM 模块）**：`main.js`（编排/生命周期）、`server.js`（HTTP 路由）、`protocol.js`（校验/`BridgeError`）、`runtime.js`（OpenCode 运行时下载与校验）、`backend.js`、`probe.js`（模型探测）、`model-status.js`、`sync.js`（`models.json` 同步、原子写）、`workbuddy-config.js`、`handoff.js`、`reasoning.js`、`repair.js`、`atomic.js`、`json.js`、`platform.js`、`system-proxy.js`
- **构建产物**：`core/dist/`（`core.mjs` 等），由 `@yao-pkg/pkg` 打包为单文件可执行 sidecar
- **打包脚本**：`scripts/build-sidecar.mjs`
- **`src-tauri/binaries/`（6 个已构建 sidecar 产物，共约 401 MB）**：
  - `wbbridge-core-aarch64-apple-darwin`、`wbbridge-core-x86_64-apple-darwin`
  - `wbbridge-core-aarch64-pc-windows-msvc.exe`、`wbbridge-core-x86_64-pc-windows-msvc.exe`
  - `wbbridge-core-aarch64-unknown-linux-gnu`、`wbbridge-core-x86_64-unknown-linux-gnu`
  - 上述二进制为构建产物，`.gitignore` **未忽略**该目录，入库或分发方式需用户确认（与 `AGENTS.md` 工程红线一致）
- **`src-tauri/`**：仅 `Cargo.toml`、`build.rs`、`icons/`（15 个图标文件，含 `icon.icns` / `icon.ico` / `tray.png`）、`binaries/`；**无 `src/` 目录**（Rust 壳主源码未入库）
- **`ui/`**：`index.html`、`renderer.js`、`style.css`、`activity.cjs`——无构建纯静态面板，遵守 CSP（`default-src 'self'`、`script-src 'self'`、`connect-src 'none'`），经 `window.buddy.*` IPC 与壳通信
- **`core/test/`**：11 个测试文件（`activity` / `atomic` / `bridge` / `lifecycle` / `platform` / `repair` / `runtime` / `shutdown` / `system-proxy` / `watchdog` / `workbuddy-config` 各 `*.test.js`）+ `fixtures/`（`runtime.mjs` / `runtime-loader.mjs`），基于 Node 内置 `node:test`
- **测试基线（本次实读执行）**：`core/` 下 `npm test`（node v24.21.0）→ **97 通过 / 0 失败**，耗时 ≈ 5.3 s
- **文档与品牌资产**：`AGENTS.md`（项目规范，含「当前基准版本」章节）、`README.md`（项目定位）、`docs/brand/`（图标源文件与渲染脚本）、`docs/research/upstream-architecture.md`（上游调研）
- **仓库状态**：分支 `dev`，远端 `origin = https://github.com/trexwb/wbBridge.git`（另有 `main`）；`git ls-files` 显示当前**仅 `README.md` 被跟踪**（`8c30d2f Initial commit`，2026-09-30 10:43:48 +0800），其余文件均未跟踪

### 三、尚未入库 / 未实现项（不得伪装成已实现）

1. **Tauri 壳源码**：`src-tauri/` 仅有 `Cargo.toml` 与 `build.rs` 脚手架，**无 `src/` 主源码**；
2. **`tauri.conf.json`**：不存在；
3. **CI / 自动化发布**：`.github/workflows/` 为空目录；
4. **签名与分发链路**：无可执行签名/打包/更新通道，**无对外分发安装包**；
5. 因此本版本**不具备"已发布"条件**，状态标记为 📝 待发布。

### 四、与 `AGENTS.md` 记录的差异（已于 2026-09-30 复核同步）

| 项 | `AGENTS.md`「当前基准版本」记录 | 本次实读核验 | 处理建议 |
|---|---|---|---|
| 测试基线 | 97 通过 / 0 失败（11 个测试文件） | **97 通过 / 0 失败**（11 个测试文件，`core/` 下 `npm test`，node v24.21.0） | ✅ 已同步至 `AGENTS.md` |
| 已构建 sidecar | 4 平台（darwin ×2、windows ×2；脚本另支持 linux ×2） | `src-tauri/binaries/` 现含 **6 个产物**（darwin/linux/windows 各 ×2，合计约 401MB） | ✅ 已同步至 `AGENTS.md` |
| 版本落点 | 表格未列 `src-tauri/Cargo.toml` | `Cargo.toml` 已带 `version = "1.0.0"` | ✅ 已同步至 `AGENTS.md`（基准表已补充壳工程落点） |

> 说明：以上三项均已按本目录实读结果同步至 `AGENTS.md`（2026-09-30 复核）；`AGENTS.md`「当前基准版本」章节仍是版本纪律的唯一权威表述。

### 五、版本纪律沿用声明

- 自本版本起，wbBridge 的版本迭代日志统一收录于 `docs/version/`，规则见 [README.md](README.md)；
- 版本号**仅在**「不同类新功能 / 不同根因新修复 + 用户明确允许」时末位 +1；
- 同一问题多轮往返、同日同模块追加修复、仅文档更新、纯文案/措辞打磨等场景**禁止**推进版本号，但仍需在本文件顶部追加分节留痕（注明日期与"不推进版本号"）；
- 历史分节**只增不改**。
