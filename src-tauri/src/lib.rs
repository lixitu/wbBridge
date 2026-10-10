// WB Bridge Tauri 壳：进程内核心的生命周期、托盘、状态推送与 IPC 代理。
// 核心（wbbridge_core::orchestration）跑在壳内的专用 tokio 运行时上，仍在 127.0.0.1:<port>
// 提供同一套 HTTP 契约；本壳负责：装入 data_dir/port → 轮询 status.json 推送事件 → 退出时经
// POST /admin/shutdown 优雅停止（等待核心完成 WorkBuddy 配置清理与串行写链落盘）。
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use wbbridge_core::orchestration::{self, StartOptions};

/// 进程内运行的核心：专用多线程 tokio 运行时 + 监听端口。
/// 运行时必须与壳自己的异步上下文隔离，否则 `block_on` 会嵌套 panic；持有它即持有核心任务。
struct RunningCore {
    runtime: tokio::runtime::Runtime,
    port: u16,
}

struct AppState {
    core: Mutex<Option<RunningCore>>,
    data_dir: PathBuf,
    quitting: AtomicBool,
    /// 核心编排任务是否已结束（由核心注入的 exit hook 置位）。
    core_stopped: AtomicBool,
    /// 核心结束时的退出码；0 表示按用户意愿停止。
    core_code: AtomicU8,
    /// 本轮停止是否由壳主动发起（主动停不当作故障上报）。
    stopping_by_request: AtomicBool,
    proxy_on: Mutex<bool>,
    tray_proxy: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    /// 托盘句柄：核心退出等状态变化时更新 tooltip。
    tray: Mutex<Option<tauri::tray::TrayIcon<tauri::Wry>>>,
    /// 核心 API key 的进程内缓存：首次从 `api-key` 文件读到后记在这里，避免每次管理调用都
    /// 重新走一遍最长 15s 的轮询等待（key 由核心写盘后不再变化，重启核心不影响该缓存）。
    api_key: Mutex<Option<String>>,
    /// 最近一次「核心没能起来」的原因（setup 或 restart_core 失败）。此刻磁盘上的
    /// status.json 只是上一轮的残影，不能当成实时状态推给面板。
    startup_error: Mutex<Option<String>>,
}

/// 壳只注册一次 exit hook（核心侧用 OnceLock 存），因此 app handle 也只需保存一份。
static SHELL_APP: OnceLock<AppHandle> = OnceLock::new();

/// 选监听端口：默认 41980，被占用时由系统分配。
fn pick_port() -> u16 {
    if let Ok(l) = TcpListener::bind(("127.0.0.1", 41980)) {
        let port = l.local_addr().map(|a| a.port()).unwrap_or(41980);
        drop(l);
        return port;
    }
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(41980)
}

fn api_key(data_dir: &Path) -> Result<String, String> {
    let file = data_dir.join("api-key");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(text) = fs::read_to_string(&file) {
            let key = text.trim().to_string();
            if !key.is_empty() {
                return Ok(key);
            }
        }
        if Instant::now() >= deadline {
            return Err("读取核心 API key 超时".into());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// 取核心 API key：优先用进程内缓存，未命中再走文件轮询并把结果回填缓存。
fn core_key(state: &AppState) -> Result<String, String> {
    if let Some(key) = state.api_key.lock().unwrap().clone() {
        return Ok(key);
    }
    let key = api_key(&state.data_dir)?;
    *state.api_key.lock().unwrap() = Some(key.clone());
    Ok(key)
}

/// 调核心管理接口（路由表与 `src-tauri/core/src/server.rs` 的 `ACTION_ROUTES` 一一对应）。
fn admin_call(port: u16, key: &str, route: &str, body: Option<&Value>) -> Result<Value, String> {
    admin_call_with(port, key, route, body, Duration::from_secs(60))
}

/// 同上，但可指定超时：退出路径必须用更短的预算（见 graceful_stop_process）。
fn admin_call_with(
    port: u16,
    key: &str,
    route: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{port}{route}");
    let call = ureq::post(&url)
        .timeout(timeout)
        .set("Authorization", &format!("Bearer {key}"))
        .send_json(body.unwrap_or(&Value::Null));
    let response = match call {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(format!("核心服务请求失败：{e}")),
    };
    // HTTP 状态码才是权威判据：只看响应体里的 error 字段会把「状态码非 2xx 但没带 error」
    // 的响应当成成功（与 `src-tauri/core/src/server.rs` 的路由约定对齐）。
    let status = response.status();
    let text = response.into_string().map_err(|e| e.to_string())?;
    let json = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    if !(200..300).contains(&status) {
        let message = json.get("error").and_then(|error| error.get("message")).and_then(Value::as_str)
            .unwrap_or("请求失败").to_string();
        return Err(message);
    }
    Ok(json)
}

/// 在壳内启动核心：专用 tokio 运行时 + 显式注入 data_dir/port（同进程改环境变量不安全）。
fn start_core(app: &AppHandle) -> Result<RunningCore, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    let port = pick_port();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("wbbridge-core")
        .build()
        .map_err(|e| format!("启动核心异步运行时失败：{e}"))?;
    let state = app.state::<AppState>();
    state.core_stopped.store(false, Ordering::SeqCst);
    state.core_code.store(0, Ordering::SeqCst);
    runtime.spawn(async move {
        let code = orchestration::run(StartOptions {
            data_dir: Some(data_dir.to_string_lossy().into_owned()),
            port: Some(port),
            // 生命周期归壳：核心不注册 SIGTERM/SIGINT，也不看父进程（同进程内无意义）。
            handle_signals: false,
        })
        .await;
        eprintln!("核心编排任务已结束（退出码 {code}）");
    });
    Ok(RunningCore { runtime, port })
}

/// 等待核心编排任务结束（exit hook 已置位）。返回是否在预算内结束。
fn wait_core_stopped(state: &AppState, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if state.core_stopped.load(Ordering::SeqCst) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 停止单个核心实例：HTTP 优雅退出 → 等待编排任务收摊 → 关闭专用运行时。
/// `key` 为 API key（拿不到就直接跳过 HTTP 走超时兜底）。
fn stop_core(core: RunningCore, key: Option<&str>, state: &AppState) {
    let Some(key) = key else {
        // 没有 key 就无法调用 /admin/shutdown；核心的串行写链还在跑，只能给运行时一个
        // 有界的排空窗口，超时会丢弃未完成任务（与 Node 侧强杀等价）。
        core.runtime.shutdown_timeout(Duration::from_secs(5));
        state.core_stopped.store(true, Ordering::SeqCst);
        return;
    };
    state.stopping_by_request.store(true, Ordering::SeqCst);
    // 退出预算必须短：核心正常时 1s 内即退出，卡住时不该让壳等满几十秒（面板点「重启」尤其明显）。
    // 优雅退出 + 等待编排任务收摊，两步都成立才算干净停止；任一不成立都走强制关闭兜底。
    if admin_call_with(core.port, key, "/admin/shutdown", None, Duration::from_secs(5)).is_ok()
        && wait_core_stopped(state, Duration::from_secs(10))
    {
        core.runtime.shutdown_timeout(Duration::from_secs(2));
        return;
    }
    // 优雅路径没走通：给在途任务一个短排空窗口后强制关闭运行时。
    eprintln!("核心未在预算内优雅退出，强制关闭其运行时");
    core.runtime.shutdown_timeout(Duration::from_secs(5));
    state.core_stopped.store(true, Ordering::SeqCst);
}

/// 停止核心：HTTP 优雅退出（核心随后清理 WorkBuddy 配置并写终态）→ 关闭运行时。
fn graceful_stop(state: &AppState) {
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(core) = state.core.lock().unwrap().take() else { return };
    let key = core_key(state).ok();
    stop_core(core, key.as_deref(), state);
}

fn show_main(app: &AppHandle) {
    // 退出流程进行中（关闭窗口后到进程结束之间可能有数秒）不要把窗口再唤回来。
    if app.state::<AppState>().quitting.load(Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 退出预算：超过则不再等待，交由核心的父进程监护（BUDDY_PARENT_PID）自行收尾。
const STOP_BUDGET: Duration = Duration::from_secs(8);

/// 在后台线程执行 graceful_stop，并在调用线程上有界等待。
/// 停止序列（HTTP 优雅停 → SIGTERM → 强杀）最坏数秒到数十秒，直接在事件循环线程上跑会让
/// 「点退出后应用没反应」，因此下沉到后台线程；即便超时也不再阻塞，核心检测到父进程消失会自行退出。
fn stop_core_bounded(app: &AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    thread::spawn(move || {
        graceful_stop(handle.state::<AppState>().inner());
        let _ = tx.send(());
    });
    if rx.recv_timeout(STOP_BUDGET).is_err() {
        eprintln!("核心停止超时（>{STOP_BUDGET:?}），交由核心的父进程监护自行退出");
    }
}

fn quit_app(app: &AppHandle) {
    stop_core_bounded(app);
    app.cleanup_before_exit();
    app.exit(0);
}

/// 非阻塞探测核心是否已经退出；已退出时返回原因描述（退出码 / 信号）。
/// `Child::try_wait` 会缓存退出状态，重复调用返回同一结果，因此可安全地周期探测。
/// 核心是否已结束运行：exit hook 置位即视为已退出，并给出原因描述。
/// 进程内形态没有子进程可探测，终态由核心自己通过 hook 交出（含退出码）。
fn core_exit(state: &AppState) -> Option<String> {
    if !state.core_stopped.load(Ordering::SeqCst) || state.stopping_by_request.load(Ordering::SeqCst) {
        return None;
    }
    Some(format!("退出码 {}", state.core_code.load(Ordering::SeqCst)))
}

/// status.json 的结构版本。与 `src-tauri/core/src/orchestration.rs` 的 `STATUS_SCHEMA_VERSION` 必须同步 +1；
/// 不一致时只提示不阻断（可能只是其中一侧升级了），避免两侧互相锁死。
/// 口径：向后兼容的**加法式**顶层字段（如新增的 `usage`，旧壳忽略未知字段即可）**不**递增本值；
/// 只有删除 / 改名顶层字段或改变既有字段语义（非兼容变更）时才同步 +1。
const STATUS_SCHEMA_VERSION: u64 = 1;

/// 核心当前是否处于「没有服务可看」的故障态，返回（面向面板的原因文案，是否因核心退出）。
///
/// 两个来源：exit hook 报告核心已结束；或壳自己启动失败（setup / restart_core）后没有核心在跑。
/// 用户主动重启/退出（`stopping_by_request`）不算故障，否则重启窗口里会误报「未运行」。
fn service_down(state: &AppState) -> Option<(String, bool)> {
    if let Some(reason) = core_exit(state) {
        return Some((
            format!("核心服务已退出（{reason}），可在面板点“重试”重启"),
            true,
        ));
    }
    let running = state.core.lock().unwrap().is_some();
    if !running {
        if let Some(message) = state.startup_error.lock().unwrap().clone() {
            return Some((message, false));
        }
    }
    None
}

/// status.json 这一轮是否需要真读内容。快路径**只在拿得到 mtime 时**才允许跳过：
/// `stamp` 为 `None`（文件系统不暴露 mtime，如某些网络挂载 / FUSE）时必须回落到读内容，
/// 否则 stamp 恒定、长度不变的改写会被永久跳过，面板状态就此冻结。
fn status_read_needed(
    stamp: Option<(std::time::SystemTime, u64)>,
    last_stamp: Option<(std::time::SystemTime, u64)>,
) -> bool {
    stamp.is_none() || stamp != last_stamp
}

/// 轮询核心写的 status.json：内容变化即推送事件、同步托盘勾选态，并监督核心任务是否已结束。
fn watch_status(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let file = state.data_dir.join("status.json");
        let mut last = String::new();
        // 上一次成功读取时的 (mtime, 长度)：稳态下核心不写盘、内容一成不变，
        // 原先每 500ms 仍要 read_to_string 一次（实测文件 8.6 KB）再整份比对。
        // 先用 mtime + 长度做便宜的前置过滤，两者都不变才跳过；拿不到 mtime 时为 None，
        // 快路径自动失效（见 status_read_needed）。
        let mut last_stamp: Option<(std::time::SystemTime, u64)> = None;
        let mut warned = false;
        // 已向面板播报的故障原因：原因变化或攒满一个重播窗口才再发，避免每 500ms 刷屏。
        let mut announced: Option<String> = None;
        let mut ticks = 0usize;
        // 退出后不再轮询、不再向已关闭的窗口 emit。
        while !state.quitting.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(500));
            ticks = ticks.wrapping_add(1);
            // 故障态下 status.json 停在上一轮的 ready（甚至更早），把它当实时状态推出去就等于
            // 让面板显示「就绪」却没有核心在跑、连重试入口都不见；改为持续播报失败原因，
            // 直到晚挂载的面板监听器收到（setup 里那一次 emit 必然早于监听注册）。
            if let Some((message, by_exit)) = service_down(&state) {
                if announced.as_deref() != Some(message.as_str()) || ticks.is_multiple_of(8) {
                    announced = Some(message.clone());
                    if let Some(tray) = state.tray.lock().unwrap().as_ref() {
                        let _ = tray.set_tooltip(Some(if by_exit {
                            "WB Bridge — 核心服务已退出，可在面板重启"
                        } else {
                            "WB Bridge — 核心服务未运行，可在面板重启"
                        }));
                    }
                    let _ = app.emit("core-failed", serde_json::json!({ "message": message }));
                }
                continue;
            }
            // 恢复运行：清掉故障播报标记并还原托盘 tooltip，同时作废「上一份内容」与「上一次
            // stamp」两重缓存 —— 重启后写出的 status.json 可能与故障前逐字节相同，不重置就
            // 永远不会再推送；只清 `last` 而留下 stamp，这道保障会被快路径吞掉。
            if announced.take().is_some() {
                last.clear();
                last_stamp = None;
                if let Some(tray) = state.tray.lock().unwrap().as_ref() {
                    let _ = tray.set_tooltip(Some("WB Bridge"));
                }
            }
            // 先用 (mtime, 长度) 做便宜的前置过滤：核心只在变化时写盘，稳态下这两者都不动，
            // 就不必每 500ms 再 read_to_string 一次整份内容（实测 status.json 8.6 KB）
            // 后再逐字节比对。stamp 只在成功读到内容之后记录，避免读失败的那一轮被跳过。
            let Ok(meta) = fs::metadata(&file) else { continue };
            let stamp = meta.modified().ok().map(|mtime| (mtime, meta.len()));
            if !status_read_needed(stamp, last_stamp) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&file) else { continue };
            last_stamp = stamp;
            if text == last {
                continue;
            }
            let Ok(parsed) = serde_json::from_str::<Value>(&text) else { continue };
            last = text;
            if !warned {
                match parsed.get("schemaVersion").and_then(Value::as_u64) {
                    Some(STATUS_SCHEMA_VERSION) => {}
                    Some(found) => {
                        warned = true;
                        eprintln!("status.json schemaVersion={found} 与壳预期的 {STATUS_SCHEMA_VERSION} 不一致，面板按可用字段渲染");
                    }
                    None => {
                        warned = true;
                        eprintln!("status.json 缺少 schemaVersion（核心可能为旧版），建议重新构建核心");
                    }
                }
            }
            let proxy = parsed.get("useSystemProxy").and_then(Value::as_bool).unwrap_or(false);
            *state.proxy_on.lock().unwrap() = proxy;
            if let Some(item) = state.tray_proxy.lock().unwrap().as_ref() {
                let _ = item.set_checked(proxy);
            }
            // 事件拆两路：core-status 去掉 activity / modelResults 两个大数组，只推轻量状态；
            // core-activity 推活动与模型明细。避免每 500ms 跨 IPC 搬运整份 status.json。
            let mut light = parsed.clone();
            let activity = light.get_mut("activity").map(std::mem::take).unwrap_or(Value::Null);
            let results = light.get_mut("modelResults").map(std::mem::take).unwrap_or(Value::Null);
            let _ = app.emit("core-status", light);
            let _ = app.emit("core-activity", serde_json::json!({ "activity": activity, "modelResults": results }));
        }
    });
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "quit" => quit_app(app),
        "proxy" => {
            let state = app.state::<AppState>();
            let next = !*state.proxy_on.lock().unwrap();
            let core = state.core.lock().unwrap().as_ref().map(|c| (c.port, state.data_dir.clone()));
            if let Some((port, data_dir)) = core {
                thread::spawn(move || {
                    let Ok(key) = api_key(&data_dir) else {
                        eprintln!("[tray] 无法读取 api-key，系统代理切换未执行");
                        return;
                    };
                    // 失败必须留下痕迹：托盘勾选会在下一次状态轮询被核心权威值纠正，
                    // 但若不记录原因，用户只会看到勾选自己弹回去。
                    if let Err(error) =
                        admin_call(port, &key, "/admin/system-proxy", Some(&serde_json::json!({ "enabled": next })))
                    {
                        eprintln!("[tray] 系统代理切换失败：{error}");
                    }
                });
            }
        }
        "pick" => {
            let app = app.clone();
            app.dialog().file()
                .add_filter("WorkBuddy models.json", &["json"])
                .pick_file(move |path| {
                    let Some(path) = path else { return };
                    let app = app.clone();
                    thread::spawn(move || {
                        let state = app.state::<AppState>();
                        let core = state.core.lock().unwrap().as_ref().map(|c| c.port);
                        if let Some(port) = core {
                            match (api_key(&state.data_dir), admin_route("import")) {
                                (Ok(key), Ok(route)) => {
                                    // 导入失败只在日志留痕：面板的 FeedbackBar 由 run() 的返回值驱动，
                                    // 托盘这条链路没有可复用的错误展示位。
                                    if let Err(error) = admin_call(
                                        port,
                                        &key,
                                        route,
                                        Some(&serde_json::json!({ "modelsFile": path.to_string() })),
                                    ) {
                                        eprintln!("[tray] 导入 WorkBuddy 配置失败：{error}");
                                    }
                                }
                                (Err(key), _) => eprintln!("[tray] 无法读取 api-key，导入未执行：{key}"),
                                (_, Err(route)) => eprintln!("[tray] 导入动作路由缺失：{route}"),
                            }
                        }
                        show_main(&app);
                    });
                });
        }
        _ => {}
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开控制面板", true, None::<&str>)?;
    let proxy = CheckMenuItem::with_id(app, "proxy", "使用系统代理", true, false, None::<&str>)?;
    let pick = MenuItem::with_id(app, "pick", "选择 WorkBuddy 配置…", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 WB Bridge", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sep1, &proxy, &pick, &sep2, &quit])?;
    let icon = if cfg!(target_os = "macos") {
        tauri::include_image!("icons/tray.png")
    } else {
        tauri::include_image!("icons/32x32.png")
    };
    let state = app.state::<AppState>();
    *state.tray_proxy.lock().unwrap() = Some(proxy.clone());
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("WB Bridge")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)
        .map(|tray| *state.tray.lock().unwrap() = Some(tray))?;
    Ok(())
}

/// 面板动作 → 核心路由的映射表。与 src-tauri/core/src/server.rs 的 ACTION_ROUTES 逐项一致
/// （由本文件的 `shell_action_routes_match_the_core_contract` 断言）；docs/contract.md 的契约表
/// 是同一份内容的文字版，改动时仍需人工同步。
const ADMIN_ROUTES: [(&str, &str); 8] = [
    ("refresh", "/admin/refresh"),
    ("probe", "/admin/probe"),
    ("import", "/admin/import"),
    ("system-proxy", "/admin/system-proxy"),
    ("shutdown", "/admin/shutdown"),
    ("provider-status", "/admin/provider-status"),
    ("set-provider-key", "/admin/set-provider-key"),
    ("clear-provider-key", "/admin/clear-provider-key"),
];

fn admin_route(action: &str) -> Result<&'static str, String> {
    ADMIN_ROUTES
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, route)| *route)
        .ok_or_else(|| format!("未知操作：{action}"))
}

#[tauri::command]
async fn core_action(app: AppHandle, state: State<'_, AppState>, action: String, payload: Option<Value>) -> Result<Value, String> {
    let route = admin_route(&action)?;
    // 锁的作用域只到「取出端口」为止：真正的阻塞调用（api_key 最多轮询 15s、admin_call 超时 60s）
    // 下沉到 spawn_blocking。留在同步命令里会占住主线程，期间 restart_core、core_running、
    // graceful_stop（含退出流程）全部串行卡死，面板与托盘一起无响应。
    let port = {
        let guard = state.core.lock().unwrap();
        guard.as_ref().map(|core| core.port)
    };
    let Some(port) = port else { return Err("核心服务未运行".into()) };
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let key = core_key(&state)?;
        admin_call(port, &key, route, payload.as_ref())
    })
    .await
    .map_err(|e| format!("动作线程已崩溃：{e}"))?
}

/// 用系统默认浏览器打开外部链接（申请 Key 的官方页等）。
///
/// Tauri WebView 里 `target="_blank"` 默认不调起系统浏览器（点击静默失败），外链必须经壳转发。
/// 安全边界：只允许 https 协议 + RFC 3986 合法字符白名单；URL 只以命令行参数传给系统打开器
/// （macOS `open` / Windows `explorer` 直接传参不经 shell 解析 / Linux `xdg-open`），
/// 白名单是最后一道防线，不依赖调用方的引号处理。应用自有命令，不经 capability 授权。
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err(format!("不允许的链接协议（仅 https）：{url}"));
    }
    if url.is_empty() || url.len() > 2048 {
        return Err("链接长度非法".to_string());
    }
    const URL_SAFE: &[u8] = b":/?#[]@!$&'()*+,;=-._~%";
    if !url
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || URL_SAFE.contains(&c))
    {
        return Err("链接包含非法字符".to_string());
    }

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // explorer 直接传参：不经 cmd /C shell 解析，杜绝元字符注入。
        let mut c = std::process::Command::new("explorer");
        c.arg(&url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    cmd.spawn().map_err(|e| format!("打开链接失败: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn restart_core(app: AppHandle) -> Result<(), String> {
    // 同上：stop_core 最坏等十余秒，必须离开主线程；AppHandle 是 'static，可在闭包内重新取状态。
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        restart_core_with(&app, &state)
    })
    .await
    .map_err(|e| format!("重启线程已崩溃：{e}"))?
}

/// 重启主体的既有逻辑（不含线程调度）：停旧实例 → 启新实例 → 记录/清除失败原因。
fn restart_core_with(app: &AppHandle, state: &State<AppState>) -> Result<(), String> {
    // 锁的作用域只到「取出句柄」为止：stop_core 会走 HTTP 优雅退出并等待编排任务收摊，最坏
    // 十余秒，持锁期间 core_running、core_action 与退出流程都会被一并阻塞。
    let existing = state.core.lock().unwrap().take();
    if let Some(core) = existing {
        let key = core_key(state).ok();
        stop_core(core, key.as_deref(), state);
    }
    state.stopping_by_request.store(false, Ordering::SeqCst);
    let core = match start_core(app) {
        Ok(core) => core,
        Err(message) => {
            *state.startup_error.lock().unwrap() = Some(message.clone());
            let _ = app.emit("core-failed", serde_json::json!({ "message": message }));
            return Err(message);
        }
    };
    *state.startup_error.lock().unwrap() = None;
    *state.core.lock().unwrap() = Some(core);
    Ok(())
}

#[tauri::command]
fn core_running(state: State<AppState>) -> bool {
    let guard = state.core.lock().unwrap();
    guard.is_some() && !state.core_stopped.load(Ordering::SeqCst)
}

/// 数据目录路径（壳实际使用的 `app_data_dir`）。原命令名 `open_models_config` 名不副实：
/// 它既不打开任何文件、也不指向 models.json，只返回 status.json 的路径。改为直接返回目录，
/// 由调用方按需拼接文件名，避免两处各写一套 app_data_dir 逻辑。
#[tauri::command]
fn data_dir_path(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

/// 日志文件名：与核心 `orchestration.rs` 的 `join_host(&[&data_dir, "opencode.log"])` 一致，
/// 轮转文件是 `<同一路径>.previous`。两处必须同步改名，否则面板会静默读到空日志。
const LOG_FILE_NAME: &str = "opencode.log";
/// 尾部字节上限：日志按 5MB 轮转，面板只看最近一段，读全量既无意义也会把大字符串塞进 IPC。
const LOG_TAIL_BYTES: u64 = 256 * 1024;
/// 尾部行数上限（比字节上限更早生效时也要报告 truncated）。
const LOG_TAIL_LINES: usize = 1200;

/// 运行日志（只读）：返回数据目录下核心日志的尾部。
///
/// 路径由壳自己按 `app_data_dir` + 固定文件名拼出，**不接受调用方传路径** —— 面板只读展示，
/// 不得借这条命令读取数据目录之外的任意文件（也不写文件）。
/// 返回 `{ text, truncated, bytes }`：`bytes` 是日志文件的当前总字节数（不是返回文本长度），
/// `truncated` 表示尾部内容因字节/行数上限被截断。
#[tauri::command]
fn read_log(app: AppHandle) -> Result<Value, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    read_log_tail(&dir.join(LOG_FILE_NAME), LOG_TAIL_BYTES, LOG_TAIL_LINES)
}

/// 读取日志尾部（纯函数，便于单测）。
///
/// 实现要点：
/// - 只 seek 到 `total - max_bytes` 起读，不把 5MB 日志整体读进内存；
/// - 从中间字节开始解码时首行必然是断的，丢掉这半行（若整个尾部就是一行超长文本则无从丢弃，
///   此时 `truncated` 已为 true，面板会提示内容被截断）；
/// - 文件不存在（核心还没写过日志）按空日志处理，不报错。
fn read_log_tail(path: &Path, max_bytes: u64, max_lines: usize) -> Result<Value, String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::json!({ "text": "", "truncated": false, "bytes": 0 }))
        }
        Err(error) => return Err(format!("读取运行日志失败：{error}")),
    };
    let total = file
        .metadata()
        .map_err(|error| format!("读取运行日志失败：{error}"))?
        .len();
    let mut truncated = total > max_bytes;
    if truncated {
        file.seek(SeekFrom::Start(total - max_bytes))
            .map_err(|error| format!("读取运行日志失败：{error}"))?;
    }
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|error| format!("读取运行日志失败：{error}"))?;
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if truncated {
        if let Some(index) = text.find('\n') {
            text.drain(..=index);
        }
    }
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.len() > max_lines {
        lines.drain(..lines.len() - max_lines);
        truncated = true;
    }
    Ok(serde_json::json!({
        "text": lines.join("\n"),
        "truncated": truncated,
        "bytes": total,
    }))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // 自动更新：检查/下载走 updater 插件，装完重启走 process::relaunch。
        // 两端都只在 tauri.conf.json 的 plugins.updater 配上端点后才真正可用；
        // 未配置时前端调用会得到明确错误，不会静默成功。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = handle.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            app.manage(AppState {
                core: Mutex::new(None),
                data_dir: data_dir.clone(),
                quitting: AtomicBool::new(false),
                core_stopped: AtomicBool::new(false),
                core_code: AtomicU8::new(0),
                stopping_by_request: AtomicBool::new(false),
                proxy_on: Mutex::new(false),
                tray_proxy: Mutex::new(None),
                tray: Mutex::new(None),
                api_key: Mutex::new(None),
                startup_error: Mutex::new(None),
            });
            // 核心的「退出进程」动作在壳内必须换成「报告已停止」：进程归壳所有，
            // 核心不得单方面 app.exit（用户点托盘退出才会真正退出）。
            let _ = SHELL_APP.set(handle.clone());
            orchestration::set_exit_hook(move |code| {
                let Some(app) = SHELL_APP.get() else { return };
                let state = app.state::<AppState>();
                state.core_code.store(code, Ordering::SeqCst);
                state.core_stopped.store(true, Ordering::SeqCst);
            });
            build_tray(&handle)?;
            match start_core(&handle) {
                Ok(core) => {
                    let state = handle.state::<AppState>();
                    *state.startup_error.lock().unwrap() = None;
                    *state.core.lock().unwrap() = Some(core);
                }
                Err(message) => {
                    // setup 里这次 emit 必然早于面板注册监听，事件必定丢失；把原因记下来交给
                    // watch_status 反复播报，面板挂载后才能看到失败原因和重试入口。
                    *handle.state::<AppState>().startup_error.lock().unwrap() = Some(message.clone());
                    let _ = handle.emit("core-failed", serde_json::json!({ "message": message }));
                }
            }
            watch_status(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    // 已在退出流程里（托盘退出 / 上一次关闭触发的 quit）：不再拦截，让窗口正常销毁。
                    return;
                }
                // 关闭窗口即退出整个应用（含内嵌核心与 WorkBuddy 发布收尾），全平台一致：
                // 不再隐藏到托盘等用户「再去关一次后台」——驻留托盘时核心仍在跑，用户会以为已经停了。
                // 关停序列最坏数秒，因此先立即隐藏窗口再在后台线程走完退出，事件循环不被冻住。
                api.prevent_close();
                let _ = window.hide();
                let app = app.clone();
                thread::spawn(move || quit_app(&app));
            }
        })
        .invoke_handler(tauri::generate_handler![
            core_action,
            restart_core,
            core_running,
            data_dir_path,
            read_log,
            open_external
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                RunEvent::ExitRequested { .. } => {
                    // 必须是**有界**停止：自动更新的「重启」也走这条分支
                    // （process.relaunch → ExitRequested(RESTART_EXIT_CODE)），
                    // 在事件循环线程上直接跑 graceful_stop 会让窗口冻到停止序列走完（最坏十几秒），
                    // 用户看到的就不是「重启」而是「卡死」。
                    stop_core_bounded(app);
                    app.cleanup_before_exit();
                }
                #[cfg(target_os = "macos")]
                RunEvent::Reopen { .. } => show_main(app),
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 临时日志文件；Drop 时清理，避免测试往磁盘留垃圾。
    struct TempLog(PathBuf);

    impl TempLog {
        fn new(name: &str, content: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("wb-bridge-log-{}-{name}.log", std::process::id()));
            let mut file = fs::File::create(&path).expect("创建临时日志");
            file.write_all(content.as_bytes()).expect("写入临时日志");
            Self(path)
        }
    }

    impl Drop for TempLog {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn absent_log_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wb-bridge-log-{}-{name}-absent.log", std::process::id()))
    }

    /// 核心还没写过日志（文件不存在）时按空日志处理：不报错、bytes 为 0。
    #[test]
    fn missing_log_reads_as_empty_without_error() {
        let value = read_log_tail(&absent_log_path("missing"), 1024, 10).expect("缺失日志不得报错");
        assert_eq!(value["text"], "");
        assert_eq!(value["truncated"], false);
        assert_eq!(value["bytes"], 0);
    }

    /// 小文件整份返回：bytes 是文件真实字节数（不是返回文本长度），truncated 为 false。
    #[test]
    fn small_log_is_returned_whole() {
        let log = TempLog::new("small", "line one\nline two\n");
        let value = read_log_tail(&log.0, 1024, 10).expect("读取小日志");
        assert_eq!(value["text"], "line one\nline two");
        assert_eq!(value["truncated"], false);
        assert_eq!(value["bytes"], 18);
    }

    /// 超过字节上限：只回尾部，且丢掉跨字节起点被截断的半行，保证首行是完整日志行。
    #[test]
    fn oversized_log_keeps_tail_and_drops_the_split_first_line() {
        let content = "header line that will be cut\nkeep me\nand me\n";
        let log = TempLog::new("bytes", content);
        let value = read_log_tail(&log.0, 20, 100).expect("读取超长日志");
        assert_eq!(value["truncated"], true);
        assert_eq!(value["text"], "keep me\nand me");
        assert_eq!(value["bytes"], content.len());
    }

    /// 超过行数上限：只保留最后 max_lines 行，并如实报告 truncated。
    #[test]
    fn oversized_log_keeps_only_the_last_lines() {
        let log = TempLog::new("lines", "a\nb\nc\nd\n");
        let value = read_log_tail(&log.0, 1024, 2).expect("读取多行日志");
        assert_eq!(value["truncated"], true);
        assert_eq!(value["text"], "c\nd");
    }

    /// 壳的动作表必须与核心的契约表逐项一致：任一侧改名或漏项，面板动作会在回环 HTTP 上 404，
    /// 而面板只会显示一句「请求失败」。
    #[test]
    fn shell_action_routes_match_the_core_contract() {
        use wbbridge_core::server::ACTION_ROUTES;
        let mut shell: Vec<(&str, &str, &str)> =
            ADMIN_ROUTES.iter().map(|(name, route)| (*name, "POST", *route)).collect();
        let mut core: Vec<(&str, &str, &str)> = ACTION_ROUTES.to_vec();
        shell.sort_unstable();
        core.sort_unstable();
        assert_eq!(shell, core);
    }

    fn idle_state() -> AppState {
        AppState {
            core: Mutex::new(None),
            data_dir: PathBuf::new(),
            quitting: AtomicBool::new(false),
            core_stopped: AtomicBool::new(false),
            core_code: AtomicU8::new(0),
            stopping_by_request: AtomicBool::new(false),
            proxy_on: Mutex::new(false),
            tray_proxy: Mutex::new(None),
            tray: Mutex::new(None),
            api_key: Mutex::new(None),
            startup_error: Mutex::new(None),
        }
    }

    /// 关窗即退出后，「点关闭」「托盘点退出」「`RunEvent::ExitRequested`」会在同一进程里前后触发
    /// `graceful_stop`；`quitting` 的 swap 是唯一的幂等保证。第二次必须直接返回且不碰状态。
    #[test]
    fn repeated_quit_requests_stop_only_once() {
        let state = idle_state();
        assert!(!state.quitting.load(Ordering::SeqCst));
        graceful_stop(&state);
        assert!(state.quitting.load(Ordering::SeqCst), "首次关停必须置位 quitting");
        // 再次触发不得 panic、不得改动已取空的槽位。
        graceful_stop(&state);
        assert!(state.core.lock().unwrap().is_none());
    }

    /// 用户主动停止（关窗或托盘退出）不是故障：即使核心已报告结束，也不能播报「核心服务已退出」，
    /// 否则关窗收尾的几秒里面板会弹出一条假的故障与「重试」入口。
    #[test]
    fn a_quit_requested_shutdown_is_not_reported_as_failure() {
        let state = idle_state();
        state.core_stopped.store(true, Ordering::SeqCst);
        state.stopping_by_request.store(true, Ordering::SeqCst);
        assert!(core_exit(&state).is_none(), "主动关停不得算作故障退出");
        assert!(service_down(&state).is_none(), "主动关停期间不得推送故障态");
        // 对照：真正的崩溃仍必须被识别，否则上面两条断言会因为永远返回 None 而失去意义。
        state.stopping_by_request.store(false, Ordering::SeqCst);
        assert!(core_exit(&state).is_some());
    }

    /// 故障播报只认真正的故障：核心在跑不报；真崩溃必须报「已退出」；**关窗退出的收尾期**
    /// （core 槽已被取走、主动停止标志在位）不得谎报故障，否则面板会在退出过程中弹出一条假的
    /// 「核心服务已退出 + 重试」；启动失败则必须报出原因且不能说成「已退出」。
    #[test]
    fn service_down_reports_only_real_failures() {
        let running = idle_state();
        *running.core.lock().unwrap() = Some(RunningCore {
            runtime: tokio::runtime::Runtime::new().expect("测试用运行时"),
            port: 41980,
        });
        assert!(service_down(&running).is_none(), "核心在跑时不得报故障");

        // 同一份状态改成「崩溃」口径：exit hook 置位且非主动停止。
        running.stopping_by_request.store(false, Ordering::SeqCst);
        running.core_stopped.store(true, Ordering::SeqCst);
        let (message, by_exit) = service_down(&running).expect("崩溃必须报故障");
        assert!(by_exit);
        assert!(message.contains("核心服务已退出"), "{message}");

        // 关窗退出的收尾期：没有核心、核心也已结束，但这是用户主动停的。
        let shutting = idle_state();
        shutting.core_stopped.store(true, Ordering::SeqCst);
        shutting.stopping_by_request.store(true, Ordering::SeqCst);
        assert!(service_down(&shutting).is_none(), "主动关停期间不得推送故障态");

        // 启动失败：没有核心在跑，原因来自 startup_error，口径不得与「已退出」混用。
        let failed = idle_state();
        *failed.startup_error.lock().unwrap() = Some("核心启动失败：端口不可用".to_string());
        let (message, by_exit) = service_down(&failed).expect("启动失败必须报故障");
        assert!(!by_exit, "启动失败不是「核心已退出」");
        assert_eq!(message, "核心启动失败：端口不可用");
    }

    /// status.json 快路径的判定：只有在「拿得到 mtime 且 (mtime, 长度) 与上次一致」时才允许跳过。
    /// 重点保护两件事：mtime 不可得时必须回落读内容（否则长度不变的改写被永久跳过、面板冻结），
    /// 以及 mtime 或长度任一变化都要重读（重启后内容可能与故障前逐字节相同，靠的就是这道判定）。
    #[test]
    fn status_read_needed_only_skips_when_mtime_is_known() {
        use std::time::UNIX_EPOCH;
        let first = (UNIX_EPOCH, 8u64);
        let later = (UNIX_EPOCH + std::time::Duration::from_secs(1), 8u64);
        let resized = (UNIX_EPOCH, 9u64);

        assert!(
            status_read_needed(None, Some(first)),
            "文件系统不暴露 mtime 时不得启用快路径"
        );
        assert!(
            status_read_needed(None, None),
            "首轮且拿不到 mtime 时必须读"
        );
        assert!(!status_read_needed(Some(first), Some(first)), "mtime 与长度都没变才允许跳过");
        assert!(status_read_needed(Some(later), Some(first)), "mtime 变了必须重读");
        assert!(status_read_needed(Some(resized), Some(first)), "长度变了必须重读");
        assert!(status_read_needed(Some(first), None), "没有上次记录时必须读（含恢复运行后重置 stamp 的情形）");
    }
}
