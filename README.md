# WB Bridge

基于 Tauri 2 的跨平台托盘应用，通过隔离的 OpenCode 运行时为 [WorkBuddy](https://www.workbuddy.cn) 提供免费模型。

## 工作方式

- 启动时按需下载官方 OpenCode 运行时（失败自动尝试 npmmirror 镜像，下载后校验版本），以隔离配置启动，不使用用户已有的 OpenCode 数据。
- 自动发现 OpenCode 免费模型，向每个模型发送简短真实请求检测可用性与工具调用能力（会消耗少量免费额度）。
- 在本地 `127.0.0.1` 提供 OpenAI 兼容接口（Chat Completions + SSE），只发布检测通过的模型。
- **`WB · auto`：不用挑模型**。把 `model` 填成 `WB · auto`（唯一被接受的名字），核心就在**当前可用**的免费模型里按请求内容选一个真实模型，并在模型列表非空时作为一条独立条目写进 WorkBuddy / CodeBuddy 的 `models.json`。完整规则见「使用 → 不想挑模型」。
- 将可用模型写入 WorkBuddy 与 CodeBuddy 的 `models.json`（只修改本应用拥有的条目，原子替换；**不产生 `.bak` 备份**；退出时清理自己名下的条目，保留用户手动配置）。
- **可选：接入你自己的平台 Key**。侧栏「平台」视图里填入 ModelScope（魔搭）/ SiliconFlow / 腾讯混元 TokenHub / 智谱 任一家 Key，该家的免费模型会和 OpenCode 自带的免费模型一起被发现、检测并发布。Key 只落盘到数据目录 `providers.json`（`0600`），面板保存后立即清空、只显示「已配置 / 未配置」，绝不进日志、`status.json`、面板偏好或子进程环境变量。
- **关闭窗口即退出应用**（macOS / Windows / Linux 一致）：壳在窗口关闭请求里走与托盘「退出」相同的优雅关停链路（停核心 → 清理 WorkBuddy 配置 → 结束进程），不驻留托盘。托盘仍在（运行期间可左键唤回面板、切代理、重选配置、退出）。核心是静态链接进壳的 Rust 库，生命周期完全由壳掌握；单独运行核心可执行文件时，`BUDDY_PARENT_PID` 父进程看门狗会在父进程消失后自行优雅退出。
- **接入边界（未放宽）**：仅监听回环、所有路由都要 Bearer 鉴权、任何非空 `Origin` 一律 403、并发 ≤8、请求体 ≤8MB。命令行与桌面 agent 直连可用，**浏览器页面里 `fetch` 不行**。

## 下载与安装

> **当前版本 1.0.0**。推送 `v*` 标签后 GitHub Actions 自动构建六平台（macOS ARM/Intel、Windows x64/ARM、Linux x64/ARM）安装包并发布 Release（含各自 `.sig` 与汇总的 `latest.json`）。

| 系统 | 状态 | 安装包 |
|---|---|---|
| macOS 10.15+（Apple Silicon） | 本机已产出 dmg 与 updater 包，安装后 GUI 已实机启动运行 | `WB Bridge_<版本>_aarch64.dmg`（本机 `npm run make:dmg` 用 hdiutil 产出） |
| Windows 10/11 x64 | CI 已产出，待实机验证 | `WB Bridge_<版本>_x64-setup.exe`（+ 同名 `.sig`；另有 `…_x64_en-US.msi`，不参与更新） |
| Linux x64 | CI 已产出，待实机验证 | `WB Bridge_<版本>_amd64.AppImage`（更新包**就是裸 `.AppImage`**，bundler 不额外产出 `.AppImage.tar.gz`）/ `.deb` |

⚠ **如实说明**：本机签名打包（产出 `.app.tar.gz` + 配对 `.sig`）、六平台 CI 构建发布、`latest.json` 的 url 可达性、签名者 key ID 与配置公钥一致、macOS 安装包实机安装运行**都已实测**；但 Windows / Linux 安装包**尚未实机安装验证**，自动更新完整链路也未端到端验证（见「已知限制」）。

### macOS 首次打开

应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截：先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

## 使用

1. 先安装并登录 WorkBuddy（CodeBuddy 同样支持，两个插件可以都装）。在插件里保存过一个自定义模型最稳妥——它会生成 `models.json`；插件目录已存在但还没有这个文件时，本应用会补建一个空配置再照常发布（已存在的文件绝不改写，插件目录不存在也不会替插件建目录）。
2. 启动 WB Bridge：自动准备运行时、扫描并检测免费模型，找到有效配置后自动导入。
3. 找不到配置时点「导入 WorkBuddy」选择 `models.json`；Windows 托盘菜单「选择 WorkBuddy 配置…」可更换位置。
4. 「使用系统代理」开关控制运行时下载与模型请求是否走系统 HTTP/HTTPS 代理。

### 不想挑模型：`WB · auto`

模型列表非空时，本工具会额外提供一把合成路由 `WB · auto`，请求打给它、由核心在**当前可用**的免费模型里按请求内容选一个真实模型（纯文本全池；带图片只挑声明支持视觉的；带工具定义避开「仅对话」且要求支持函数调用；同级内随机分散）。

- **在 WorkBuddy / CodeBuddy 里**：这条会作为独立条目出现在插件的模型选择器里（与逐模型条目一起写入 `models.json`），选中即可，插件不需要知道任何真实模型 id；该路径已实机验证。
- **在任何 OpenAI 兼容客户端里**：Base URL 填 `http://127.0.0.1:41980/v1`（端口以面板 / `status.json` 为准，被占用时壳会另选端口），API Key 填数据目录 `api-key` 文件的内容，模型名填 `WB · auto`：

  ```sh
  curl -s http://127.0.0.1:41980/v1/chat/completions -H "Authorization: Bearer $(cat "$HOME/Library/Application Support/cn.hzdamei.wbbridge/api-key")" -H "Content-Type: application/json" -d '{"model":"WB · auto","messages":[{"role":"user","content":"你好"}]}'
  ```

  先用 `GET /v1/models`（同样要带 Bearer）确认它在列表末位，再发请求。
- **响应里的 `model` 是实际选中模型的全限定 id**（如 `modelscope/Qwen/Qwen3-8B`），用量与逐模型状态也都记在这个真实 id 上；它报错时该模型按既有判决被移出可用集合，恢复要靠面板的「重新检测」或「刷新」。
- 面板的模型列表与用量页里看不到「WB · auto」这一行——它不是模型，是一把路由。

界面：左侧栏为分组导航（模型 / 运行 / 集成 / 其他，底部为运行设置），6 个入口依次为**模型与服务、平台、运行日志、用量与额度、插件集成、关于与更新**，点击即在侧栏高亮当前视图；点击模型行后详情在**右侧常驻分栏**显示，按 `Esc`、点详情头部「收起详情」或窗口失焦即可收起。

## 从源码构建

依赖：Rust stable（壳与核心均 `rust-version = 1.90`，下限由锁定的依赖图决定）、Node.js **24+**（`engines.node >= 24`，**只**用于 Vite 构建面板与仓库根的 `scripts/*.mjs` 脚本，核心运行不依赖它）、各平台 Tauri 系统依赖（Linux 需 webkit2gtk 等）。

```sh
npm install
npm test             # 核心测试：cargo test --manifest-path src-tauri/core/Cargo.toml（316 项 = 294 单测 + 11 JS 对拍 + 11 红线）
npm run test:prefs   # 面板偏好单测（node --test，8 项，不联网）
npm run test:ops     # 操作守卫单测（node --test，11 项，纯函数）
npm run test:manifest# 更新清单生成单测（node --test，9 项，用临时产物目录跑真脚本）
npm run test:updater-key # 签名注入单测（node --test，13 项，不碰 ~/.tauri）
npm run rust:check   # cargo check 核心
npm run lint         # eslint .（面板）
cargo clippy --all-targets --manifest-path src-tauri/core/Cargo.toml   # 核心门禁：0 warning
cargo clippy --no-deps --manifest-path src-tauri/Cargo.toml     # 壳门禁：0 warning
npm run dev          # 开发运行（tauri dev，构建期由 beforeDevCommand 拉起 vite:dev）
npm run build        # 桌面应用构建（vite:build && tauri:build && make:dmg；产物在 src-tauri/target/*/release/bundle/）
npm run version:check # 校验 7 处版本号落点一致（package.json、tauri.conf.json、两侧 Cargo.toml、AGENTS.md 三行）
```

> ⚠ 自己从源码构建时需要 updater 签名私钥：`tauri.conf.json` 打开了 `bundle.createUpdaterArtifacts`，构建入口 `npm run tauri:build` = `node scripts/with-updater-key.mjs tauri build`。该包装器**只做签名注入、不做前置校验**：按「进程环境 > 仓库 `.env.local` > `.env` > `~/.tauri/wbBridge.env`、`~/.tauri/wbBridge-updater.env` > 兜底 `~/.tauri/wbBridge-updater.key`」汇齐私钥与口令（逐项打印来源文件名、显式空口令压过文件），把 `.key` 路径读成内联全文注入 `tauri build` 只认的 `TAURI_SIGNING_PRIVATE_KEY`（并删掉与之互斥的 `_PATH`）。它**不判形态、不试签，公私钥配对不符也只告警不阻断**，全程不打印密钥与口令；拿不到私钥时，要等整套 Rust 编译跑完才在打包那一步失败。自己签就生成一把（`npm run tauri -- signer generate -p "" -w ~/.tauri/my.key`）并把 `plugins.updater.pubkey` 换成自己的公钥——**配置里只有第一条公钥生效**，换钥必须同步换 pubkey，否则产物的 `.sig` 与配置公钥不配对、客户端验签必失败；只想出安装包不要更新链，可临时关掉 `createUpdaterArtifacts`。私钥绝不入库、不进日志。

核心也可脱离桌面壳单独运行（同一份编排代码）：

```sh
cargo run --manifest-path src-tauri/core/Cargo.toml
curl -H "Authorization: Bearer $(cat "$HOME/Library/Application Support/Buddy Bridge/api-key")" http://127.0.0.1:41980/health
```

> ⚠ 数据目录随运行形态不同，排查时不要混用：桌面应用使用 Tauri `app_data_dir`（macOS `~/Library/Application Support/cn.hzdamei.wbbridge`，identifier `cn.hzdamei.wbbridge`）；独立核心二进制使用平台默认目录（macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`），可用 `BUDDY_DATA_DIR` 覆盖。`/health` 同样需要 Bearer 鉴权。

## 工程结构

```
src-tauri/core/ 核心代理的 Rust 实现（crate `wbbridge-core`，16 个 lib 模块 + 独立入口 src/main.rs）
                作为 path 依赖静态链接进壳；编排层 orchestration.rs 是原 main.js 的等价实现
src/            控制面板（Vue 3 + Vite：index.html / main.js / App.vue）
  views/        界面分区组件：SideBar.vue（分组导航 + 运行设置）、ModelList.vue、ModelDetails.vue（详情右栏）、
                ProvidersView.vue（平台 Key）、LogsView.vue、UsageView.vue、IntegrationView.vue、AboutView.vue、
                ServiceStatus.vue、MetricsBar.vue、FeedbackBar.vue；components/ModelRow.vue 为模型行
  core/         前端内核 —— 面板与 Tauri IPC、状态轮询的唯一边界（bridge.js、activity.js、ops.js、prefs.js、update.js）
                注意：src/core/ 是前端代码，不是后端；后端核心在 src-tauri/core/
src/styles/     面板全局样式（variables.css 为设计 token 唯一来源）
dist/           Vite 构建产物（tauri.conf.json 的 frontendDist）
src-tauri/      Tauri 2 壳：窗口/托盘/核心生命周期/IPC 命令/状态推送（bundle.externalBin 为空）
scripts/        构建/发布辅助脚本（bump-version.mjs / check-version.mjs 版本号，gen-latest-json.mjs 更新清单，
                with-updater-key.mjs 是 tauri:build 的签名注入包装器，make-dmg.sh 用 hdiutil 出 macOS 的 .dmg）
vite.config.js  前端构建配置（root: src，outDir: ../dist，dev 端口 41990）
docs/           已清空，从 0 重新记录
.github/        CI（release.yml：核心测试 + 三平台六架构构建 + 更新清单汇总）
```

## 已知限制

- 免费模型名单、额度与可用性由上游 OpenCode 决定，本应用不控制也不缓存额度。
- 未做商用代码签名/公证：Windows 可能提示未知发布者，macOS 见上方放行说明。
- Windows / Linux 包由 CI 构建，尚未实机验证。
- 仅 PAC/SOCKS 代理暂不支持。
- 面板以只读展示为主：运行日志读核心日志文件尾部（壳侧 `read_log`，尾部截断）、用量与额度展示核心累计的真实请求计数（`status.json` 顶层 `usage`，只统计 `source == "request"` 的真实客户端请求，探测不计、客户端取消不计成功、跨重启延续）、插件集成只读展示配置定位与发布结果（唯一动作是复用既有的「导入 WorkBuddy」）、关于与更新展示版本与数据目录 + **自动更新区**（启动后静默检查，发现新版本才出现更新条，下载与安装始终由用户点击触发）。这些视图自身不写文件、不访问 OpenCode；**唯一的联网动作是自动更新**，且联网发生在 Rust 侧（Tauri updater 插件），不经面板 `fetch`。
- 浅色主题下仍有 3 处次要文字（副标题、页脚说明、耗时行）沿用 `--muted`，对比度低于 WCAG AA 的 4.5:1；属既有问题，尚未处理。
- 面板布局改造只在浏览器引擎内经 CDP 实测过，**未做 Tauri GUI 逐视图版式核验**。
- **自动更新已接线、CI 也产出过真实 Release，但端到端仍未验证**：完整链路「检查 → 下载 → 安装 → 重启生效」一次都没走过；对未接入更新器的更早版本用户不生效，升级需手动下载安装包一次。
- `cargo fmt --check` 全仓并不干净（未作为门禁），代码风格以 clippy 0 warning 为准。

## 验证记录

自 1.0.0 起在此处从 0 重新记录（更早的 Node/sidecar 时代基线与 Node → Rust 迁移复验记录已随 `docs/` 清空一并移除）。
