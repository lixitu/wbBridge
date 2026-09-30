// WB Bridge Tauri 壳：sidecar 生命周期、托盘、状态推送与 IPC 代理。
// 核心是打包为单文件可执行的 Node 服务（wbbridge-core-<target-triple>），本壳负责：
//   启动注入 BUDDY_PORT / BUDDY_DATA_DIR → 轮询 status.json 推送事件 → 退出时经
//   POST /admin/shutdown 优雅停止（等待核心完成 WorkBuddy 配置清理）。
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;

/// 核心进程句柄与访问信息。
struct CoreProcess {
    child: Child,
    port: u16,
}

struct AppState {
    core: Mutex<Option<CoreProcess>>,
    data_dir: PathBuf,
    quitting: AtomicBool,
    proxy_on: Mutex<bool>,
    tray_proxy: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
}

/// 当前平台对应的 sidecar 文件名（与 scripts/build-sidecar.mjs 的产物命名一致）。
/// Tauri 打包时会把 externalBin 的目标三元组后缀去掉，因此打包布局内是无后缀的裸名。
fn sidecar_names() -> Vec<String> {
    let triple = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else {
        "aarch64-unknown-linux-gnu"
    };
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    vec![format!("wbbridge-core-{triple}{ext}"), format!("wbbridge-core{ext}")]
}

/// 在 Tauri 各打包布局中定位 sidecar：可执行文件旁 / binaries 目录 / 资源目录 / 开发目录。
fn resolve_sidecar(app: &AppHandle) -> Option<PathBuf> {
    let names = sidecar_names();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in &names {
                candidates.push(dir.join(name));
                candidates.push(dir.join("binaries").join(name));
            }
        }
    }
    if let Ok(resource) = app.path().resource_dir() {
        for name in &names {
            candidates.push(resource.join("binaries").join(name));
            candidates.push(resource.join(name));
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        if let Some(root) = Path::new(&manifest).parent() {
            for name in &names {
                candidates.push(root.join("src-tauri").join("binaries").join(name));
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

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

/// 调核心管理接口（与 src/server.js 的路由一一对应）。
fn admin_call(port: u16, key: &str, route: &str, body: Option<&Value>) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{port}{route}");
    let call = ureq::post(&url)
        .timeout(Duration::from_secs(60))
        .set("Authorization", &format!("Bearer {key}"))
        .send_json(body.unwrap_or(&Value::Null));
    let response = match call {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(format!("核心服务请求失败：{e}")),
    };
    let text = response.into_string().map_err(|e| e.to_string())?;
    let json = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    if json.get("error").is_some() {
        let message = json["error"]["message"].as_str().unwrap_or("请求失败").to_string();
        return Err(message);
    }
    Ok(json)
}

fn spawn_core(app: &AppHandle) -> Result<CoreProcess, String> {
    let binary = resolve_sidecar(app)
        .ok_or_else(|| "找不到核心服务程序（wbbridge-core）。安装包可能不完整，请重新下载。".to_string())?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    let port = pick_port();
    let mut cmd = Command::new(&binary);
    cmd.env("BUDDY_DATA_DIR", &data_dir).env("BUDDY_PORT", port.to_string())
        .env("BUDDY_PARENT_PID", std::process::id().to_string());
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| format!("启动核心服务失败：{e}"))?;
    if let Some(out) = child.stdout.take() {
        thread::spawn(move || {
            let mut reader = std::io::BufReader::new(out);
            let mut line = String::new();
            loop {
                use std::io::BufRead;
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => println!("[core] {}", line.trim_end()),
                }
            }
        });
    }
    Ok(CoreProcess { child, port })
}

/// 停止单个核心进程：HTTP 优雅退出 → SIGTERM → 强杀（供退出与重启共用）。
fn graceful_stop_process(core: &mut CoreProcess, data_dir: &Path) {
    if let Ok(key) = api_key(data_dir) {
        if admin_call(core.port, &key, "/admin/shutdown", None).is_ok() {
            let deadline = Instant::now() + Duration::from_secs(25);
            while Instant::now() < deadline {
                match core.child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => thread::sleep(Duration::from_millis(200)),
                    Err(_) => break,
                }
            }
        }
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(core.child.id() as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        let _ = core.child.kill();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match core.child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(_) => return,
        }
    }
    let _ = core.child.kill();
    let _ = core.child.wait();
}

/// 停止核心：HTTP 优雅退出（核心随后清理 WorkBuddy 配置并写终态）→ SIGTERM → 强杀。
fn graceful_stop(state: &AppState) {
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(mut core) = state.core.lock().unwrap().take() else { return };
    graceful_stop_process(&mut core, &state.data_dir);
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn quit_app(app: &AppHandle) {
    graceful_stop(app.state::<AppState>().inner());
    app.cleanup_before_exit();
    app.exit(0);
}

/// 轮询核心写的 status.json，变化即推送事件并同步托盘勾选态。
fn watch_status(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let file = state.data_dir.join("status.json");
        let mut last = String::new();
        loop {
            thread::sleep(Duration::from_millis(500));
            let Ok(text) = fs::read_to_string(&file) else { continue };
            if text == last {
                continue;
            }
            let Ok(parsed) = serde_json::from_str::<Value>(&text) else { continue };
            last = text;
            let proxy = parsed.get("useSystemProxy").and_then(Value::as_bool).unwrap_or(false);
            *state.proxy_on.lock().unwrap() = proxy;
            if let Some(item) = state.tray_proxy.lock().unwrap().as_ref() {
                let _ = item.set_checked(proxy);
            }
            let _ = app.emit("core-status", parsed);
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
                            if let Ok(key) = api_key(&state.data_dir) {
                                let _ = admin_call(port, &key, "/admin/import", Some(&serde_json::json!({ "modelsFile": path.to_string() })));
                            }
                        }
                        drop(state);
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
        .build(app)?;
    Ok(())
}

#[tauri::command]
fn core_action(action: String, payload: Option<Value>, state: State<AppState>) -> Result<Value, String> {
    let route = match action.as_str() {
        "refresh" => "/admin/refresh",
        "probe" => "/admin/probe",
        "import" => "/admin/import",
        "system-proxy" => "/admin/system-proxy",
        other => return Err(format!("未知操作：{other}")),
    };
    let guard = state.core.lock().unwrap();
    let Some(core) = guard.as_ref() else { return Err("核心服务未运行".into()) };
    let key = api_key(&state.data_dir)?;
    admin_call(core.port, &key, route, payload.as_ref())
}

#[tauri::command]
fn restart_core(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    if let Some(mut core) = state.core.lock().unwrap().take() {
        graceful_stop_process(&mut core, &state.data_dir);
    }
    let core = spawn_core(&app)?;
    *state.core.lock().unwrap() = Some(core);
    Ok(())
}

#[tauri::command]
fn core_running(state: State<AppState>) -> bool {
    let mut guard = state.core.lock().unwrap();
    match guard.as_mut() {
        Some(core) => core.child.try_wait().map(|r| r.is_none()).unwrap_or(false),
        None => false,
    }
}

#[tauri::command]
fn open_models_config(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = dir.join("status.json");
    Ok(path.to_string_lossy().into_owned())
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
                proxy_on: Mutex::new(false),
                tray_proxy: Mutex::new(None),
            });
            build_tray(&handle)?;
            match spawn_core(&handle) {
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
        .invoke_handler(tauri::generate_handler![core_action, restart_core, core_running, open_models_config])
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
