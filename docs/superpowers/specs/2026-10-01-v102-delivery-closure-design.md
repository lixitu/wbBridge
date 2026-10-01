# WB Bridge 下一阶段规划与 v1.0.2 交付收口设计

- 日期：2026-10-01（同日 **v1.0.1 发布后的实读校订版**）
- 状态：待用户复审
- 适用版本：v1.0.2（产品版本，patch 递进）
- 前置基线（实读核验）：
  - v1.0.1 **已发布**，远端标签已修正：`v1.0.1 → 679a2cb`（此前误指向 `f046208`，见「九、本次校订的实读证据」）
  - 核心 `cargo test` **207 通过 / 0 失败**（lib 187 + `js_parity` 11 + `red_lines` 9）
  - 壳 `cargo test --lib`（`src-tauri/`）**8 通过 / 0 失败**（第九轮「关窗即退出」补 3 项）
  - `cargo clippy`：核心 `--all-targets` 与壳 `--no-deps --all-targets` 均 **0 warning**

> **本版校订要点（相对上一稿）**
> 1. **DoD 第 3 条原本无法成立**：v1.0.1 未接线 updater，因此**不含更新器**，无法自我升级到 v1.0.2。自动更新链路只能从 **v1.0.2 → v1.0.3** 起验证。已改写。
> 2. ~~`latest.json` **不需要自建脚本**：官方文档明确由 tauri-action 生成。~~ **实施时再次推翻并回滚本条**：本工作流有 6 个并发作业往同一个 tag 上传，`tauri-action` 每次都会重写一遍清单、互相覆盖并静默漏平台。最终形态 = `includeUpdaterJson: false` + 末尾单一 `update-manifest` 作业跑 `scripts/gen-latest-json.mjs`（即原 A4 方案恢复，且平台键改由 artifact 目录名推导）。
> 3. ~~A4 表**漏了 3 个前端 npm 依赖**（插件 JS 绑定只能从 `@tauri-apps/plugin-*` 导入，`withGlobalTauri` 不注入插件 API）。~~ **本条也是错的**：实读 `@tauri-apps/cli` 内的 `api-iife.js` 证实 `withGlobalTauri` **会**注入 `window.__TAURI__.updater` / `.process`，因此**一个 npm 包都没加**（面板保持零外部请求、CSP 不变）。
> 4. `wb.lastConfigPath` 与壳 `settings.json` 的 `workBuddyModelsFile` 构成**两处真相**，违反本方案自己写的原则 → 从 localStorage 记忆项中移除。
> 5. `wb.theme`（浅色/深色/跟随系统）**不是持久化而是新 UI 能力**：现在只有 `@media (prefers-color-scheme: dark)` 一处，没有可切换的主题机制。已拆为 A8b 并单列风险。
> 6. 新增 **A9 发布与标签纪律**（把 v1.0.1 的真实事故固化成门禁）与 **A10 失败路径与回滚**。
> 7. 新增 **四、执行顺序与依赖**、**七、风险登记**、**九、实读证据**。

## 一、阶段总览与版本策略

用户确认 A、B、C、D 四类工作全部要做，按优先级分阶段推进：

| 阶段 | 内容 | 版本落点 | 状态 |
|---|---|---|---|
| A | 交付可信度：实机验证、打包、CI、自动更新、面板偏好持久化 | v1.0.2 | 本次设计范围 |
| B | 用户价值新功能 | v1.0.3 | 待细化 |
| C | 工程质量：契约门禁、fmt 门禁、测试补齐、文档口径自动校验 | 贯穿全程 | 待细化 |
| D | 能力边界扩展：PAC/SOCKS 代理、非 OpenCode 上游、更多平台 | 单独立项 | 待细化 |

**版本策略（用户明确要求）**：一律 patch 递增，即 v1.0.2 → v1.0.3 → v1.0.4，**不得出现 minor 版本（不做 v1.1.0）**。此规则需在后续授权后同步进 `AGENTS.md` 的版本规则分节，替换现有表述。

**A 阶段终点**：L3 更新闭环。L4（macOS 公证 / Windows 代码签名）当前不具备商业证书，只预留挂载点与注释，待证书就绪后直接接入。

**updater 交互形态**：U1 —— 启动后延迟静默检查，有新版本才在「关于与更新」视图显示更新条与「下载并安装」按钮；下载与安装始终由用户点击触发，装完提示重启。

**⚠ 新增前置事实（v1.0.1 发布过程中的真实事故）**：`v1.0.1` 标签最初指向 `f046208`（= 当时的 `main`，其 `tauri.conf.json` 版本仍是 **1.0.0**），导致 CI 产出 `WB Bridge_1.0.0_aarch64.dmg`，且该安装包**不含最近 4 个提交**。根因不是构建脚本，而是「标签打在不含版本推进的提交上」，而现有 `check-version.mjs` 只校验**同一提交内 5 处落点互相一致**，对这类错误天然静默。→ 已固化为 **A9**。

## 二、v1.0.2 目标

把 v1.0.1 从"编译通过 + 单测绿"变成"用户能装上、能自动升级"的成品。**不新增业务功能**（例外只有两项，且都不改变核心行为）：

1. 面板偏好持久化与窗口状态记忆（A8）；
2. 主题三态切换（A8b，**由 A8 拆出**，属新增 UI 能力，可整体推迟到 v1.0.3 而不阻塞收口）。

## 三、工作包

### A1 实机验证（第一项执行）

- **先确认验证对象**：验证前实读 `git rev-parse HEAD` 与 `src-tauri/tauri.conf.json` 的 `version`，确保跑的就是要发布的那份代码（v1.0.1 事故的直接教训）。
- `npm run tauri:dev` 真实启动桌面 GUI，逐项验证：面板 5 个侧栏视图、模型详情右侧常驻分栏（Esc / 收起按钮 / 窗口失焦三种收起方式）、托盘（左键唤回面板、切换代理、重选配置、退出）。
- 重点验证 v1.0.1 最后提交的「关窗即退出应用」，**必测三点**（此前只到"编译 + 单测"级证据）：
  1. 点关闭后进程与监听端口是否真的在 `STOP_BUDGET = 8s` 内一起消失（`pgrep` + `lsof -iTCP` 复核，不留孤儿）；
  2. macOS 红按钮 / `Cmd+W` / Dock 图标行为符合预期（应用不驻留），Windows/Linux 标题栏关闭等价；
  3. 关窗到进程退出这几秒里窗口保持隐藏，面板**不弹**假的「核心服务已退出 + 重试」（该口径已由 `service_down_reports_only_real_failures` 钉住，仍需实机确认）。
- 顺带验证运行日志中已观察到的上游提示：`MaxListenersExceededWarning ... 11 event listeners added to [M$]`（来自被托管的 OpenCode/Bun 进程，本次会话仅 1 次、`phase: ready`、6 请求 6 成功）。判据：**同一进程内 count 是否持续上涨**；不涨则记为上游一次性提示，涨则按 A10 的降级路径处理。
- 结果写入 `docs/validation.md` 新分节，标注实机证据与发现的问题；**同时把 `AGENTS.md`「验证边界」表里 GUI 与本轮壳侧改动两行从 ❌ 改为 ✅ 并附证据**（禁止保留已验证为真的"未验证"口径，也禁止反向虚标）。

### A2 本地产包

- 先 `export PATH="$HOME/.cargo/bin:$PATH"`（本机 cargo 不在默认 PATH），再执行 `npm run build` 产出 dmg。**建议把这条与 nvm 载入一并写进 `AGENTS.md`「环境前置」**，避免每轮重复。
- dmg 安装到 `/Applications` 冒烟：启动、导入配置、模型检测、关窗退出。
- 补两项冒烟：① 全新数据目录首启（`api-key` 生成、`0600` 权限、`/agent` 校验通过、`phase` 进入 `ready`）；② 二次启动不产生第二个实例（核心 `service.pid` 锁 + `tauri-plugin-single-instance` 均按预期）。
- 记录 dmg 体积与内存占用，与迁移前 sidecar 版（`README.md:89` 口径：实测约 25 MB）对比。

### A3 CI 跑通（含门禁，先修已知问题再推标签）

- **修 3 处已知问题**（此前仅通过 YAML 结构校验，从未真实运行）：
  1. 三个 build job 的 `node-version: 22` → **24**（根 `package.json` 的 `engines.node >= 24`，当前不一致）；
  2. 产物 glob 目前只收 `**/*.dmg` / `**/*.app`，接线 updater 后必须补 `**/*.app.tar.gz`、`**/*.nsis.zip`、`**/*.AppImage.tar.gz` 及各自 `.sig`；
  3. 增加 **A9 的标签↔版本门禁**（缺它就可能重演 1.0.0 事故）。
- **`latest.json` 不自建脚本**：官方文档明确 "Tauri Action generates a static JSON file for you to use on CDNs such as GitHub Releases"。因此只需保证 `bundle.createUpdaterArtifacts: true` + 签名环境变量到位，由 `tauri-action` 在创建 Release 时汇总生成；原 `scripts/gen-latest-json.mjs` 方案作废。
  - ⚠ 待实测：三个平台 job 并发写同一 Release 的 `latest.json` 是否会互相覆盖。若覆盖，改为在三个 build 之后加**单一 publish job** 统一生成清单。
- **Draft 语义要写进文档**：`releaseDraft: true` + endpoint `.../releases/latest/download/latest.json` 的组合下，GitHub 的 `releases/latest` **不含 draft 与 prerelease** → 人工点 Publish 之前老客户端拿不到清单。这是**期望行为**（draft 即灰度闸门），但必须在 `docs/wiki/版本与发布.md` 与发布说明里写明，否则会被误判成"更新功能坏了"。
- 推送 `v1.0.2` tag 触发流水线，产出六平台产物并发布 Release。
- **红线约束**：Agent 不得自动 `git commit` / `git push` / `git tag`。推送 tag 这一步需用户自行执行或明确授权。

### A4 updater 接线（参考 fastenerTradeWorkbench，精简版）

**实现形态**：复用参考项目的极简做法，不自建 Rust 更新命令。

| 层 | 动作 | 位置 |
|---|---|---|
| Rust 依赖 | 增加 `tauri-plugin-updater = "2"`、`tauri-plugin-process = "2"` | `src-tauri/Cargo.toml` |
| ~~**前端依赖（原稿遗漏）**~~ **实施时更正** | **最终没有加任何 npm 包**：实读 `@tauri-apps/cli` 内 `api-iife.js` 证实 `withGlobalTauri` 会注入 `window.__TAURI__.updater` / `.process`，面板经 `bridge.js` 用全局绑定即可（零外部请求、CSP 不变） | 根 `package.json`（未改动） |
| Rust 注册 | 官方 README 形态：`#[cfg(desktop)] app.handle().plugin(tauri_plugin_updater::Builder::new().build())?` 放 `setup` 内；`tauri_plugin_process::init()` 与其他插件一样挂 builder 链 | `src-tauri/src/lib.rs` |
| 权限 | 增加 `updater:default`、**`process:allow-restart`**（**不用 `process:default`**：它含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链直接杀进程；A8 再补 `window-state`）。插件权限**必须**在 capabilities 声明；应用自有命令无需列 | `src-tauri/capabilities/default.json` |
| 配置 | `bundle.createUpdaterArtifacts: true`；`plugins.updater` 填 `pubkey`（**必须是内联公钥字符串，不能写文件路径**）与 `endpoints: ["https://github.com/trexwb/wbBridge/releases/latest/download/latest.json"]`。生产模式强制 HTTPS，**不得**为图方便开 `dangerousInsecureTransportProtocol` | `src-tauri/tauri.conf.json` |
| 前端封装 | 新增 `checkUpdate` / `downloadUpdate` / `relaunchApp`，与既有 `dataDir()` / `readLog()` 同风格，组件不直触 `window.__TAURI__`，也不在面板里 `fetch` endpoint（更新请求由 Rust 侧发起，不经 WebView CSP） | `src/core/bridge.js` |
| 视图 | 现有"未接入自动更新"说明区块（`src/views/AboutView.vue` 第 62-68 行的 `<div class="panel">` 更新段，已实读确认）整体替换为更新区 | `src/views/AboutView.vue` |
| CI | 注入 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（GitHub **Secrets**）。**实测更正**：`tauri build\|bundle` 只读 `TAURI_SIGNING_PRIVATE_KEY`，其值可以是私钥全文**或私钥文件的绝对路径**；`TAURI_SIGNING_PRIVATE_KEY_PATH` 只对 `tauri signer sign` 生效（等价 `-f`）。产物 glob 需补 `.sig` / `.app.tar.gz` / `.AppImage.tar.gz`；**`latest.json` 改由末尾单一 `update-manifest` 作业生成**（`scripts/gen-latest-json.mjs`），macOS 作业另需一步把无架构的 `WB Bridge.app.tar.gz` 改名成 `WB Bridge_<arch>.app.tar.gz`，否则两个 mac runner 会在 Release 上同名互覆盖 | `.github/workflows/release.yml` |

**明确不做**（相对参考项目削减）：自定义 Rust 更新命令、发现新版本即后台自动下载、toast + 二次确认弹窗。

**更新路径**：启动后延迟静默检查 → 有新版本才在「关于与更新」显示更新条 → 用户点「下载并安装」→ 进度条 → 完成提示重启 → 重启生效。

**⚠ relaunch 与本项目既有链路的三处交互（列为必测，不是已验证）**：
1. **单实例互斥**：`.plugin(tauri_plugin_single_instance::init(...))`（`src-tauri/src/lib.rs:598`）会在旧进程尚未退出时把新进程直接转交给旧实例并让其退出 → 表现为"更新装了但没重启"。需在实机确认 `relaunch()` 的先后时序，必要时改为「先走自身退出链路，再由用户重开」或给 relaunch 加等待。
2. **核心单实例锁**：新实例启动时若旧实例的 `service.pid` 尚未清理（`orchestration.rs:1374` 在关停里删），核心会以 `ALREADY_RUNNING` 失败退出 → 面板应显示启动失败原因而非静默。
3. **关窗即退出**：下载/安装进行中点关闭 = 中断更新（v1.0.1 行为）。UI 必须显式提示「下载期间关窗会中断本次更新」，且中断不得留下半更新状态。

**Vue 动效要求**（复用 wbBridge 既有 token，已实读确认 `src/styles/variables.css:64-69` 存在 `--dur-1/2/3`、`--ease-standard/enter/emphasis`；禁止硬编码时长）：

1. 进度条：`transition: width var(--dur-2)` + 渐变填充；下载中叠加 shimmer 流光。
2. 状态区进出：`rise-in var(--dur-3) var(--ease-enter)`，与主区入场动效同源。
3. 检查中：复用既有 `.spinner.sm`，不新造组件。
4. 发现新版本：更新条左侧绿点脉冲一次（`--dur-3`），吸引注意但不闪烁。
5. 可访问性：`prefers-reduced-motion` 下位移与淡入取消，spinner 与 shimmer 循环保留。
6. 状态机三态齐备：检查失败 / 无更新 / 有更新都要有明确文案（面板规范：动作期间 spinner，失败必须显示原因，不得吞错）。

**密钥纪律**：minisign 私钥存 GitHub Secrets，公钥内置配置；私钥绝不入库、不进日志。公钥入库是允许的（它不是秘密），`.env.local` 与任何私钥文件严禁提交。

**需实测**：v1.0.2 → v1.0.3 的完整升级链路（**不是** v1.0.1 → v1.0.2，见 DoD 修正）。

### A5 文档口径同步

- 同步 `AGENTS.md`（版本规则改为 patch 递进；面板"以只读展示为主"口径补充 updater 例外；GUI 验证边界行按 A1 结果改标）、`README.md`、`docs/wiki/已知限制与未验证项.md`、`docs/wiki/版本与发布.md`、`docs/wiki/关于与更新` 相关页。
- 删除或改写"未接入自动更新""升级需重新下载安装包"等已过时表述，**但必须同时写明**：v1.0.1 及更早版本无更新器，升级到 v1.0.2 仍需手动装一次安装包。
- **发布日志落点（项目纪律要求，原稿遗漏）**：`docs/version/RELEASE-v1.0.md` 顶部追加 **v1.0.2 分节**（最新在前），并新增 `docs/version/RELEASE-NOTES-v1.0.2.md` 作为 GitHub Release 正文（命名规则见 `docs/version/README.md`）。
- 修 `src-tauri/Cargo.toml:24` 注释里过期的「它的 **195** 个测试」→ 207（实读发现的口径漂移）。
- `docs/contract.md` 若因新增壳命令产生缺项，同步补齐（该文件目前无自动门禁）。

### A6 基线复验

收口门槛为**六项**全绿（原稿五项漏了壳侧单测）：

1. `cargo test`（`src-tauri/core/`）→ 207 通过 / 0 失败
2. `cargo test --lib`（`src-tauri/`）→ 8 通过 / 0 失败（新增插件/偏好相关行为后必须继续只增不减）
3. `cargo clippy --all-targets`（核心）与 `cargo clippy --no-deps --all-targets`（壳）→ 0 warning
4. `npx eslint .` → 0 problems
5. `npm run version:check` → 5 处一致
6. `npm run vite:build` → 成功

**建议补第 7 项（前端零测试是当前空白）**：`src/core/prefs.js` 的纯函数（默认值合并、损坏 JSON 降级、键白名单）用 **`node --test`** 覆盖，**不引入新测试框架**（Node 在本仓只服务构建与脚本，符合既有纪律；测试不得触碰 Vue 组件与 DOM）。

### A7 L4 预留位

- 写明代码签名 / 公证的挂载位置与注释，标注"待商业证书直接接入"：
  - macOS：`tauri.conf.json` 的 `bundle.macOS.signingIdentity` 当前为 `"-"`（**ad-hoc**，实读确认）；商业证书就绪后替换为 Developer ID，并接 Apple 公证（`APPLE_ID` / `APPLE_TEAM_ID` / app-specific password 一类环境变量）。ad-hoc 的意义仅在于避免 Apple Silicon 产物从 GitHub 下载后被判定为损坏，**不等于**通过 Gatekeeper。
  - Windows：代码签名（证书指纹 / PFX）挂载点，具体配置键名**接入前须实读届时官方文档**，本稿不预设。
- 文档继续保持"未做商用签名/公证"的诚实口径，不宣称已有签名。
- ⚠ 关联风险：macOS 上未公证的产物经更新器替换后，用户侧仍可能被 Gatekeeper 拦下。若实测受阻，A10 的"手动下载安装包"回滚路径即为兜底，并在发布说明中明示。

### A8 面板偏好持久化（localStorage + 窗口状态）

**原则**：前端存储只放"界面偏好"，绝不放权威配置。api-key、代理设置、模型列表仍由核心 / 壳掌握，避免出现两处真相。

**localStorage 记忆项（校订后）**

| # | 记忆项 | 键（建议） | 默认值 | 校订说明 |
|---|---|---|---|---|
| 1 | 上次停留的侧栏视图 | `wb.view` | 模型与服务 | 恢复前校验视图名仍在入口表内，否则回落默认 |
| 2 | 模型详情栏展开状态 | `wb.details` | 收起 | **不再记住"上次选中模型"**：客户端展示 ID 是 `OC · 名称`，模型名单随探测变化，恢复一个已不存在的选中项只会造成空详情。若确要恢复，必须先校验该 ID 存在于当前 `modelResults` |
| 3 | 日志视图：自动刷新开关、过滤关键词、自动滚底 | `wb.logs` | 开 / 空 / 开 | 过滤词只存字符串，不得回显到日志区之外 |
| 4 | 用量视图：时间范围 | `wb.usage.range` | 今日 | `usage` 只有累计口径（`since` + 总量 + 逐模型），"今日"需前端按 `since` 自行判定；若拿不到可靠时间边界，此项应降级为"全部"，**不得显示假数字** |
| 5 | 自动检查更新开关 + 上次检查时间戳 | `wb.update` | 开 / 无 | 与 A4 的静默检查节流共用同一处存储 |
| ~~6~~ | ~~上次导入的 models.json 路径~~ | — | — | **移除**：壳的 `settings.json` 已持久化 `workBuddyModelsFile`，再存一份 localStorage 就是两处真相。需要展示时经既有状态快照 / 只读命令读壳的值 |
| 7 | 主题偏好（浅色 / 深色 / 跟随系统） | `wb.theme` | 跟随系统 | **拆到 A8b**：当前无可切换主题机制，这是新 UI 能力而非持久化 |

**窗口状态**：引入 `tauri-plugin-window-state` 记住窗口尺寸与位置（存磁盘，不用 localStorage）；恢复时须尊重现有最小尺寸 860×560。

**统一实现方式**：新增 `src/core/prefs.js` 作为唯一读写边界（与 bridge.js 同风格，封装 JSON 序列化、默认值合并、异常降级），组件只调它，不直接访问 localStorage。

**IndexedDB**：本期不引入。现有日志为尾部截断读取、用量为累计计数，无大体量缓存需求；待出现"保留最近 N 天日志历史"一类需求时再评估。

**安全约束**：localStorage / IndexedDB 中禁止出现 api-key、`OPENCODE_SERVER_PASSWORD`、token 或完整配置内容；写入前由 prefs.js 做**键白名单校验**（白名单外的键一律拒写），并由 A6 第 7 项的单测钉住。

**需实测 / 风险点**：

- 关窗即退出（v1.0.1 行为）与窗口状态保存的时序：需确认在优雅关停链路下窗口尺寸与位置仍被正确写盘。**兜底方案已定**：插件提供 `saveWindowState(StateFlags.ALL)`，在退出链路（`quit_app` 前）显式调用一次，不依赖窗口销毁事件。
- 偏好数据损坏或结构变更时按默认值降级，JSON 解析失败不得抛错阻塞渲染。
- 偏好写入失败（隐私模式 / 配额）必须静默降级为"本次会话不记忆"，**不得**把异常冒到面板。

**可访问性**：主题切换控件需键盘可达，遵循既有 focus-visible 与 prefers-reduced-motion 约束。

### A8b 主题三态切换（由 A8 拆出，可推迟）

- 现状（实读）：`src/styles/variables.css:93` 只有一个 `@media (prefers-color-scheme: dark)` 块，**没有** `data-theme` / class 机制，因此"手动切主题"要先把深色块改造为「属性优先、媒体查询兜底」的双轨，再逐 token 复核浅色值。
- 已知连带缺陷：浅色主题下小字号说明文字对比度偏低（`src/App.vue:262`、`:264`、`src/components/ModelRow.vue:110` 仍用 `--muted`，而规范要求这类文字用 `--muted-strong`）。做主题切换前应先把这条修掉，否则三态里有一态是坏的。
- 结论：**不阻塞 v1.0.2 收口**；若排期紧张，A8 只落 1–5 项，主题留到 v1.0.3。

### A9 发布与标签纪律（新增，直接源于 v1.0.1 事故）

1. **顺序铁律**：先用 `npm run version:set -- <x.y.z>` 改写 5 处落点并**提交**，再在该提交上打 `v<x.y.z>` 标签。标签永远不允许指向"不含本次版本推进"的提交。
2. **CI 门禁**（`release.yml` 的 test job 内，`check-version.mjs` 之后）：当 `GITHUB_REF_NAME` 以 `v` 开头时，比较去掉前导 `v` 与 `src-tauri/tauri.conf.json` 的 `version`，不一致 **exit 1**；`workflow_dispatch` 触发的构建跳过该比较。这样"标签与源码版本不匹配"会当场红，而不是安静地产出错名安装包。
3. **产物名自检**：build job 上传前打印 `ls` 出的 bundle 文件名，Release 正文里附一行"产物版本 = tag 版本"的自检结论。
4. **重打标签的操作边界**：删除/移动**已推送**的标签属共享状态变更，只能由用户执行或逐次明确授权。
5. 把上述 1–3 写进 `docs/wiki/版本与发布.md` 与 `AGENTS.md` 的 Git 提交规范分节。

### A10 失败路径与回滚（新增）

- **更新检查失败**（endpoint 404 / 网络不可达 / 清单签名不符）：更新条显示具体原因，服务不受影响；探测与请求链路**绝不**因更新器状态而中断。
- **下载或安装失败**：保留当前版本继续运行，错误可见；不得出现"半更新"状态（安装失败即视为未安装）。
- **更新后首启失败**：面板必须显示 `phase: error` 与原因（沿用既有 `core-failed` 通道），并在「关于与更新」提示可重新下载上一版安装包。
- **回滚兜底**：每个已发布版本的历史安装包必须在 Release 页保留可下载，文档写明"手动装回上一版"的步骤。
- **上游运行时异常**（如 OpenCode 事件流持续报错、监听器数持续上涨）：优先用壳的 `restart_core` 能力恢复，并把现象记进 `docs/validation.md`。

## 四、执行顺序与依赖（新增）

| 序号 | 工作包 | 依赖 | 完成判据（退出条件） |
|---|---|---|---|
| 1 | A1 实机验证 | 无 | GUI 全项有真实证据；`AGENTS.md` 验证边界两行改标；问题清单产出 |
| 2 | A2 本地产包 | A1 | `/Applications` 冒烟通过；体积/内存数据入档。**产物已产出**（macOS aarch64 dmg 3,720,766 B + updater 包与 `.sig`，只读挂载与 `codesign --verify --deep --strict` 已过），**冒烟与内存观察仍未做**（依赖 A1） |
| 3 | A8 偏好持久化（不含 A8b） | 无 | `prefs.js` 读写边界 + 白名单单测绿；跨重启生效；无密钥落前端存储 |
| 4 | A9 标签纪律 + CI 门禁 | 无（可与 3 并行） | 门禁在误配标签时能红；文档同步 |
| 5 | A4 updater 接线（本地）✅ 已完成 | A9（私钥/签名链路）、A3 的 glob 修正 | 本地 `tauri build` 产出 `.app.tar.gz` + `.sig`；三处 relaunch 交互实测有结论。**前半已达成**（2026-10-01 第十轮：依赖 + builder 注册、`createUpdaterArtifacts`、`plugins.updater` 公钥换成本机新生成的 `126D4E208E0F17BA`、`process:allow-restart`、面板更新区与 `src/core/update.js`、`scripts/gen-latest-json.mjs` + CI 单一 `update-manifest` 写者 + macOS 补架构后缀，本机签名构建产出配对 `.sig`）；**后半未做**，属必须实机点一遍的类别，依赖 A1 |
| 6 | A3 CI 跑通 | A4 | 六平台产物 + `latest.json` 真实出现在 Release；产物名 = tag 版本 |
| 7 | A4 更新闭环实测 | A3、A6 | v1.0.2 → v1.0.3 演练通过（可用一次性测试仓库，不污染 `trexwb/wbBridge` 的 releases） |
| 8 | A5 文档 + A6 基线 + A7 预留位 | 全部 | 六项基线全绿；发布日志两处分节落盘。**A5/A6 本轮已完成**（AGENTS.md 验证边界 + docs/wiki 6 页 + contract + validation + README；核心 207、壳 8、两侧 clippy 0 warning、eslint 干净、vite 构建通过）；**发布日志的 v1.0.2 分节须等版本号推进授权后才能写**（当前仍 1.0.1），A7 仅保留配置位 |

**关键路径**：1 → 2 → 5 → 6 → 7。A8b（主题）不在关键路径上，可整体推到 v1.0.3。

## 五、红线遵守

- 禁止重构既有架构；禁止把 Node 运行期重新引入核心链路（A6 第 7 项的 `node --test` 只测前端纯函数，不触碰 `src-tauri/core/`）。
- 不自动执行 `git commit` / `git push` / `git tag`。
- 密钥零泄漏：updater 私钥、api-key、`OPENCODE_SERVER_PASSWORD` 不得进入仓库、日志或 `status.json`；**公钥可入库**。
- 不提交 `api-key`、`.env.local`、`status.json`、`settings.json`、`node_modules`、`dist`、`target`。
- 版本号只动末尾一位，且需用户明确授权。
- 新增：updater endpoint 必须 HTTPS，禁止用 `dangerousInsecureTransportProtocol` 绕过；`pubkey` 内联、不接受文件路径。
- 新增：前端存储白名单——localStorage 只允许上表列出的键，禁止任何凭据、配置内容、模型清单副本。
- 新增：面板仍是"只读展示 + 动作触发"，updater 是**唯一**由前端触发的下载动作，且必须经 `bridge.js` 封装、由 Rust 侧发请求。

## 六、验收标准（DoD）

1. 本机可安装 dmg 并正常启动、导入、检测、关窗退出（A1/A2 的真实证据在 `docs/validation.md`）。
2. CI 真实跑通并产出六平台 Release 产物，**且产物文件名里的版本号与触发标签一致**（防事故复发）。
3. **（已修正）** 自动更新链路自 **v1.0.2 → v1.0.3** 起可实测通过：已装 v1.0.2 的机器能在「关于与更新」看到更新条、完成下载并安装、重启后版本号变化。
   **v1.0.1 → v1.0.2 不在此列**：v1.0.1 未接线 updater，其用户必须手动下载安装包一次，文档与发布说明必须明说。
4. A6 六项基线全绿（含壳侧 `cargo test --lib` 只增不减）。
5. `docs/validation.md` 有实机运行的真实证据记录，未验证项在 wiki 中同步更新；已验证项不得继续标 ❌。
6. 面板偏好与窗口尺寸位置跨重启生效，且 localStorage 中不含任何密钥或配置内容（由 `prefs.js` 白名单单测 + 实机 grep 存储内容双重确认）。
7. A10 的三条失败路径（检查失败 / 安装失败 / 更新后首启失败）都有可见文案与可执行回滚步骤，不出现静默吞错。

## 七、风险登记（新增）

| # | 风险 | 影响 | 处置 |
|---|---|---|---|
| R1 | 标签指向不含版本推进的提交（v1.0.1 已发生一次） | 产物错名 + 发布旧代码 | A9 门禁；操作只能由用户执行 |
| R2 | `relaunch()` 与 `single-instance` / `service.pid` 时序 | 更新装了没重启、新实例报 ALREADY_RUNNING | A4 必测三项；必要时改为"退出后由用户重开" |
| R3 | 三平台 job 并发写同一 `latest.json` | 清单只含单一平台，其余平台收不到更新 | A3 实测；必要时改单一 publish job 汇总 |
| R4 | macOS 仅 ad-hoc 签名，更新后可能被 Gatekeeper 拦 | 用户升级失败 | A7 挂载点 + A10 手动回滚；发布说明明示 |
| R5 | Draft Release 期间 endpoint 取不到清单 | 被误判为"更新功能坏了" | 文档写明 draft 即灰度闸门，Publish 后生效 |
| R6 | A8b 主题改造牵动全部颜色 token | 浅色对比度缺陷被放大 | 拆出关键路径；先修 `--muted` → `--muted-strong` 三处 |
| R7 | 上游 OpenCode 事件流监听器数持续上涨 | 长时运行内存增长 | A1 观察判据；上涨则降频/重启运行时（属行为变更，需授权） |
| R8 | 前端零测试基线 | 偏好降级逻辑回归无人守 | A6 第 7 项 `node --test` 覆盖 `prefs.js` 纯函数 |

## 八、未决项

- B 阶段（v1.0.3）功能清单待单独 brainstorm。建议候选（来自本轮实读发现，尚未排序）：契约表第三份（`docs/contract.md`）自动门禁、浅色主题与对比度整改、日志保留 N 天历史（届时再评估 IndexedDB）、上游 OpenCode 版本固定策略。
- C、D 阶段的详细范围待细化。其中两项成本极低、收益直接，建议**提前插入 v1.0.2**：A9 的标签↔版本门禁、`node-version` 22 → 24。
- L4 商业证书获取时间未定。
- 私钥存放与 GitHub Secrets 写入者（2026-10-01 第十轮更新）：更新签名私钥已在**本机**新生成并存于 `~/.tauri/wbBridge-updater-20261001.key`（权限 `0600`，未设密码），对应公钥已内联进 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`（key ID `126D4E208E0F17BA`）。**仍未决**：GitHub Secrets（`TAURI_SIGNING_PRIVATE_KEY` + 空 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`）由谁在 `trexwb/wbBridge` 仓库写入——**任何 `v*` tag 推送前必须先配好这两个 Secret**，否则 CI 会在生成 updater 产物那一步失败；以及私钥的备份位置与轮换责任人。
- ⚠ 遗留隐患：更早生成的 `~/.tauri/wbBridge-updater.key`（权限 `0600`）密码不可恢复，且与现配置公钥**不配对**，已不被任何配置引用；但同目录的 `~/.tauri/wbBridge.env` 权限是 **0644**（同机其他用户可读）且内含明文密码串，属既有隐患。建议由用户自行确认该钥无其他用途后删除旧钥、并把 `*.env` 收紧到 `0600`；本仓库未收录任何私钥或密码内容。

## 九、本次校订的实读证据（全部为本仓库真实状态，2026-10-01）

| 结论 | 证据 |
|---|---|
| v1.0.1 标签已指向新提交 `679a2cb`；`v1.0.0` 仍指 `f046208` | `git ls-remote --tags origin` |
| 壳测试基线为 8 通过（不是 5） | `cargo test --lib`（`src-tauri/`）实跑：`8 passed; 0 failed` |
| ~~`withGlobalTauri: true`，但插件 API 仍需 npm 包~~ **已推翻** | 实读 `node_modules/@tauri-apps/cli-darwin-arm64/cli.darwin-arm64.node` 内嵌的 `api-iife.js`：`withGlobalTauri` 会把插件 API 注入 `window.__TAURI__.updater` / `.process`，因此**未新增任何 npm 包** |
| 当前 capabilities 已含 `core:default` 与 window/event/dialog 条目，未含插件权限 | `src-tauri/capabilities/default.json`（本轮已补 `updater:default` + `process:allow-restart`） |
| `bundle.targets = "all"`、`externalBin = []`、macOS `signingIdentity = "-"`（ad-hoc）、`plugins = {}` | `tauri.conf.json`（本轮已加 `createUpdaterArtifacts: true` 与 `plugins.updater`） |
| 改造前 CI 四处 `node-version: 22`、glob 只收 dmg/app | `.github/workflows/release.yml`（本轮四处 job 已统一 `node-version: 24`，glob 已补 updater 产物与 `.sig`） |
| 前端只有 `vue` 一个 runtime 依赖，插件 JS 包需新增 | `package.json` → `dependencies: {vue}`、`devDependencies` 5 项（**实施结论：插件 JS 包不新增**，见下面被推翻的那行） |
| ~~`latest.json` 由 tauri-action 生成，无需自建脚本~~ **已推翻** | 官方文档那句只对**单作业**成立。本工作流 6 个并发作业同写一个 tag 的清单会互相覆盖 → 用 `includeUpdaterJson: false` + 单一 `update-manifest` 作业（`scripts/gen-latest-json.mjs`） |
| `pubkey` 必须内联、生产强制 HTTPS | Tauri v2 文档 plugin/updater 配置表 |
| ~~`.env` 对更新签名无效 / `_PATH` 是本地用法~~ **已实测更正** | 本机跑过：`tauri build` 只读 `TAURI_SIGNING_PRIVATE_KEY`，**该值可以是私钥文件路径**（`export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/wbBridge-updater-20261001.key"` 实测产出配对 `.sig`）；`TAURI_SIGNING_PRIVATE_KEY_PATH` 只对 `tauri signer sign` 生效 |
| macOS 的 updater 产物名不含版本与架构 | 本机实测：`src-tauri/target/release/bundle/macos/WB Bridge.app.tar.gz` → CI 必须补架构后缀，否则两个 mac runner 的 Release 资产同名互覆盖 |
| AboutView 更新区块确在第 62-68 行 | `src/views/AboutView.vue` 实读 |
| 动效 token `--dur-1/2/3`、`--ease-*` 存在 | `src/styles/variables.css:64-69` |
| 主题只有媒体查询，无切换机制 | `grep data-theme src/` 无命中；`variables.css:93` 唯一 `prefers-color-scheme` |
| 壳注册了 `tauri-plugin-single-instance`（relaunch 交互相关） | `src-tauri/src/lib.rs:598` |
| 核心 `service.pid` 在关停中清理（ALREADY_RUNNING 相关） | `src-tauri/core/src/orchestration.rs:1374,1530-1541` |
| 上游监听器告警只出现 1 次且服务健康 | `opencode.log`（37 行）+ `status.json`：`phase: ready`、`usage.total 6/6/0` |
| `src-tauri/Cargo.toml:24` 注释仍写「195 个测试」（过期） | 实读该行 |

## 十、备注

本文档按用户既有纪律由 Agent 生成并校订，**未执行 git commit**，提交与否由用户决定。文中所有"已实读/已实测"结论均可由第九节命令复现；所有未实测项已标注为待验证，不得当作已完成。
