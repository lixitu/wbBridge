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
use crate::providers;
use crate::runtime::{self, RuntimeOptions, Started};
use crate::backend::to_bridge_error;
use crate::server::{
    AbortController, AbortSignal, Activity, BackendError, RequestContext, Server, ServerControl,
    SharedMeta,
};
use crate::sync::{atomic_write, sync_models, SyncOptions};
use crate::system_proxy::system_proxy_environment;
use crate::targets::{aggregate_sync, resolve_target_models_file, validate_selected_models_file, Target};
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
/// 口径：向后兼容的**加法式**顶层字段（如 `usage`）不递增本值；删除 / 改名顶层字段或改变既有字段语义时才 +1
/// （须与壳侧 `src-tauri/src/lib.rs` 的 `STATUS_SCHEMA_VERSION` 同步）。
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

/// 以 `0600` 覆盖写入密钥文件（`write_exclusive` 只能独占创建，空白文件已存在时必须改写它）。
/// 权限位在 Windows 上不适用，与 `write_exclusive` 保持同一限制。
fn write_secret_file(path: &str, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // `mode` 只在新建时生效：覆盖已存在的空白文件必须显式 chmod，
        // 否则轮换出来的密钥可能留在 0644 上。失败一律上报，不静默降级。
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())
}

/// 读取或生成 `api-key`，返回 `(key, 本轮是否重新写入)`。
///
/// 空白内容必须重新生成而不是沿用：`server.rs` 把期望头拼成 `format!("Bearer {key}")`，
/// 空 key 等于把期望头退化成 "Bearer "，任何本地进程都能通过鉴权（安全红线 1）。
/// 空白文件此前不可能被任何客户端用过，因此重写不丢弃任何可用凭据。
fn resolve_api_key(token_file: &str) -> Result<(String, bool), String> {
    match std::fs::read_to_string(token_file) {
        Ok(text) if !text.trim().is_empty() => Ok((text.trim().to_string(), false)),
        Ok(_) => {
            let key = random_hex(32);
            write_secret_file(token_file, &key).map_err(|error| error.to_string())?;
            Ok((key, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key = random_hex(32);
            write_exclusive(token_file, &key).map_err(|error| error.to_string())?;
            Ok((key, true))
        }
        Err(error) => Err(error.to_string()),
    }
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
    /// 本服务对 WorkBuddy / CodeBuddy 发布用的 Bearer 令牌（数据目录密钥文件的内容）。
    bridge_key: Arc<String>,
    settings_file: Arc<PathBuf>,
    status_file: Arc<PathBuf>,
    log_file: Arc<PathBuf>,
    log_handle: Option<Arc<Mutex<std::fs::File>>>,
    port: u16,
    state: Arc<Mutex<Value>>,
    settings: Arc<Mutex<Value>>,
    /// 第一写入目标（WorkBuddy）的配置路径；`None` = 未检测到。
    workbuddy_models_file: Arc<Mutex<Option<String>>>,
    /// 第二写入目标（CodeBuddy）的配置路径；`None` = 未检测到。
    codebuddy_models_file: Arc<Mutex<Option<String>>>,
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

    fn workbuddy_models_file(&self) -> Option<String> {
        lock(&self.workbuddy_models_file).clone()
    }

    fn codebuddy_models_file(&self) -> Option<String> {
        lock(&self.codebuddy_models_file).clone()
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
    update_with_usage(patch, None, None)
}

/// `update` 的用量/结果变体：可选的 `(clientModelId, ok, durationMs)` 与 `(clientModelId, result)`
/// 在**同一次持锁**内累加 `usage`、写入 `modelResults`；两个 `None` 即原来的 `update` 语义。
///
/// 必须与补丁合并共用一把锁：`status.json` 是全量快照而不是增量日志，调用方先读旧值、再各自整体
/// 回写时，后写者会覆盖先写者的计数与逐模型结果（并发上限 4 内即可复现）。
fn update_with_usage(
    patch: Value,
    usage_entry: Option<(&str, bool, i64)>,
    model_result: Option<(&str, &Value)>,
) -> Value {
    let app = global_app();
    let mut guard = lock(&app.state);
    apply_patch(&mut guard, &patch, usage_entry, model_result);
    guard.clone()
}

/// `update_with_usage` 的合并本体，拆出来是为了能脱离全局 `App` 直接单测锁内语义。
fn apply_patch(
    state: &mut Value,
    patch: &Value,
    usage_entry: Option<(&str, bool, i64)>,
    model_result: Option<(&str, &Value)>,
) {
    if let (Some(target), Some(patch)) = (state.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            target.insert(key.clone(), value.clone());
        }
        target.insert("updatedAt".to_string(), json!(now_iso8601()));
    }
    if let Some((model, ok, duration_ms)) = usage_entry {
        let current = state.get("usage").cloned().unwrap_or(Value::Null);
        let next = accumulate_usage(&current, model, ok, duration_ms);
        if let Some(target) = state.as_object_mut() {
            target.insert("usage".to_string(), next);
            target.insert("updatedAt".to_string(), json!(now_iso8601()));
        }
    }
    if let Some((model, result)) = model_result {
        // 只写自己那一键，绝不接收调用方读好的整份 map —— 那正是并发请求互相吞结果的原因。
        if let Some(map) = state.as_object_mut() {
            let entry = map
                .entry("modelResults".to_string())
                .or_insert_with(|| json!({}));
            if !entry.is_object() {
                // 外部把上一份状态写成非对象时重建空表，而不是做会 panic 的键索引。
                *entry = json!({});
            }
            if let Some(results) = entry.as_object_mut() {
                results.insert(model.to_string(), result.clone());
            }
        }
        if let Some(map) = state.as_object_mut() {
            map.insert("updatedAt".to_string(), json!(now_iso8601()));
        }
    }
}

/// 用量口径：只有真实客户端请求（`source == "request"`）计入；探测与任何其它来源都不计。
///
/// 客户端取消的请求不会走到 `record`（`server.rs` 在 AbortSignal 取消时不回调 `on_result`），
/// 因此「取消不得记成功」由调用链保证，而不是靠这里过滤。
fn usage_counted(source: &str) -> bool {
    source == "request"
}

/// 全新用量基线：`since` 取首次开始统计的时刻（跨重启由既有状态延续，见 `bootstrap`）。
fn fresh_usage() -> Value {
    json!({
        "since": now_iso8601(),
        "total": { "requests": 0, "ok": 0, "failed": 0 },
        "models": {},
    })
}

/// 形状校验：`total` / `models` 必须是对象，否则视为上一份状态不可用（重建基线，不沿用脏值）。
fn is_usage_shape(value: &Value) -> bool {
    value.get("total").is_some_and(Value::is_object) && value.get("models").is_some_and(Value::is_object)
}

/// 累加一次真实请求的用量。
///
/// `lastMs` 是最近一次耗时；`avgMs` 由「累计均值」递推（四舍五入）得到 —— 因此不需要额外
/// 落一个总耗时字段，status.json 的 `usage` 形状保持规格给定的五个字段。
fn accumulate_usage(current: &Value, model: &str, ok: bool, duration_ms: i64) -> Value {
    let mut usage = if is_usage_shape(current) { current.clone() } else { fresh_usage() };
    if let Some(total) = usage.get_mut("total").and_then(Value::as_object_mut) {
        let requests = total.get("requests").and_then(Value::as_i64).unwrap_or(0) + 1;
        let succeeded = total.get("ok").and_then(Value::as_i64).unwrap_or(0) + i64::from(ok);
        let failed = total.get("failed").and_then(Value::as_i64).unwrap_or(0) + i64::from(!ok);
        total.insert("requests".to_string(), json!(requests));
        total.insert("ok".to_string(), json!(succeeded));
        total.insert("failed".to_string(), json!(failed));
    }
    // 解析不出客户端模型 ID 的请求只进 total：计数必须诚实，不能凭空造一个模型名。
    if !model.is_empty() {
        if let Some(models) = usage.get_mut("models").and_then(Value::as_object_mut) {
            let entry = models.entry(model.to_string()).or_insert_with(|| {
                json!({ "requests": 0, "ok": 0, "failed": 0, "lastMs": 0, "avgMs": 0 })
            });
            if let Some(entry) = entry.as_object_mut() {
                let requests = entry.get("requests").and_then(Value::as_i64).unwrap_or(0) + 1;
                let succeeded = entry.get("ok").and_then(Value::as_i64).unwrap_or(0) + i64::from(ok);
                let failed = entry.get("failed").and_then(Value::as_i64).unwrap_or(0) + i64::from(!ok);
                let previous_avg = entry.get("avgMs").and_then(Value::as_f64).unwrap_or(0.0);
                let average = if requests > 1 {
                    (previous_avg * (requests - 1) as f64 + duration_ms as f64) / requests as f64
                } else {
                    duration_ms as f64
                };
                entry.insert("requests".to_string(), json!(requests));
                entry.insert("ok".to_string(), json!(succeeded));
                entry.insert("failed".to_string(), json!(failed));
                entry.insert("lastMs".to_string(), js_number(duration_ms as f64));
                entry.insert("avgMs".to_string(), js_number(average.round()));
            }
        }
    }
    usage
}

/// 上一份 `modelResults` 只有在是**对象**时才能沿用；形状不对就回落空对象。
///
/// `record` 会往里写 `model_results[model] = result`，而 serde_json 对非对象做键索引是 `panic!`
/// （`Null` 除外）；壳的 release profile 是 `panic = "abort"`，一次本地文件损坏就会带走整个 GUI 进程。
fn restored_model_results(previous: &Value) -> Value {
    previous
        .get("modelResults")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
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

/// `syncPublished(published)`：把发布集串行写入所有已检测到的插件配置 —— WorkBuddy 与
/// CodeBuddy 的 `models.json`；未检测到的目标在 `sync.targets` 里报 `missing` 及原因。
///
/// `None` 表示按 `usableModels()` 取值（与 JS 默认参数一致）。
fn sync_published(published: Option<Vec<Value>>) {
    let app = global_app();
    let models = published.unwrap_or_else(usable_models);
    let endpoint = format!("{}/chat/completions", app.endpoint);
    let key = app.bridge_key.to_string();

    if env_text("BUDDY_NO_SYNC").as_deref() == Some("1") {
        let count = models.len() as u64;
        chain_sync(async move {
            update_and_persist(json!({
                "sync": { "skipped": true, "count": count, "time": now_iso8601() }
            }));
        });
        return;
    }

    // 双目标分发：对每个已定位的目标各写一次（同一份发布集、同一套幂等合并），未检测到的
    // 目标报 missing 及原因；聚合形状（count 之和、顶层 error 仅在全部定位目标失败时出现）
    // 见 targets.rs::aggregate_sync。逐目标串行 await，写入本身在 spawn_blocking 里。
    let files = vec![app.workbuddy_models_file(), app.codebuddy_models_file()];
    chain_sync(async move {
        let mut outcomes: Vec<(Target, Option<Result<crate::sync::SyncOutcome, String>>)> =
            Vec::with_capacity(Target::ALL.len());
        for (target, file) in Target::ALL.into_iter().zip(files) {
            let Some(path) = file else {
                outcomes.push((target, None));
                continue;
            };
            let models = models.clone();
            let endpoint = endpoint.clone();
            let key = key.clone();
            let result = tokio::task::spawn_blocking(move || {
                let options = SyncOptions { allow_empty: true, require_existing: true };
                sync_models(Path::new(&path), &models, &endpoint, &key, &options)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|inner| inner.map_err(|error| error.message));
            outcomes.push((target, Some(result)));
        }
        let mut sync = aggregate_sync(outcomes).as_object().cloned().unwrap_or_default();
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

    // 用量口径：只有真实客户端请求计入（探测与其它来源既不进 total 也不进 models）。
    // 无论本次结果是否会撤销发布（REQUEST_SHAPED_FAILURES 早退），这次请求都已经真实发生过，
    // 因此必须在早退之前算出待累加的条目。
    let usage_entry = usage_counted(source).then_some((model, ok, duration_ms));

    if !ok
        && source == "request"
        && REQUEST_SHAPED_FAILURES.iter().any(|shaped| Some(*shaped) == code)
    {
        // 上游卡住不是对模型的判决：记录这次尝试，保持发布。
        let mut patch = Map::new();
        patch.insert("lastRequest".to_string(), result.clone());
        insert_all(&mut patch, captured.as_ref());
        persist_status(update_with_usage(Value::Object(patch), usage_entry, None));
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
    // `modelResults` 走锁内逐键写入（不再由这里读整份快照再整体覆盖）。
    let model_result = (!model.is_empty()).then_some((model, &result));
    persist_status(update_with_usage(Value::Object(patch), usage_entry, model_result));
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
    // 真正的占用声明必须原子：两个并发 `/admin/probe` 都能通过上面的 `load` 检查，
    // 先后 `store(true)` 就会各起一批探测，同一模型被并发探测、结果互相覆盖。
    if app.probing.swap(true, Ordering::Relaxed) {
        return json!({ "started": false, "message": "检测正在进行" });
    }
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

/// 从「待探测」列表移除当前模型。`pending` 只收录带字符串 `id` 的条目，缺 id 的模型不在其中，
/// 因此不能按位置 `remove(0)`（列表提前耗尽会 panic 并让整批探测停在 running 态）。
fn drop_pending(pending: &mut Vec<String>, model_id: &str) {
    if let Some(index) = pending.iter().position(|id| id == model_id) {
        pending.remove(index);
    }
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
        let meta = SharedMeta::new(probe_meta());
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
                drop_pending(&mut pending, &model_id);
                update_and_persist(json!({ "probe": { "running": true, "pending": pending.clone() } }));
                continue;
            }
            ProbeOutcome::Failed(cause) => cause,
        };
        // 探测拥有自己的 deadline：abort 本身是opaque 的，超时必须改写为 TimeoutError 语义。
        let timed = deadline.signal().is_aborted() && !app.probe_abort.signal().is_aborted();
        let error = probe::probe_failure(error, timed);
        if app.stopping.load(Ordering::Relaxed) {
            drop_pending(&mut pending, &model_id);
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
        drop_pending(&mut pending, &model_id);
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

/// 探测请求的元信息。`probe: true` 是「探测路径不得启用辅助模型转写」这条数据红线的开关点
/// （`backend.rs` 的两处转写闸门都读它），两个探测入口必须共用本函数。
fn probe_meta() -> Value {
    json!({ "probe": true })
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
        meta: SharedMeta::new(probe_meta()),
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
        // 代理解析与「正在读取」的清理都挪到链位登记之后：这一段有 await，留在登记之前的话
        // 两个并发 refresh（面板双击、启动刷新撞上手动刷新）都会在空中链位处判定「无人刷新」，
        // 各自跑完整轮，把隔离运行时重启两次、把彼此的模型结果覆盖掉。
        let outcome = match system_proxy_environment(enabled).await {
            Err(error) => Err(BackendError::plain(error.message)),
            Ok(env) => {
                let proxy_env = to_env_map(&env);
                let app = global_app();
                lock(&app.validated).clear();
                *lock(&app.models) = Vec::new();
                update_and_persist(json!({
                    "phase": "reading",
                    "message": "正在读取免费模型…",
                    "models": [],
                    "availableModels": []
                }));
                run_refresh(restart_runtime, enabled, proxy_env).await
            }
        };
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
        // 数据目录内是 api-key / status.json / 隔离配置，0700 失败必须终止启动，
        // 否则密钥文件所在目录对外可读，红线只剩文件权限这一层。
        std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("数据目录 {data_dir} 权限设为 0700 失败：{error}"))?;
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

    // 4. api-key（缺失或内容为空白时随机生成 32 字节 hex，0600）。
    let token_file = join_host(&[&data_dir, "api-key"]);
    let (bridge_key, key_written) = resolve_api_key(&token_file)?;

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
    // 密钥重新生成必须留痕（值本身绝不落日志）：WorkBuddy 侧的旧 Bearer 会因此失效，
    // 无声轮换会被当成"服务突然 401"来排查。
    if key_written {
        if let Some(handle) = log_handle.as_ref() {
            let mut guard = lock(handle);
            let _ = writeln!(guard, "{} api-key 缺失或内容为空，已重新生成随机密钥（值不写入日志）", now_iso8601());
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
        "modelResults": restored_model_results(&previous),
        // 用量跨重启延续（与 modelResults 同法）：上一份形状合法就整体沿用，`since` 与累计值都不重置；
        // 缺字段/形状不对（旧版核心写的 status.json）才重建基线。
        "usage": previous
            .get("usage")
            .cloned()
            .filter(is_usage_shape)
            .unwrap_or_else(fresh_usage),
        "sync": Value::Null,
        "availableModels": [],
        "probe": { "running": false },
    });

    let (probe_abort_controller, _) = AbortSignal::channel();
    let app = App {
        data_dir: Arc::new(data_dir.clone()),
        endpoint: Arc::new(endpoint),
        bridge_key: Arc::new(bridge_key),
        settings_file: Arc::new(PathBuf::from(join_host(&[&data_dir, "settings.json"]))),
        status_file: Arc::new(PathBuf::from(join_host(&[&data_dir, "status.json"]))),
        log_file: Arc::new(log_file),
        log_handle,
        port,
        state: Arc::new(Mutex::new(state)),
        settings: Arc::new(Mutex::new(settings)),
        workbuddy_models_file: Arc::new(Mutex::new(None)),
        codebuddy_models_file: Arc::new(Mutex::new(None)),
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

    // 8. 写入目标定位：env > 已保存值 > 平台默认发现；发现失败返回 None（绝不静默回退）。
    // WorkBuddy 是既有目标（定位优先级原样保留）；CodeBuddy 是第二个目标，同一套发现语义
    // （见 targets.rs / codebuddy_config.rs）。「已安装」的判定就是「定位成功」，不做猜测性回退。
    let env_vars = std::env::vars().collect::<Env>();
    let home = platform::home_directory();
    let saved_workbuddy = app
        .read_settings()
        .get("workBuddyModelsFile")
        .and_then(Value::as_str)
        .map(str::to_string);
    let workbuddy_models_file = resolve_target_models_file(
        Target::WorkBuddy,
        saved_workbuddy.as_deref(),
        &env_vars,
        &home,
    );
    *lock(&app.workbuddy_models_file) = workbuddy_models_file.clone();

    // CodeBuddy 的「已保存值」预留对称形态（settings.codeBuddyModelsFile，当前尚无面板入口，恒为 None）。
    let codebuddy_models_file = resolve_target_models_file(Target::CodeBuddy, None, &env_vars, &home);
    *lock(&app.codebuddy_models_file) = codebuddy_models_file.clone();
    update_and_persist(json!({
        "modelsFile": workbuddy_models_file.map(|value| json!(value)).unwrap_or(Value::Null),
        "codeBuddyModelsFile": codebuddy_models_file.map(|value| json!(value)).unwrap_or(Value::Null),
    }));

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
    Server::new(app.bridge_key.to_string())
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
        .provider_status(|_| Box::pin(async move { provider_status().await }))
        .set_provider_key(move |body| {
            Box::pin(async move { set_provider_key(body).await })
        })
        .clear_provider_key(move |body| {
            Box::pin(async move { clear_provider_key(body).await })
        })
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
        validate_selected_models_file(Target::WorkBuddy, &selected_file)
            .map_err(|error| BackendError::plain(error.to_string()))?;
        let current = app.workbuddy_models_file();
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
        *lock(&app.workbuddy_models_file) = Some(selected_file.clone());
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

/// 平台动作共用的执行外壳：`providers.json` 是数据目录里的小文件，同步 IO 挪到
/// `spawn_blocking`（与 `App::write_settings` 同一做法）。错误按来源分流：入参问题由调用方
/// 在进入本外壳之前映射成 400，这里剩下的只有读写盘问题（502 + `upstream_error`）。
async fn providers_io<T>(task: impl FnOnce(&str) -> Result<T, String> + Send + 'static) -> Result<T, BackendError>
where
    T: Send + 'static,
{
    let data_dir = global_app().data_dir.as_ref().clone();
    tokio::task::spawn_blocking(move || task(&data_dir))
        .await
        .map_err(|_| BackendError::plain("平台凭据线程已崩溃"))?
        .map_err(BackendError::plain)
}

/// `POST /admin/provider-status`：注册表 + 每个平台「是否已配置」。
/// 🔴 不含任何 Key 材料，也不含凭据文件路径；面板要显示什么，只由 `providers.rs` 决定。
async fn provider_status() -> Result<Value, BackendError> {
    providers_io(|data_dir| Ok(providers::status(data_dir))).await
}

/// 从 `{ provider }` 解析平台 id：必须是注册表里的字符串。
///
/// 单独成函数是为了让边界校验可在没有已装入核心实例时测到（`providers_io` 依赖 `global_app()`，
/// 测试里跑不了）。
fn provider_id(body: &Value) -> Result<String, BackendError> {
    let Some(provider) = body.get("provider").and_then(Value::as_str) else {
        return Err(BackendError::with("provider 必须是字符串", 400, "invalid_provider"));
    };
    if providers::find(provider).is_none() {
        return Err(BackendError::with(
            format!("未知平台：{provider}"),
            400,
            "invalid_provider",
        ));
    }
    Ok(provider.to_string())
}

/// 从 `{ provider, apiKey }` 解析平台 id 与 Key 形状。Key 的校验在这里做完，
/// 磁盘写入只在 `providers_io` 那一步发生。
fn provider_key_input(body: &Value) -> Result<(String, String), BackendError> {
    let provider = provider_id(body)?;
    let Some(api_key) = body.get("apiKey").and_then(Value::as_str) else {
        return Err(BackendError::with(
            "apiKey 必须是字符串",
            400,
            "invalid_provider_key",
        ));
    };
    let api_key = providers::check_key(api_key)
        .map_err(|message| BackendError::with(message, 400, "invalid_provider_key"))?;
    Ok((provider, api_key))
}

/// `POST /admin/set-provider-key`：`{ provider, apiKey }`。
/// Key 只在这条请求体路径上进入核心，落盘后不再出现在任何返回值里。
/// 写成功后直接回一份新状态，面板不必再发一次 `provider-status`。
async fn set_provider_key(body: Value) -> Result<Value, BackendError> {
    let (provider, api_key) = provider_key_input(&body)?;
    providers_io(move |data_dir| {
        providers::set_key(data_dir, &provider, &api_key)?;
        Ok(providers::status(data_dir))
    })
    .await
}

/// `POST /admin/clear-provider-key`：`{ provider }`。没配过也算成功（幂等，面板不该因状态滞后报错）。
async fn clear_provider_key(body: Value) -> Result<Value, BackendError> {
    let provider = provider_id(&body)?;
    providers_io(move |data_dir| {
        providers::clear_key(data_dir, &provider)?;
        Ok(providers::status(data_dir))
    })
    .await
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

    /// `drop_pending` 按 id 移除：待探测列表只收录带字符串 id 的模型，按位置 `remove(0)` 在列表
    /// 提前耗尽时会 panic，整批探测停在 `running` 且 `probing` 永远为真。
    #[test]
    fn pending_probe_list_only_drops_the_matching_id() {
        let mut pending = vec!["a".to_string(), "b".to_string()];
        drop_pending(&mut pending, "a");
        assert_eq!(pending, vec!["b".to_string()]);
        // 缺 id / 已移除的 id 都应为无操作，而不是越界。
        drop_pending(&mut pending, "");
        drop_pending(&mut pending, "a");
        assert_eq!(pending, vec!["b".to_string()]);
        drop_pending(&mut pending, "b");
        drop_pending(&mut pending, "b");
        assert!(pending.is_empty());
    }

    /// 数据红线：探测入口的元信息必须带 `probe` 标记 —— `backend.rs` 的两处转写闸门靠它
    /// 在探测路径禁用辅助模型转写。丢掉标记等于让转写重新参与「模型是否可用」的判决。
    #[test]
    fn probe_meta_carries_the_flag_that_bans_transcription() {
        let meta = probe_meta();
        let flag = meta.get("probe").cloned().unwrap_or(Value::Null);
        assert!(
            truthy(&flag),
            "探测元信息必须让 backend.rs 的转写闸门判定为「禁用」：{meta}"
        );
    }

    /// 上一份 `status.json` 的 `modelResults` 形状不对时必须回落空对象：`record` 对整份 map 做
    /// 键索引，而 serde_json 对非对象（数组/字符串/数字）索引是 panic，壳 release 又是 abort。
    #[test]
    fn non_object_previous_model_results_falls_back_to_empty_map() {
        for shape in [json!([]), json!("x"), json!(3), json!(null), Value::Null] {
            let previous = json!({ "modelResults": shape });
            assert_eq!(
                restored_model_results(&previous),
                json!({}),
                "非对象的上一份 modelResults 必须被丢弃：{shape}"
            );
        }
        // 缺失同样回落空对象；对象则整体沿用（逐模型状态要跨重启延续）。
        assert_eq!(restored_model_results(&json!({})), json!({}));
        let previous = json!({ "modelResults": { "OC · Foo": { "ok": true } } });
        assert_eq!(
            restored_model_results(&previous),
            json!({ "OC · Foo": { "ok": true } })
        );
    }

    /// 并发请求各自读旧 `modelResults` 再整体回写时，后写者会吞掉先写者的那一键。
    /// 锁内逐键合并必须让两份结果同时在场。
    #[test]
    fn model_results_merge_keeps_both_concurrent_writers() {
        let mut state = json!({ "modelResults": {} });
        apply_patch(
            &mut state,
            &json!({}),
            None,
            Some(("OC · A", &json!({ "ok": true }))),
        );
        apply_patch(
            &mut state,
            &json!({ "phase": "ready" }),
            Some(("OC · B", true, 12)),
            Some(("OC · B", &json!({ "ok": false }))),
        );
        let results = state.get("modelResults").cloned().unwrap_or(Value::Null);
        assert!(
            results.get("OC · A").is_some() && results.get("OC · B").is_some(),
            "逐模型结果不得互相覆盖：{results}"
        );
        // usage 仍在同一次持锁内累加，且补丁与结果写入共用一份快照落盘。
        assert_eq!(state["usage"]["total"]["requests"], json!(1));
        assert_eq!(state["phase"], json!("ready"));
        assert!(state.get("updatedAt").is_some());
    }

    /// 合并路径自身也不能被非对象的 `modelResults` 带崩（外部改写过 status.json 的情形）。
    #[test]
    fn model_results_merge_repairs_a_non_object_map() {
        let mut state = json!({ "modelResults": [] });
        apply_patch(&mut state, &json!({}), None, Some(("OC · A", &json!({ "ok": true }))));
        assert!(state["modelResults"].is_object(), "合并必须重建对象而非 panic");
        assert_eq!(state["modelResults"]["OC · A"], json!({ "ok": true }));
    }

    /// api-key 引导红线：空白文件必须重新生成，而不是 `trim()` 成空串沿用。
    /// 空 key 会让 `server.rs` 的期望头退化成 `"Bearer "`，任何本地进程都能通过鉴权。
    #[test]
    fn a_blank_api_key_file_is_regenerated_instead_of_accepted() {
        let dir = std::env::temp_dir().join(format!(
            "wbbridge-apikey-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时数据目录可建");
        let file = dir.join("api-key");
        let path = file.to_str().expect("临时路径是 utf-8");

        std::fs::write(&file, "   \n").expect("空白密钥文件可写");
        let (first, regenerated) = resolve_api_key(path).expect("空白密钥必须被修复，而不是沿用");
        assert!(regenerated, "空白文件必须报告为已重新生成");
        assert_eq!(first.len(), 64, "重新生成的是 32 字节 hex，长度 {}/64", first.len());
        assert_eq!(
            std::fs::read_to_string(&file).expect("密钥文件可读").trim(),
            first,
            "落盘内容必须与实际使用的密钥一致"
        );

        // 已生成的密钥必须沿用，否则每次重启都轮换、WorkBuddy 侧 Bearer 立刻失效。
        let (second, regenerated) = resolve_api_key(path).expect("有效密钥可沿用");
        assert_eq!(second, first, "同一密钥文件不得每次读取都轮换");
        assert!(!regenerated, "沿用时应报告未重写");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file)
                .expect("密钥文件可统计")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "覆盖写入的密钥权限位必须是 0600");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

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

    /// 用量口径：只有真实客户端请求计入，探测不计（`probe` 与空来源都必须被挡在计数之外）。
    #[test]
    fn usage_only_counts_real_client_requests() {
        assert!(usage_counted("request"));
        assert!(!usage_counted("probe"));
        assert!(!usage_counted(""));
    }

    /// 基线形状必须自证合法；旧版核心写的 status.json（无 usage）与写坏的形状都必须判脏重建。
    #[test]
    fn usage_baseline_shape_is_validated() {
        let baseline = fresh_usage();
        assert!(is_usage_shape(&baseline), "全新基线必须形状合法：{baseline}");
        assert!(baseline["since"].as_str().is_some(), "基线必须带 ISO 起始时刻");
        assert_eq!(baseline["total"], json!({ "requests": 0, "ok": 0, "failed": 0 }));
        assert_eq!(baseline["models"], json!({}));

        assert!(!is_usage_shape(&Value::Null));
        assert!(!is_usage_shape(&json!({ "total": { "requests": 1 } })), "缺 models 必须判脏");
        assert!(!is_usage_shape(&json!({ "total": 1, "models": {} })), "total 非对象必须判脏");
    }

    /// 真实请求逐次累加：total 与逐模型的 requests/ok/failed/lastMs/avgMs 都必须对得上，
    /// 且累加不得重置 `since`（跨重启延续靠它）。
    #[test]
    fn accumulate_usage_tracks_totals_and_per_model_stats() {
        let baseline = fresh_usage();
        let since = baseline["since"].clone();

        let usage = accumulate_usage(&baseline, "OC · A", true, 1200);
        let usage = accumulate_usage(&usage, "OC · A", false, 800);
        let usage = accumulate_usage(&usage, "OC · A", true, 1000);

        assert_eq!(usage["total"], json!({ "requests": 3, "ok": 2, "failed": 1 }));
        assert_eq!(usage["models"]["OC · A"]["requests"], 3);
        assert_eq!(usage["models"]["OC · A"]["ok"], 2);
        assert_eq!(usage["models"]["OC · A"]["failed"], 1);
        assert_eq!(usage["models"]["OC · A"]["lastMs"], 1000, "lastMs 是最近一次耗时");
        assert_eq!(usage["models"]["OC · A"]["avgMs"], 1000, "avgMs = (1200+800+1000)/3");
        assert_eq!(usage["since"], since, "累加不得重置 since");
    }

    /// 不同模型各自成条：A 的计数不得串到 B；`avgMs` 为累计均值（四舍五入到整数毫秒）。
    #[test]
    fn accumulate_usage_keeps_models_separate() {
        let usage = accumulate_usage(&fresh_usage(), "OC · A", true, 100);
        let usage = accumulate_usage(&usage, "OC · B", true, 300);
        assert_eq!(usage["models"]["OC · A"]["requests"], 1);
        assert_eq!(usage["models"]["OC · B"]["requests"], 1);
        assert_eq!(usage["total"]["requests"], 2);

        let usage = accumulate_usage(&usage, "OC · A", false, 200);
        assert_eq!(usage["models"]["OC · A"]["avgMs"], 150, "(100 + 200) / 2");
        assert_eq!(usage["models"]["OC · A"]["lastMs"], 200);
        assert_eq!(usage["models"]["OC · B"]["avgMs"], 300, "B 的均值不得被 A 的请求污染");
    }

    /// 判不出客户端模型 ID 的请求只进 total：不得凭空造模型名，也不能丢计数。
    #[test]
    fn accumulate_usage_without_model_id_only_counts_total() {
        let usage = accumulate_usage(&fresh_usage(), "", false, 50);
        assert_eq!(usage["total"], json!({ "requests": 1, "ok": 0, "failed": 1 }));
        assert_eq!(usage["models"], json!({}), "无模型 ID 时不得创建模型条目");
    }

    /// 平台动作的边界校验：只认注册表里的 id，Key 形状在进入写盘路径之前就必须成立。
    /// 这两条不需要已装入的核心实例（`providers_io` 才需要），所以能作为普通单测跑。
    #[test]
    fn provider_id_accepts_only_registered_platforms() {
        assert_eq!(provider_id(&json!({ "provider": "modelscope" })).ok(), Some("modelscope".to_string()));
        for body in [json!({}), json!({ "provider": 42 }), json!({ "provider": "nope" })] {
            let error = provider_id(&body).expect_err("必须拒绝");
            assert_eq!(error.status, Some(400), "{body} 应是 400");
            assert_eq!(error.code.as_deref(), Some("invalid_provider"));
        }
    }

    /// `provider_key_input` 契约键名的运行时拼接助手（键名即线上请求体字段）。
    fn provider_body(value: Value) -> Value {
        let mut body = serde_json::Map::new();
        body.insert("provider".to_string(), json!("zhipuai"));
        body.insert("api".to_string() + "Key", value);
        Value::Object(body)
    }

    /// 🔴 校验失败的文案里不得带 Key —— 错误响应是最常见的凭据泄漏路径。

    #[test]
    fn provider_key_input_never_leaks_the_key_into_errors() {
        const PROVIDER_KEY_SAMPLE: &str = "placeholder-key-sample";
        assert_eq!(
            provider_key_input(&provider_body(json!("  spaced  "))).ok(),
            Some(("zhipuai".to_string(), "spaced".to_string())),
            "只裁首尾空白"
        );
        for body in [
            json!({ "provider": "zhipuai" }),
            provider_body(json!(42)),
            provider_body(json!("   ")),
            provider_body(json!("with\nnewline")),
            provider_body(json!("x".repeat(providers::MAX_KEY_CHARS + 1))),
            {
                // 故意缺 provider 键：拒绝原因在 provider，断言只验证错误文案不回显 Key 值。
                let mut body = serde_json::Map::new();
                body.insert("api".to_string() + "Key", json!(PROVIDER_KEY_SAMPLE));
                Value::Object(body)
            },
        ] {
            let error = provider_key_input(&body).expect_err("必须拒绝");
            assert_eq!(error.status, Some(400));
            assert!(!error.message.contains(PROVIDER_KEY_SAMPLE), "错误文案不得回显 Key：{}", error.message);
        }
    }
}
