# 版本迭代日志

> 本目录专门存储 wbBridge 每次升级迭代的发布日志。
> 命名规则：`RELEASE-v{主版本}.md`（如 v1.0.x → RELEASE-v1.0.md，v1.1.x → RELEASE-v1.1.md）
>
> GitHub Release 正文按完整版本单独成文：`RELEASE-NOTES-v{完整版本}.md`（如 `RELEASE-NOTES-v1.0.1.md`），内容可直接复制到 Release body。
>
> **版本纪律（与根目录 `AGENTS.md`「当前基准版本」章节严格对齐）**：
> - **版本单一来源**：根目录 `package.json` 的 `version`（当前 **1.0.1**）；`src-tauri/tauri.conf.json`
>   与 `src-tauri/Cargo.toml` 的 `[package] version` 是同步落点（当前同为 **1.0.1**）
> - `src-tauri/core/Cargo.toml` 的内部 crate 版本（当前 **0.1.0**，crate `wbbridge-core`）与产品版本**有意解耦**：
>   它是库自身的演进节奏，**不是**版本号落点，`version:check` 不校验它，也不随产品版本递增
> - 版本号末位仅在「不同类新功能 / 不同根因新修复 + 用户明确允许」时 +1
> - **绝对禁止推进版本号的场景**：同一问题多轮往返跟进、同日同模块追加修复、用户明确要求不改版本号、仅文档更新（`docs/`、`README.md`、`AGENTS.md` 等）、纯文案/注释/日志措辞/去抖体验打磨
> - `status.json` 中由 `src-tauri/core/src/orchestration.rs` 写入的 `version: "0.2.0"`（历史沿革值，另有
>   `schemaVersion: 1` 描述快照结构）为**历史沿革值**，**不得在无用户指令的情况下静默改动**
> - 用户要求版本回退时，根 `package.json`、`AGENTS.md`「当前基准版本」、本目录索引（以及
>   `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）必须同时回退到用户指定值
> - **即使不推进版本号，也必须追加发布日志**：对应主版本文件顶部新分节，注明日期与"不推进版本号"；历史日志只增不改

---

## 日志索引

| 文件 | 覆盖版本 | 状态 |
|------|---------|------|
| [v1.0 日志](RELEASE-v1.0.md) | v1.0.1（当前基准版本）＋ v1.0.0 基线快照，最新在前 | 📝 待发布 |
| [v1.0.1 GitHub Release 正文](RELEASE-NOTES-v1.0.1.md) | v1.0.1 | 📝 待发布（尚未提交、尚未打标签） |

---

## 版本号落点（实读核验，2026-10-01 随 v1.0.1 推进更新）

| 位置 | 当前值 | 说明 |
|------|--------|------|
| 根目录 `package.json` → `version` | **1.0.1** | **版本唯一来源**；`name = wb-bridge`、`private: true`、`type: module`、`engines.node >= 24`。根级脚本：`test`（`cargo test --manifest-path src-tauri/core/Cargo.toml`）/`rust:check`/`lint`（仅 eslint）/`dev`（`tauri dev`）/`vite:dev`/`vite:build`/`build`/`tauri`/`tauri:dev`/`tauri:build`/`version:set`/`version:check`。Node 在此只服务面板构建与版本脚本 |
| `src-tauri/tauri.conf.json` → `version` | **1.0.1** | 打包与更新元数据读此值；`productName = WB Bridge`、`identifier = app.wbbridge.desktop`、`frontendDist = ../dist`、`bundle.externalBin = []`（核心已静态链接，无 sidecar）；窗口默认 **1120 × 720**、最小 **860 × 560**（2026-10-01 面板布局改造后） |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.1** | 壳工程侧同步落点（`name = wbbridge`，`rust-version = 1.77`，`tauri = "2"`）；已在 `AGENTS.md`「当前基准版本」表登记 |
| `src-tauri/core/Cargo.toml` → `[package] version` | `0.1.0` | **内部库 crate 版本，有意与产品版本解耦**（crate `wbbridge-core`，`rust-version = 1.75`，`publish = false`），不是版本落点、不被 `version:check` 校验、不随产品版本递增 |
| `src-tauri/core/src/orchestration.rs`（`status.json` 内置） | `0.2.0` | 历史沿革值，沿自上游参考实现（参考 https://github.com/louchi1984-coder/ow-bridge），界面上可见；同处另写 `schemaVersion: 1`；非版本来源 |
| `src/core/bridge.js` 等面板代码 | 无版本字面量 | 面板为 **Vue 3 + Vite** 源码（`src/` → 构建到 `dist/`），版本号由 `vite.config.js` 在构建期从 `package.json` 注入 `__APP_VERSION__`；`src/core/` 是**前端内核**（IPC 边界），与已归档的 Node 后端无关 |
| `.github/workflows/release.yml` | **已写入，未在 CI 实跑** | 六平台构建工作流已按 Rust 形态重写，目前只做过本地 YAML 结构校验；`version:check` 亦在流水线内声明 |

> 版本号一致性由脚本核验：本地 `npm run version:check`（校验 5 处落点：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 与 `AGENTS.md` 的两行基准表）。`release.yml` 的测试 job 也声明了同一脚本，**但该工作流迁移后尚未在 CI 实际运行**，因此"CI 已核验"目前不成立；人工复核仍建议在改动版本号后 `grep -rn "1\.0\.0"` 扫一遍文档（本版已把 `docs/wiki/*`、`README.md`、`docs/version/*` 的产品版本同步为 1.0.1；`RELEASE-v1.0.md` 与 `docs/validation.md` 中残留的 `1.0.0` 属于**历史分节与 v1.0.0 基线口径**，不改写）。

---

## 约定

- **按主版本一个文件**：`RELEASE-v{主版本}.md`，次版本迭代作为分节追加到文件顶部（最新在前）
- **发布前**：在对应主版本文件追加该次版本分节；仅当 `AGENTS.md`「当前基准版本」允许的唯一场景出现时，才同步递增版本号各落点
- **发布后**：分节状态标记 ✅ 已发布；本目录内已发布内容只增不改（历史日志不可篡改）
- 待发布内容先以 📝 待发布 状态记录，发布时更新状态与日期
- **当前发布状态说明（2026-10-01 更新）**：本目录建立时（2026-09-30）仓库**尚无对外分发的安装包**——当时壳源码、`tauri.conf.json` 与 CI 都未入库。现壳与核心都已在仓库工作区内（`src-tauri/src/lib.rs` 持有核心生命周期、`tauri.conf.json`、`capabilities/`、icons 齐全；核心为 `src-tauri/core/` 的 Rust crate，静态链接进壳），面板改为 Vue 3 + Vite 构建（`src/` → `dist/`），签名环境变量模板（根 `.env.example`；`src-tauri/updater-signing.env.example` 已删除）与 `@tauri-apps/cli` 也就位；`git ls-files` 已有 **103** 个被跟踪文件（含 `src-tauri/core/` 与 Vue 面板），且迁移后**没有产出过安装包、桌面 GUI 没有实机启动、`release.yml` 未在 CI 跑过**，故 v1.0.0 与 v1.0.1 分节状态均为 📝 待发布；待提交 + 实机冒烟 + 产出安装包后再改标 ✅ 已发布
- **去重整理（沿用参考项目 discipline）**：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本分节保留编号与合并指向（不删版本号、不重复正文）
- **文档类更新不推进版本号**：本目录建立（`docs/version/README.md` + `RELEASE-v1.0.md`）与本次 Node → Rust 迁移后的文档更正均属纯文档更新，按纪律**不推进版本号**（当时产品版本保持 **1.0.0**），只在对应分节留痕（见 `RELEASE-v1.0.md` v1.0.0 分节末条）。2026-10-01 第八轮**引入不同根因的安全/并发修复与面板新视图，且用户明确要求推进版本号**，遂 `1.0.0 → 1.0.1`（`npm run version:set -- 1.0.1`，5 处落点一致）
- **界面调整同样不推进版本号**：2026-10-01 的面板布局改造（侧栏分组导航、详情改为右侧常驻分栏、默认窗口 980×680 → **1120×720**、侧栏 `--sidebar-w` 224 → **208px**、新增 `--muted-strong`；该轮留下的 4 个「规划中」入口已在同日第四轮实现为只读视图，同样不推进版本号）属同一未发布版本内的界面调整，**不推进版本号**；已在 `RELEASE-v1.0.md` 的 v1.0.0 分节以「面板布局改造说明」留痕，验证证据见 `docs/validation.md` 同轮条目
