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

/// 调核心管理接口（与 src/core/src/server.js 的路由一一对应）。
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
    // 的响应当成成功（与 src/core/src/server.js 的路由约定对齐）。
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
const STATUS_SCHEMA_VERSION: u64 = 1;

/// 轮询核心写的 status.json：内容变化即推送事件、同步托盘勾选态，并监督核心任务是否已结束。
fn watch_status(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let file = state.data_dir.join("status.json");
        let mut last = String::new();
        let mut warned = false;
        let mut reported = false;
        // 退出后不再轮询、不再向已关闭的窗口 emit。
        while !state.quitting.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(500));
            // 核心内部异常（运行时/下载/OpenCode 子进程意外退出）时 status.json 会停在最后的
            // ready，只有靠 exit hook 的置位才能发现；发现后立刻通知面板（露出「重试」）并改托盘 tooltip。
            if !reported {
                if let Some(reason) = core_exit(&state) {
                    reported = true;
                    if let Some(tray) = state.tray.lock().unwrap().as_ref() {
                        let _ = tray.set_tooltip(Some("WB Bridge — 核心服务已退出，可在面板重启"));
                    }
                    let _ = app.emit("core-failed", serde_json::json!({ "message": format!("核心服务已退出（{reason}），可在面板点“重试”重启") }));
                }
            }
            // 重启后恢复监听：核心再次在跑时清掉本轮故障上报标记。
            if reported && state.core.lock().unwrap().is_some() && !state.core_stopped.load(Ordering::SeqCst) {
                reported = false;
                if let Some(tray) = state.tray.lock().unwrap().as_ref() {
                    let _ = tray.set_tooltip(Some("WB Bridge"));
                }
            }
            let Ok(text) = fs::read_to_string(&file) else { continue };
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
                    if let Ok(key) = api_key(&data_dir) {
                        let _ = admin_call(port, &key, "/admin/system-proxy", Some(&serde_json::json!({ "enabled": next })));
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
                            if let (Ok(key), Ok(route)) = (api_key(&state.data_dir), admin_route("import")) {
                                let _ = admin_call(port, &key, route, Some(&serde_json::json!({ "modelsFile": path.to_string() })));
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

/// 面板动作 → 核心路由的映射表。与 src-tauri/core/src/server.rs 的 ACTION_ROUTES 及 docs/contract.md
/// 的契约表一一对应；三处集合是否一致由契约测试断言。
const ADMIN_ROUTES: [(&str, &str); 5] = [
    ("refresh", "/admin/refresh"),
    ("probe", "/admin/probe"),
    ("import", "/admin/import"),
    ("system-proxy", "/admin/system-proxy"),
    ("shutdown", "/admin/shutdown"),
];

fn admin_route(action: &str) -> Result<&'static str, String> {
    ADMIN_ROUTES
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, route)| *route)
        .ok_or_else(|| format!("未知操作：{action}"))
}

#[tauri::command]
fn core_action(action: String, payload: Option<Value>, state: State<AppState>) -> Result<Value, String> {
    let route = admin_route(&action)?;
    // 先把端口拷出来再释放锁：api_key 最多轮询 15s、admin_call 超时 60s，持锁做网络调用会把
    // restart_core、core_running、graceful_stop（含退出流程）串行阻塞数十秒到分钟级。
    let port = {
        let guard = state.core.lock().unwrap();
        guard.as_ref().map(|core| core.port)
    };
    let Some(port) = port else { return Err("核心服务未运行".into()) };
    let key = core_key(&state)?;
    admin_call(port, &key, route, payload.as_ref())
}

#[tauri::command]
fn restart_core(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    // 锁的作用域只到「取出句柄」为止：stop_core 会走 HTTP 优雅退出并等待编排任务收摊，最坏
    // 十余秒，持锁期间 core_running、core_action 与退出流程都会被一并阻塞。
    let existing = state.core.lock().unwrap().take();
    if let Some(core) = existing {
        let key = core_key(&state).ok();
        stop_core(core, key.as_deref(), &state);
    }
    state.stopping_by_request.store(false, Ordering::SeqCst);
    let core = start_core(&app)?;
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

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
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
                    *handle.state::<AppState>().core.lock().unwrap() = Some(core);
                }
                Err(message) => {
                    let _ = handle.emit("core-failed", serde_json::json!({ "message": message }));
                }
            }
            watch_status(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().state::<AppState>().quitting.load(Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![core_action, restart_core, core_running, data_dir_path])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                RunEvent::ExitRequested { .. } => {
                    graceful_stop(app.state::<AppState>().inner());
                    app.cleanup_before_exit();
                }
                #[cfg(target_os = "macos")]
                RunEvent::Reopen { .. } => show_main(app),
                _ => {}
            }
        });
}
