# WB Bridge v1.0.0 发布说明

> 本文件面向 GitHub Release 页面直接粘贴使用，内容全部基于仓库源码与文档实证（2026-10-01 基线）。
> 与 `docs/version/RELEASE-v1.0.md`（内部版本日志）互为补充：本文件是发布页文案，日志文件是版本留痕。

---

## 版本概述

**WB Bridge v1.0.0** 是项目的**首个基线版本**。它是一个基于 Tauri 2 的**跨平台托盘应用**，通过**隔离的 OpenCode 运行时**为 [WorkBuddy](https://www.workbuddy.cn) 提供免费模型服务。

一句话定位：**本地跑一个不污染你现有环境的 OpenCode，把探测通过的免费模型以 OpenAI 兼容接口喂给 WorkBuddy。**

> **发布状态说明（如实标注）**：核心已用 Rust 重写并静态链接进 Tauri 壳，面板为 Vue 3 + Vite 构建，二者均已入库；但**迁移后尚未产出过安装包、桌面 GUI 未实机启动、`release.yml` 未在 CI 实际运行、签名链路未执行**。本条为基线快照发布，功能可用性以"独立核心二进制冒烟 + 单元/对拍/红线测试"为证据，**GUI 交互未经实机验证**。

---

## 核心能力

| 能力 | 说明 |
|---|---|
| **免费模型发现与检测** | 自动发现 OpenCode 免费模型，向每个模型发送简短真实请求，检测**可用性与工具调用能力**（会消耗少量免费额度），**只发布检测通过的模型** |
| **转换为 OpenAI 兼容接口** | 本地 `127.0.0.1:<port>` 提供 `/v1/models` 与 `/v1/chat/completions`（含 **SSE 流式**），强制 `Authorization: Bearer <api-key>` 鉴权，拒绝一切浏览器 Origin，最多 **4 个并发请求** |
| **隔离托管 OpenCode 运行时** | 优先复用本机已有 `opencode`，否则从 npm registry（官方源 → npmmirror 镜像）下载官方 tarball，校验 `sha512-` 完整性后解包到数据目录，以**隔离环境变量 + 独立随机端口** `serve --pure` 启动，**绝不污染用户 OpenCode 的配置、数据、缓存与登录态** |
| **导入 / 同步 WorkBuddy** | 自动定位并导入 WorkBuddy 的 `models.json`；写入时**原子写 + 增量合并**，只增删本应用名下的条目，写入前备份，退出时清理，**保留用户手动配置** |
| **系统代理** | 托盘菜单「使用系统代理」开关，控制运行时下载与模型请求是否走系统 HTTP/HTTPS 代理 |
| **托盘常驻** | 关闭窗口后常驻托盘；托盘菜单为「打开控制面板 / 使用系统代理 / 选择 WorkBuddy 配置… / 退出 WB Bridge」，左键点击托盘图标显示主窗口；从托盘退出时优雅停止服务并完成配置清理 |
| **生命周期安全** | 核心是静态链接进壳的 Rust 库，壳可重启核心，核心任何退出路径只能触发回调、**绝不能终止壳进程**；单独运行核心可执行文件时，`BUDDY_PARENT_PID` 父进程看门狗会在父进程消失后自行优雅退出，不留占端口的孤儿进程 |

---

## v1.0.0 关键变更

### 1. 核心由 Node 迁移为 Rust 并静态链接进壳

- 后端核心为 Rust crate `wbbridge-core`（`src-tauri/core/`，16 个 lib 模块 + 独立入口），作为 path 依赖**静态链接进 Tauri 壳**，编排层 `orchestration.rs` 等价于原 `main.js`。
- **不再有 sidecar**：`src-tauri/tauri.conf.json` 的 `bundle.externalBin` 为 `[]`，`src-tauri/binaries/` 目录已删除，不再分发伴随可执行文件；用户**无需安装 Node**即可运行应用（Node 仅用于 Vite 构建面板与两个版本号脚本）。
- 原 Node 核心（`core/src/*.js` 与其 97 项测试）已整体归档到仓库之外；Rust 核心通过冻结在 `src-tauri/core/tests/fixtures/*.json` 的 JS 真相快照（**271 例 / 11 个模块**）保持行为一致，**对拍测试不再需要 Node 环境**。
- 新增 `POST /admin/shutdown` 管理接口与 `BUDDY_PARENT_PID` 父进程看门狗：Windows 下 SIGTERM 不可靠、壳可能被强杀，两者共同保证退出时总能优雅清理 WorkBuddy 配置。
- 壳侧暴露 Tauri 命令 `core_action` / `restart_core` / `core_running` / `data_dir_path`，并轮询 `status.json` 推送 `core-status` / `core-failed` 事件；服务端口默认 **41980**，被占用时回退系统分配端口。

### 2. 界面改造（本轮）

- **模型详情改为右侧常驻分栏**：不再是"点开模型行就地展开"，选中模型后**列表与详情并排显示**；详情宽 `clamp(300px, 45%, 360px)`，**可收起**（`Esc` / 详情头部「收起详情」按钮 / 窗口失焦**三条等价路径**），**无遮罩层、不覆盖列表**。
- **删除窄屏上下堆叠降级**：`App.vue` 移除 `<900px` 堆叠逻辑，**任何窗口宽度都不降级为上下堆叠**。
- **默认窗口尺寸调整**：由 980×680 调整为 **1120×720**，最小尺寸 **860×560**（不变）。
- **侧栏改为分组导航**：宽度 `--sidebar-w` 由 224px 调整为 **208px**，按「模型 / 运行 / 集成 / 其他」分组，底部为运行设置。（布局改造当时另有 4 个入口是禁用占位，同日稍后的「侧栏 4 入口实现」轮已把它们做成可点的只读视图，见下节「界面与交互」。）
- `styles/variables.css` 新增 `--muted-strong` 变量。

> 以上界面改造属同一未发布版本的界面调整，按版本纪律**不推进版本号**，产品版本保持 **1.0.0**。验证证据见 `docs/validation.md`「面板布局改造」——**仅在浏览器引擎内经 CDP 实测**（1120×720 / 860×560 两档无横向溢出、键盘可达、收起路径生效、深浅色对比度达标），**非 Tauri GUI 实机**。

### 3. Tauri 2 壳替代 Electron 上游实现

- 安装包体积与内存占用相比 Electron 版显著下降（迁移前 sidecar 版实测 dmg 约 25 MB）。
- 托盘、导入、系统代理、模型检测与自动导入等行为与上游原版一致；界面保留原设计系统并优化了层级、留白、悬停反馈与深浅色主题。
- logo 全新设计（悬索桥 + W 形缆线），全平台图标已就绪（`src-tauri/icons/`）。

---

## 界面与交互

- **左侧栏（208px）**：分组导航「模型 / 运行 / 集成 / 其他」，底部为运行设置。**5 个入口都已实现**：模型与服务（主从两栏）、运行日志、用量与额度、WorkBuddy 集成、关于与更新；后 4 个是**只读视图**（不新增写动作，面板内唯一写动作仍是既有 `import`），无「规划中」标签与禁用占位。
- **主区**：默认单列展示模型列表（含服务状态、指标条与反馈条）。选中模型后主区变为两栏 `minmax(0, 1fr) var(--details-w)`，右侧为**常驻详情分栏**。
- **详情分栏收起**：按 `Esc`、点击详情头部「收起详情」按钮、或窗口失焦，三者等价；收起后回到单列。
- **窗口**：默认 1120×720，最小 860×560；支持深色 / 浅色主题。
- **托盘**：关闭窗口不退出，常驻托盘；托盘菜单可打开控制面板、切换「使用系统代理」、选择 `models.json` 位置、退出应用。

---

## 安装与使用

### 安装

| 系统 | 安装包 | 状态 |
|---|---|---|
| macOS 10.15+（Apple Silicon） | `WB Bridge_1.0.0_aarch64.dmg` | **待重新构建**（迁移后未产出过安装包） |
| Windows 10/11 x64 | `WB Bridge_1.0.0_x64-setup.exe` | CI 构建，待实机验证 |
| Linux x64 | `.AppImage` / `.deb` | CI 构建，待实机验证 |

推送 `v*` 标签后 GitHub Actions 会构建**六平台**（macOS ARM/Intel、Windows x64/ARM、Linux x64/ARM）安装包并发布 Release。

> ⚠ 如实说明：Rust 化后重写的 `.github/workflows/release.yml` **从未在 CI 上实际运行过**，目前只在本地做过 YAML 结构校验；上表构建结论包含迁移前历史记录。

**macOS 首次打开**：应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截，先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

### 使用

1. 先安装并登录 WorkBuddy，在 WorkBuddy 中保存一个自定义模型（生成 `models.json`）。
2. 启动 WB Bridge：自动准备运行时、扫描并检测免费模型，找到有效配置后自动导入。
3. 找不到配置时点「导入 WorkBuddy」选择 `models.json`；Windows 托盘菜单「选择 WorkBuddy 配置…」可更换位置。
4. 「使用系统代理」开关控制运行时下载与模型请求是否走系统 HTTP/HTTPS 代理。

> 数据目录随运行形态不同，排查时不要混用：桌面应用使用 Tauri `app_data_dir`（macOS `~/Library/Application Support/app.wbbridge.desktop`，identifier `app.wbbridge.desktop`）；独立核心二进制使用平台默认目录（macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`），可用 `BUDDY_DATA_DIR` 覆盖。`/health` 同样需要 Bearer 鉴权。

### 从源码构建

依赖：Rust stable（壳 `rust-version = 1.77`、核心 `1.75`）、Node.js **24+**（仅用于 Vite 构建面板与版本号脚本）、各平台 Tauri 系统依赖（Linux 需 webkit2gtk 等）。

```sh
npm install
npm test             # 核心测试：cargo test（207 项 = 187 单测 + 11 JS 对拍 + 9 红线）
npm run rust:check   # cargo check 核心
npm run lint         # eslint .（面板）
npm run dev          # 开发运行（tauri dev）
npm run build        # 桌面应用构建（产物在 src-tauri/target/release/bundle/）
npm run version:check # 校验 5 处版本号落点一致
```

核心也可脱离桌面壳单独运行（同一份编排代码）：

```sh
cargo run --manifest-path src-tauri/core/Cargo.toml
```

---

## 架构与技术栈

三层结构：

| 层 | 位置 | 技术 | 现状 |
|---|---|---|---|
| 核心 | `src-tauri/core/src/`（crate `wbbridge-core`） | Rust + tokio + axum + reqwest + serde_json（`preserve_order`） | 已入库，测试齐全；既可独立成进程，也嵌入壳 |
| 控制面板 | `src/`（Vue 3 SFC）+ `vite.config.js` → 产物 `dist/` | Vue 3 + Vite，无 CDN、无外部请求 | 已入库 |
| 桌面壳 | `src-tauri/`（crate `wbbridge`） | Tauri 2（Rust），托盘 + IPC + 状态轮询 | 已入库，直接 `cargo build` 出成品（无 sidecar） |

- 壳与面板的唯一边界是前端内核 `src/core/bridge.js`（包装 Tauri `invoke` / `listen`）与 `src/core/activity.js`。注意 `src/core/` 是**前端代码**，后端核心在 `src-tauri/core/`。
- 核心链路：运行时准备（`runtime.rs`）→ 模型发现与探测（`probe.rs`）→ 本地 HTTP 服务与鉴权（`server.rs`）→ WorkBuddy 配置同步（`sync.rs` / `workbuddy_config.rs`）→ 系统代理（`system_proxy.rs`）→ 退出清理（`handoff.rs`）。

---

## 已知限制与未验证项

以下条目**必须如实对待，均不代表已验证通过**：

**未验证项**

1. **桌面 GUI 从未实机启动**：迁移后只验证过编译、clippy 与「独立核心二进制」的一次冒烟（一次性数据目录内完成运行时下载、隔离 OpenCode 启动、`/agent` 校验、发现 8 个免费模型并探测、干净关停）；托盘、面板交互、壳侧重启与优雅退出链路**未经真实运行验证**；面板布局改造仅在浏览器引擎内经 CDP 实测。
2. **迁移后未产出过安装包**：`src-tauri/target/release/bundle/` 不存在，`.app` / `.dmg` / Windows / Linux 包**待重新构建**。
3. **CI 未实跑**：`.github/workflows/release.yml` 已按 Rust 形态重写，仅做过本地 YAML 结构校验。
4. **签名与更新链路未执行**：`tauri.conf.json` 的 `plugins` 目前为空对象（未配置 updater），签名生成与签名构建均未实际跑过，**无对外分发安装包**。
5. **Windows ARM64 与 Linux 包**由 CI 构建，**尚未实机验证**。
6. **`cargo fmt --check` 全仓并不干净**（未作为门禁），代码风格以 clippy 0 warning 为准。

**功能限制**

7. **侧栏 5 个入口均已实现**（模型与服务 / 运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新），后 4 个是**只读视图**：运行日志经壳的 `read_log` 只读取日志尾部、用量与额度只渲染 `status.json` 顶层 `usage`、集成视图的唯一写动作仍是既有 `import`；**「关于与更新」未接入自动更新**，升级需重新下载安装包。这些视图**尚未在 Tauri GUI 内实机点击验证**（同"未验证项"第 1 条），面板中不再出现「规划中」标签或禁用占位。
8. **浅色主题对比度遗留问题**：仍有 3 处次要文字（副标题、页脚说明、耗时行）沿用 `--muted`，对比度**低于 WCAG AA 的 4.5:1**；属既有问题，尚未处理。
9. **免费模型名单、额度与可用性由上游 OpenCode 决定**，本应用不控制也不缓存额度。
10. **仅 PAC / SOCKS 代理暂不支持**（与上游原版一致）。
11. **未做商用代码签名 / 公证**：Windows 可能提示未知发布者，macOS 见上方放行说明。

**结论**：本版本**不具备"已发布"条件**，定位为首个基线快照；请勿将其视为已完成实机验证的稳定发行版。

---

## 版本与验证基线

| 项 | 值 | 来源 |
|---|---|---|
| 产品版本（单一来源） | **1.0.0** | 根 `package.json` 的 `version`（`name = wb-bridge`，`private: true`，`engines.node >= 24`） |
| 壳工程同步落点 | **1.0.0** | `src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 的 `[package] version` |
| 核心 crate 内部版本 | `0.1.0` | `src-tauri/core/Cargo.toml`（与产品版本有意解耦，不参与 `version:check`） |
| 状态内置版本 | `0.2.0` | `src-tauri/core/src/orchestration.rs` 写入 `status.json` 的历史沿革值，界面上可见 |
| 版本一致性核验 | 5 处落点 | `npm run version:check`（`scripts/check-version.mjs`），本次复验通过 |
| **测试基线** | **207 通过 / 0 失败** | `src-tauri/core/` 下 `cargo test`：lib 单测 **187** + `tests/js_parity.rs` **11** + `tests/red_lines.rs` **9**；壳 `src-tauri/` 下 `cargo test --lib` 5 通过 |
| Lint 门禁 | 核心 `cargo clippy --all-targets`、壳 `cargo clippy --no-deps` 均 **0 warning** | 本次实跑 |
| 对拍基线 | JS 真相快照 **271 例 / 11 个 fixture 模块**（atomic、handoff、json、model_status、platform、protocol、reasoning、repair、sync、system_proxy、workbuddy_config） | `src-tauri/core/tests/fixtures/*.json`，不再需要 Node |
| 运行时要求 | Rust stable（壳 1.77 / 核心 1.75）；Node >= 24（仅面板构建与版本脚本）；OpenCode 运行时版本由 registry 最新版决定（不固定） | 各 `Cargo.toml`、根 `package.json` |
| 验证记录 | Node/sidecar 时代基线、2026-10-01 Node → Rust 迁移复验、同日面板布局改造三段 | `docs/validation.md` |

完整的版本号基线、代码与资产快照、与 `AGENTS.md` 的差异对照，见 [`docs/version/RELEASE-v1.0.md`](RELEASE-v1.0.md)。
