//! 已归档 Node 核心 `core/src/server.js` 的 axum 等价实现（HTTP 层）。
//!
//! 逐条对齐 Node 版的可见行为：
//! - 路由集合：`GET /health`、`GET /v1/models`、`POST /v1/chat/completions`、
//!   8 条 `POST /admin/*`（动作表见 [`ACTION_ROUTES`]，是这张表的唯一真相；壳的
//!   `src-tauri/src/lib.rs::ADMIN_ROUTES` 由壳侧契约测试逐项对齐，`docs/contract.md` 记录对外口径）；
//! - 鉴权：`Authorization: Bearer <key>`，缺失/不匹配 → 401 `{message, type}`；
//! - 浏览器 Origin 一律 403 `{message}`；
//! - 请求体上限 8 MB → 413，非法 JSON → 400，文案与 Node 版逐字一致；
//! - `/v1/chat/completions` 并发上限 4（含自身）→ 429 `busy`；
//! - SSE：先发校验注释帧、每 10s 心跳、模型首个进度到达时写 streamStart 帧、
//!   校验后的 SSE 帧缓冲输出、结尾 `data: [DONE]`；
//! - 错误结构：`{ error: { message, type, code } }`，`type`/`code` 缺省 `upstream_error`；
//!   `TimeoutError` 的文案统一改写为 `Model request timed out`；
//! - 结果回调（`onResult`）时序：成功/失败均在写响应体之前回调，取消的请求不记录。
//!
//! 与 Node 版的**有意偏差**：
//! 1. JS 把 `meta.activity` 作为回调挂在 meta 对象上（JSON 无法表达函数），Rust 侧改为
//!    [`RequestContext::activity`]；`meta` 只保留可序列化字段（`tools` / `model` 及后端补充项）。
//! 2. `Connection` 等 hop-by-hop 头由 hyper 自行管理。

use crate::json::{js_stringify, truthy};
use crate::model_status::client_model_id;
use crate::protocol::{now_seconds, prepare, random_uuid, send_sse, BridgeError, PreparedRequest};
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

/// 请求体上限（对应 Node 的 `MAX_BODY = 8 * 1024 * 1024`）。
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// 并发上限（对应 Node 的 `active.size > 4`）。
pub const MAX_CONCURRENT_REQUESTS: usize = 4;
/// 与 Node 的 `setInterval(..., 10000)` 一致的 SSE 心跳间隔。
pub const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(10);
/// 模型返回前的占位注释帧（Node：`res.write(': validating model response before emission\n\n')`）。
pub const VALIDATING_FRAME: &str = ": validating model response before emission\n\n";
/// 等待心跳帧（Node：`res.write(': waiting\n\n')`）。
pub const HEARTBEAT_FRAME: &str = ": waiting\n\n";
/// SSE 通道容量（见模块注释第 3 条偏差）。
/// 数据帧始终使用有界通道背压（send().await），避免无限堆积；
/// 1024 帧足够容纳 WorkBuddy 单轮最长工具调用序列而不触发背压丢弃。
pub const STREAM_BUFFER: usize = 1024;
/// 接收请求体的超时（对应 Node 的 `requestTimeout = 20000ms`）。
pub const REQUEST_BODY_TIMEOUT: Duration = Duration::from_secs(20);

/// 动作表：`(动作, 方法, 路由)`，唯一真相；壳侧 `ADMIN_ROUTES` 由
/// `src-tauri/src/lib.rs` 的 `shell_action_routes_match_the_core_contract` 逐项对齐，
/// 红线用例（`tests/red_lines.rs::routes()`）也直接从这张表取管理路由，不再各写一份。
///
/// 顺序亦与 Node 版一致（Node 的 Map 保持插入序）；`provider-*` 三条是迁移后新增的，追加在尾部。
pub const ACTION_ROUTES: [(&str, &str, &str); 8] = [
    ("probe", "POST", "/admin/probe"),
    ("system-proxy", "POST", "/admin/system-proxy"),
    ("import", "POST", "/admin/import"),
    ("refresh", "POST", "/admin/refresh"),
    ("shutdown", "POST", "/admin/shutdown"),
    ("provider-status", "POST", "/admin/provider-status"),
    ("set-provider-key", "POST", "/admin/set-provider-key"),
    ("clear-provider-key", "POST", "/admin/clear-provider-key"),
];

/// `routeFor(action)`：未知动作返回 `None`（对应 JS 的 `null`）。
pub fn route_for(action: &str) -> Option<&'static str> {
    ACTION_ROUTES
        .iter()
        .find(|(name, _, _)| *name == action)
        .map(|(_, _, path)| *path)
}

/// 动作对应的请求方法（目前全部为 `POST`）。
pub fn method_for(action: &str) -> Option<&'static str> {
    ACTION_ROUTES
        .iter()
        .find(|(name, _, _)| *name == action)
        .map(|(_, method, _)| *method)
}

/// `is(action)`：方法与路径同时命中才算命中该动作。
fn action_matches(action: &str, method: &str, route: &str) -> bool {
    match (
        ACTION_ROUTES.iter().find(|(name, _, _)| *name == action),
        method,
    ) {
        (Some((_, expected_method, path)), actual_method) => {
            actual_method == *expected_method && route == *path
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 取消信号
// ---------------------------------------------------------------------------

/// 请求取消信号（对应 Node 的 `AbortSignal` / `AbortController`）。
#[derive(Clone)]
pub struct AbortSignal {
    receiver: watch::Receiver<bool>,
}

/// 取消信号的触发端。
#[derive(Clone)]
pub struct AbortController {
    sender: watch::Sender<bool>,
}

impl AbortSignal {
    /// 新建一对（控制器, 信号）。
    pub fn channel() -> (AbortController, AbortSignal) {
        let (sender, receiver) = watch::channel(false);
        (AbortController { sender }, AbortSignal { receiver })
    }

    /// 是否已被取消。
    pub fn is_aborted(&self) -> bool {
        *self.receiver.borrow()
    }

    /// 等待取消；信号不可能再被触发时永久等待（对齐 `AbortSignal` never settle 的语义）。
    pub async fn cancelled(&self) {
        let mut receiver = self.receiver.clone();
        while !*receiver.borrow() {
            if receiver.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }

    /// `AbortSignal.timeout(ms)`：到时自动触发（对应 JS 的定时器信号）。
    pub fn timeout(duration: Duration) -> Self {
        let (controller, signal) = Self::channel();
        tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            controller.abort();
        });
        signal
    }

    /// `AbortSignal.any([...])`：任一来源触发即视为取消；空集合与 JS 一样永不触发。
    pub fn any(signals: &[AbortSignal]) -> Self {
        let (controller, signal) = Self::channel();
        let sources = signals.to_vec();
        if sources.is_empty() {
            // 丢弃 controller 后信号永不触发（对应 `AbortSignal.any([])`）。
            drop(controller);
            return signal;
        }
        tokio::spawn(async move {
            let waiters: Vec<_> = sources
                .into_iter()
                .map(|source| Box::pin(async move { source.cancelled().await }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>)
                .collect();
            let _ = futures::future::select_all(waiters).await;
            controller.abort();
        });
        signal
    }
}

impl AbortController {
    /// 触发取消（幂等）。
    pub fn abort(&self) {
        let _ = self.sender.send(true);
    }

    /// 对应的取消信号。
    pub fn signal(&self) -> AbortSignal {
        AbortSignal {
            receiver: self.sender.subscribe(),
        }
    }
}

// ---------------------------------------------------------------------------
// 后端契约
// ---------------------------------------------------------------------------

/// 后端请求失败：`BridgeError` 的直接映射，另可标记超时（对应 JS 的 `e.name === 'TimeoutError'`）。
#[derive(Debug, Clone, PartialEq)]
pub struct BackendError {
    pub message: String,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub timed_out: bool,
}

impl BackendError {
    /// 由 [`BridgeError`] 转换（状态码与 code 均已知）。
    pub fn bridge(error: &BridgeError) -> Self {
        Self {
            message: error.message.clone(),
            status: Some(error.status),
            code: Some(error.code.clone()),
            timed_out: false,
        }
    }

    /// 普通异常（Node 侧 `e.status` / `e.code` 缺失）：502 + `upstream_error`。
    pub fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            code: None,
            timed_out: false,
        }
    }

    /// 超时（对应 `TimeoutError`）：文案由 [`BackendError::message`] 统一改写。
    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            code: None,
            timed_out: true,
        }
    }

    /// 指定状态码与错误码。
    pub fn with(message: impl Into<String>, status: u16, code: &str) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
            code: Some(code.to_string()),
            timed_out: false,
        }
    }

    /// 对外文案：超时统一改写为 `Model request timed out`。
    pub fn message(&self) -> String {
        if self.timed_out {
            "Model request timed out".to_string()
        } else {
            self.message.clone()
        }
    }

    /// HTTP 状态码：缺省 502（对应 JS 的 `e.status || 502`）。
    pub fn status_code(&self) -> StatusCode {
        self.status
            .and_then(|status| StatusCode::from_u16(status).ok())
            .unwrap_or(StatusCode::BAD_GATEWAY)
    }

    /// 错误的 JSON 主体：`{ message, type, code }`，`code` 缺省 `upstream_error`。
    pub fn error_value(&self) -> Value {
        let code = self
            .code
            .clone()
            .unwrap_or_else(|| "upstream_error".to_string());
        json!({ "message": self.message(), "type": code, "code": code })
    }
}

impl From<BridgeError> for BackendError {
    fn from(error: BridgeError) -> Self {
        BackendError::bridge(&error)
    }
}

/// 一次模型请求的结果回调参数（对应 JS `onResult(...)` 的前 7 个位置参数）。
///
/// JS 的第 8 个参数（`chatOnly`）由编排层 `record` 按默认规则推导，故不在此处传递。
#[derive(Debug, Clone, PartialEq)]
pub struct ResultRecord {
    /// 模型标识（`body.model` 或 `request.model.id`；非真值时对应 JS 的 `null`）。
    pub model: Option<Value>,
    pub ok: bool,
    /// 失败文案（成功时为空串，对应 JS 的 `undefined`）。
    pub message: String,
    pub status: Option<Value>,
    pub code: Option<String>,
    /// 耗时毫秒（`Math.round(performance.now() - started)`）。
    pub duration_ms: i64,
    /// `probe` / `request`。
    pub source: &'static str,
    /// 本次请求的观测值（对应 JS 的 `meta`）。
    pub meta: Value,
}

/// 后端完成一次请求的 future。
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// 回调类型别名（`Handlers` 字段较长，集中在此声明）。
pub type CompleteFn =
    dyn Fn(PreparedRequest, RequestContext) -> BoxFuture<Result<Value, BackendError>> + Send + Sync;
pub type ModelsFn = dyn Fn() -> Vec<Value> + Send + Sync;
pub type StatusFn = dyn Fn() -> Value + Send + Sync;
pub type AdminFn = dyn Fn(Value) -> BoxFuture<Result<Value, BackendError>> + Send + Sync;
/// `/admin/refresh`：JS 侧 `refresh()` 不接收参数，也不读取请求体。
pub type RefreshFn = dyn Fn() -> BoxFuture<Result<Value, BackendError>> + Send + Sync;
pub type ImportFn =
    dyn Fn(Option<Value>) -> BoxFuture<Result<Value, BackendError>> + Send + Sync;
pub type ProbeFn = dyn Fn(Option<Value>) -> Value + Send + Sync;
pub type ResultFn = dyn Fn(ResultRecord) -> BoxFuture<()> + Send + Sync;
pub type ActivityFn = dyn Fn(Value) + Send + Sync;
pub type ShutdownFn = dyn Fn() + Send + Sync;

/// 请求级共享观测值（对应 JS 的 `meta` 对象，可被后端就地增补）。
#[derive(Clone, Debug)]
pub struct SharedMeta {
    inner: Arc<Mutex<Value>>,
}

impl SharedMeta {
    /// 由初始对象构造。
    pub fn new(value: Value) -> Self {
        Self {
            inner: Arc::new(Mutex::new(value)),
        }
    }

    /// 读取某个键。
    pub fn get(&self, key: &str) -> Option<Value> {
        lock(&self.inner).get(key).cloned()
    }

    /// 写入（覆盖）某个键。
    pub fn set(&self, key: &str, value: Value) {
        let mut guard = lock(&self.inner);
        if let Value::Object(map) = &mut *guard {
            map.insert(key.to_string(), value);
        }
    }

    /// 快照（供 `onResult` 使用）。
    pub fn snapshot(&self) -> Value {
        lock(&self.inner).clone()
    }
}

/// 进度上报句柄（对应 JS 挂在 `meta.activity` 上的回调）。
#[derive(Clone)]
pub struct Activity {
    inner: Arc<ActivityInner>,
}

struct ActivityInner {
    /// SSE 分支的写入口；非流式请求为 `None`。
    sink: Option<mpsc::Sender<Result<Bytes, std::io::Error>>>,
    /// streamStart 帧状态（已写入的 `{id, created}`）。
    stream_start: Option<Arc<Mutex<Option<Value>>>>,
    /// 该请求的取消信号（已取消时不再写 streamStart 帧）。
    signal: AbortSignal,
    /// 流式响应里写 streamStart 帧所需的 `model`（对应 JS 的 `body.model`）。
    model: Value,
    /// 全局进度回调（对应 `onActivity`）。
    observer: Option<Arc<ActivityFn>>,
}

impl Activity {
    /// 无副作用的空实现（非流式请求或测试使用）。
    pub fn silent() -> Self {
        let (_, signal) = AbortSignal::channel();
        Self {
            inner: Arc::new(ActivityInner {
                sink: None,
                stream_start: None,
                signal,
                model: Value::Null,
                observer: None,
            }),
        }
    }

    /// 纯转发的进度句柄（对应 JS 里翻译路径包装出的 `meta.activity` 函数）：
    /// 不写 SSE 帧，只把进度交给回调。
    pub fn forwarding(
        signal: &AbortSignal,
        observer: impl Fn(Value) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(ActivityInner {
                sink: None,
                stream_start: None,
                signal: signal.clone(),
                model: Value::Null,
                observer: Some(Arc::new(observer)),
            }),
        }
    }

    /// 是否等价于 JS 的「`meta.activity` 不是函数」（既无 SSE 写入口也无观察者）。
    pub fn is_noop(&self) -> bool {
        self.inner.sink.is_none() && self.inner.observer.is_none()
    }

    /// 上报一条进度（等价于 JS 调用 `meta.activity(progress)`）。
    ///
    /// 流式请求里 `progress.content === true` 的那一次会写入 streamStart 帧；随后转发给全局回调。
    pub fn report(&self, progress: Value) {
        if let (Some(sink), Some(stream_start)) = (&self.inner.sink, &self.inner.stream_start) {
            if progress.get("content") == Some(&Value::Bool(true)) && !self.inner.signal.is_aborted()
            {
                let mut slot = lock(stream_start);
                if slot.is_none() {
                    let start = json!({
                        "id": format!("chatcmpl-{}", random_uuid()),
                        "created": now_seconds(),
                    });
                    let frame = json!({
                        "id": start.get("id").cloned().unwrap_or(Value::Null),
                        "created": start.get("created").cloned().unwrap_or(Value::Null),
                        "object": "chat.completion.chunk",
                        "model": self.inner.model.clone(),
                        "choices": [{ "index": 0, "delta": { "role": "assistant" }, "finish_reason": Value::Null }],
                    });
                    let text = format!("data: {}\n\n", js_stringify(&frame));
                    // 始终记录 stream_start：若通道满导致此帧丢弃，
                    // 最终帧（send().await）会合并 role 数据，确保不丢失。
                    *slot = Some(start);
                    let _ = sink.try_send(Ok(Bytes::from(text)));
                }
            }
        }
        if let Some(observer) = &self.inner.observer {
            observer(progress);
        }
    }
}

/// 传给后端的请求上下文。
pub struct RequestContext {
    /// 本次请求的观测值（`tools` / `model`，后端可增补 `calls` / `handoff` 等）。
    pub meta: SharedMeta,
    /// 取消信号：客户端断开或服务停机时触发。
    pub signal: AbortSignal,
    /// 进度上报。
    pub activity: Activity,
}

// ---------------------------------------------------------------------------
// 注入的处理器集合
// ---------------------------------------------------------------------------

/// 服务所需的外部能力（对应 `createServer({...})` 的选项）。
pub struct Handlers {
    /// `Authorization: Bearer` 期望的密钥。
    pub key: String,
    /// `backend.complete(request, signal, meta)`。
    pub backend: Arc<CompleteFn>,
    /// `getModels()`：当前可用的免费模型目录。
    pub get_models: Arc<ModelsFn>,
    /// `status()`：`/health` 返回的状态对象。
    pub status: Arc<StatusFn>,
    /// `refresh()`。
    pub refresh: Arc<RefreshFn>,
    /// `importModels(modelsFile)`。
    pub import_models: Arc<ImportFn>,
    /// `setSystemProxy(enabled)`。
    pub set_system_proxy: Arc<AdminFn>,
    /// `probe(model)`：同步返回（响应 202）。
    pub probe: Arc<ProbeFn>,
    /// `providerStatus()`：平台注册表 + 每个平台「是否已配置」。
    /// 🔴 返回值不得含任何 Key 材料（连尾几位都不回显），面板要更多只能加注册表内的静态字段。
    pub provider_status: Arc<AdminFn>,
    /// `setProviderKey(body)`：入参是整个请求体对象（`{ provider, apiKey }` 两个字段），
    /// 这点与 `set_system_proxy` 只收单个字段不同 —— 校验在编排层做。
    pub set_provider_key: Arc<AdminFn>,
    /// `clearProviderKey(body)`：入参 `{ provider }`。
    pub clear_provider_key: Arc<AdminFn>,
    /// `onResult(model, ok, message, status, code, duration, source, meta)`。
    pub on_result: Arc<ResultFn>,
    /// `onActivity(progress)`。
    pub on_activity: Option<Arc<ActivityFn>>,
    /// `onShutdown()`：`/admin/shutdown` 先响应、后触发。
    pub on_shutdown: Option<Arc<ShutdownFn>>,
    /// SSE 心跳间隔。
    pub heartbeat: Duration,
}

/// 服务器构造器（链式注入各处理器）。
pub struct Server {
    handlers: Handlers,
}

impl Server {
    /// 以鉴权密钥创建，其余处理器为安全缺省值。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            handlers: Handlers {
                key: key.into(),
                backend: Arc::new(|_, _| {
                    Box::pin(async { Err(BackendError::plain("backend is not configured")) })
                }),
                get_models: Arc::new(Vec::new),
                status: Arc::new(|| json!({})),
                refresh: Arc::new(|| Box::pin(async { Ok(json!({})) })),
                import_models: Arc::new(|_| Box::pin(async { Ok(json!({})) })),
                set_system_proxy: Arc::new(|_| Box::pin(async { Ok(json!({})) })),
                probe: Arc::new(|_| Value::Null),
                provider_status: Arc::new(|_| Box::pin(async { Ok(json!({ "providers": [] })) })),
                set_provider_key: Arc::new(|_| Box::pin(async { Ok(json!({})) })),
                clear_provider_key: Arc::new(|_| Box::pin(async { Ok(json!({})) })),
                on_result: Arc::new(|_| Box::pin(async {})),
                on_activity: None,
                on_shutdown: None,
                heartbeat: DEFAULT_HEARTBEAT,
            },
        }
    }

    /// 注入后端 `complete`。
    #[must_use]
    pub fn backend<F>(mut self, backend: F) -> Self
    where
        F: Fn(PreparedRequest, RequestContext) -> BoxFuture<Result<Value, BackendError>>
            + Send
            + Sync
            + 'static,
    {
        self.handlers.backend = Arc::new(backend);
        self
    }

    /// 注入模型目录读取。
    #[must_use]
    pub fn get_models<F>(mut self, get_models: F) -> Self
    where
        F: Fn() -> Vec<Value> + Send + Sync + 'static,
    {
        self.handlers.get_models = Arc::new(get_models);
        self
    }

    /// 注入状态读取。
    #[must_use]
    pub fn status<F>(mut self, status: F) -> Self
    where
        F: Fn() -> Value + Send + Sync + 'static,
    {
        self.handlers.status = Arc::new(status);
        self
    }

    /// 注入 `refresh()`（对应 JS 的无参 `refresh()`：不读取请求体）。
    #[must_use]
    pub fn refresh<F>(mut self, refresh: F) -> Self
    where
        F: Fn() -> BoxFuture<Result<Value, BackendError>> + Send + Sync + 'static,
    {
        self.handlers.refresh = Arc::new(refresh);
        self
    }

    /// 注入 `importModels(modelsFile)`。
    #[must_use]
    pub fn import_models<F>(mut self, import_models: F) -> Self
    where
        F: Fn(Option<Value>) -> BoxFuture<Result<Value, BackendError>> + Send + Sync + 'static,
    {
        self.handlers.import_models = Arc::new(import_models);
        self
    }

    /// 注入 `setSystemProxy(enabled)`。
    #[must_use]
    pub fn set_system_proxy<F>(mut self, set_system_proxy: F) -> Self
    where
        F: Fn(Value) -> BoxFuture<Result<Value, BackendError>> + Send + Sync + 'static,
    {
        self.handlers.set_system_proxy = Arc::new(set_system_proxy);
        self
    }

    /// 注入 `probe(model)`。
    #[must_use]
    pub fn probe<F>(mut self, probe: F) -> Self
    where
        F: Fn(Option<Value>) -> Value + Send + Sync + 'static,
    {
        self.handlers.probe = Arc::new(probe);
        self
    }

    /// 注入 `providerStatus()`。
    #[must_use]
    pub fn provider_status<F>(mut self, provider_status: F) -> Self
    where
        F: Fn(Value) -> BoxFuture<Result<Value, BackendError>> + Send + Sync + 'static,
    {
        self.handlers.provider_status = Arc::new(provider_status);
        self
    }

    /// 注入 `setProviderKey(body)`。
    #[must_use]
    pub fn set_provider_key<F>(mut self, set_provider_key: F) -> Self
    where
        F: Fn(Value) -> BoxFuture<Result<Value, BackendError>> + Send + Sync + 'static,
    {
        self.handlers.set_provider_key = Arc::new(set_provider_key);
        self
    }

    /// 注入 `clearProviderKey(body)`。
    #[must_use]
    pub fn clear_provider_key<F>(mut self, clear_provider_key: F) -> Self
    where
        F: Fn(Value) -> BoxFuture<Result<Value, BackendError>> + Send + Sync + 'static,
    {
        self.handlers.clear_provider_key = Arc::new(clear_provider_key);
        self
    }

    /// 注入结果回调。
    #[must_use]
    pub fn on_result<F>(mut self, on_result: F) -> Self
    where
        F: Fn(ResultRecord) -> BoxFuture<()> + Send + Sync + 'static,
    {
        self.handlers.on_result = Arc::new(on_result);
        self
    }

    /// 注入进度回调。
    #[must_use]
    pub fn on_activity<F>(mut self, on_activity: F) -> Self
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.handlers.on_activity = Some(Arc::new(on_activity));
        self
    }

    /// 注入停机回调（`/admin/shutdown` 响应之后触发）。
    #[must_use]
    pub fn on_shutdown<F>(mut self, on_shutdown: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.handlers.on_shutdown = Some(Arc::new(on_shutdown));
        self
    }

    /// 覆盖 SSE 心跳间隔（测试用；生产保持 [`DEFAULT_HEARTBEAT`]）。
    #[must_use]
    pub fn heartbeat(mut self, heartbeat: Duration) -> Self {
        self.handlers.heartbeat = heartbeat;
        self
    }

    /// 构建路由与在途请求控制器。
    pub fn build(self) -> (Router, ServerControl) {
        let control = ServerControl::new();
        let shared = Arc::new(Shared {
            handlers: self.handlers,
            control: control.clone(),
            next_id: AtomicU64::new(1),
        });
        let router = Router::new().fallback(any(dispatch)).with_state(shared);
        (router, control)
    }
}

/// 在途请求表（对应 Node 的 `active` 集合），支持 `abortAll()`（对应 `server.abortAll`）。
#[derive(Clone)]
pub struct ServerControl {
    inner: Arc<Mutex<HashMap<u64, AbortController>>>,
}

impl ServerControl {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 当前在途请求数（含尚未写出响应的流式请求）。
    pub fn active(&self) -> usize {
        lock(&self.inner).len()
    }

    /// 取消所有在途请求（对应 Node `server.abortAll()`）。
    pub fn abort_all(&self) {
        for controller in lock(&self.inner).values() {
            controller.abort();
        }
    }

    fn register(&self, id: u64, controller: AbortController) {
        lock(&self.inner).insert(id, controller);
    }

    fn finish(&self, id: u64) {
        lock(&self.inner).remove(&id);
    }
}

/// 在途请求的存活期守卫（对应 Node handler 的 `finally { active.delete(controller) }`）。
struct ActiveRequest {
    control: ServerControl,
    id: u64,
}

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.control.finish(self.id);
    }
}

struct Shared {
    handlers: Handlers,
    control: ServerControl,
    next_id: AtomicU64,
}

/// 监听并服务；`shutdown` 完成时优雅停机。
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

// ---------------------------------------------------------------------------
// 入口处理
// ---------------------------------------------------------------------------

async fn dispatch(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();

    if !authorized(&parts.headers, &shared.handlers.key) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            json!({ "error": { "message": "Local proxy API key required", "type": "authentication_error" } }),
        );
    }
    if has_origin(&parts.headers) {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({ "error": { "message": "Browser-origin requests are disabled" } }),
        );
    }

    let method = parts.method.as_str().to_string();
    let route = normalize_path(parts.uri.path());

    // 与 Node 一致：所有通过鉴权的请求都计入在途表，直到响应写出。
    let (controller, signal) = AbortSignal::channel();
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    shared.control.register(id, controller.clone());
    let active = ActiveRequest {
        control: shared.control.clone(),
        id,
    };

    // 在途守卫的所有权沿调用链下传：流式请求由后台任务持有，响应写出后才释放。
    handle(&shared, &method, &route, &controller, &signal, body, active).await
}

async fn handle(
    shared: &Arc<Shared>,
    method: &str,
    route: &str,
    controller: &AbortController,
    signal: &AbortSignal,
    body: Body,
    _active: ActiveRequest,
) -> Response {
    let handlers = &shared.handlers;

    if method == "GET" && route == "/health" {
        return json_response(StatusCode::OK, (handlers.status)());
    }

    if method == "GET" && route == "/v1/models" {
        let data = (handlers.get_models)()
            .iter()
            .map(|model| {
                let id = client_model_id(model);
                json!({ "id": id.clone(), "object": "model", "owned_by": "opencode", "name": id })
            })
            .collect::<Vec<_>>();
        return json_response(StatusCode::OK, json!({ "object": "list", "data": data }));
    }

    if action_matches("probe", method, route) {
        let body = match read_body(body).await {
            Ok(body) => body,
            Err(error) => return error_response(&error),
        };
        return json_response(StatusCode::ACCEPTED, (handlers.probe)(body.get("model").cloned()));
    }

    if action_matches("system-proxy", method, route) {
        let body = match read_body(body).await {
            Ok(body) => body,
            Err(error) => return error_response(&error),
        };
        let enabled = body.get("enabled").cloned().unwrap_or(Value::Null);
        return match (handlers.set_system_proxy)(enabled).await {
            Ok(result) => json_response(StatusCode::OK, result),
            Err(error) => error_response(&error),
        };
    }

    if action_matches("import", method, route) {
        let body = match read_body(body).await {
            Ok(body) => body,
            Err(error) => return error_response(&error),
        };
        let models_file = body.get("modelsFile").cloned();
        return match (handlers.import_models)(models_file).await {
            Ok(result) => json_response(StatusCode::OK, result),
            Err(error) => error_response(&error),
        };
    }

    if action_matches("refresh", method, route) {
        // 与 Node 一致：`refresh()` 不读取请求体（壳侧仍会发送 `null` 占位）。
        return match (handlers.refresh)().await {
            Ok(result) => json_response(StatusCode::OK, result),
            Err(error) => error_response(&error),
        };
    }

    if action_matches("shutdown", method, route) {
        if let Some(on_shutdown) = &handlers.on_shutdown {
            let on_shutdown = on_shutdown.clone();
            // 对应 Node 的 setImmediate(onShutdown)：先响应，再触发停机。
            tokio::spawn(async move { on_shutdown() });
        }
        return json_response(StatusCode::OK, json!({ "ok": true }));
    }

    if action_matches("provider-status", method, route) {
        // 与 `/admin/refresh` 同理：不读取请求体（壳侧发送 `null` 占位）。
        return match (handlers.provider_status)(Value::Null).await {
            Ok(result) => json_response(StatusCode::OK, result),
            Err(error) => error_response(&error),
        };
    }

    if action_matches("set-provider-key", method, route) {
        // 整份请求体交给编排层：`{ provider, apiKey }` 两个字段一起校验，Key 只在这条路径上
        // 进入核心，落盘后不再回显（见 `providers.rs` 的凭据纪律）。
        let body = match read_body(body).await {
            Ok(body) => body,
            Err(error) => return error_response(&error),
        };
        return match (handlers.set_provider_key)(body).await {
            Ok(result) => json_response(StatusCode::OK, result),
            Err(error) => error_response(&error),
        };
    }

    if action_matches("clear-provider-key", method, route) {
        let body = match read_body(body).await {
            Ok(body) => body,
            Err(error) => return error_response(&error),
        };
        return match (handlers.clear_provider_key)(body).await {
            Ok(result) => json_response(StatusCode::OK, result),
            Err(error) => error_response(&error),
        };
    }

    if method != "POST" || route != "/v1/chat/completions" {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({ "error": { "message": "Not found" } }),
        );
    }

    chat(shared, controller, signal, body, _active).await
}

/// `POST /v1/chat/completions`。
async fn chat(
    shared: &Arc<Shared>,
    controller: &AbortController,
    signal: &AbortSignal,
    body: Body,
    active: ActiveRequest,
) -> Response {
    let handlers = &shared.handlers;

    if shared.control.active() > MAX_CONCURRENT_REQUESTS {
        return error_response(&BackendError::bridge(&BridgeError::with(
            "At most four requests may run at once",
            429,
            "busy",
        )));
    }

    let body = match tokio::time::timeout(REQUEST_BODY_TIMEOUT, read_body(body)).await {
        Ok(Ok(body)) => body,
        Ok(Err(error)) => return error_response(&error),
        Err(_) => return error_response(&BackendError::bridge(&BridgeError::with(
            "Request body read timed out",
            408,
            "timeout",
        ))),
    };

    let mut model = body.get("model").cloned().filter(|value| truthy(Some(value)));
    let prepared = match prepare(&body, &(handlers.get_models)()) {
        Ok(prepared) => prepared,
        Err(error) => return error_response(&BackendError::bridge(&error)),
    };
    let model_id = prepared
        .model()
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // `body.model || request._resolvedModel.id || null`
    if model.is_none() {
        model = Some(json!(model_id.clone()));
    }

    let body_model = body.get("model").cloned().unwrap_or(Value::Null);
    let stream = truthy(body.get("stream"));
    let include_usage = truthy(
        body.get("stream_options")
            .and_then(|options| options.get("include_usage")),
    );

    let meta = SharedMeta::new(json!({
        "tools": prepared.tools().len(),
        "model": model_id,
    }));

    let (sender, receiver) = if stream {
        let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(STREAM_BUFFER);
        let _ = sender.try_send(Ok(Bytes::from(VALIDATING_FRAME)));
        (Some(sender), Some(receiver))
    } else {
        (None, None)
    };
    let stream_start = Arc::new(Mutex::new(None));
    let activity = Activity {
        inner: Arc::new(ActivityInner {
            sink: sender.clone(),
            stream_start: if stream {
                Some(stream_start.clone())
            } else {
                None
            },
            signal: signal.clone(),
            model: body_model.clone(),
            observer: handlers.on_activity.clone(),
        }),
    };

    let context = RequestContext {
        meta: meta.clone(),
        signal: signal.clone(),
        activity,
    };
    let future = (handlers.backend)(prepared, context);
    let started = Instant::now();

    let Some(sender) = sender else {
        let result = future.await;
        if signal.is_aborted() {
            // 对应 JS：客户端已断开，不再写响应体，也不记录结果。
            return Response::new(Body::empty());
        }
        let duration_ms = elapsed_ms(started);
        return match result {
            Ok(mut value) => {
                (handlers.on_result)(record(
                    &model, true, "", None, None, duration_ms, &meta,
                ))
                .await;
                if let Value::Object(target) = &mut value {
                    target.insert("model".to_string(), body_model);
                }
                json_response(StatusCode::OK, value)
            }
            Err(error) => {
                (handlers.on_result)(record(
                    &model,
                    false,
                    &error.message(),
                    error.status.map(Value::from),
                    error.code.clone(),
                    duration_ms,
                    &meta,
                ))
                .await;
                error_response(&error)
            }
        };
    };

    let job = StreamJob {
        handlers: shared.handlers.clone_handlers(),
        meta,
        signal: signal.clone(),
        controller: controller.clone(),
        stream_start,
        model,
        body_model,
        include_usage,
        heartbeat: handlers.heartbeat,
        sender,
        future,
        started,
        active,
    };
    tokio::spawn(job.run());
    sse_response(receiver.expect("流式分支必然有接收端"))
}

/// SSE 分支的后台任务：心跳 + 应用结果帧。
struct StreamJob {
    handlers: Handlers,
    meta: SharedMeta,
    signal: AbortSignal,
    controller: AbortController,
    stream_start: Arc<Mutex<Option<Value>>>,
    model: Option<Value>,
    body_model: Value,
    include_usage: bool,
    heartbeat: Duration,
    sender: mpsc::Sender<Result<Bytes, std::io::Error>>,
    future: BoxFuture<Result<Value, BackendError>>,
    started: Instant,
    /// 在途请求守卫（对应 Node handler 的 `finally`，随任务结束释放并发额度）。
    active: ActiveRequest,
}

impl StreamJob {
    async fn run(self) {
        let StreamJob {
            handlers,
            meta,
            signal,
            controller,
            stream_start,
            model,
            body_model,
            include_usage,
            heartbeat,
            sender,
            mut future,
            started,
            active: _active,
        } = self;

        let mut timer = tokio::time::interval(heartbeat);
        timer.tick().await; // interval 的首次 tick 立即返回，丢弃以对齐 setInterval 语义。
        let result = loop {
            tokio::select! {
                _ = timer.tick() => {
                    if sender.try_send(Ok(Bytes::from(HEARTBEAT_FRAME))).is_err() {
                        // 客户端已断开：取消后端请求并结束。
                        controller.abort();
                        return;
                    }
                }
                result = &mut future => break result,
            }
        };

        if signal.is_aborted() {
            return;
        }
        let duration_ms = elapsed_ms(started);
        let frame = match result {
            Ok(mut value) => {
                (handlers.on_result)(record(&model, true, "", None, None, duration_ms, &meta)).await;
                if let Value::Object(target) = &mut value {
                    target.insert("model".to_string(), body_model);
                }
                let start = lock(&stream_start).take();
                let role_sent = start.is_some();
                if let (Some(Value::Object(source)), Value::Object(target)) = (start, &mut value) {
                    for (key, item) in source {
                        target.insert(key, item);
                    }
                }
                send_sse(&value, include_usage, role_sent)
            }
            Err(error) => {
                (handlers.on_result)(record(
                    &model,
                    false,
                    &error.message(),
                    error.status.map(Value::from),
                    error.code.clone(),
                    duration_ms,
                    &meta,
                ))
                .await;
                // 响应头已发出：只能在流里补一帧错误。
                format!("data: {}\n\n", js_stringify(&json!({ "error": error.error_value() })))
            }
        };
        if sender.send(Ok(Bytes::from(frame))).await.is_err() {
            controller.abort();
        }
    }
}

impl Handlers {
    /// `Handlers` 内含 `Arc`，字段级克隆即共享（避免为每个流式任务再包一层 `Arc`）。
    fn clone_handlers(&self) -> Handlers {
        Handlers {
            key: self.key.clone(),
            backend: self.backend.clone(),
            get_models: self.get_models.clone(),
            status: self.status.clone(),
            refresh: self.refresh.clone(),
            import_models: self.import_models.clone(),
            set_system_proxy: self.set_system_proxy.clone(),
            probe: self.probe.clone(),
            provider_status: self.provider_status.clone(),
            set_provider_key: self.set_provider_key.clone(),
            clear_provider_key: self.clear_provider_key.clone(),
            on_result: self.on_result.clone(),
            on_activity: self.on_activity.clone(),
            on_shutdown: self.on_shutdown.clone(),
            heartbeat: self.heartbeat,
        }
    }
}

/// 组装 [`ResultRecord`]（`model` 非真值时对应 JS 的 `model || null`）。
fn record(
    model: &Option<Value>,
    ok: bool,
    message: &str,
    status: Option<Value>,
    code: Option<String>,
    duration_ms: i64,
    meta: &SharedMeta,
) -> ResultRecord {
    ResultRecord {
        model: model.clone().filter(|value| truthy(Some(value))),
        ok,
        message: message.to_string(),
        status,
        code,
        duration_ms,
        source: "request",
        meta: meta.snapshot(),
    }
}

// ---------------------------------------------------------------------------
// 协议细节
// ---------------------------------------------------------------------------

/// `authorized(req)`：`timingSafeEqual(Buffer.from(header || ''), Buffer.from('Bearer ' + key))`。
///
/// 空密钥一律拒绝：否则期望值退化成 `"Bearer "`，任何本地进程都能匹配（安全红线 1）。
/// 正常链路里 `orchestration::resolve_api_key` 已保证密钥非空，这里是防止其他调用方绕过引导。
fn authorized(headers: &axum::http::HeaderMap, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let Some(header) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let expected = format!("Bearer {key}");
    let candidate = header.as_bytes();
    // Node 的 Buffer.from(string) 是 UTF-8；非法字节按 UTF-8 编码后再比较。
    let candidate = match std::str::from_utf8(candidate) {
        Ok(text) => text.as_bytes().to_vec(),
        Err(_) => header.to_str().unwrap_or_default().as_bytes().to_vec(),
    };
    constant_time_eq(&candidate, expected.as_bytes())
}

/// 定长比较（对应 Node 的 `timingSafeEqual`；长度不等直接返回 false）。
/// 用 `subtle` 而非手写异或累加：后者可能被优化成提前短路，把逐字节差异变成时间差异。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    subtle::ConstantTimeEq::ct_eq(left, right).into()
}

/// `req.headers.origin` 为真值时拒绝浏览器来源（空值等同缺省，与 JS 的 `|| ''` 一致）。
fn has_origin(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get_all(header::ORIGIN)
        .iter()
        .any(|value| !value.as_bytes().is_empty())
}

/// `readBody(req)`：8 MB 上限 → 413；非法 JSON → 400 `Invalid JSON`。
async fn read_body(body: Body) -> Result<Value, BackendError> {
    let mut stream = body.into_data_stream();
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| BackendError::plain(error.to_string()))?;
        buffer.extend_from_slice(&chunk);
        if buffer.len() > MAX_BODY_BYTES {
            return Err(BackendError::bridge(&BridgeError::with(
                "请求体超过 8 MB 上限",
                413,
                "invalid_request",
            )));
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    crate::json::parse_json(&text)
        .map_err(|_| BackendError::bridge(&BridgeError::new("Invalid JSON")))
}

/// `new URL(req.url, 'http://127.0.0.1').pathname` 的最小等价实现。
///
/// 规则：去掉 `?query` / `#fragment`；`//authority/path` 取 `path`；百分号转义按 UTF-8 解码。
fn normalize_path(raw: &str) -> String {
    let path = raw
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let path = match path.strip_prefix("//") {
        Some(rest) => match rest.find('/') {
            Some(index) => &rest[index..],
            None => "/",
        },
        None => path,
    };
    percent_decode(path)
}

/// 百分号解码（对应 WHATWG URL 的路径解码；非法序列按 UTF-8 替换字符处理）。
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &bytes[index + 1..index + 3];
            if hex.iter().all(u8::is_ascii_hexdigit) {
                let hex = std::str::from_utf8(hex).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn elapsed_ms(started: Instant) -> i64 {
    (started.elapsed().as_secs_f64() * 1000.0).round() as i64
}

/// 与 Node 的 `json(res, status, data)` 等价：`Content-Type: application/json` + `JSON.stringify`。
fn json_response(status: StatusCode, data: Value) -> Response {
    let mut response = Response::new(Body::from(js_stringify(&data)));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn error_response(error: &BackendError) -> Response {
    json_response(error.status_code(), json!({ "error": error.error_value() }))
}

/// 与 Node 的 `res.writeHead(200, {'Content-Type': 'text/event-stream', ...})` 等价。
fn sse_response(receiver: mpsc::Receiver<Result<Bytes, std::io::Error>>) -> Response {
    let mut response = Response::new(Body::from_stream(ReceiverStream::new(receiver)));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Method, Request as HttpRequest};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use tower::ServiceExt;

    const KEY: &str = "test-key";

    fn model() -> Value {
        json!({ "id": "free-1", "name": "Free One" })
    }

    fn chat_body(stream: bool) -> Value {
        json!({
            "model": "free-1",
            "stream": stream,
            "messages": [{ "role": "user", "content": "hi" }],
        })
    }

    fn request(method: Method, path: &str, key: Option<&str>, body: Option<Value>) -> Request {
        let mut builder = HttpRequest::builder().method(method).uri(path);
        if let Some(key) = key {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(js_stringify(&value)))
                .expect("请求可构造"),
            None => builder.body(Body::empty()).expect("请求可构造"),
        }
    }

    fn get(path: &str) -> Request {
        request(Method::GET, path, Some(KEY), None)
    }

    fn post(path: &str, body: Value) -> Request {
        request(Method::POST, path, Some(KEY), Some(body))
    }

    async fn body_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("响应体可读");
        serde_json::from_slice(&bytes).expect("响应体是合法 JSON")
    }

    async fn body_text(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("响应体可读");
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn recording_server() -> (Router, Arc<Mutex<Vec<ResultRecord>>>) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let server = Server::new(KEY)
            .backend(|_, _| Box::pin(async { Ok(json!({ "choices": [] })) }))
            .get_models(|| vec![model()])
            .status(|| json!({ "phase": "ready" }))
            .on_result(move |record| {
                let sink = sink.clone();
                Box::pin(async move { lock(&sink).push(record) })
            });
        (server.build().0, records)
    }

    // --- 路由表 / 协议辅助 ---

    #[test]
    fn action_table_is_post_admin_routes() {
        assert_eq!(ACTION_ROUTES.len(), 8);
        for (action, method, path) in ACTION_ROUTES {
            assert_eq!(method, "POST", "{action} 必须是 POST");
            assert!(path.starts_with("/admin/"), "{action} 必须挂在 /admin/ 下");
            assert_eq!(route_for(action), Some(path));
            assert_eq!(method_for(action), Some(method));
            assert!(action_matches(action, "POST", path));
            assert!(!action_matches(action, "GET", path));
            assert!(!action_matches(action, "POST", "/v1/models"));
        }
        assert_eq!(route_for("nonexistent-action"), None);
        assert_eq!(method_for("nonexistent-action"), None);
    }

    #[test]
    fn normalize_path_matches_whatwg_url() {
        assert_eq!(normalize_path("/v1/models"), "/v1/models");
        assert_eq!(normalize_path("/v1/models?x=1"), "/v1/models");
        assert_eq!(normalize_path("/v1/models#frag"), "/v1/models");
        assert_eq!(normalize_path("/v1/%6Dodels"), "/v1/models");
        assert_eq!(normalize_path("//v1/models"), "/models");
        assert_eq!(normalize_path("//127.0.0.1/v1/models"), "/v1/models");
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn authorization_matches_bearer_key() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(!authorized(&headers, KEY));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-key"),
        );
        assert!(authorized(&headers, KEY));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-key "),
        );
        assert!(!authorized(&headers, KEY));
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("test-key"));
        assert!(!authorized(&headers, KEY));
    }

    #[test]
    fn origin_header_is_rejected_when_non_empty() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(!has_origin(&headers));
        headers.insert(header::ORIGIN, HeaderValue::from_static(""));
        assert!(!has_origin(&headers));
        headers.insert(header::ORIGIN, HeaderValue::from_static("http://evil.test"));
        assert!(has_origin(&headers));
    }

    #[test]
    fn backend_error_shape_defaults_to_upstream_error() {
        let error = BackendError::plain("boom");
        assert_eq!(error.status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            error.error_value(),
            json!({ "message": "boom", "type": "upstream_error", "code": "upstream_error" })
        );

        let error = BackendError::timeout("The operation was aborted due to timeout");
        assert_eq!(error.message(), "Model request timed out");
        assert_eq!(
            error.error_value()["message"],
            Value::String("Model request timed out".to_string())
        );

        let error = BackendError::bridge(&BridgeError::with("nope", 429, "busy"));
        assert_eq!(error.status_code(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.error_value()["code"], Value::String("busy".to_string()));
    }

    #[tokio::test]
    async fn read_body_enforces_size_and_json_rules() {
        let oversized = vec![b'0'; MAX_BODY_BYTES + 1];
        let error = read_body(Body::from(oversized)).await.expect_err("应超限");
        assert_eq!(error.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.message(), "请求体超过 8 MB 上限");
        assert_eq!(error.code.as_deref(), Some("invalid_request"));

        let error = read_body(Body::from("not json")).await.expect_err("应非法");
        assert_eq!(error.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(error.message(), "Invalid JSON");

        // 边界：恰好 8 MB 的合法 JSON 必须通过。
        let padding = "x".repeat(MAX_BODY_BYTES - 10);
        let exact = format!("{{\"pad\":\"{padding}\"}}");
        assert_eq!(exact.len(), MAX_BODY_BYTES);
        let value = read_body(Body::from(exact)).await.expect("恰好 8 MB 应通过");
        assert_eq!(value.get("pad").and_then(Value::as_str).map(str::len), Some(padding.len()));
    }

    // --- 路由行为 ---

    #[tokio::test]
    async fn health_requires_bearer_and_returns_status() {
        let (router, _) = recording_server();

        let response = router
            .clone()
            .oneshot(request(Method::GET, "/health", None, None))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "Local proxy API key required", "type": "authentication_error" } })
        );

        let response = router
            .clone()
            .oneshot(request(Method::GET, "/health", Some("wrong"), None))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = router.oneshot(get("/health")).await.expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!({ "phase": "ready" }));
    }

    #[tokio::test]
    async fn browser_origin_is_forbidden() {
        let (router, _) = recording_server();
        let request = HttpRequest::builder()
            .method(Method::GET)
            .uri("/health")
            .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
            .header(header::ORIGIN, "http://localhost:3000")
            .body(Body::empty())
            .expect("请求可构造");

        let response = router.oneshot(request).await.expect("路由可用");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "Browser-origin requests are disabled" } })
        );
    }

    #[tokio::test]
    async fn models_list_uses_client_ids() {
        let (router, _) = recording_server();
        let response = router.oneshot(get("/v1/models")).await.expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            body_json(response).await,
            json!({
                "object": "list",
                "data": [{ "id": "OC · Free One", "object": "model", "owned_by": "opencode", "name": "OC · Free One" }],
            })
        );
    }

    #[tokio::test]
    async fn unknown_routes_and_method_mismatches_are_404() {
        let (router, _) = recording_server();
        for request in [
            get("/nope"),
            get("/admin/refresh"),
            post("/v1/models", json!({})),
            post("/v1/chat/completions/extra", json!({})),
        ] {
            let response = router.clone().oneshot(request).await.expect("路由可用");
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(
                body_json(response).await,
                json!({ "error": { "message": "Not found" } })
            );
        }
    }

    #[tokio::test]
    async fn admin_actions_dispatch_to_handlers() {
        let refreshed = Arc::new(AtomicBool::new(false));
        let imported = Arc::new(Mutex::new(None));
        let proxy = Arc::new(Mutex::new(None));
        let probed = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));

        let refreshed_sink = refreshed.clone();
        let imported_sink = imported.clone();
        let proxy_sink = proxy.clone();
        let probed_sink = probed.clone();
        let shutdown_sink = shutdown.clone();

        let server = Server::new(KEY)
            .refresh(move || {
                let sink = refreshed_sink.clone();
                Box::pin(async move {
                    sink.store(true, Ordering::SeqCst);
                    Ok(json!({ "count": 2 }))
                })
            })
            .import_models(move |file| {
                let sink = imported_sink.clone();
                Box::pin(async move {
                    *lock(&sink) = file;
                    Ok(json!({ "ok": true }))
                })
            })
            .set_system_proxy(move |enabled| {
                let sink = proxy_sink.clone();
                Box::pin(async move {
                    *lock(&sink) = Some(enabled.clone());
                    if !enabled.is_boolean() {
                        return Err(BackendError::plain("代理开关必须是布尔值"));
                    }
                    Ok(json!({ "useSystemProxy": enabled }))
                })
            })
            .probe(move |model| {
                *lock(&probed_sink) = model.clone();
                json!({ "started": true })
            })
            .on_shutdown(move || shutdown_sink.store(true, Ordering::SeqCst));
        let (router, _) = server.build();

        let response = router
            .clone()
            .oneshot(post("/admin/refresh", json!({})))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!({ "count": 2 }));
        assert!(refreshed.load(Ordering::SeqCst));

        let response = router
            .clone()
            .oneshot(post("/admin/import", json!({ "modelsFile": "/tmp/a.json" })))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            lock(&imported).clone(),
            Some(Value::String("/tmp/a.json".to_string()))
        );

        let response = router
            .clone()
            .oneshot(post("/admin/system-proxy", json!({ "enabled": true })))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(lock(&proxy).clone(), Some(Value::Bool(true)));

        let response = router
            .clone()
            .oneshot(post("/admin/system-proxy", json!({ "enabled": "yes" })))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(body_json(response).await["error"]["type"], "upstream_error");

        let response = router
            .clone()
            .oneshot(post("/admin/probe", json!({ "model": "free-1" })))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(body_json(response).await, json!({ "started": true }));
        assert_eq!(lock(&probed).clone(), Some(json!("free-1")));

        let response = router
            .clone()
            .oneshot(post("/admin/shutdown", json!({})))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!({ "ok": true }));
        for _ in 0..50 {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(shutdown.load(Ordering::SeqCst), "停机回调应在响应之后触发");

        // `/admin/refresh` 不读取请求体（与 Node 的 `refresh()` 一致）：空体也返回 200。
        let response = router
            .clone()
            .oneshot(request(Method::POST, "/admin/refresh", Some(KEY), None))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!({ "count": 2 }));

        // 其余动作路由要求 JSON 请求体（与 Node 一致）。
        let response = router
            .oneshot(request(Method::POST, "/admin/probe", Some(KEY), None))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "Invalid JSON", "type": "invalid_request", "code": "invalid_request" } })
        );
    }

    /// 平台凭据的三个动作在 HTTP 层的形态：`set-provider-key` / `clear-provider-key` 的处理器
    /// 收到**整份请求体**（`provider` 与 `apiKey` 由编排层一起校验），`provider-status` 不读请求体。
    ///
    /// 🔴 后两段断言是「凭据零回显」红线在路由层的守卫：无论成功还是失败响应，Key 都不得出现在
    /// 响应体里（连错误文案都不行 —— 报错回显入参是最常见的泄漏方式）。
    #[tokio::test]
    async fn provider_actions_take_whole_body_and_never_echo_the_key() {
        const SECRET: &str = "sk-provider-secret-do-not-echo";
        // 编排层的真实返回形态（`providers::status`）；定义成函数而非闭包，避免 handler 闭包借用栈。
        fn shape() -> Value {
            json!({ "providers": [{ "id": "modelscope", "label": "ModelScope", "configured": true }] })
        }
        let received = Arc::new(Mutex::new(Vec::new()));
        let status_sink = received.clone();
        let set_sink = received.clone();
        let clear_sink = received.clone();

        let server = Server::new(KEY)
            .provider_status(move |_| {
                let sink = status_sink.clone();
                Box::pin(async move {
                    lock(&sink).push(Value::Null);
                    Ok(shape())
                })
            })
            .set_provider_key(move |body| {
                let sink = set_sink.clone();
                Box::pin(async move {
                    let recorded = body.clone();
                    lock(&sink).push(recorded);
                    if body.get("provider").and_then(Value::as_str) != Some("modelscope") {
                        return Err(BackendError::with("未知平台", 400, "invalid_provider"));
                    }
                    Ok(shape())
                })
            })
            .clear_provider_key(move |body| {
                let sink = clear_sink.clone();
                Box::pin(async move {
                    lock(&sink).push(body);
                    Ok(shape())
                })
            });
        let (router, _) = server.build();

        // 只读动作与 refresh 同形：空请求体也能拿到状态。
        let response = router
            .clone()
            .oneshot(request(Method::POST, "/admin/provider-status", Some(KEY), None))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, shape());

        let response = router
            .clone()
            .oneshot(post(
                "/admin/set-provider-key",
                json!({ "provider": "modelscope", "apiKey": SECRET }),
            ))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        let text = body_json(response).await.to_string();
        assert!(!text.contains(SECRET), "成功响应不得含 Key");

        let response = router
            .clone()
            .oneshot(post(
                "/admin/set-provider-key",
                json!({ "provider": "not-registered", "apiKey": SECRET }),
            ))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let failure = body_json(response).await;
        assert_eq!(failure["error"]["type"], "invalid_provider");
        assert!(
            !failure.to_string().contains(SECRET),
            "失败响应不得回显 Key"
        );

        let response = router
            .oneshot(post(
                "/admin/clear-provider-key",
                json!({ "provider": "modelscope" }),
            ))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, shape());

        // 处理器实际收到的入参：status 是占位 Null，两个写动作拿到的是整份请求体。
        let recorded = lock(&received).clone();
        assert_eq!(recorded.len(), 4);
        assert_eq!(recorded[0], Value::Null);
        assert_eq!(
            recorded[1],
            json!({ "provider": "modelscope", "apiKey": SECRET }),
            "set-provider-key 应收到整份请求体"
        );
        assert_eq!(recorded[2], json!({ "provider": "not-registered", "apiKey": SECRET }));
        assert_eq!(recorded[3], json!({ "provider": "modelscope" }));
    }

    // --- /v1/chat/completions ---
    #[tokio::test]
    async fn completion_json_rewrites_model_and_records_success() {
        let (router, records) = recording_server();
        let response = router
            .oneshot(post("/v1/chat/completions", chat_body(false)))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        let value = body_json(response).await;
        assert_eq!(value["model"], json!("free-1"));

        let records = lock(&records);
        assert_eq!(records.len(), 1);
        assert!(records[0].ok);
        assert_eq!(records[0].source, "request");
        assert_eq!(records[0].message, "");
        assert_eq!(records[0].model, Some(json!("free-1")));
        assert_eq!(records[0].meta["model"], json!("free-1"));
        assert_eq!(records[0].meta["tools"], json!(0));
    }

    #[tokio::test]
    async fn completion_failure_maps_status_and_records_once() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let server = Server::new(KEY)
            .backend(|_, _| {
                Box::pin(async {
                    Err(BackendError::bridge(&BridgeError::with(
                        "bad envelope",
                        502,
                        "invalid_model_output",
                    )))
                })
            })
            .get_models(|| vec![model()])
            .on_result(move |record| {
                let sink = sink.clone();
                Box::pin(async move { lock(&sink).push(record) })
            });
        let (router, _) = server.build();

        let response = router
            .clone()
            .oneshot(post("/v1/chat/completions", chat_body(false)))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "bad envelope", "type": "invalid_model_output", "code": "invalid_model_output" } })
        );
        {
            let records = lock(&records);
            assert_eq!(records.len(), 1);
            assert!(!records[0].ok);
            assert_eq!(records[0].code.as_deref(), Some("invalid_model_output"));
        }

        // prepare 失败（未知模型）在 Node 侧发生在 attempted 之前：不写结果回调。
        let response = router
            .oneshot(post(
                "/v1/chat/completions",
                json!({ "model": "nope", "messages": [{ "role": "user", "content": "hi" }] }),
            ))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "Select an available free model from /v1/models", "type": "model_not_found", "code": "model_not_found" } })
        );
    }

    #[tokio::test]
    async fn timeout_errors_are_rewritten() {
        let server = Server::new(KEY)
            .backend(|_, _| Box::pin(async { Err(BackendError::timeout("opaque abort")) }))
            .get_models(|| vec![model()]);
        let (router, _) = server.build();

        let response = router
            .oneshot(post("/v1/chat/completions", chat_body(false)))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "Model request timed out", "type": "upstream_error", "code": "upstream_error" } })
        );
    }

    #[tokio::test]
    async fn oversized_body_is_rejected_before_backend() {
        let (router, records) = recording_server();
        let padding = "x".repeat(MAX_BODY_BYTES);
        let response = router
            .oneshot(post(
                "/v1/chat/completions",
                json!({ "model": "free-1", "messages": [{ "role": "user", "content": padding }] }),
            ))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "请求体超过 8 MB 上限", "type": "invalid_request", "code": "invalid_request" } })
        );
        assert!(lock(&records).is_empty());
    }

    #[tokio::test]
    async fn cancelled_request_is_not_recorded() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let server = Server::new(KEY)
            .backend(|_, context| {
                Box::pin(async move {
                    context.signal.cancelled().await;
                    Ok(json!({ "choices": [] }))
                })
            })
            .get_models(|| vec![model()])
            .on_result(move |record| {
                let sink = sink.clone();
                Box::pin(async move { lock(&sink).push(record) })
            });
        let (router, control) = server.build();

        let pending = {
            let router = router.clone();
            tokio::spawn(async move {
                router
                    .oneshot(post("/v1/chat/completions", chat_body(false)))
                    .await
            })
        };
        for _ in 0..50 {
            if control.active() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(control.active(), 1);
        control.abort_all();

        let response = pending.await.expect("任务可等待").expect("路由可用");
        assert!(body_text(response).await.is_empty());
        assert!(lock(&records).is_empty(), "取消的请求不得写入结果");
        assert_eq!(control.active(), 0, "在途计数应随请求结束释放");
    }

    #[tokio::test]
    async fn concurrency_is_capped_at_four() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let started = Arc::new(AtomicU64::new(0));
        let gate_sink = gate.clone();
        let started_sink = started.clone();

        let server = Server::new(KEY)
            .backend(move |_, _| {
                let gate = gate_sink.clone();
                let started = started_sink.clone();
                Box::pin(async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    gate.notified().await;
                    Ok(json!({ "choices": [] }))
                })
            })
            .get_models(|| vec![model()]);
        let (router, _) = server.build();

        let mut pending = Vec::new();
        for _ in 0..MAX_CONCURRENT_REQUESTS {
            let router = router.clone();
            pending.push(tokio::spawn(async move {
                router
                    .oneshot(post("/v1/chat/completions", chat_body(false)))
                    .await
            }));
        }
        for _ in 0..200 {
            if started.load(Ordering::SeqCst) == MAX_CONCURRENT_REQUESTS as u64 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(started.load(Ordering::SeqCst), MAX_CONCURRENT_REQUESTS as u64);

        let response = router
            .clone()
            .oneshot(post("/v1/chat/completions", chat_body(false)))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            body_json(response).await,
            json!({ "error": { "message": "At most four requests may run at once", "type": "busy", "code": "busy" } })
        );

        gate.notify_waiters();
        for task in pending {
            let response = task.await.expect("任务可等待").expect("路由可用");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    // --- SSE ---

    #[tokio::test]
    async fn stream_emits_handshake_heartbeat_frames_and_done() {
        let activities = Arc::new(Mutex::new(Vec::new()));
        let activity_sink = activities.clone();
        let server = Server::new(KEY)
            .backend(|_, context| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(90)).await;
                    context.activity.report(json!({ "text": "hel" }));
                    context.activity.report(json!({ "content": true }));
                    Ok(json!({
                        "id": "chatcmpl-1",
                        "object": "chat.completion",
                        "created": 1,
                        "model": "free-1",
                        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
                        "usage": { "prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3 },
                    }))
                })
            })
            .get_models(|| vec![model()])
            .on_activity(move |progress| lock(&activity_sink).push(progress))
            .heartbeat(Duration::from_millis(20));
        let (router, _) = server.build();

        let response = router
            .oneshot(post(
                "/v1/chat/completions",
                json!({
                    "model": "free-1",
                    "stream": true,
                    "stream_options": { "include_usage": true },
                    "messages": [{ "role": "user", "content": "hi" }],
                }),
            ))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).map(|v| v.as_bytes()),
            Some("text/event-stream".as_bytes())
        );
        let text = body_text(response).await;

        let frames: Vec<&str> = text.split("\n\n").collect();
        assert_eq!(frames[0], VALIDATING_FRAME.trim_end_matches("\n\n"), "首帧必须是校验注释");
        assert!(
            text.matches(HEARTBEAT_FRAME).count() >= 2,
            "等待期间应至少两次心跳：{text}"
        );
        let start = frames
            .iter()
            .find(|frame| frame.contains("\"delta\":{\"role\":\"assistant\"}"))
            .expect("应有 streamStart 帧");
        assert!(start.contains("\"object\":\"chat.completion.chunk\""));
        assert!(start.contains("\"model\":\"free-1\""));
        assert!(
            text.contains("\"content\":\"hello\""),
            "内容帧应存在：{text}"
        );
        assert!(text.contains("\"finish_reason\":\"stop\""));
        assert!(text.contains("\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}"));
        assert!(text.trim_end().ends_with("data: [DONE]"));
        // streamStart 已写 role 帧，正式帧里不应再重复 role。
        assert_eq!(text.matches("\"role\":\"assistant\"").count(), 1);

        let activities = lock(&activities);
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0]["text"], json!("hel"));
    }

    #[tokio::test]
    async fn stream_without_activity_keeps_role_frame() {
        let server = Server::new(KEY)
            .backend(|_, _| {
                Box::pin(async {
                    Ok(json!({
                        "id": "chatcmpl-2",
                        "object": "chat.completion",
                        "created": 2,
                        "model": "free-1",
                        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
                    }))
                })
            })
            .get_models(|| vec![model()]);
        let (router, _) = server.build();

        let response = router
            .oneshot(post(
                "/v1/chat/completions",
                json!({
                    "model": "free-1",
                    "stream": true,
                    "messages": [{ "role": "user", "content": "hi" }],
                }),
            ))
            .await
            .expect("路由可用");
        let text = body_text(response).await;
        assert_eq!(
            text.matches("\"role\":\"assistant\"").count(),
            1,
            "未收到进度时 role 帧由 sendSSE 补发：{text}"
        );
        assert!(!text.contains("\"usage\""));
        assert!(text.trim_end().ends_with("data: [DONE]"));
    }

    #[tokio::test]
    async fn stream_failure_after_headers_becomes_error_frame() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let server = Server::new(KEY)
            .backend(|_, _| {
                Box::pin(async {
                    Err(BackendError::bridge(&BridgeError::with(
                        "upstream exploded",
                        502,
                        "invalid_model_output",
                    )))
                })
            })
            .get_models(|| vec![model()])
            .on_result(move |record| {
                let sink = sink.clone();
                Box::pin(async move { lock(&sink).push(record) })
            });
        let (router, _) = server.build();

        let response = router
            .oneshot(post("/v1/chat/completions", chat_body(true)))
            .await
            .expect("路由可用");
        assert_eq!(response.status(), StatusCode::OK);
        let text = body_text(response).await;
        assert_eq!(
            text,
            format!(
                "{VALIDATING_FRAME}data: {}\n\n",
                js_stringify(&json!({ "error": { "message": "upstream exploded", "type": "invalid_model_output", "code": "invalid_model_output" } }))
            )
        );
        let records = lock(&records);
        assert_eq!(records.len(), 1);
        assert!(!records[0].ok);
        assert_eq!(records[0].code.as_deref(), Some("invalid_model_output"));
    }

    #[tokio::test]
    async fn stream_failure_before_validation_has_no_role_frame() {
        let server = Server::new(KEY)
            .backend(|_, _| Box::pin(async { Err(BackendError::plain("boom")) }))
            .get_models(|| vec![model()]);
        let (router, _) = server.build();

        let response = router
            .oneshot(post("/v1/chat/completions", chat_body(true)))
            .await
            .expect("路由可用");
        let text = body_text(response).await;
        assert!(!text.contains("role"));
        assert!(text.contains("\"message\":\"boom\""));
    }

    // --- 端到端（真实 TCP，验证 header 与 streaming 真能上线） ---

    #[tokio::test]
    async fn serves_over_tcp_with_bearer_auth() {
        let server = Server::new(KEY)
            .get_models(|| vec![model()])
            .status(|| json!({ "phase": "ready" }));
        let (router, _) = server.build();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("端口可绑定");
        let address = listener.local_addr().expect("地址可读");
        tokio::spawn(async move {
            let _ = serve(listener, router, std::future::pending::<()>()).await;
        });

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("可连接");
        stream
            .write_all(
                format!(
                    "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .expect("可写入");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("可读取");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(
            response.to_lowercase().contains("content-type: application/json"),
            "{response}"
        );
        assert!(response.contains("{\"phase\":\"ready\"}"), "{response}");
    }
}
