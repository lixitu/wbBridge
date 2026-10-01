# WB Bridge

基于 Tauri 2 的跨平台托盘应用，通过隔离的 OpenCode 运行时为 [WorkBuddy](https://www.workbuddy.cn) 提供免费模型。功能与界面参考上游实现（https://github.com/louchi1984-coder/ow-bridge，Electron 版）并优化，核心代理逻辑与之保持行为一致。

## 工作方式

- 启动时按需下载官方 OpenCode 运行时（失败自动尝试 npmmirror 镜像，下载后校验版本），以隔离配置启动，不使用用户已有的 OpenCode 数据。
- 自动发现 OpenCode 免费模型，向每个模型发送简短真实请求检测可用性与工具调用能力（会消耗少量免费额度）。
- 在本地 `127.0.0.1` 提供 OpenAI 兼容接口（Chat Completions + SSE），只发布检测通过的模型。
- 将可用模型写入 WorkBuddy 的 `models.json`（只修改本应用拥有的条目，写入前备份；退出时清理，保留用户手动配置）。
- **关闭窗口即退出应用**（macOS / Windows / Linux 行为一致）：壳在窗口关闭请求里走与托盘「退出」完全相同的优雅关停链路（停核心 → 清理 WorkBuddy 配置 → 结束进程），不再驻留托盘、无需用户二次退出。托盘仍在（运行期间可左键唤回面板、切代理、重选配置、退出）。核心是静态链接进壳的 Rust 库（不再是独立进程），生命周期完全由壳掌握：壳可重启核心，核心任何退出路径都只能触发回调而绝不能终止壳进程。单独运行核心可执行文件时，`BUDDY_PARENT_PID` 父进程看门狗会在父进程消失后自行优雅退出，不留占端口的孤儿进程。

## 下载与安装

| 系统 | 状态 | 安装包 |
|---|---|---|
| macOS 10.15+（Apple Silicon） | 迁移前（Node sidecar 版）已本机构建并冒烟；Rust 版尚未产出过安装包，GUI 未实机启动 | `WB Bridge_1.0.1_aarch64.dmg`（待重新构建） |
| Windows 10/11 x64 | CI 构建，待实机验证 | `WB Bridge_1.0.1_x64-setup.exe` |
| Linux x64 | CI 构建，待实机验证 | `.AppImage` / `.deb` |

推送 `v*` 标签后 GitHub Actions 自动构建六平台（macOS ARM/Intel、Windows x64/ARM、Linux x64/ARM）安装包并发布 Release。

> ⚠ 如实说明：核心 Rust 化后重写的 `.github/workflows/release.yml` **从未在 CI 上实际运行过**，目前只在本地做过 YAML 结构校验；下表与上文的历史构建结论均属迁移前记录。

### macOS 首次打开

应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截：先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

## 使用

1. 先安装并登录 WorkBuddy，在 WorkBuddy 中保存一个自定义模型（生成 `models.json`）。
2. 启动 WB Bridge：自动准备运行时、扫描并检测免费模型，找到有效配置后自动导入。
3. 找不到配置时点「导入 WorkBuddy」选择 `models.json`；Windows 托盘菜单「选择 WorkBuddy 配置…」可更换位置。
4. 「使用系统代理」开关控制运行时下载与模型请求是否走系统 HTTP/HTTPS 代理。

界面说明：左侧栏为分组导航（模型 / 运行 / 集成 / 其他，底部为运行设置），5 个入口均可点击并在侧栏高亮当前视图——模型与服务、运行日志、用量与额度、WorkBuddy 集成、关于与更新；点击模型行后详情在**右侧常驻分栏**显示，按 `Esc`、点详情头部「收起详情」或窗口失焦即可收起。

## 从源码构建

依赖：Rust stable（壳 `rust-version = 1.77`、核心 `1.75`）、Node.js **24+**（`engines.node >= 24`，**只**用于 Vite 构建面板与两个版本号脚本）、各平台 Tauri 系统依赖（Linux 需 webkit2gtk 等）。

```sh
npm install
npm test             # 核心测试：cargo test --manifest-path src-tauri/core/Cargo.toml（207 项 = 187 单测 + 11 JS 对拍 + 9 红线）
npm run rust:check   # cargo check 核心
npm run lint         # eslint .（面板）
cargo clippy --all-targets --manifest-path src-tauri/core/Cargo.toml   # 核心门禁：0 warning
cargo clippy --no-deps --manifest-path src-tauri/Cargo.toml     # 壳门禁：0 warning
npm run dev          # 开发运行（tauri dev，构建期由 beforeDevCommand 拉起 vite:dev）
npm run build        # 桌面应用构建（vite:build && tauri build，产物在 src-tauri/target/release/bundle/）
npm run version:check # 校验 5 处版本号落点一致
```

核心也可脱离桌面壳单独运行（同一份编排代码）：

```sh
cargo run --manifest-path src-tauri/core/Cargo.toml        # 或 cargo build --release --manifest-path src-tauri/core/Cargo.toml
curl -H "Authorization: Bearer $(cat "$HOME/Library/Application Support/Buddy Bridge/api-key")" \
     http://127.0.0.1:41980/health
```

> ⚠ 数据目录随运行形态不同，排查时不要混用：桌面应用使用 Tauri `app_data_dir`（macOS `~/Library/Application Support/app.wbbridge.desktop`，identifier `app.wbbridge.desktop`）；独立核心二进制使用平台默认目录（macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`），可用 `BUDDY_DATA_DIR` 覆盖。`/health` 同样需要 Bearer 鉴权。

## 工程结构

```
src-tauri/core/ 核心代理的 Rust 实现（crate `wbbridge-core`，16 个 lib 模块 + 独立入口 src/main.rs）
                作为 path 依赖静态链接进壳；编排层 orchestration.rs 是原 main.js 的等价实现
src/            控制面板（Vue 3 + Vite：index.html / main.js / App.vue）
  views/        界面分区组件：SideBar.vue（分组导航 + 运行设置）、ModelList.vue、ModelDetails.vue（详情右栏）、
                ServiceStatus.vue、MetricsBar.vue、FeedbackBar.vue；components/ModelRow.vue 为模型行
  core/         前端内核 —— 面板与 Tauri IPC、状态轮询的唯一边界（bridge.js、activity.js）
                注意：src/core/ 是前端代码，不是后端；后端核心在 src-tauri/core/
src/styles/     面板全局样式
dist/           Vite 构建产物（tauri.conf.json 的 frontendDist）
src-tauri/      Tauri 2 壳：窗口/托盘/核心生命周期/IPC 命令/状态推送（bundle.externalBin 为空）
scripts/        仅版本号脚本（bump-version.mjs / check-version.mjs）
vite.config.js  前端构建配置（root: src，outDir: ../dist，dev 端口 41990）
docs/           验证记录（validation.md）、版本日志（version/）、接口契约（contract.md）、上游调研（research/）
.github/        CI（release.yml：核心测试 + 三平台六架构构建；迁移后尚未实际运行）
```

## 与上游实现（Electron 版）的差异

- 壳从 Electron 换为 Tauri 2：安装包体积显著下降（迁移前 sidecar 版实测 dmg 约 25 MB），内存占用显著降低；核心已用 Rust 重写并**静态链接进壳**，既不需要用户安装 Node，也不再随包分发 sidecar 可执行文件（`bundle.externalBin` 为空，`src-tauri/binaries/` 已移除）。
- 新增 `POST /admin/shutdown` 管理接口与 `BUDDY_PARENT_PID` 父进程看门狗（独立运行核心时生效）：Windows 下 SIGTERM 不可靠、壳可能被强杀，两者共同保证退出时总能优雅清理 WorkBuddy 配置。
- 托盘、导入、系统代理、模型检测与自动导入等行为与原版一致；界面保留原设计系统并优化了层级、留白、悬停反馈与深浅色主题。面板布局按本项目方案重做：侧栏为**分组导航**（模型 / 运行 / 集成 / 其他 + 运行设置），模型详情不再是「点开模型行就地展开」，而是**右侧常驻分栏**——选中模型后列表与详情并排显示，详情宽 `clamp(300px, 45%, 360px)`，可用 `Esc`、详情头部「收起详情」或窗口失焦收起，任何窗口宽度都不降级为上下堆叠。默认窗口 **1120×720**（最小 860×560），侧栏宽 208px。侧栏 5 个入口全部可点（模型与服务 / 运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新），非模型视图在主区独立滚动，同样不引入按宽度堆叠的降级分支。
- logo 全新设计（悬索桥 + W 形缆线），全平台图标已就绪（`src-tauri/icons/`）。
- 原 Node 核心（`core/src/*.js` 与其 97 项测试）已整体归档到仓库之外（`/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`）；Rust 核心通过冻结在 `src-tauri/core/tests/fixtures/*.json` 的 JS 真相快照（**271 例 / 11 个模块**：atomic、handoff、json、model_status、platform、protocol、reasoning、repair、sync、system_proxy、workbuddy_config）保持行为一致，对拍测试不再需要 Node 环境。

## 已知限制

- 免费模型名单、额度与可用性由上游 OpenCode 决定，本应用不控制也不缓存额度。
- 未做商用代码签名/公证：Windows 可能提示未知发布者，macOS 见上方放行说明。
- Windows ARM64 与 Linux 包由 CI 构建，尚未实机验证。
- 仅 PAC/SOCKS 代理暂不支持（与原版一致）。
- 面板以只读展示为主：运行日志读核心日志文件尾部（壳侧 `read_log`，尾部截断）、用量与额度展示核心累计的真实请求计数（`status.json` 顶层 `usage`，只统计 `source == "request"` 的真实客户端请求，探测不计、客户端取消不计成功、跨重启延续）、WorkBuddy 集成只读展示配置定位与发布结果（唯一动作是复用既有的「导入 WorkBuddy」）、关于与更新展示版本与数据目录（**未接入自动更新**，升级需重新下载安装包）。这些视图不新增任何写文件或联网行为。
- 浅色主题下仍有 3 处次要文字（副标题、页脚说明、耗时行）沿用 `--muted`，对比度低于 WCAG AA 的 4.5:1；属既有问题，尚未处理。
- Rust 化后的桌面 GUI **从未实机启动过**：目前只验证过编译、两个 crate 的 clippy 干净，以及独立核心二进制的一次冒烟运行（在一次性数据目录内完成运行时下载、隔离 OpenCode 启动、`/agent` 校验、发现 8 个免费模型并探测、干净关停）。面板布局改造另在浏览器引擎内经 CDP 实测（非 Tauri GUI 实机）。
- `cargo fmt --check` 全仓并不干净（未作为门禁），代码风格以 clippy 0 warning 为准。

## 验证记录

见 `docs/validation.md`（含 Node/sidecar 时代基线、2026-10-01 Node → Rust 迁移复验，以及同日面板布局改造三段）。
