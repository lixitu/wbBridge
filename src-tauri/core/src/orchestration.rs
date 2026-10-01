//! `core/src/main.js` 的 Rust 等价实现（方案B 阶段三编排层）。
//!
//! 与 Node 版的对应关系：
//! - 启动时序（数据目录 → pid 锁 → api-key → settings → status 快照 → 日志轮转 → 配置定位）逐条对齐；
//! - `update/record/syncPublished/startProbes/refresh/importModels/setSystemProxy/shutdown` 全部落在这里，
//!   共享状态用 `Arc<Mutex<...>>` 全局单例 + 串行写链（对应 JS 的 `statusWrites` / `syncWrites`）；
//! - 探测批的共享 deadline、chatOnly 降级、格式失败不撤销发布等语义与 `probeModel` 一致。
//!
//! 两种运行形态共用本模块：独立可执行（`src/main.rs`，`run(true)`）与嵌入 Tauri 壳
//! （`src-tauri`，`run(false)` + `set_exit_hook`）。壳形态下「进程退出」必须交回壳决定，
//! 所以关停的落地动作是可注入的 hook，默认仍是 `std::process::exit`。
//!
//! 与 Node 版的**有意偏差**（均不影响对外契约）：
//! 1. JS 用 `setTimeout(...).unref()` 的父进程看门狗，这里用独立 tokio 任务轮询（等价 3s 周期）；
//! 2. `process.exit(code)` 由关停任务在串行写链落盘后调用（嵌入壳时改为调用 exit hook）；
//! 3. 子进程退出监听（`watchRuntime`）用 `try_wait` 轮询代替 Node 的 `exit` 事件。

use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;

use crate::model_status::{model_result, now_iso8601, status_value, with_request_meta};
use crate::platform::{self, join_host};
use crate::probe::{self, PROBE_TIMEOUT_MS};
use crate::protocol::{prepare, BridgeError};
use crate::runtime::{self, RuntimeOptions, Started};
use crate::backend::to_bridge_error;
use crate::server::{
    AbortController, AbortSignal, Activity, BackendError, RequestContext, Server, ServerControl,
    SharedMeta,
};
use crate::sync::{atomic_write, sync_models, SyncOptions};
use crate::system_proxy::system_proxy_environment;
use crate::workbuddy_config::{resolve_models_file, validate_models_file};
use crate::Env;

/// 独立可执行的用法说明（`src/main.rs` 打印）。
pub const HELP: &str = "\
wbbridge-core —— WB Bridge 核心代理（Rust 实现）

用法：
  wbbridge-core              启动核心代理（127.0.0.1:41980，OpenAI 兼容接口）
  wbbridge-core --help       显示本帮助
  wbbridge-core --version    输出版本

环境变量：
  BUDDY_PORT          监听端口（默认 41980，范围 1024–65535）
  BUDDY_DATA_DIR      数据目录（默认按平台推导）
  BUDDY_MODELS_FILE   WorkBuddy models.json 路径覆盖
  BUDDY_NO_SYNC=1     跳过启动导入与后续同步
  BUDDY_PARENT_PID    父进程 pid；父进程消失时本进程自行关停
  BUDDY_OPENCODE_PATH 首选 OpenCode 可执行文件路径";

/// 已有实例在跑（`service.pid` 指向存活进程）；启动按此原因退出码 2 结束。
pub const ALREADY_RUNNING: &str = "WB Bridge is already running";

/// 格式/执行类失败：来源为真实请求时不撤销已发布模型（数据红线）。
const REQUEST_SHAPED_FAILURES: [&str; 4] = [
    "invalid_model_output",
    "invalid_tool_call",
    "native_tool_activity",
    "output_truncated",
];

/// 翻译器首选名单（对应 `TRANSLATOR_ORDER`）。
const TRANSLATOR_ORDER: [&str; 4] = [
    "opencode/big-pickle",
    "opencode/nemotron-3.5-lightning-free",
    "opencode/space-bunny-free",
    "opencode/mimo-v2.6-flash-free",
];

/// status.json 的结构版本号；壳侧读取时校验，不一致只提示不阻断。
const STATUS_SCHEMA_VERSION: u32 = 1;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ---------------------------------------------------------------------------
// 进程级单例与小型工具
// ---------------------------------------------------------------------------

/// 当前实例。用 `Mutex<Option<Arc<App>>>` 而不是 `OnceLock`：嵌入壳时核心可以被「重启」，
/// 每轮启动都要能装入新的实例（旧实例的关停链已排空，不会有人再读它）。
static APP: Mutex<Option<Arc<App>>> = Mutex::new(None);

/// 关停完成后要执行的退出动作。独立进程下是 `std::process::exit`；嵌入壳时由壳注入
/// 「标记核心已停止并通知面板」的回调 —— 壳不能被核心单方面终止。
static EXIT_HOOK: OnceLock<Box<dyn Fn(u8) + Send + Sync>> = OnceLock::new();

/// 注入关停落地动作（仅嵌入壳时需要；重复调用取首次）。
pub fn set_exit_hook(hook: impl Fn(u8) + Send + Sync + 'static) {
    let _ = EXIT_HOOK.set(Box::new(hook));
}

fn exit_process(code: u8) {
    match EXIT_HOOK.get() {
        Some(hook) => hook(code),
        None => std::process::exit(i32::from(code)),
    }
}

fn global_app() -> Arc<App> {
    lock(&APP)
        .clone()
        .expect("App must be initialized before use")
}

fn env_text(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn js_number(millis: f64) -> Value {
    Value::from(millis as i64)
}

fn now_millis() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(inner) => *inner,
        Value::Number(number) => number.as_f64().map(|value| value != 0.0).unwrap_or(true),
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

/// `writeLock` 等价物：以 `0600` 权限独占创建（对应 `flag: 'wx'`）。
fn write_exclusive(path: &str, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())
}

/// 向 `opencode.log` 追加一行（带 ISO 时间戳）；日志句柄缺省时静默丢弃。
fn log_line(text: &str) {
    // bootstrap 之前也可能被调用（例如端口校验失败），实例未装入时静默丢弃。
    let Some(app) = lock(&APP).clone() else { return };
    if let Some(handle) = app.log_handle.as_ref() {
        let mut guard = lock(handle);
        let _ = writeln!(guard, "{} {text}", now_iso8601());
    }
}

/// `crypto.randomBytes(n).toString('hex')` 的等价物。
fn random_hex(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes * 2);
    while text.len() < bytes * 2 {
        text.push_str(&Uuid::new_v4().simple().to_string());
    }
    text.truncate(bytes * 2);
    text
}

// ---------------------------------------------------------------------------
// 共享状态
// ---------------------------------------------------------------------------

/// 「已落地的后台任务」槽位：编号 + 完成信号（对应 JS 里可被 await 的 promise）。
///
/// 编号用于判定链头是否已被更晚排队的任务替换（`drain` 的收敛条件）。
#[derive(Clone)]
struct Slot {
    id: u64,
    done: watch::Receiver<bool>,
    /// 本轮任务的结果，供「复用进行中任务」的后到调用者取回（目前只有 refresh 链写入）。
    /// 用 Arc 是因为克隆 Slot 时后到者必须读到同一个 outcome。
    outcome: Arc<Mutex<Option<Result<Value, BackendError>>>>,
}

impl Slot {
    fn new(id: u64, done: watch::Receiver<bool>) -> Self {
        Self {
            id,
            done,
            outcome: Arc::new(Mutex::new(None)),
        }
    }
}

/// 串行链的任务编号发生器（statusWrites / syncWrites / refreshing / probeTask 共用）。
static CHAIN_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_chain_id() -> u64 {
    CHAIN_SEQ.fetch_add(1, Ordering::SeqCst)
}

/// 在串行链上登记一个任务：在**同一次持锁**里读出前驱、把自己写成新链头。
///
/// 必须原子：分两次加锁时两个线程可能都读到同一个前驱、各自只等它，写链就此分叉 —— 两条分支
/// 并发执行（status.json 不再串行、models.json 的读-改-写互相覆盖），并且后登记的一方会覆盖
/// 先登记的 Slot，`drain()` 等不到被覆盖的那条写。
fn reserve_chain_slot(
    chain: &Arc<Mutex<Option<Slot>>>,
) -> (Option<Slot>, watch::Sender<bool>, Slot) {
    let (done_tx, done_rx) = watch::channel(false);
    let slot = Slot::new(next_chain_id(), done_rx);
    let mut head = lock(chain);
    let predecessor = head.clone();
    *head = Some(slot.clone());
    (predecessor, done_tx, slot)
}

/// 活动表条目（对应 `activities` Map 的值）。
struct ActivityEntry {
    started_at: f64,
    last_event_at: Option<f64>,
    last_content_at: Option<f64>,
    written_at: f64,
    fields: Value,
}

/// 编排层共享状态：JS 里 `state / settings / models / validated / runtime / ...` 的合集。
#[derive(Clone)]
struct App {
    data_dir: Arc<String>,
    endpoint: Arc<String>,
    api_key: Arc<String>,
    settings_file: Arc<PathBuf>,
    status_file: Arc<PathBuf>,
    log_file: Arc<PathBuf>,
    log_handle: Option<Arc<Mutex<std::fs::File>>>,
    port: u16,
    state: Arc<Mutex<Value>>,
    settings: Arc<Mutex<Value>>,
    models_file: Arc<Mutex<Option<String>>>,
    models: Arc<Mutex<Vec<Value>>>,
    validated: Arc<Mutex<HashSet<String>>>,
    binary: Arc<Mutex<Option<String>>>,
    runtime: Arc<Mutex<Option<Arc<Started>>>>,
    stopping: Arc<AtomicBool>,
    probing: Arc<AtomicBool>,
    serving: Arc<AtomicBool>,
    activities: Arc<Mutex<HashMap<String, ActivityEntry>>>,
    probe_abort: Arc<AbortController>,
    server_control: Arc<Mutex<Option<ServerControl>>>,
    /// 串行 status.json 写链（对应 `statusWrites`）。
    status_task: Arc<Mutex<Option<Slot>>>,
    /// 串行 models.json 写链（对应 `syncWrites`）。
    sync_task: Arc<Mutex<Option<Slot>>>,
    /// `refresh()` 的去重句柄（对应 `refreshing`）。
    refresh: Arc<Mutex<Option<Slot>>>,
    /// 代次编号：只有当前这一轮 refresh 才有权释放链位（避免误清后启动的一轮）。
    refresh_generation: Arc<Mutex<u64>>,
    /// 探测批任务（shutdown 需要等待它）。
    probe_task: Arc<Mutex<Option<Slot>>>,
    /// 优雅停机的触发端（对应 `server.close()`）。
    server_stop: Arc<Mutex<Option<watch::Sender<bool>>>>,
    /// 关停结果（退出码）广播。
    shutdown_signal: Arc<Mutex<Option<watch::Sender<Option<u8>>>>>,
}

impl App {
    fn snapshot(&self) -> Value {
        lock(&self.state).clone()
    }

    fn read_settings(&self) -> Value {
        lock(&self.settings).clone()
    }

    async fn write_settings(&self, value: Value) -> Result<(), String> {
        *lock(&self.settings) = value.clone();
        let text = serde_json::to_string(&value).unwrap_or_default();
        let file = self.settings_file.as_ref().clone();
        tokio::task::spawn_blocking(move || atomic_write(&file, &text).map(|_| ()).map_err(|e| e.message))
            .await
            .map_err(|error| error.to_string())?
    }

    fn models(&self) -> Vec<Value> {
        lock(&self.models).clone()
    }

    fn models_file(&self) -> Option<String> {
        lock(&self.models_file).clone()
    }

    fn runtime(&self) -> Option<Arc<Started>> {
        lock(&self.runtime).clone()
    }
}

/// 更新内存状态并返回合并后的快照（对应 `update(patch)` 里 `state = {...}` 的部分）。
///
/// 落盘由 `persist_status` 触发 —— 二者分开是为了保证「先取快照、后释放锁」，
/// 避免 std::Mutex 跨越 await 持有。
fn update(patch: Value) -> Value {
    let app = global_app();
    let mut guard = lock(&app.state);
    if let (Some(target), Some(patch)) = (guard.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            target.insert(key.clone(), value.clone());
        }
        target.insert("updatedAt".to_string(), json!(now_iso8601()));
    }
    guard.clone()
}

/// 把状态快照串行写入 `status.json`（对应 `statusWrites` 链）。
fn persist_status(snapshot: Value) {
    let app = global_app();
    let text = serde_json::to_string_pretty(&snapshot).unwrap_or_default();
    let file = app.status_file.as_ref().clone();
    let (predecessor, done_tx, _head) = reserve_chain_slot(&app.status_task);
    tokio::spawn(async move {
        if let Some(predecessor) = predecessor {
            let _ = predecessor.done.clone().wait_for(|done| *done).await;
        }
        if let Err(error) = tokio::task::spawn_blocking(move || atomic_write(&file, &text)).await {
            eprintln!("Status write failed: {error}");
        }
        let _ = done_tx.send(true);
    });
}

/// 「更新 + 落盘」一步完成（对应 JS 的单次 `update` 调用点）。
fn update_and_persist(patch: Value) {
    persist_status(update(patch));
}

/// 串行执行 models.json 任务（对应 `syncWrites` 链）。
fn chain_sync(task: impl std::future::Future<Output = ()> + Send + 'static) {
    let app = global_app();
    let (predecessor, done_tx, _head) = reserve_chain_slot(&app.sync_task);
    tokio::spawn(async move {
        if let Some(predecessor) = predecessor {
            let _ = predecessor.done.clone().wait_for(|done| *done).await;
        }
        task.await;
        let _ = done_tx.send(true);
    });
}

/// 等待某条串行写链排空（对应 JS 的 `await syncWrites` / `await statusWrites`）。
async fn drain(chain: &Arc<Mutex<Option<Slot>>>) {
    loop {
        let Some(slot) = lock(chain).clone() else {
            return;
        };
        let _ = slot.done.clone().wait_for(|done| *done).await;
        // 等待期间可能有更晚的任务排队，链头换人则继续等。
        let head = lock(chain).clone();
        match head {
            None => return,
            Some(head) => {
                if head.id == slot.id {
                    return;
                }
            }
        }
    }
}

/// 等待 syncWrites 链清空（shutdown 与 import 的判定路径需要）。
async fn drain_sync() {
    drain(&global_app().sync_task).await;
}

/// 等待 statusWrites 链清空。
async fn drain_status() {
    drain(&global_app().status_task).await;
}

/// 当前可发布的模型（对应 `usableModels()`）：通过校验且探测结果为 ok。
fn usable_models() -> Vec<Value> {
    let app = global_app();
    let validated = lock(&app.validated).clone();
    let state = app.snapshot();
    let results = state
        .get("modelResults")
        .cloned()
        .unwrap_or(Value::Null);
    app.models()
        .into_iter()
        .filter(|model| {
            let id = model.get("id").and_then(Value::as_str).unwrap_or_default();
            validated.contains(id)
                && results.get(id).and_then(|entry| entry.get("ok")) == Some(&json!(true))
        })
        .map(|mut model| {
            let id = model.get("id").and_then(Value::as_str).unwrap_or_default();
            let chat_only = results
                .get(id)
                .and_then(|entry| entry.get("chatOnly"))
                .unwrap_or(&Value::Bool(false))
                .clone();
            if let Some(object) = model.as_object_mut() {
                object.insert("chatOnly".to_string(), chat_only);
            }
            model
        })
        .collect()
}

/// `attachTranslator`：翻译模型按首选名单挑选，未命中时退化为第一个可用模型并留痕。
/// `failed` 为当前请求失败的模型 id —— 与 JS 一致，绝不把失败模型选为自己的转写器。
fn attach_translator(started: &Started) {
    started.backend.set_translator(move |failed: &str, _shape: &str| {
        let ids = usable_models()
            .iter()
            .filter_map(|model| model.get("id").and_then(Value::as_str))
            .filter(|id| *id != failed)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let preferred = TRANSLATOR_ORDER
            .iter()
            .find(|candidate| ids.contains(&candidate.to_string()))
            .map(|candidate| (*candidate).to_string());
        // 上游改名后首选名单会整体失配；退化为首个可用模型时必须留痕，否则事后无法解释
        // 「为什么是这个模型被用作翻译器」。选择结果同时写入 status.json 的 translator 字段。
        let chosen = match preferred.clone().or_else(|| ids.first().cloned()) {
            Some(chosen) => chosen,
            None => return None,
        };
        if preferred.as_deref() != Some(chosen.as_str()) {
            log_line(&format!(
                "翻译模型退化为 {chosen}（首选名单未命中，候选：{}）",
                if ids.is_empty() {
                    "无".to_string()
                } else {
                    ids.join(", ")
                }
            ));
        }
        if global_app().snapshot().get("translator").and_then(Value::as_str) != Some(chosen.as_str()) {
            update_and_persist(json!({ "translator": chosen }));
        }
        Some(chosen)
    });
}

// ---------------------------------------------------------------------------
// 同步发布（syncPublished）
// ---------------------------------------------------------------------------

/// `syncPublished(published)`：把发布集串行写入 WorkBuddy `models.json`。
///
/// `None` 表示按 `usableModels()` 取值（与 JS 默认参数一致）。
fn sync_published(published: Option<Vec<Value>>) {
    let app = global_app();
    let models = published.unwrap_or_else(usable_models);
    let endpoint = format!("{}/chat/completions", app.endpoint);
    let key = app.api_key.to_string();

    if env_text("BUDDY_NO_SYNC").as_deref() == Some("1") {
        let count = models.len() as u64;
        chain_sync(async move {
            update_and_persist(json!({
                "sync": { "skipped": true, "count": count, "time": now_iso8601() }
            }));
        });
        return;
    }

    let file = app.models_file();
    chain_sync(async move {
        let outcome: Value = match file {
            None => json!({ "error": "未找到有效的 WorkBuddy 配置，请点击导入并选择 models.json；首次使用请先在 WorkBuddy 保存一个自定义模型。" }),
            Some(path) => {
                let result = tokio::task::spawn_blocking(move || {
                    let options = SyncOptions { allow_empty: true, require_existing: true };
                    sync_models(Path::new(&path), &models, &endpoint, &key, &options)
                })
                .await;
                match result {
                    Err(error) => json!({ "error": error.to_string() }),
                    Ok(Ok(outcome)) => outcome.to_json(),
                    Ok(Err(error)) => json!({ "error": error.message }),
                }
            }
        };
        let mut sync = outcome.as_object().cloned().unwrap_or_default();
        sync.insert("time".to_string(), json!(now_iso8601()));
        update_and_persist(json!({ "sync": Value::Object(sync) }));
    });
}

// ---------------------------------------------------------------------------
// 结果记录（record）
// ---------------------------------------------------------------------------

fn insert_all(target: &mut Map<String, Value>, source: Option<&Value>) {
    if let Some(object) = source.and_then(Value::as_object) {
        for (key, value) in object {
            target.insert(key.clone(), value.clone());
        }
    }
}

/// `record(...)`：一次模型请求/探测的结果落地。
///
/// 数据红线：来源为真实请求的格式类失败不得撤销已发布模型；探测路径绝不进入翻译。
/// 形参与 JS `record(modelID, ok, error, status, code, durationMs, source, chatOnly, meta)`
/// 一一对应，改动任一顺序都会破坏对齐，因此不按 clippy 建议聚合参数。
#[allow(clippy::too_many_arguments)]
async fn record(
    model: &str,
    ok: bool,
    error: Option<&str>,
    status: Option<Value>,
    code: Option<&str>,
    duration_ms: i64,
    source: &str,
    chat_only: Option<bool>,
    meta: &Value,
) {
    let app = global_app();
    if app.stopping.load(Ordering::Relaxed) {
        return;
    }
    let mut result = model_result(ok, error.unwrap_or(""), status.as_ref(), code);
    with_request_meta(&mut result, meta);
    let Some(object) = result.as_object_mut() else { return };
    object.insert("durationMs".to_string(), js_number(duration_ms as f64));
    object.insert("source".to_string(), json!(source));
    let chat_only_value = match chat_only {
        Some(value) => json!(value),
        // JS 默认式：source === 'request' && state.modelResults[m]?.chatOnly === true
        None => json!(source == "request"
            && app
                .snapshot()
                .get("modelResults")
                .and_then(|results| results.get(model))
                .and_then(|entry| entry.get("chatOnly"))
                == Some(&json!(true))),
    };
    object.insert("chatOnly".to_string(), chat_only_value);

    // 保留原始审批请求，使被拦截的原生动作事后仍可诊断。
    let captured: Option<Value> = meta
        .get("permissions")
        .and_then(Value::as_array)
        .filter(|permissions| !permissions.is_empty())
        .map(|permissions| {
            json!({ "lastPermission": { "time": result["time"].clone(), "entries": permissions } })
        });

    if !ok
        && source == "request"
        && REQUEST_SHAPED_FAILURES.iter().any(|shaped| Some(*shaped) == code)
    {
        // 上游卡住不是对模型的判决：记录这次尝试，保持发布。
        let mut patch = Map::new();
        patch.insert("lastRequest".to_string(), result.clone());
        insert_all(&mut patch, captured.as_ref());
        persist_status(update(Value::Object(patch)));
        return;
    }

    {
        let mut validated = lock(&app.validated);
        if ok {
            validated.insert(model.to_string());
        } else {
            validated.remove(model);
        }
    }

    // 同一次结果只写一次 status.json：两次 update 会让每次变更触发一次原子写与一次事件推送。
    let mut patch = Map::new();
    patch.insert("lastRequest".to_string(), result.clone());
    if !model.is_empty() {
        let mut model_results = app
            .snapshot()
            .get("modelResults")
            .cloned()
            .unwrap_or_else(|| json!({}));
        model_results[model] = result;
        patch.insert("modelResults".to_string(), model_results);
    }
    patch.insert(
        "availableModels".to_string(),
        Value::Array(
            usable_models()
                .iter()
                .map(|model| model.get("id").cloned().unwrap_or(Value::Null))
                .collect(),
        ),
    );
    insert_all(&mut patch, captured.as_ref());
    persist_status(update(Value::Object(patch)));
}

// ---------------------------------------------------------------------------
// 在途活动（activities / publishActivity / noteActivity）
// ---------------------------------------------------------------------------

/// `publishActivity()`：把在途请求进度写入状态。
fn publish_activity() {
    let app = global_app();
    let now = now_millis();
    let entries = lock(&app.activities);
    let list = entries
        .values()
        .map(|entry| {
            let mut object = Map::new();
            object.insert(
                "model".to_string(),
                entry.fields.get("model").cloned().unwrap_or(Value::Null),
            );
            object.insert(
                "sessionID".to_string(),
                entry.fields.get("sessionID").cloned().unwrap_or(Value::Null),
            );
            object.insert(
                "status".to_string(),
                entry.fields.get("status").cloned().unwrap_or(json!("waiting")),
            );
            object.insert("waitedMs".to_string(), js_number(now - entry.started_at));
            object.insert(
                "sinceEventMs".to_string(),
                entry.last_event_at.map(|at| js_number(now - at)).unwrap_or(Value::Null),
            );
            object.insert(
                "sinceContentMs".to_string(),
                entry
                    .last_content_at
                    .map(|at| js_number(now - at))
                    .unwrap_or(Value::Null),
            );
            for key in ["repairModel", "attempt", "error"] {
                if let Some(value) = entry.fields.get(key) {
                    object.insert(key.to_string(), value.clone());
                }
            }
            Value::Object(object)
        })
        .collect::<Vec<_>>();
    drop(entries);
    persist_status(update(json!({ "activity": list })));
}

/// `noteActivity(progress)`：在途进度合并 + 1s 去抖 + 5s 保活定时器。
fn note_activity(progress: Value) {
    let app = global_app();
    let Some(session_id) = progress
        .get("sessionID")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    if progress.get("type") == Some(&json!("request.done")) {
        lock(&app.activities).remove(&session_id);
        publish_activity();
        return;
    }
    let now = now_millis();
    let published = {
        let mut activities = lock(&app.activities);
        let entry = activities.entry(session_id.clone()).or_insert_with(|| ActivityEntry {
            started_at: now,
            last_event_at: None,
            last_content_at: None,
            written_at: 0.0,
            fields: json!({ "sessionID": session_id, "status": "waiting" }),
        });
        // Object.assign 语义：进度里缺省的键保留旧值（快照中的 Null 即 JS 的 undefined）。
        if let (Some(entry_fields), Some(progress_object)) =
            (entry.fields.as_object_mut(), progress.as_object())
        {
            for (key, value) in progress_object {
                if key == "model" && value.is_null() {
                    continue;
                }
                entry_fields.insert(key.clone(), value.clone());
            }
        }
        entry.last_event_at = Some(now);
        if truthy(progress.get("content").unwrap_or(&Value::Null)) {
            entry.last_content_at = Some(now);
        }
        // 紧急进度（bridge.phase / retry / permission / error）立即写，其余 1s 去抖。
        let progress_type = progress.get("type").and_then(Value::as_str).unwrap_or_default();
        let status = progress.get("status").and_then(Value::as_str).unwrap_or_default();
        let urgent = progress_type == "bridge.phase"
            || status == "retry"
            || status == "permission"
            || truthy(progress.get("error").unwrap_or(&Value::Null));
        if !urgent && now - entry.written_at < 1000.0 {
            false
        } else {
            entry.written_at = now;
            true
        }
    };
    if published {
        publish_activity();
    }
}

/// 5s 保活定时器：上游静默时等待时长必须继续增长（对应 `activityTimer`）。
fn spawn_activity_timer(app: Arc<App>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if app.stopping.load(Ordering::Relaxed) {
                break;
            }
            if !lock(&app.activities).is_empty() {
                publish_activity();
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 探测（startProbes）
// ---------------------------------------------------------------------------

/// `startProbes(modelID, reveal, autoImport)` 的入口守卫与响应体（同步返回，任务后台执行）。
fn start_probes(model_id: Option<Value>, reveal: bool, auto_import: bool) -> Value {
    let app = global_app();
    if app.stopping.load(Ordering::Relaxed) || lock(&app.refresh).is_some() {
        return json!({ "error": { "message": "请等待模型读取完成", "type": "invalid_request_error" } });
    }
    if app.probing.load(Ordering::Relaxed) {
        return json!({ "started": false, "message": "检测正在进行" });
    }
    let models = app.models();
    let selected: Vec<Value> = match model_id.as_ref().and_then(Value::as_str) {
        Some(id) => models
            .iter()
            .filter(|model| model.get("id").and_then(Value::as_str) == Some(id))
            .cloned()
            .collect(),
        None => models.clone(),
    };
    if selected.is_empty() {
        return json!({ "error": { "message": "模型不在当前目录中", "type": "invalid_request_error" } });
    }
    app.probing.store(true, Ordering::Relaxed);
    let pending: Vec<String> = selected
        .iter()
        .filter_map(|model| model.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    let (done_tx, done_rx) = watch::channel(false);
    let id = next_chain_id();
    tokio::spawn(async move {
        run_probe_batch(selected, pending, reveal, auto_import).await;
        let _ = done_tx.send(true);
    });
    *lock(&app.probe_task) = Some(Slot::new(id, done_rx));
    json!({ "started": true })
}

async fn run_probe_batch(
    selected: Vec<Value>,
    mut pending: Vec<String>,
    reveal: bool,
    auto_import: bool,
) {
    let app = global_app();
    let all_models = app.models();
    for model in selected {
        if app.stopping.load(Ordering::Relaxed) {
            break;
        }
        let model_id = model
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut patch = json!({
            "probe": { "running": true, "current": model_id.clone(), "pending": pending.clone() }
        });
        if reveal {
            let mut revealed = app
                .snapshot()
                .get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            revealed.push(model.clone());
            patch["models"] = json!(revealed);
        }
        update_and_persist(patch);

        let started = Instant::now();
        let meta = SharedMeta::new(json!({ "probe": true }));
        // 每个模型一个 deadline：重试与首次尝试共用同一预算，整批探测耗时有界。
        let (deadline, signal) = AbortSignal::channel();
        let timer = tokio::spawn({
            let deadline = deadline.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(PROBE_TIMEOUT_MS)).await;
                deadline.abort();
            }
        });

        let outcome = probe_single_model(&model, &all_models, &meta, signal).await;
        timer.abort();
        let duration_ms = started.elapsed().as_millis() as i64;

        let error = match outcome {
            ProbeOutcome::Passed => {
                record(&model_id, true, None, None, None, duration_ms, "probe", None, &meta.snapshot()).await;
                pending.remove(0);
                update_and_persist(json!({ "probe": { "running": true, "pending": pending.clone() } }));
                continue;
            }
            ProbeOutcome::Failed(cause) => cause,
        };
        // 探测拥有自己的 deadline：abort 本身是opaque 的，超时必须改写为 TimeoutError 语义。
        let timed = deadline.signal().is_aborted() && !app.probe_abort.signal().is_aborted();
        let error = probe::probe_failure(error, timed);
        if app.stopping.load(Ordering::Relaxed) {
            pending.remove(0);
            update_and_persist(json!({ "probe": { "running": true, "pending": pending.clone() } }));
            continue;
        }
        if error.code == "no_action" {
            // 文本回复说明模型可用、只是没产生动作：按仅对话发布而不是判失败（数据红线）。
            record(
                &model_id,
                true,
                Some("探测时只返回文本、未产生动作；已按仅对话发布"),
                None,
                Some("chat_only"),
                duration_ms,
                "probe",
                Some(true),
                &meta.snapshot(),
            )
            .await;
        } else if probe::format_unsupported(&error) {
            let degrade = chat_only_attempt(&model).await;
            match degrade {
                Ok(()) => {
                    record(
                        &model_id,
                        true,
                        Some(&format!("工具转换不兼容：{}", error.message)),
                        None,
                        Some("chat_only"),
                        duration_ms,
                        "probe",
                        Some(true),
                        &meta.snapshot(),
                    )
                    .await;
                }
                Err(chat_error) => {
                    if !app.stopping.load(Ordering::Relaxed) {
                        record(
                            &model_id,
                            false,
                            Some(&chat_error.message),
                            Some(status_value(i64::from(chat_error.status))),
                            Some(&chat_error.code),
                            duration_ms,
                            "probe",
                            None,
                            &meta.snapshot(),
                        )
                        .await;
                    }
                }
            }
        } else {
            let message = if timed { "Model probe timed out".to_string() } else { error.message.clone() };
            record(
                &model_id,
                false,
                Some(&message),
                Some(status_value(i64::from(error.status))),
                Some(&error.code),
                duration_ms,
                "probe",
                None,
                &meta.snapshot(),
            )
            .await;
        }
        pending.remove(0);
        update_and_persist(json!({ "probe": { "running": true, "pending": pending.clone() } }));
    }
    if auto_import && !app.stopping.load(Ordering::Relaxed) {
        sync_published(None);
    }
    app.probing.store(false, Ordering::Relaxed);
    update_and_persist(json!({ "probe": { "running": false } }));
}

enum ProbeOutcome {
    Passed,
    Failed(BridgeError),
}

/// 单模型探测（共享 deadline 由外层控制器提供）。成功返回 Passed；
/// 失败返回原始错误，交由 `probeFailure` / chatOnly 降级分支处理。
async fn probe_single_model(
    model: &Value,
    all_models: &[Value],
    meta: &SharedMeta,
    signal: AbortSignal,
) -> ProbeOutcome {
    let app = global_app();
    if model.get("toolcall") == Some(&json!(false)) {
        return ProbeOutcome::Failed(BridgeError::with(
            "OpenCode catalog does not advertise tool support",
            502,
            "invalid_tool_call",
        ));
    }
    let runtime_slot = app.runtime.clone();
    let model = model.clone();
    let all_models = all_models.to_vec();
    let outcome = probe::probe_model(
        move |token| {
            let backend = lock(&runtime_slot).as_ref().map(|started| started.backend.clone());
            let prepared = prepare(&probe::probe_body(&model, &token), &all_models);
            let meta = meta.clone();
            let signal = signal.clone();
            async move {
                let request = match prepared {
                    Err(error) => return Err(error),
                    Ok(request) => request,
                };
                let Some(backend) = backend else {
                    return Err(BridgeError::with("backend is not ready", 502, "upstream_error"));
                };
                let context = RequestContext {
                    meta,
                    signal,
                    activity: Activity::silent(),
                };
                backend
                    .complete(request, context)
                    .await
                    .map_err(|error| to_bridge_error(&error))
            }
        },
        1,
    )
    .await;
    match outcome {
        Ok(_) => ProbeOutcome::Passed,
        Err(error) => ProbeOutcome::Failed(error),
    }
}

/// 在已有对象上覆盖单个字段（`json!` 不支持展开语法）。
fn with_field(base: &Value, key: &str, value: Value) -> Value {
    let mut object = base.as_object().cloned().unwrap_or_default();
    object.insert(key.to_string(), value);
    Value::Object(object)
}

/// formatUnsupported 后的纯对话尝试：证明模型至少能按 chatOnly 通道服务。
async fn chat_only_attempt(model: &Value) -> Result<(), BridgeError> {
    let app = global_app();
    let model_id = model.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let body = json!({
        "model": model_id,
        "messages": [{ "role": "user", "content": "Reply only OK." }],
    });
    let chat_model = with_field(model, "chatOnly", json!(true));
    let prepared = prepare(&body, &[chat_model])?;
    let backend = lock(&app.runtime)
        .as_ref()
        .map(|started| started.backend.clone())
        .ok_or_else(|| BridgeError::with("backend is not ready", 502, "upstream_error"))?;
    let signal = AbortSignal::any(&[
        app.probe_abort.signal(),
        AbortSignal::timeout(Duration::from_secs(30)),
    ]);
    let context = RequestContext {
        meta: SharedMeta::new(json!({})),
        signal,
        activity: Activity::silent(),
    };
    backend.complete(prepared, context).await.map_err(|error| to_bridge_error(&error)).map(|_| ())
}

// ---------------------------------------------------------------------------
// 读取与重启（refresh / readModels / setSystemProxy）
// ---------------------------------------------------------------------------

/// `refresh(restartRuntime, useSystemProxy)` 的去重入口（对应 `refreshing` promise 复用）。
async fn refresh(restart_runtime: bool, use_system_proxy: Option<bool>) -> Result<Value, BackendError> {
    let app = global_app();
    let existing = lock(&app.refresh).clone();
    if let Some(slot) = existing {
        // 后到的调用复用同一个进行中的 refresh。JS 里所有调用者 await 的是同一个 promise，
        // 失败会传播给每一个等待者；这里必须把结果原样取回，否则第二个调用者会把失败显示成成功。
        let _ = slot.done.clone().wait_for(|done| *done).await;
        if let Some(outcome) = lock(&slot.outcome).clone() {
            return outcome;
        }
        // done 已置却没有结果 = 这一轮没能跑到终点（运行时提前关停等），退回原有的成功口径。
        return Ok(json!({
            "count": app
                .snapshot()
                .get("models")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0)
        }));
    }
    if app.probing.load(Ordering::Relaxed) || app.stopping.load(Ordering::Relaxed) {
        return Err(BackendError::plain("请等待检测完成"));
    }
    let current_proxy = truthy(app.snapshot().get("useSystemProxy").unwrap_or(&Value::Bool(false)));
    let enabled = use_system_proxy.unwrap_or(current_proxy);
    let proxy_env = to_env_map(
        &system_proxy_environment(enabled)
            .await
            .map_err(|error| BackendError::plain(error.message))?,
    );

    lock(&app.validated).clear();
    *lock(&app.models) = Vec::new();
    update_and_persist(json!({
        "phase": "reading",
        "message": "正在读取免费模型…",
        "models": [],
        "availableModels": []
    }));

    // 任务在链位登记之后才开始（gate），否则先结束的这一轮会把链位清成 None 再被写回。
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel::<()>();
    let (result_tx, result_rx) = tokio::sync::oneshot::channel::<Result<Value, BackendError>>();
    let generation = {
        let mut current = lock(&app.refresh_generation);
        *current += 1;
        *current
    };
    let (_predecessor, done_tx, slot) = reserve_chain_slot(&app.refresh);
    let recorded = slot.outcome.clone();
    tokio::spawn(async move {
        let _ = gate_rx.await;
        let outcome = run_refresh(restart_runtime, enabled, proxy_env).await;
        // 对应 JS 的 `finally { refreshing = null }`：只有当前代次才有权释放链位。
        if *lock(&global_app().refresh_generation) == generation {
            *lock(&global_app().refresh) = None;
        }
        // 先写结果、后置 done：复用这一轮的后到者从 done 醒来时一定读得到 outcome。
        *lock(&recorded) = Some(outcome.clone());
        let _ = result_tx.send(outcome);
        let _ = done_tx.send(true);
    });
    let _ = gate_tx.send(());
    result_rx
        .await
        .unwrap_or_else(|_| Err(BackendError::plain("refresh task dropped")))
}

/// `readModels()`：`/admin/refresh` 的实际语义 —— 重启隔离运行时并重新揭示探测进度。
/// 与 JS 一致：探测只在未处于关停流程时启动，且不自动导入。
async fn read_models() -> Result<Value, BackendError> {
    let app = global_app();
    let result = refresh(true, None).await?;
    if !app.stopping.load(Ordering::Relaxed) {
        start_probes(None, true, false);
    }
    Ok(result)
}

async fn run_refresh(
    restart_runtime: bool,
    use_system_proxy: bool,
    proxy_env: Env,
) -> Result<Value, BackendError> {
    let app = global_app();
    if restart_runtime {
        let binary = match lock(&app.binary).clone() {
            Some(binary) => binary,
            None => return Err(BackendError::plain("运行时未就绪")),
        };
        let log_file = std::fs::OpenOptions::new()
            .append(true)
            .open(app.log_file.as_ref())
            .map_err(|error| BackendError::plain(error.to_string()))?;
        let next = Arc::new(
            runtime::start_backend(&binary, &app.data_dir, log_file, &proxy_env)
                .await
                .map_err(BackendError::plain)?,
        );
        if app.stopping.load(Ordering::Relaxed) {
            next.stop().await;
            return Ok(json!({}));
        }
        attach_translator(&next);
        let old = {
            let mut guard = lock(&app.runtime);
            let previous = guard.clone();
            *guard = Some(next.clone());
            previous
        };
        update_and_persist(json!({ "opencodeVersion": next.version.clone() }));
        watch_runtime(next.clone());
        update_and_persist(json!({ "useSystemProxy": use_system_proxy }));
        if let Some(old) = old {
            old.stop().await;
        }
    }
    let backend = match app.runtime() {
        Some(started) => started.backend.clone(),
        None => return Err(BackendError::plain("后端未就绪")),
    };
    let outcome = backend.models().await;
    if app.stopping.load(Ordering::Relaxed) {
        return Ok(json!({}));
    }
    let discovered = match outcome {
        Err(error) => {
            update_and_persist(json!({
                "phase": "error",
                "message": format!("读取失败：{}", error.message())
            }));
            return Err(error);
        }
        Ok(models) => models,
    };
    *lock(&app.models) = discovered.clone();
    update_and_persist(json!({
        "phase": "ready",
        "message": format!("运行中 · {} 个免费模型", discovered.len())
    }));
    Ok(json!({ "count": discovered.len() }))
}

/// `watchRuntime`：子进程意外退出即置错误状态并关停（对应 `child.on('exit')`）。
fn watch_runtime(started: Arc<Started>) {
    let app = global_app().clone();
    let slot = started.child_slot();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let current = lock(&app.runtime).clone();
            let Some(current) = current else { break };
            // 已被更新的运行时替换或正在关停：本监听器就此退出。
            if !Arc::ptr_eq(&current, &started) || app.stopping.load(Ordering::Relaxed) {
                break;
            }
            // 只做非阻塞观察；回收与发信号始终归 stop_backend。
            let exited = {
                let mut guard = lock(&slot);
                match guard.as_mut() {
                    Some(child) => child.try_wait().ok().flatten().is_some(),
                    None => false,
                }
            };
            if exited && !app.stopping.load(Ordering::Relaxed) {
                update_and_persist(json!({
                    "phase": "error",
                    "message": "OpenCode 服务退出，请重启代理"
                }));
                // 对应 JS `shutdown(1)`：本监听器不拥有退出流程，交给关停任务落地。
                spawn_shutdown(1);
                break;
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 关停（shutdown）
// ---------------------------------------------------------------------------

/// `shutdown(code)`：幂等关停，串行写完所有状态后交出退出码。
async fn shutdown(code: u8) -> u8 {
    let app = global_app();
    if app.stopping.swap(true, Ordering::SeqCst) {
        // 关停进行中：等待先到的那一次给出退出码。
        let receiver = lock(&app.shutdown_signal).as_ref().map(|sender| sender.subscribe());
        return match receiver {
            Some(mut receiver) => loop {
                if let Some(value) = *receiver.borrow_and_update() {
                    break value;
                }
                if receiver.changed().await.is_err() {
                    break code;
                }
            },
            None => code,
        };
    }
    app.probe_abort.abort();
    if let Some(control) = lock(&app.server_control).clone() {
        control.abort_all();
    }
    if let Some(sender) = lock(&app.server_stop).as_ref() {
        let _ = sender.send(true);
    }
    sync_published(Some(Vec::new()));
    let runtime = lock(&app.runtime).clone();
    if let Some(runtime) = runtime {
        runtime.stop().await;
    }
    let refreshing = lock(&app.refresh).clone();
    if let Some(refreshing) = refreshing {
        let _ = refreshing.done.clone().wait_for(|done| *done).await;
    }
    let probe_task = lock(&app.probe_task).clone();
    if let Some(probe_task) = probe_task {
        let _ = probe_task.done.clone().wait_for(|done| *done).await;
    }
    if code == 0 {
        update_and_persist(json!({ "phase": "stopped", "message": "已停止" }));
    }
    drain_sync().await;
    drain_status().await;
    let _ = std::fs::remove_file(join_host(&[&app.data_dir, "service.pid"]));
    if let Some(sender) = lock(&app.shutdown_signal).as_ref() {
        let _ = sender.send(Some(code));
    }
    code
}

/// 关停并落地退出：独立进程形态直接结束进程，嵌入壳形态调用壳注入的 hook。
fn spawn_shutdown(code: u8) {
    tokio::spawn(async move {
        let code = shutdown(code).await;
        exit_process(code);
    });
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 启动参数。字段为 `None` 时回落到环境变量（独立进程形态由壳或用户注入）。
///
/// 嵌入 Tauri 壳时必须显式给出 `data_dir` / `port`：同进程内改环境变量会与已启动的
/// tokio 运行时和其他线程产生数据竞争，且 `std::env::set_var` 在并发下本就不安全。
#[derive(Clone, Default)]
pub struct StartOptions {
    pub data_dir: Option<String>,
    pub port: Option<u16>,
    /// 独立进程下接管 SIGTERM/SIGINT；嵌入壳时生命周期归壳，必须为 `false`。
    pub handle_signals: bool,
}

/// 核心编排主流程：装入实例 → 后台任务 → 启动时序 → 等待关停结果。
pub async fn run(options: StartOptions) -> u8 {
    let app = match bootstrap(&options) {
        Ok(app) => app,
        Err(message) => {
            eprintln!("WB Bridge 启动失败：{message}");
            // 与 JS 一致：已有实例在跑时按 2 退出（壳据此提示「核心已在运行」），其余失败按 1。
            let code = if message == ALREADY_RUNNING { 2 } else { 1 };
            exit_process(code);
            return code;
        }
    };
    let (sender, mut receiver) = watch::channel::<Option<u8>>(None);
    *lock(&app.shutdown_signal) = Some(sender);

    if options.handle_signals {
        install_signal_handlers();
    }
    if let Some(parent_pid) = env_text("BUDDY_PARENT_PID").and_then(|text| text.trim().parse::<u32>().ok()).filter(|pid| *pid > 0) {
        spawn_parent_watchdog(app.clone(), parent_pid);
    }
    spawn_activity_timer(app.clone());

    let startup = startup_sequence(&app).await;
    match startup {
        Ok(message) => println!("{message}"),
        Err(error) => {
            update_and_persist(json!({ "phase": "error", "message": error.message() }));
            // 与 JS 一致：服务尚未开始监听才立即关停并按 1 收摊；已在监听则保留运行，
            // 让壳通过 /admin/shutdown 结束这次启动。
            if !app.serving.load(Ordering::Relaxed) {
                let code = shutdown(1).await;
                exit_process(code);
                return code;
            }
        }
    }

    loop {
        if let Some(code) = *receiver.borrow_and_update() {
            return code;
        }
        if receiver.changed().await.is_err() {
            return 0;
        }
    }
}

/// 壳被强杀时 sidecar 不能变孤儿：父进程消失即自行关停，清理配置并释放端口。
fn spawn_parent_watchdog(app: Arc<App>, parent_pid: u32) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            if app.stopping.load(Ordering::Relaxed) {
                return;
            }
            if !pid_alive(parent_pid) {
                // 对应 JS `shutdown(0)`：孤儿进程按正常退出码收摊。
                spawn_shutdown(0);
                return;
            }
        }
    });
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // signal 0 只做存在性检查，不给被监视进程发送任何信号。
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        // STILL_ACTIVE(259) 表示进程仍在运行。
        ok != 0 && exit_code == 259
    }
}

#[cfg(not(any(unix, windows)))]
fn pid_alive(_pid: u32) -> bool {
    true
}

/// 启动时序中与「建目录/建锁/读取配置」相关的部分；成功后初始化全局 App。
fn bootstrap(options: &StartOptions) -> Result<Arc<App>, String> {
    // 1. 端口校验（非 1024–65535 的整数直接失败退出）。
    let port: u16 = match options.port.or_else(|| env_text("BUDDY_PORT").and_then(|text| text.trim().parse::<u16>().ok())) {
        None => 41980,
        Some(value) => {
            if (1024..=65535).contains(&value) {
                value
            } else {
                return Err("Invalid BUDDY_PORT".to_string());
            }
        }
    };

    // 2. 数据目录：显式注入（嵌入壳时由壳给出 app_data_dir）> 环境变量 > 平台默认。
    let data_dir = options
        .data_dir
        .clone()
        .or_else(|| env_text("BUDDY_DATA_DIR"))
        .unwrap_or_else(platform::data_directory);
    std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700));
    }

    // 3. 单实例锁（service.pid）：进程仍存活即视为已在运行。
    let lock_file = join_host(&[&data_dir, "service.pid"]);
    match std::fs::read_to_string(&lock_file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
        Ok(text) => {
            let pid = text.trim().parse::<u32>().unwrap_or(0);
            // 嵌入壳形态下锁文件记的是壳自身的 pid：核心被非正常终止时可能没来得及删锁，
            // 若把自己当成「另一个实例」，面板的「重启」就永远起不来了。
            let mine = pid == std::process::id();
            if pid > 0 && !mine && pid_alive(pid) {
                return Err(ALREADY_RUNNING.to_string());
            }
            let _ = std::fs::remove_file(&lock_file);
        }
    }
    write_exclusive(&lock_file, &std::process::id().to_string()).map_err(|error| error.to_string())?;

    // 4. api-key（缺失时随机生成 32 字节 hex，0600）。
    let token_file = join_host(&[&data_dir, "api-key"]);
    let api_key = match std::fs::read_to_string(&token_file) {
        Ok(text) => text.trim().to_string(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key = random_hex(32);
            write_exclusive(&token_file, &key).map_err(|error| error.to_string())?;
            key
        }
        Err(error) => return Err(error.to_string()),
    };

    // 5. settings.json（容错解析）。
    let settings: Value = std::fs::read_to_string(join_host(&[&data_dir, "settings.json"]))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));

    // 6. status.json 的上一次快照（modelResults 需要延续）。
    let previous: Value = std::fs::read_to_string(join_host(&[&data_dir, "status.json"]))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));

    // 7. 日志轮转（>5MB 归档为 .previous）。
    let log_file = PathBuf::from(join_host(&[&data_dir, "opencode.log"]));
    if let Ok(metadata) = std::fs::metadata(&log_file) {
        if metadata.len() > 5 * 1024 * 1024 {
            let _ = std::fs::rename(&log_file, format!("{}.previous", log_file.display()));
        }
    }
    let mut log_options = std::fs::OpenOptions::new();
    log_options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        log_options.mode(0o600);
    }
    let log_handle = log_options
        .open(&log_file)
        .map(|file| Arc::new(Mutex::new(file)))
        .ok();
    // 手工运行核心（没有壳注入 BUDDY_DATA_DIR）时数据目录会落到平台默认位置，与壳运行时的
    // app_data_dir 可能不是同一处。显式留痕，避免排查时把两套数据目录混为一谈。
    if options.data_dir.is_none() && env_text("BUDDY_DATA_DIR").is_none() {
        if let Some(handle) = log_handle.as_ref() {
            let mut guard = lock(handle);
            let _ = writeln!(guard, "{} 未注入 BUDDY_DATA_DIR，使用平台默认数据目录 {data_dir}（Tauri 壳运行时以壳的 app_data_dir 为准）", now_iso8601());
        }
    }

    let use_system_proxy = settings.get("useSystemProxy") == Some(&json!(true))
        || (platform::host_platform() == "win32"
            && settings.get("useSystemProxy") != Some(&json!(false)));
    let endpoint = format!("http://127.0.0.1:{port}/v1");

    let state = json!({
        "schemaVersion": STATUS_SCHEMA_VERSION,
        "useSystemProxy": use_system_proxy,
        "phase": "starting",
        "message": "正在启动",
        "endpoint": endpoint,
        "pid": std::process::id(),
        "version": "0.2.0",
        "opencodeVersion": Value::Null,
        "models": [],
        "modelResults": previous.get("modelResults").cloned().unwrap_or_else(|| json!({})),
        "sync": Value::Null,
        "availableModels": [],
        "probe": { "running": false },
    });

    let (probe_abort_controller, _) = AbortSignal::channel();
    let app = App {
        data_dir: Arc::new(data_dir.clone()),
        endpoint: Arc::new(endpoint),
        api_key: Arc::new(api_key),
        settings_file: Arc::new(PathBuf::from(join_host(&[&data_dir, "settings.json"]))),
        status_file: Arc::new(PathBuf::from(join_host(&[&data_dir, "status.json"]))),
        log_file: Arc::new(log_file),
        log_handle,
        port,
        state: Arc::new(Mutex::new(state)),
        settings: Arc::new(Mutex::new(settings)),
        models_file: Arc::new(Mutex::new(None)),
        models: Arc::new(Mutex::new(Vec::new())),
        validated: Arc::new(Mutex::new(HashSet::new())),
        binary: Arc::new(Mutex::new(None)),
        runtime: Arc::new(Mutex::new(None)),
        stopping: Arc::new(AtomicBool::new(false)),
        probing: Arc::new(AtomicBool::new(false)),
        serving: Arc::new(AtomicBool::new(false)),
        activities: Arc::new(Mutex::new(HashMap::new())),
        probe_abort: Arc::new(probe_abort_controller),
        server_control: Arc::new(Mutex::new(None)),
        status_task: Arc::new(Mutex::new(None)),
        sync_task: Arc::new(Mutex::new(None)),
        refresh: Arc::new(Mutex::new(None)),
        refresh_generation: Arc::new(Mutex::new(0)),
        probe_task: Arc::new(Mutex::new(None)),
        server_stop: Arc::new(Mutex::new(None)),
        shutdown_signal: Arc::new(Mutex::new(None)),
    };
    let app = Arc::new(app);
    *lock(&APP) = Some(Arc::clone(&app));

    // 8. WorkBuddy 配置路径：env > 面板已保存值 > 平台默认发现；发现失败返回 null（绝不静默回退）。
    let saved = app
        .read_settings()
        .get("workBuddyModelsFile")
        .and_then(Value::as_str)
        .map(str::to_string);
    let models_file = resolve_models_file(
        saved.as_deref(),
        &std::env::vars().collect::<Env>(),
        &platform::home_directory(),
    );
    *lock(&app.models_file) = models_file.clone();
    update_and_persist(json!({ "modelsFile": models_file.map(|value| json!(value)).unwrap_or(Value::Null) }));

    Ok(app)
}

async fn startup_sequence(app: &App) -> Result<String, BackendError> {
    update_and_persist(json!({ "phase": "starting" }));
    // 与 JS 的 `await syncPublished([])` 一致：先把 WorkBuddy 名下的旧条目清掉再开始下载，
    // 否则启动窗口内 WorkBuddy 仍能看到上一轮发布的、此刻并不保证可用的模型。
    sync_published(Some(Vec::new()));
    drain_sync().await;

    // 系统代理：全新安装优先跟随系统代理，但没有手工代理的机器必须能直连下载；
    // 显式保存过的「开」保持严格。
    let strict = app.read_settings().get("useSystemProxy") == Some(&json!(true));
    let current_proxy = truthy(app.snapshot().get("useSystemProxy").unwrap_or(&Value::Bool(false)));
    let startup_proxy = match system_proxy_environment(current_proxy).await {
        Ok(environment) => to_env_map(&environment),
        Err(error) => {
            if strict {
                return Err(BackendError::plain(error.message));
            }
            let environment = system_proxy_environment(false)
                .await
                .map_err(|fallback| BackendError::plain(fallback.message))?;
            update_and_persist(json!({ "useSystemProxy": false }));
            log_line(&format!(
                "未检测到可用的系统代理，首次下载改为直连：{}",
                error.message
            ));
            to_env_map(&environment)
        }
    };

    let binary = runtime::find_runtime(
        &app.data_dir,
        Arc::new(move |message: &str| {
            update_and_persist(json!({ "message": message }));
        }),
        RuntimeOptions {
            log: Some(Arc::new(|message: &str| log_line(message))),
            proxy_env: startup_proxy.clone(),
            ..RuntimeOptions::default()
        },
    )
    .await
    .map_err(BackendError::plain)?;
    *lock(&app.binary) = Some(binary.clone());

    // HTTP 服务先监听：后端就绪检查（/agent、refresh）期间壳即可读取状态。
    let (router, control) = build_server(app);
    *lock(&app.server_control) = Some(control);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", app.port))
        .await
        .map_err(|error| BackendError::plain(format!("Listen failed: {error}")))?;
    app.serving.store(true, Ordering::Relaxed);
    let (stop_tx, stop_rx) = watch::channel(false);
    *lock(&app.server_stop) = Some(stop_tx);
    tokio::spawn(async move {
        if let Err(error) =
            crate::server::serve(listener, router, wait_until_stopped(stop_rx)).await
        {
            eprintln!("HTTP server failed: {error}");
        }
    });
    update_and_persist(json!({ "message": "正在启动隔离模型服务" }));

    let log = std::fs::OpenOptions::new()
        .append(true)
        .open(app.log_file.as_ref())
        .map_err(|error| BackendError::plain(error.to_string()))?;
    let started = Arc::new(
        runtime::start_backend(&binary, &app.data_dir, log, &startup_proxy)
            .await
            .map_err(BackendError::plain)?,
    );
    attach_translator(&started);
    *lock(&app.runtime) = Some(started.clone());
    update_and_persist(json!({ "opencodeVersion": started.version.clone() }));
    watch_runtime(started.clone());

    // 确认这是隔离运行时自带的审批 agent，而不是用户自己的构建 agent。
    let agents = started
        .backend
        .request("/agent", "GET", None, None, None)
        .await
        .map_err(|error| BackendError::plain(error.message))?;
    let has_bridge_agent = agents
        .as_array()
        .map(|list| {
            list.iter()
                .any(|agent| agent.get("name").and_then(Value::as_str) == Some("buddy-bridge"))
        })
        .unwrap_or(false);
    if !has_bridge_agent {
        return Err(BackendError::plain("Dedicated approval-gated agent missing"));
    }

    refresh(false, None).await?;
    start_probes(None, true, true);
    Ok(format!(
        "WB Bridge ready at {}; {} free models",
        app.endpoint,
        app.models().len()
    ))
}

async fn wait_until_stopped(mut receiver: watch::Receiver<bool>) {
    let _ = receiver.wait_for(|stopped| *stopped).await;
}

fn to_env_map(environment: &Value) -> Env {
    environment
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        value.as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect::<Env>()
        })
        .unwrap_or_default()
}

/// 组装 HTTP 服务（对应 `createServer({ ... })`）。
fn build_server(app: &App) -> (axum::Router, ServerControl) {
    Server::new(app.api_key.to_string())
        .backend(|request, context| {
            let started = global_app().runtime();
            Box::pin(async move {
                let Some(started) = started else {
                    return Err(BackendError::plain("backend is not ready"));
                };
                started.backend.complete(request, context).await
            })
        })
        .get_models(usable_models)
        .status(|| global_app().snapshot())
        .refresh(|| Box::pin(async move { read_models().await }))
        .import_models(move |models_file| {
            Box::pin(async move { import_models(models_file).await })
        })
        .set_system_proxy(move |enabled| {
            Box::pin(async move { set_system_proxy(enabled).await })
        })
        .probe(start_probes_admin)
        .on_result(|outcome| {
            Box::pin(async move {
                // 客户端取消的请求绝不记为成功（server.rs 已按 JS 语义过滤，这里兜一层）。
                let snapshot = outcome;
                let model = snapshot
                    .model
                    .as_ref()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                record(
                    &model,
                    snapshot.ok,
                    if snapshot.ok {
                        None
                    } else {
                        Some(&snapshot.message)
                    },
                    snapshot.status,
                    snapshot.code.as_deref(),
                    snapshot.duration_ms,
                    snapshot.source,
                    None,
                    &snapshot.meta,
                )
                .await;
            })
        })
        .on_activity(note_activity)
        .on_shutdown(|| spawn_shutdown(0))
        .build()
}

/// `POST /admin/probe`：JS 的 `probe(body.model)`（单模型手动探测，不自动导入）。
fn start_probes_admin(model: Option<Value>) -> Value {
    match model {
        Some(value) => start_probes(value.get("model").cloned(), false, false),
        None => start_probes(None, false, false),
    }
}

async fn import_models(selected: Option<Value>) -> Result<Value, BackendError> {
    let app = global_app();
    let phase = app
        .snapshot()
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if app.stopping.load(Ordering::Relaxed)
        || app.probing.load(Ordering::Relaxed)
        || lock(&app.refresh).is_some()
        || phase != "ready"
    {
        return Err(BackendError::plain("请等待读取和检测完成后导入"));
    }
    if let Some(value) = selected.filter(|value| !value.is_null()) {
        let Some(selected_file) = value.as_str().map(str::to_string) else {
            return Err(BackendError::plain("modelsFile 必须是字符串"));
        };
        validate_models_file(&selected_file)
            .map_err(|error| BackendError::plain(error.to_string()))?;
        let current = app.models_file();
        let outdated = current
            .as_deref()
            .is_some_and(|path| path != selected_file)
            && current
                .as_deref()
                .map(|path| std::fs::metadata(path).is_ok())
                .unwrap_or(false);
        if outdated {
            sync_published(Some(Vec::new()));
            drain_sync().await;
            if let Some(error) = app
                .snapshot()
                .get("sync")
                .and_then(|sync| sync.get("error"))
                .and_then(Value::as_str)
            {
                return Err(BackendError::plain(error.to_string()));
            }
        }
        let next_settings = with_field(
            &app.read_settings(),
            "workBuddyModelsFile",
            json!(selected_file.clone()),
        );
        app.write_settings(next_settings)
            .await
            .map_err(BackendError::plain)?;
        *lock(&app.models_file) = Some(selected_file.clone());
        update_and_persist(json!({ "modelsFile": selected_file }));
    }
    sync_published(None);
    drain_sync().await;
    let sync = app.snapshot().get("sync").cloned().unwrap_or(Value::Null);
    if let Some(error) = sync.get("error").and_then(Value::as_str) {
        return Err(BackendError::plain(error.to_string()));
    }
    Ok(sync)
}

async fn set_system_proxy(enabled: Value) -> Result<Value, BackendError> {
    let app = global_app();
    let Some(enabled) = enabled.as_bool() else {
        return Err(BackendError::plain("代理开关必须是布尔值"));
    };
    if lock(&app.refresh).is_some() || app.probing.load(Ordering::Relaxed) || app.stopping.load(Ordering::Relaxed) {
        return Err(BackendError::plain("请等待读取和检测完成"));
    }
    refresh(true, Some(enabled)).await?;
    if app.stopping.load(Ordering::Relaxed) {
        return Ok(json!({ "useSystemProxy": enabled }));
    }
    let next_settings = with_field(&app.read_settings(), "useSystemProxy", json!(enabled));
    app.write_settings(next_settings)
        .await
        .map_err(BackendError::plain)?;
    start_probes(None, true, false);
    Ok(json!({ "useSystemProxy": enabled }))
}

fn install_signal_handlers() {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
            let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
            tokio::select! {
                _ = sigterm.recv() => {}
                _ = sigint.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        spawn_shutdown(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 串行链登记必须原子：并发登记时只允许一个线程拿到「无前驱」。
    /// 若两个线程读到同一个 predecessor，写链就分叉 —— 两条分支并发执行同一次写，
    /// 且后登记的一方覆盖先登记的 Slot，`drain()` 等不到被覆盖的那条写。
    #[test]
    fn concurrent_chain_registration_never_forks() {
        let chain: Arc<Mutex<Option<Slot>>> = Arc::new(Mutex::new(None));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let chain = chain.clone();
            workers.push(std::thread::spawn(move || {
                let (predecessor, _done_tx, slot) = reserve_chain_slot(&chain);
                (predecessor.map(|previous| previous.id), slot.id)
            }));
        }
        let pairs: Vec<(Option<u64>, u64)> = workers.into_iter().map(|w| w.join().unwrap()).collect();

        let heads: HashSet<u64> = pairs.iter().map(|(_, id)| *id).collect();
        assert_eq!(heads.len(), 8, "每个任务都要拿到唯一的链位编号");

        let no_predecessor = pairs.iter().filter(|(previous, _)| previous.is_none()).count();
        assert_eq!(no_predecessor, 1, "只有第一个登记者可以没有前驱");

        let mut predecessors: Vec<u64> = pairs.iter().filter_map(|(previous, _)| *previous).collect();
        let before = predecessors.len();
        predecessors.sort_unstable();
        predecessors.dedup();
        assert_eq!(
            predecessors.len(),
            before,
            "出现重复 predecessor = 写链分叉（红线：status.json 必须串行、models.json 不得并发读-改-写）"
        );
        assert!(lock(&chain).is_some(), "链头必须指向最后登记的任务");
    }

    /// 复用进行中 refresh 时，后到者持有的是 Slot 的**克隆**，必须与任务写入的是同一份 outcome。
    /// JS 里所有调用者 await 同一个 promise，失败同样传播；若克隆不共享，第二个调用者会把
    /// 失败显示成成功。
    #[test]
    fn reused_slot_clone_shares_the_recorded_outcome() {
        let (_predecessor, _done_tx, slot) = reserve_chain_slot(&Arc::new(Mutex::new(None)));
        let waiter = slot.clone();
        assert!(lock(&waiter.outcome).is_none(), "未记录结果时没有可复用的 outcome");

        *lock(&slot.outcome) = Some(Err(BackendError::plain("运行时下载失败")));
        let reused = lock(&waiter.outcome).clone();
        match reused {
            Some(Ok(_)) => panic!("复用者不应看到成功"),
            Some(Err(error)) => assert_eq!(error.message, "运行时下载失败"),
            None => panic!("复用者必须读得到首个调用者的失败"),
        }
    }
}
