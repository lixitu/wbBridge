# 壳 ↔ 核心 管理接口契约

> 本文件是「面板动作 → 核心路由」唯一的人工可读契约。以下三处实现必须保持一致：
> 1. `src-tauri/src/lib.rs` 的 `ADMIN_ROUTES` 与 `admin_route()`（壳侧动作表，托盘与面板共用）
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
| `refresh` | `/admin/refresh` | POST | 无 | 重新读取免费模型目录（含重启运行时） |
| `probe` | `/admin/probe` | POST | `{ "model"?: string }` | 探测模型；不传 model 表示探测全部 |
| `import` | `/admin/import` | POST | `{ "modelsFile"?: string }` | 导入 / 切换 WorkBuddy 配置路径 |
| `system-proxy` | `/admin/system-proxy` | POST | `{ "enabled": boolean }` | 切换系统代理并重读模型 |
| `shutdown` | `/admin/shutdown` | POST | 无 | 先响应、再触发一次优雅退出 |

面板侧链路：`src/core/bridge.js` 的 `action(name, value)`（前端内核，不是后端）→ Tauri 命令
`core_action(action, payload)` → `admin_route()` → `admin_call()`；例外是面板的 `restart` 动作，它不经
`/admin/*`，直接映射为壳命令 `restart_core`。壳另有 **三个**命令（均不经 `/admin/*`）：

| 壳命令 | 载荷 | 返回 / 语义 |
|---|---|---|
| `core_running` | 无 | 查询核心是否在运行 |
| `data_dir_path` | 无 | 取当前数据目录路径 |
| `read_log` | **无参数** | 只读数据目录下 `opencode.log` 的**尾部**，返回 `{ text, truncated, bytes }`；尾部截断 256KB / 最多 1200 行，不读 `opencode.log.previous`；不接受路径入参、不写任何文件 |

新增 / 改名只读命令必须同步 `src-tauri/src/lib.rs` 的 `generate_handler`；自有命令不经 capability 授权，
`capabilities/default.json` 无需改动。

**面板的第二条边界：Tauri 官方插件命令（自动更新）**，它**不经** `core_action` / `/admin/*`，因此不在上面的动作表里：

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
