# 壳 ↔ 核心 管理接口契约

> 本文件是「面板动作 → 核心路由」唯一的人工可读契约。以下三处实现必须保持一致：
> 1. `src-tauri/src/lib.rs` 的 `ADMIN_ROUTES` 与 `admin_route()`（壳侧动作表，托盘与面板共用）
> 2. `src-tauri/core/src/server.rs` 的 `ACTION_ROUTES` 与 `route_for()`（核心侧路由表）
> 3. 本文件的表格
>
> ⚠ 自动化现状（如实标注）：原先实读这三处并断言集合相等的契约测试 `src/core/test/contract.test.js`
> 已随 Node 核心一起归档，**当前仓库没有三处一致性的自动门禁**，改动任一动作表必须人工核对另外两处。
> 现存的自动保护只有 `src-tauri/core/src/server.rs` 内的单测（断言每个动作路由都挂在 `/admin/` 下、且不与
> `/v1/*` 等业务路由冲突）与 `src-tauri/core/tests/red_lines.rs`（鉴权、Origin、限流等安全红线）。
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
`/admin/*`，直接映射为壳命令 `restart_core`。壳另有 `core_running`、`data_dir_path` 两个命令，状态由壳
轮询 `status.json` 后以 `core-status` / `core-failed` 事件推给面板。

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
