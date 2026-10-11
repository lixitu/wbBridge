# 壳 ↔ 核心 管理接口契约

> 本文件是「面板动作 → 核心路由」唯一的人工可读契约。以下三处实现必须保持一致：
> 1. `src-tauri/src/lib.rs` 的 `ADMIN_ROUTES` 与 `admin_route()`（壳侧动作表；托盘 pick 分支仍经
>    `admin_route("import")`，面板入口已收口为 8 个类型化命令，见下节）
> 2. `src-tauri/core/src/server.rs` 的 `ACTION_ROUTES` 与 `route_for()`（核心侧路由表）
> 3. 本文件的表格
>
> ⚠ 自动化现状（如实标注）：原先实读这三处并断言集合相等的契约测试 `src/core/test/contract.test.js`
> 已随 Node 核心一起归档。**两张代码内的动作表现在有自动门禁**：`src-tauri/src/lib.rs` 的
> `shell_action_routes_match_the_core_contract` 实读 `ADMIN_ROUTES` 与 `src-tauri/core/src/server.rs` 的
> `ACTION_ROUTES` 并断言逐项一致（`cargo test --lib`，在 `src-tauri/` 下运行）。
> **本文件的表格仍是第三份手写副本，没有自动校验**，改动任一动作表时须人工核对这里。
> 其余自动保护：`src-tauri/core/src/server.rs` 内单测（每个动作路由挂在 `/admin/` 下、不与 `/v1/*` 冲突）
> 与 `src-tauri/core/tests/red_lines.rs`（鉴权、Origin、限流、运行时下载来源等安全红线）。
>
> 核心是静态链接进壳的 Rust 库（crate `wbbridge-core`，编排层 `src-tauri/core/src/orchestration.rs`），
> 不再是独立 sidecar 进程；但两侧仍按本机回环 HTTP 通信，本契约的路由与鉴权语义不变。

## 动作表

| 动作（面板 / 托盘） | HTTP 路由 | 方法 | 载荷 | 说明 |
|---|---|---|---|---|
| `refresh` | `/admin/refresh` | POST | 无 | 重新读取**全部**免费模型目录（含重启运行时），并重新发布 |
| `probe` | `/admin/probe` | POST | `{ "model"?: string, "providers"?: string[] }` | 探测模型：`model` = 只重检这一个模型；`providers` = **只重取并重检这几个注册表平台**的模型（其余平台的目录与探测结果原样保留），与 `model` 互斥；两者都不传 = 探测全部。平台 id 必须来自注册表。该端点无论接受还是拒绝都返回 **202**，拒绝形态是 `{ "error": { "message", "type": "invalid_request_error" } }`（拼错/空数组/非数组/同时传两者）——**绝不**把形态不对的请求静默降级成全量探测（那会按整份目录消耗探测额度） |
| `import` | `/admin/import` | POST | `{ "modelsFile"?: string }` | 导入 / 切换 WorkBuddy 配置路径 |
| `system-proxy` | `/admin/system-proxy` | POST | `{ "enabled": boolean }` | 切换系统代理并重读模型 |
| `shutdown` | `/admin/shutdown` | POST | 无 | 先响应、再触发一次优雅退出 |
| `provider-status` | `/admin/provider-status` | POST | 无（不读请求体，同 `refresh`） | 平台注册表 + 每个平台「是否已配置」；**返回体不含任何 Key 材料** |
| `set-provider-key` | `/admin/set-provider-key` | POST | `{ "provider": string, "apiKey": string }` | 写入/更新一个平台的用户自持 Key（落 `<数据目录>/providers.json`，`0600`）；成功返回新的 `provider-status` 形态 |
| `clear-provider-key` | `/admin/clear-provider-key` | POST | `{ "provider": string }` | 删除一个平台的 Key（未配置过也返回成功，幂等）；同样返回新状态 |

> 🔴 凭据口径（这三条动作是本契约里**唯一**允许把用户凭据带进核心的入口）：
> `apiKey` 只经请求体进入，只落 `providers.json`；不出现在响应体、`status.json`、日志、面板偏好
> （`src/core/prefs.js` 的键白名单）与子进程环境（`runtime::ENV_ALLOW`）里。校验失败只回文案与
> `invalid_provider` / `invalid_provider_key`，**不回显入参**。守卫点：`src-tauri/core/src/providers.rs`
> 的单测、`server.rs::provider_actions_take_whole_body_and_never_echo_the_key`、
> `tests/red_lines.rs::provider_registry_is_reviewed_and_status_echoes_no_key_material`。
> 平台 id 只认 `providers.rs::PROVIDERS` 这张随版本发布的常量表，注册表外的一律 400。
> ⚠ **Stage 1 状态（2026-10-03）**：这三条只有核心 + 壳侧链路，**面板还没有入口视图**（Stage 5 才接），
> 且写入的 Key 目前**没有消费者**（`isolated_config()` 注入属 Stage 3）——按它们做动作不会多发布一个模型。

面板侧链路：`src/core/bridge.js` 的 `action(name, value)`（前端内核，不是后端）先把动作分发到
**8 个类型化 Tauri 命令**（`core_refresh`/`core_probe`/`core_import`/`core_system_proxy`/`core_shutdown`/
`core_provider_status`/`core_set_provider_key`/`core_clear_provider_key`，取代旧 `core_action(action, payload)`
通用代理：动作名不再是任意字符串，每条只映射一个 `/admin/*` 路由，参数经 serde 类型校验，路由与载荷语义不变）
→ `admin_route()`/`admin_call()`；例外是面板的 `restart` 动作，它不经 `/admin/*`，直接映射为壳命令
`restart_core`。壳另有 **四个**命令（均不经 `/admin/*`）：

| 壳命令 | 载荷 | 返回 / 语义 |
|---|---|---|
| `core_running` | 无 | 查询核心是否在运行 |
| `data_dir_path` | 无 | 取当前数据目录路径 |
| `read_log` | **无参数** | 只读数据目录下 `opencode.log` 的**尾部**，返回 `{ text, truncated, bytes }`；尾部截断 256KB / 最多 1200 行，不读 `opencode.log.previous`；不接受路径入参、不写任何文件 |
| `open_external` | `url`（仅 https） | 用系统浏览器打开外链；安全边界 = 仅 https + RFC 3986 字符白名单，URL 只以命令行参数传给系统打开器（不经 shell 解析） |

新增 / 改名只读命令必须同步 `src-tauri/src/lib.rs` 的 `generate_handler`；自有命令不经 capability 授权，
`capabilities/default.json` 无需改动。

**面板的第二条边界：Tauri 官方插件命令（自动更新）**，它**不经**上面的 8 个 `core_*` 管理命令 / `/admin/*`，因此不在上面的动作表里：

| 面板调用（`src/core/bridge.js`） | 底层插件命令 | 说明 |
|---|---|---|
| `checkUpdate()` | `plugin:updater|check` | 返回可序列化快照 `{ version, notes }` 或 `{ ok:false, error }`；`Update` 句柄只留在 `bridge.js` 模块内，不下传给视图 |
| `downloadUpdate(onProgress)` | `plugin:updater|download` / `|install` | 进度事件 `{ received, total }`；**只有上游给出 `contentLength` 时才有百分比**，否则只报已收字节；失败保留句柄以便重试 |
| `relaunchApp()` | `plugin:process|restart` | 走 `ExitRequested` → 壳的**有界**停止（`stop_core_bounded`）→ `cleanup_before_exit`，不是硬杀进程 |

- 这三个调用是面板**唯一**的联网入口，联网发生在 Rust 侧（插件命令），不经 WebView `fetch`，故 CSP `default-src 'self'` 不必放宽。
- 插件命令**必须**在 `src-tauri/capabilities/default.json` 声明：现为 `updater:default` + `process:allow-restart`；刻意不使用 `process:default`（含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链）。
- 更新状态机集中在 `src/core/update.js`（`idle|checking|available|downloading|ready|uptodate|error`），视图只渲染与触发，不得自行持有状态。
- `plugins.updater.pubkey` 必须是**内联公钥字符串**（不能写文件路径），端点强制 HTTPS。

状态由壳轮询 `status.json` 后推给面板：`core-status` 是**剥掉 `activity` 与 `modelResults` 的轻量快照**（顶层 `usage` 仍随该快照下发），
这两个字段单独走 `core-activity`（每 500ms 轮询只在内容变化时发），失败走 `core-failed`——**壳判定核心不可用（启动失败或任务已结束）期间不再推送残影 `status.json`，改为每 ~4s 重播一次 `core-failed`**，因此面板监听器必须对同一原因的重复投递幂等；核心恢复后壳会作废「上一份内容」缓存并重新推送一次完整状态。面板
`src/core/bridge.js` 必须把两路合并成完整状态再交给订阅者，否则逐模型明细与活动文案永远为空。

## 鉴权与结果判据

- 所有 `/admin/*` 必须带 `Authorization: Bearer <api-key>`；`/health` 同样要鉴权。key 由核心生成并写入
  数据目录下的 `api-key` 文件（权限 0600），仅本机可读。核心侧比较使用
  `src-tauri/core/src/server.rs::constant_time_eq`（`subtle::ConstantTimeEq`，定时安全）；请求带任意非空
  `Origin` 头一律 403。
- 壳侧调用是 `admin_call(port, key, route, body)`（回环 HTTP，默认超时 60s；`/admin/shutdown` 收紧为 5s）。
  成功判据以 **HTTP 状态码 2xx** 为准（壳侧与核心侧一致）；非 2xx 时错误信息取响应体 `error.message`，
  缺省为「请求失败」。
- `/admin/shutdown` 返回非 2xx 表示核心未能优雅退出。壳侧不再有「SIGTERM → 强杀」这一步（核心与壳同进程）：
  改为等待核心的退出回调（`orchestration::set_exit_hook`，预算 10s），超时后强制关闭核心的专用 tokio
  运行时（`shutdown_timeout`，在途任务被丢弃，等价于旧版强杀）。核心任何退出路径都只能触发回调，
  绝不能终止壳进程。
- 面板主动终止核心、或核心任务非预期结束后的重启，均走同一个 `shutdown` 动作（核心侧会先完成
  WorkBuddy 配置清理再收尾），重启由壳命令 `restart_core` 重新装配一次核心实例。

## 对外模型接口：`WB · auto` 合成模型名（v1.1.14 落地、v1.1.15 起写入插件配置，2026-10-11 实机取证已端到端跑通）

> 实现：`src-tauri/core/src/auto.rs`（纯函数选择器）+ `src-tauri/core/src/server.rs` 的两处拦截
> （`/v1/models` 追加条目、`chat()` 在记账键首次取读前改写 `body["model"]`）
> + `src-tauri/core/src/sync.rs` 的 `auto_entry`（写进插件配置的那一条）。
> 设计与逐条取舍见 `docs/plans/2026-10-10-wb-auto-smart-router.md`（§10 是落地实录）。
> 🔴 **名字**：对外唯一字面量是 `WB · auto`（分隔符逐字节 = 空格 + U+00B7 + 空格，与 `OC · 名称` 同形）；
> v1.1.14 之前写作 `WB.auto`，v1.1.15 全局改名后**旧名字不再被任何入口接受**（`1.1.11~1.1.14` 从未构建、从未打标签，v1.1.15 现已发布且只用新名，改名不伤及已交付二进制）。

| 项 | 契约 |
|---|---|
| `GET /v1/models` | **池非空**时列表末位多一条 `{ "id": "WB · auto", "object": "model", "owned_by": "opencode", "name": "WB · auto" }`；池为空（探测全失败/尚未探测）**不追加**——不给客户端一个必然 400 的入口 |
| `POST /v1/chat/completions` | `model` 填 `WB · auto` 即由核心按请求内容在**可用池**里选一个实际模型转发；其余字段、错误码、SSE 帧序与手动指定模型时**逐字一致** |
| 选档规则 | 纯文本 → 全池（含仅对话模型）；带图片 part → 要求 `images`；带非空 `tools` → 要求 `toolcall` 且非 `chatOnly`；图片 + 工具 → 要求两者；该档候选为空 → **退回全池**（不新增错误码，随后由 `prepare` 给出既有 400）；同档多候选 → 均匀随机取一（分散负载） |
| 响应与记账 | 响应的 `model` 字段（流式与非流式都一样）= **实际选中模型的全限定 id**（如 `modelscope/Qwen/Qwen3-8B`），**不是** `WB · auto`；`status.json` 的 `usage.models`、`modelResults` 与 `validated` 的键同样是这个实际 id |
| 失败即摘除 | 复用既有链路，本功能零新增判决：受限 / 超时 / 不可用类的失败会 `validated.remove(实际 id)` → `/v1/models` 与面板状态当次即收缩；格式类四项（`invalid_model_output` / `invalid_tool_call` / `native_tool_activity` / `output_truncated`）按数据红线**不摘除**；客户端取消、429 `busy`、408 读体超时、本地 400 全部发生在记账之前，**不构成对任何模型的判决** |
| 插件配置（**v1.1.15 起的改动**） | 发布集非空时，`WB · auto` 会作为一条**独立条目**写进 WorkBuddy 与 CodeBuddy 的 `models.json`，排在本工具名下逐模型条目**之后**，并同样进该文档的可用模型列表——插件的模型选择器里因此选得到它。v1.1.14 裁定的是「绝不写进插件」，用户实测后推翻（「不然谁都不知道如何使用」）。字段形态（保守声明，能力字段宁少勿多）：`{ id, name }` = `WB · auto`（该 `id` **刻意绕过** `client_model_id`，否则会生成 `OC · WB · auto`、与 `chat()` 的字面比较永不匹配）；`vendor` = `Custom`；`url` / `apiKey` / `buddyBridgeOwner` 与逐模型条目同源；`supportsToolCall` = **池里是否存在能接工具调用的候选**（`auto::any_tool_capable`，与核心选档共用同一谓词，因此两侧不可能分叉）；`supportsImages` = `false`；`maxInputTokens` = 池内**最小**上下文（每次同步重算，取 `input`、缺失回落 `context`，非数值/0/负数跳过；一个都没有就**不写该键**）；**不写** `reasoning` 与 `maxOutputTokens`。发布集为空则整条不写；用户手动建的同名 `id` 条目按数据红线**不被覆盖**（本条直接放弃）。✅ 2026-10-11 实机取证：两个插件的 `models.json` 均含 `WB · auto`，且 `status.json.usage` 累计 37 次真实请求（ok 32 / failed 5），证实插件侧能识别并按它发请求 |
| `sync.count` 语义（不变） | `status.json` 的 `sync.count` 与 `sync.targets.<目标>.count` **只数真实模型**，不把 `WB · auto` 计入；`status.json` 的模型列表（面板数据源）也**不含**该合成路由，面板不会因此多出一行 |
| 接入边界（不变） | 仅监听 `127.0.0.1`；一切请求都要 `Authorization: Bearer <api-key>`；**任何非空 `Origin` 一律 403**（浏览器内嵌 `fetch` 用不了，CLI / 桌面 agent 直连可用）；并发 ≤8、请求体 ≤8MB |

## 写入目标与检测规则

模型发布（`sync_published`）向**所有已检测到的插件**分发写入，目标集合与定位优先级由
`src-tauri/core/src/targets.rs` 唯一定义（`Target::ALL`，顺序即分发顺序）：

| 目标 | 配置文件（同名同形） | 定位优先级 | `status.json` 字段 | `sync.targets` 键 |
|---|---|---|---|---|
| WorkBuddy | `models.json`（默认 `~/.workbuddy/`） | `BUDDY_MODELS_FILE` > settings `workBuddyModelsFile` > `WORKBUDDY_CONFIG_DIR` > `WORKBUDDY_DATA_FOLDER_NAME` > 平台默认 | `modelsFile` | `workBuddy` |
| CodeBuddy | `models.json`（默认 `~/.codebuddy/`） | `BUDDY_CODEBUDDY_MODELS_FILE` > settings `codeBuddyModelsFile`（预留，当前恒空）> `CODEBUDDY_CONFIG_DIR` > `CODEBUDDY_DATA_FOLDER_NAME` > 平台默认 | `codeBuddyModelsFile` | `codeBuddy` |

- **「已安装」的判定只有一条：定位函数返回 `Some`**（候选文件必须通过 `validate_models_file` 的路径与形状校验）。不猜进程、不猜注册表；显式位置失效时绝不静默回退（与 WorkBuddy 既有约定一致）。
- 两个目标共用同一套幂等写入（`sync::sync_models`：OWNER 归属标记、冲突不覆盖、文件锁、二次读取、原子替换），CodeBuddy 不是第二套实现。**不再写 `.bak` 备份**（2026-10-09 用户裁定：备份从来没有读取方、面板也没有恢复入口，却按发布次数在插件配置目录里无限堆积）；每次同步（含内容无变化的那次）会按同名族白名单 `<配置文件名>.buddy-bridge-<纯 ASCII 数字>.bak` 清扫旧版本攒下的存量备份，用户自己命名的 `*.bak` 与任何其它文件一律不碰。
- **v1.1.15 起，发布集非空时两个目标的 `models.json` 都会多一条 `WB · auto`**（形态见上节「插件配置」行）：由 `sync::auto_entry` 生成、走同一套 OWNER 归属与冲突不覆盖规则，排在逐模型条目之后；发布集为空时不写、下一轮空同步也会把它一并删掉。生产发布链是唯一开启该写入的地方（`orchestration::sync_published` 传 `SyncOptions.auto_route = true`），**对拍路径与任何调用方默认都是 `false`**——JS 侧从未有这一条，默认值保证 `tests/fixtures` 逐字节不变。
- `sync` 形状（加法式变更，`schemaVersion` 不递增）：新增 `sync.targets.{workBuddy,codeBuddy}`，每个目标 `status` ∈ `ok|missing|error`，`ok` 携带 `count/changed`（早期版本还带 `backup`，随「不再写备份」一并移除，面板从未读过它），`missing` 携带 `reason`，`error` 携带 `error`；顶层 `count` = 各成功目标之和；顶层 `error` 仅在「至少一个目标被定位且全部定位目标都失败」时出现（部分失败不算顶层失败）；新增顶层 `codeBuddyModelsFile` 字段。⚠ `count` **不含**那条 `WB · auto`（`owned_count` 显式排除别名），所以「插件里比 `count` 多显示一行」是预期结果，不是记账错乱。
- 面板侧：集成视图逐目标展示状态（缺 `targets` 时回退单行展示，兼容旧核心）；底部摘要行的措辞已不绑定单一目标。

## 运行形态与传输落点

| 形态 | 入口 | 端口 | 数据目录（`api-key` / `status.json` 所在处） |
|---|---|---|---|
| 桌面壳（正式形态） | `src-tauri/src/lib.rs::start_core` → `orchestration::run(StartOptions { data_dir, port, handle_signals: false })` | `pick_port()`：默认 41980，被占用时回退到系统分配的空闲端口 | Tauri `app_data_dir`（macOS `~/Library/Application Support/app.wbbridge.desktop`，identifier `app.wbbridge.desktop`） |
| 独立核心二进制 | `src-tauri/core/src/main.rs`（`handle_signals: true`，读 `BUDDY_*` 环境变量） | `BUDDY_PORT`，默认 41980 | `BUDDY_DATA_DIR`，否则平台默认（macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`） |

> 两种形态共用同一份编排代码，但数据目录互不相通；`/health` 排查时必须按运行形态取对应目录里的 `api-key`，
> 壳形态的端口也可能不是 41980（以 `status.json` 的 `endpoint` 为准）。

```sh
# 独立核心二进制（平台默认目录）
curl -H "Authorization: Bearer $(cat "$HOME/Library/Application Support/Buddy Bridge/api-key")" \
     http://127.0.0.1:41980/health
# 桌面壳形态（Tauri app_data_dir，端口见同目录 status.json 的 endpoint）
curl -H "Authorization: Bearer $(cat "$HOME/Library/Application Support/app.wbbridge.desktop/api-key")" \
     http://127.0.0.1:41980/health
```
> 壳不再 `spawn` 核心进程，而是在自己的专用 tokio 运行时上装配核心任务并持有其完整生命周期；
> `bundle.externalBin` 为空数组，`src-tauri/binaries/` 已删除。
