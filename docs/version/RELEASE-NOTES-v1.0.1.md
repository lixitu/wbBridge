# WB Bridge v1.0.1

> **GitHub Release 正文**（推送 `v1.0.1` 标签或手动运行 `release.yml` 时，可直接复制本文件内容作为 Release body）。
> 发布日期：2026-10-01 ｜ 上一版本：v1.0.0 ｜ 内部 crate `wbbridge-core` 版本仍为 `0.1.0`（与产品版本解耦，不随本次递增）

**本版主题：安全与供应链加固 + 并发/生命周期正确性 + 面板只读视图补全。**

---

## ⚠ 发布状态（请如实阅读）

本次为**基线快照发布**，与 v1.0.0 一样尚未完成实机验证：

- ❌ **桌面 GUI 从未实机启动**（`npm run tauri:dev` 与打包 `.app` 均未跑过）。壳侧改动只有「编译通过 + 核心独立运行行为 + 单元测试」为证据。
- ❌ **`.github/workflows/release.yml` 未在 CI 实际跑通**（仅本地 YAML 结构校验）。
- ❌ **迁移后未产出过安装包**；签名与自动更新链路**未接线**（`tauri.conf.json` 无 `createUpdaterArtifacts` / `plugins.updater`），因此**没有 `.sig` 与 `latest.json`，也没有应用内更新**。升级方式＝重新下载并安装新安装包。
- ✅ 已实测：核心 `cargo test` **207 通过 / 0 失败**、核心与壳 `cargo clippy` **0 warning**、壳 `cargo test --lib` **8 通过**、`npx eslint .` **0 problems**、`npm run vite:build` 成功、`npm run version:check` **5 处一致（1.0.1）**、核心独立进程冒烟（下载运行时 → 隔离启动 → `/agent` 校验 → 刷新免费模型 → 探测 → 鉴权 401/403 → `/v1/models` → 优雅关停）。

---

## 🔴 安全修复（Supply chain / Auth）

1. **运行时下载来源强制属于白名单 registry**（`src-tauri/core/src/runtime.rs`）
   v1.0.0 的校验只比对 registry 字符串自身的 origin 与 tarball 的**路径前缀** `/包名/-/`，**从未把 `metadata.tarball` 的 origin 与白名单比对**，而下载循环恰恰先取 `metadata.tarball`。被篡改的 registry 元数据因此可以把 tarball 指向任意主机，`sha512` 形同虚设（哈希与 tarball 同源，校验通过也毫无意义）。
   现在按 `Url::origin()` 逐字节比对 `registry.npmjs.org` / `registry.npmmirror.com`，站外 URL 一律拒绝；新增红线守卫 `runtime_downloads_never_leave_the_registry_allow_list` 断言「一次都没向白名单外发出过请求」。
   *反向验证*：临时关闭该比对后，下载器的**第一个请求**就是站外 tarball —— 洞是活的。

2. **空白 `api-key` 文件不再被沿用**（`orchestration.rs::resolve_api_key` + `server.rs::authorized`）
   已存在但内容为空白（全空格/换行）的 `api-key` 会被 `trim()` 后继续沿用，期望头因此退化成 `Authorization: Bearer ` —— 本机任意进程都能匹配。
   现在空白内容一律重新生成 32 字节随机 hex，覆盖写入时**强制 `0600`**（`OpenOptions::mode` 只在新建时生效，故补 `set_permissions`，失败即上报不静默降级），`opencode.log` 只记「已重新生成」而**绝不记录键值**；`authorized()` 另加「空密钥一律拒绝」兜底，新增红线 `an_empty_api_key_authorizes_nothing` 逐路由覆盖。
   *反向验证*：删掉空键兜底后，`GET /health` + `Bearer ` 即被放行。

3. **文件权限失败不再被吞掉**
   `runtime.rs` 3 处（安装后的运行时二进制 `0755`、候选复制的临时文件 `0755`、隔离 XDG 目录 `0700`）与 `orchestration.rs` 数据目录 `0700` 一处的 `let _ = set_permissions(..)` 改为**传播错误**。隔离目录里放着 `OPENCODE_SERVER_PASSWORD` 与隔离配置副本，chmod 失败必须终止启动，而不是留下一个组/其他用户可读的「隔离」目录。

4. **管理路由表单源化**（防红线漏覆盖）
   `tests/red_lines.rs::routes()` 不再手写 `/admin/*` 清单，改为从 `server::ACTION_ROUTES` 派生；壳侧新增契约测试 `shell_action_routes_match_the_core_contract` 断言 `ADMIN_ROUTES` 与核心表逐项一致。今后新增管理动作会自动进入鉴权 / Origin / 空键三组红线，不会再被手写清单漏掉。

## ⚡ 并发与生命周期修复

5. **面板不再被动作冻结**（`src-tauri/src/lib.rs`）
   `core_action` / `restart_core` 原为**同步**命令，内含最长 15s 的 key 轮询 + 60s 阻塞式回环 HTTP + 最坏十余秒的停止等待，直接卡住 WebView 事件循环。现改为 **async 命令**，阻塞段下沉 `tauri::async_runtime::spawn_blocking`；前端 `invoke` 契约**未变**。

6. **启动失败不再伪装成就绪**
   此前 `setup` 里的 `core-failed` 必然早于面板注册监听（事件丢失），随后 `watch_status` 无条件把磁盘上**上一轮**的 `status.json` 当 `core-status` 推出去 ⇒ 面板显示「就绪」却没有核心在跑、重试入口消失。
   现在壳把失败原因记进 `startup_error`，故障态**不再推送残影状态**，改为按原因去重并每 ~4s **重播** `core-failed`（晚挂载的监听器仍能收到原因与重试入口）；恢复运行时清掉播报标记**并作废内容缓存**（重启后的 `status.json` 可能与故障前逐字节相同，不重置就永远不再推送）。

7. **探测队列按 id 移除，消除 panic 卡死路径**
   `run_probe_batch` 原先遍历 `selected` 却对 `pending` 做无条件 `pending.remove(0)`；`pending` 只收录带字符串 id 的模型，因此任一模型缺 id 就会在 tokio 任务内 panic，`probing` 永久为真，之后所有刷新 / 导入都被「请等待检测完成」挡死。现改为 `drop_pending(&mut pending, id)`（按 id 定位再删）。

8. **并发刷出去重（TOCTOU）**
   - `refresh()`：原先在登记刷新链位**之前**存在 `await`（代理解析），两个并发刷新（面板双击、启动刷新撞上手动刷新）都判定「无人刷新」，各自跑完整轮 —— 隔离运行时被重启两次、模型结果互相覆盖。现把「代理解析 + 清空为 reading」移到链位登记之后（spawned task 内）。
   - `start_probes`：占用声明由 `load` + `store` 改为**原子 `swap`**，两个并发 `/admin/probe` 不再能双双通过检查、各起一批探测。

## 🪟 行为变更：关闭窗口 = 退出应用（全平台一致）

此前**关闭窗口只是隐藏到托盘**，核心服务继续在后台运行，用户必须再去托盘点「退出 WB Bridge」才真正停服 —— 常见误解是「窗口关了，服务还占着端口和模型发布」。

现在 macOS / Windows / Linux 行为统一：

- `CloseRequested` → 立即隐藏窗口 → 后台线程执行与托盘「退出」**完全相同**的关停链路（`/admin/shutdown` 优雅停核心 → WorkBuddy 配置清理 → 关闭 tokio 运行时 → `exit(0)`），不再驻留托盘。
- 关停预算仍是 `STOP_BUDGET = 8s`；超时则记录并交由核心父进程看门狗收尾，**不留占端口的孤儿进程**。
- 收尾期间托盘左键「打开控制面板」不会把窗口唤回（`show_main` 见 `quitting` 即返回）。
- 托盘本身**保留**：运行期间仍可用于唤回面板、切换系统代理、重选 WorkBuddy 配置、以及「退出 WB Bridge」（与关窗等价）。
- ⚠️ **macOS 习惯变化**：红色关闭按钮 / `Cmd+W` 现在会终止应用，应用不会留在 Dock/菜单栏常驻；需要再次使用请重新打开应用。
- ⚠️ 该改动属**必须实机点一遍**的类别，本版**未在真实 GUI 中验证**（见上方发布状态）。

## 🖥 面板与新入口

9. **侧栏 5 个入口全部实现**（此前 4 个是「规划中」禁用占位）
   模型与服务（主从两栏）｜运行日志｜用量与额度｜WorkBuddy 集成｜关于与更新。后 4 个为**只读视图**：面板内唯一写动作仍是既有 `import`，不新增写路径、不读任意路径。
   - 新增**只读**壳命令 `read_log`（无参数，读数据目录 `opencode.log` 尾部，256KB / ≤1200 行，返回 `{text, truncated, bytes}`；不接受路径入参、不写文件）。IPC 现为 5 命令：`core_action` / `restart_core` / `core_running` / `data_dir_path` / `read_log`。
   - `status.json` 新增顶层 `usage`（`since` + `total{requests,ok,failed}` + 逐模型 `lastMs/avgMs`）：**探测不计**、**客户端取消整条不计**，与 `modelResults` 同法跨重启延续。`STATUS_SCHEMA_VERSION` 两侧保持 `1`（加法式兼容字段）。

10. **导入不再强制弹文件选择框**
    `/admin/import` 在不传 `modelsFile` 时就是「同步到已解析的配置」；只有定位不到配置文件时才需要用户手选。弹框因此是**兜底**而非无条件前置步骤，点「导入 WorkBuddy」不再每次都被要求选目录。

11. **界面与交互一致性固化**
    详情面板改为右侧**常驻分栏**（无遮罩、不 `position: fixed`、任何窗口宽度都不降级为上下堆叠；`Esc` / 「收起详情」按钮 / 窗口失焦三条等价收起路径）；默认窗口 980×680 → **1120×720**（最小 860×560），侧栏宽 224 → **208px** 并改分组导航。
    交互反馈分层：等待用 `spinner` / `.skeleton` / `.busy-bar`，结果用三态 `FeedbackBar`（pending 态不给关闭按钮）；**进度只显示壳推送的真实数据**，前端拿不到进度的动作一律只给文字。图标一律内联 SVG（`stroke="currentColor"`），颜色/时长/阴影全部取 `src/styles/variables.css` token，`prefers-reduced-motion` 下位移与淡入取消但循环动画保留。

## 📊 测试与质量基线

| 项 | v1.0.0 | v1.0.1 |
|---|---|---|
| 核心 `cargo test` | 197（lib 179 + js_parity 11 + red_lines 7） | **207（lib 187 + js_parity 11 + red_lines 9）** |
| 壳 `cargo test --lib` | — | **8 通过**（日志尾部 4 项 + 核心路由契约 1 项 + 退出链路 3 项） |
| `cargo clippy --all-targets`（核心 / 壳） | 0 warning | **0 warning** |
| `npx eslint .` | 0 problems | **0 problems** |
| JS↔Rust 对拍 | 271 个快照用例（11 组） | 同（默认不启动 Node，不联网、不真实下载） |

新增守卫/单测：tarball 来源红线、空 api-key 红线、tar 单成员解包（只取 `package/bin/<binary>`，其余成员绝不落盘、缺失成员不回退取别的成员）、空白密钥再生、`drop_pending` 按 id 移除、壳核心路由契约一致、关窗即退出的关停幂等与故障播报口径（3 项壳侧单测，含删掉 `stopping_by_request` 守卫即转红的反向验证）。

## 📦 安装包

由 CI（推 `v1.0.1` 标签）构建六平台产物；**本版尚未产出过任何安装包**，下列文件名待 CI 实跑后生效：

| 平台 | 文件 |
|---|---|
| macOS（Apple Silicon） | `WB Bridge_1.0.1_aarch64.dmg` + `.app`（ad-hoc 签名，无 Developer ID） |
| macOS（Intel） | `WB Bridge_1.0.1_x64.dmg` |
| Windows x64 | `WB Bridge_1.0.1_x64-setup.exe` + `.msi`（**未签名**，SmartScreen 需「更多信息 → 仍要运行」） |
| Windows ARM64 | `WB Bridge_1.0.1_arm64-setup.exe` |
| Linux x64 | `WB Bridge_1.0.1_amd64.AppImage` + `.deb` |
| Linux ARM64 | `WB Bridge_1.0.1_arm64.AppImage` + `.deb` |

> 上表六个 target 与 `release.yml` 的构建矩阵一一对应（macOS `aarch64`/`x86_64-apple-darwin`、Windows `x86_64`/`aarch64-pc-windows-msvc`、Linux `x86_64`/`aarch64-unknown-linux-gnu`）；文件名遵循 Tauri 默认的 `{productName}_{version}_{arch}.{ext}`，**尚未由 CI 产出验证**。

自动更新**未接入**：请重新下载安装包升级。

## ⚠ 已知限制

1. GUI 未实机验证（托盘、面板交互、壳侧重启与优雅退出链路）。
2. `release.yml` 未在 CI 跑过；其 `setup-node` 仍为 `node-version: 22`，与根 `package.json` 的 `engines.node >= 24` **不一致**（仅影响前端构建环境，未改）。
3. 「关于与更新」视图不接入自动更新；`src-tauri/updater-signing.env.example` 已删除，签名变量模板只留根 `.env.example` 一份。
4. 免费模型名单、额度与可用性由上游 OpenCode 决定，本工具不控制也不缓存额度。
5. 浅色主题下 3 处次要文字（`App.vue` 副标题与页脚、`ModelRow.vue` 耗时行）仍用 `--muted`，对比度约 4.01/4.01/3.73，**低于 WCAG AA 的 4.5:1**；既有问题，本版未处理。
6. 无确定性单测的既有条目：`service.pid` 单实例、探测路径禁用辅助模型转写、转写排除刚失败模型、格式类失败不撤发布、仅回环绑定、目录 `0700` 的失败分支、并发 `refresh()` 去重（依赖 await 交错，难以稳定构造）。

## 🧭 升级与兼容

- **对外契约未变**：HTTP 路由、错误码、`status.json` 既有字段、IPC 命令名与事件名全部保持 v1.0.0 原样；本版对 `status.json` 只做**加法式**新增（顶层 `usage`），`STATUS_SCHEMA_VERSION` 仍为 `1`。
- **数据目录不变**（macOS `~/Library/Application Support/app.wbbridge.desktop`（壳）或 `…/Buddy Bridge`（独立进程））。旧 `api-key` 仍可沿用；若旧文件内容为空白，本版会在首次启动时**重新生成**，WorkBuddy 侧需要重新导入以取新键。
- WorkBuddy `models.json` 写入规则不变：只清理/更新 `OWNER = buddy-bridge-v1` 名下条目，其余条目与元数据原样保留。
- 从 v1.0.0 直接替换安装包即可，无需迁移步骤；无需安装 Node。
- **交互习惯变化**：关闭窗口即退出应用（含后台服务），不再有「窗口关了服务仍在跑」的托盘驻留态；要让服务继续运行请**最小化**窗口而不是点关闭。

## 🙋 反馈

Issues：<https://github.com/trexwb/wbBridge/issues>

---

*本说明由仓库实读结果与当日真实测试输出整理；未验证项已显式标注，不代表功能已在真实桌面环境验证通过。*
