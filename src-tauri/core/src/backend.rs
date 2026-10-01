//! `core/src/backend.js` 的 Rust 等价实现 —— 方案B 阶段三「OpenCode 客户端」。
//!
//! 与 Node 版的对应关系：
//! - `Backend.request` → [`Backend::request`]（reqwest 异步请求 + `AbortSignal` 竞争取消）；
//! - `Backend.watchEvents/streamEvents/handleEvent` → 单条常驻 `/event` SSE 连接，
//!   断线 1s 后重连，事件解析逻辑逐分支对齐；
//! - `Backend.complete` 的 `Promise.race([watch, request])` → [`Sticky`]：watch promise 一旦
//!   落定（handoff 或 chat-only 拒绝），后续每一轮 race 都必须像 JS 一样立刻取到同一结果；
//! - 信封解码失败后的「一次纠正 → 翻译器转写 → 再纠正」三步链路与 handoff 兜底完全保留。
//!
//! 与 Node 版的**有意偏差**：
//! 1. JS 把 `meta.activity` 作为回调挂在 meta 对象上；Rust 侧由 [`RequestContext`] 携带，
//!    `active` 表只登记「JS 里 meta.activity 是函数」的请求（见 [`Activity::is_noop`]）。
//! 2. `pendingPermissions` 失败日志里的 `error.name` 用 `code`／`TimeoutError`／`Error` 近似，
//!    仅影响诊断文案。
//! 3. `freeModels` 的排序用字节序比较替代 `localeCompare`（模型 id 为 ASCII slug，结果一致）。

use crate::handoff::{build_handoff, handoff_input, reject_feedback, validate_action};
use crate::json::{display, js_stringify, js_trim, truthy};
use crate::protocol::{completion, decode, random_uuid, BridgeError, PreparedRequest};
use crate::repair::{raw_material, repair, resend_prompt, RepairDeps};
use crate::server::{
    AbortController, AbortSignal, Activity, BackendError, BoxFuture, RequestContext, SharedMeta,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::StreamExt;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;

/// `encodeURIComponent` 的路径参数等价物（保留 JS 的 unreserved 集）。
const URI_ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

fn encode_uri_component(value: &str) -> String {
    utf8_percent_encode(value, URI_ENCODE).to_string()
}

/// `nativePermissions`：审批闸门必须保持官方语义（红线，不得改动）。
pub fn native_permissions() -> Value {
    let mut map = Map::new();
    for (permission, action) in [
        ("*", "ask"),
        ("question", "deny"),
        ("websearch", "deny"),
        ("codesearch", "deny"),
        ("webfetch", "deny"),
        ("task", "deny"),
        ("plan_enter", "deny"),
        ("plan_exit", "deny"),
        ("todowrite", "deny"),
    ] {
        map.insert(permission.to_string(), json!(action));
    }
    Value::Object(map)
}

/// `POST /session` 里 `permission` 字段的数组形态（entries 顺序与 JS 一致）。
pub fn permission_list() -> Value {
    Value::Array(
        native_permissions()
            .as_object()
            .expect("native_permissions 必然是对象")
            .iter()
            .map(|(permission, action)| {
                json!({ "permission": permission, "pattern": "*", "action": action })
            })
            .collect(),
    )
}

/// `freeModels(providers)`：输入/输出/缓存全免费、支持文本输出、未下线的模型。
pub fn free_models(providers: &Value) -> Result<Vec<Value>, BridgeError> {
    let provider = providers
        .get("all")
        .and_then(Value::as_array)
        .and_then(|all| {
            all.iter()
                .find(|item| item.get("id") == Some(&json!("opencode")))
        });
    let Some(provider) = provider else {
        return Err(BridgeError::new("OpenCode provider missing"));
    };
    let mut models: Vec<Value> = Vec::new();
    if let Some(entries) = provider.get("models").and_then(Value::as_object) {
        for (key, model) in entries {
            let cost = model.get("cost");
            let free = truthy(cost)
                && cost.and_then(|c| c.get("input")) == Some(&json!(0))
                && cost.and_then(|c| c.get("output")) == Some(&json!(0))
                && cache_cost(cost, "read") == Some(json!(0))
                && cache_cost(cost, "write") == Some(json!(0));
            if !free {
                continue;
            }
            if model
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("output"))
                .and_then(|output| output.get("text"))
                == Some(&json!(false))
            {
                continue;
            }
            if model.get("status") == Some(&json!("deprecated")) {
                continue;
            }
            let name = match model.get("name") {
                Some(value) if truthy(Some(value)) => value.clone(),
                _ => json!(key),
            };
            let mut entry = Map::new();
            entry.insert("id".to_string(), json!(format!("opencode/{key}")));
            entry.insert("name".to_string(), name);
            insert_defined(&mut entry, "context", nested(model, &["limit", "context"]));
            insert_defined(&mut entry, "input", nested(model, &["limit", "input"]));
            entry.insert(
                "images".to_string(),
                json!(nested(model, &["capabilities", "input", "image"]) == Some(&json!(true))),
            );
            insert_defined(&mut entry, "output", nested(model, &["limit", "output"]));
            entry.insert(
                "toolcall".to_string(),
                json!(model.get("capabilities").and_then(|c| c.get("toolcall")) == Some(&json!(true))),
            );
            entry.insert(
                "reasoning".to_string(),
                json!(model.get("capabilities").and_then(|c| c.get("reasoning")) == Some(&json!(true))),
            );
            entry.insert(
                "variants".to_string(),
                match model.get("variants") {
                    Some(value) if !value.is_null() => value.clone(),
                    _ => json!({}),
                },
            );
            models.push(Value::Object(entry));
        }
    }
    models.sort_by(|a, b| {
        let left = a.get("id").and_then(Value::as_str).unwrap_or_default();
        let right = b.get("id").and_then(Value::as_str).unwrap_or_default();
        left.cmp(right)
    });
    Ok(models)
}

fn cache_cost(cost: Option<&Value>, key: &str) -> Option<Value> {
    let read = cost
        .and_then(|cost| cost.get("cache"))
        .and_then(|cache| cache.get(key))
        .cloned();
    // JS 的 `c.cache?.read ?? 0`：nullish 时按 0 处理。
    Some(read.filter(|value| !value.is_null()).unwrap_or(json!(0)))
}

fn insert_defined(entry: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    // JS 里 `context: m.limit?.context` 缺失时是 `undefined`，序列化即丢键。
    if let Some(value) = value.filter(|value| !value.is_null()) {
        entry.insert(key.to_string(), value.clone());
    }
}

fn nested<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

/// `shrinkPermission(value, limit)`：审批载荷可能带整份文件内容，保留结构、截断长字符串。
pub fn shrink_permission(value: &Value, limit: usize) -> Value {
    match value {
        Value::String(text) => {
            // JS 的 `slice(0, limit)` 按 UTF-16 计数；字符边界处近似为 chars 计数。
            let count = text.chars().count();
            if count > limit {
                let head: String = text.chars().take(limit).collect();
                json!(format!("{head}…[{count} chars]"))
            } else {
                value.clone()
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| shrink_permission(item, limit)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), shrink_permission(item, limit)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `allowedTools(request)`：`tool_choice: none` 清空；强制工具时只留该工具。
fn allowed_tools(request: &PreparedRequest) -> Vec<Value> {
    if *request.choice() == json!("none") {
        return Vec::new();
    }
    let forced = request.forced().and_then(Value::as_str);
    request
        .tools()
        .iter()
        .filter(|tool| match forced {
            Some(name) => tool
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                == Some(name),
            None => true,
        })
        .cloned()
        .collect()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn take_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

const FIVE_SECONDS: Duration = Duration::from_secs(5);
const TRANSLATE_DEADLINE: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct ActiveMeta {
    meta: SharedMeta,
    activity: Activity,
}

#[derive(Default)]
struct BackendState {
    active: HashMap<String, ActiveMeta>,
    /// 事件流里先于审批到达的 tool part（callID → {tool, input}），FIFO 限 50。
    tool_parts: VecDeque<(String, Value)>,
    pending_approvals: HashMap<String, Value>,
    /// JS 的 `set(id, undefined)` 也占位（`has()` 为真），故值用 Option。
    usage_by_session: HashMap<String, Option<Value>>,
}

/// `core/src/backend.js` 的 `Backend` 类。
#[derive(Clone)]
pub struct Backend {
    inner: Arc<Inner>,
}

/// 辅助模型选择钩子：`(失败模型 id, 形态) → 可选的转写模型 id`。
type TranslatorHook = Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

/// `repair` 需要的「再跑一轮补全」回调。
type CompleteFn = Box<dyn FnMut(&Value) -> BoxFuture<Result<Value, BridgeError>> + Send>;

/// `repair` 需要的「校验候选信封」回调。
type ValidateFn = Box<dyn FnMut(&Value) -> Result<Value, BridgeError> + Send>;

struct Inner {
    client: reqwest::Client,
    base: String,
    password: String,
    log: Arc<dyn Fn(&str) + Send + Sync>,
    state: Mutex<BackendState>,
    events: Mutex<Option<AbortController>>,
    translator: Mutex<Option<TranslatorHook>>,
}

/// watch promise 一旦落定就永远返回同一结果（对应 JS 里可被反复 `Promise.race` 的 promise）。
struct Sticky {
    handle: JoinHandle<Result<Value, BackendError>>,
    cached: Option<Result<Value, BackendError>>,
}

impl Future for Sticky {
    type Output = Result<Value, BackendError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(cached) = &self.cached {
            return Poll::Ready(cached.clone());
        }
        match Pin::new(&mut self.handle).poll(cx) {
            Poll::Ready(Ok(value)) => {
                self.cached = Some(value.clone());
                Poll::Ready(value)
            }
            Poll::Ready(Err(_)) => {
                let error = Err(BackendError::plain("The operation was aborted"));
                self.cached = Some(error.clone());
                Poll::Ready(error)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Backend {
    /// `new Backend(base, password, timeout, log)`：Node 版 timeout 传 `undefined`，
    /// 这里默认不设每请求超时（`request` 显式传 5s 的调用点除外）。
    pub fn new(
        base: impl Into<String>,
        password: impl Into<String>,
        log: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                client: reqwest::Client::new(),
                base: base.into(),
                password: password.into(),
                log: Arc::new(log),
                state: Mutex::new(BackendState::default()),
                events: Mutex::new(None),
                translator: Mutex::new(None),
            }),
        }
    }

    /// 注入翻译器选择函数（对应 JS 的 `backend.translator = (failed, shape) => …`）。
    ///
    /// 形参是「刚失败的模型 id」与「修复形状」：JS 侧选择翻译器时先排除失败模型本身，
    /// 否则一个格式不合格的模型会给自己转写，修复结果仍来自同一缺陷。
    pub fn set_translator(
        &self,
        translator: impl Fn(&str, &str) -> Option<String> + Send + Sync + 'static,
    ) {
        *lock(&self.inner.translator) = Some(Arc::new(translator));
    }

    fn log(&self, message: &str) {
        (self.inner.log)(message);
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        let credentials = BASE64.encode(format!("opencode:{}", self.inner.password));
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::try_from(format!("Basic {credentials}")).unwrap_or_else(
                |_| reqwest::header::HeaderValue::from_static("Basic "),
            ),
        );
        headers
    }

    /// `request(route, method, body, signal, timeout)`。
    ///
    /// 对 runtime.js 公开：启动健康轮询直接复用带鉴权的这一条通道。
    pub async fn request(
        &self,
        route: &str,
        method: &str,
        body: Option<&Value>,
        signal: Option<&AbortSignal>,
        timeout: Option<Duration>,
    ) -> Result<Value, BackendError> {
        let url = format!("{}{}", self.inner.base, route);
        let parsed_method = method
            .parse::<reqwest::Method>()
            .unwrap_or(reqwest::Method::GET);
        let mut call = self
            .inner
            .client
            .request(parsed_method, &url)
            .headers(self.headers());
        if let Some(duration) = timeout {
            call = call.timeout(duration);
        }
        if let Some(payload) = body {
            call = call.json(payload);
        }
        let response = match signal {
            Some(signal) => tokio::select! {
                result = call.send() => result,
                _ = signal.cancelled() => return Err(BackendError::plain("The operation was aborted")),
            },
            None => call.send().await,
        };
        let response = response.map_err(|error| {
            // JS 把底层网络错误原样上抛（无 status/code）→ 502 upstream_error。
            BackendError::plain(error.to_string())
        })?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|error| BackendError::plain(error.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(BackendError::with(
                format!(
                    "OpenCode HTTP {status}: {}",
                    take_chars(&text, 600)
                ),
                if status >= 500 { 502 } else { status },
                "upstream_error",
            ));
        }
        serde_json::from_str::<Value>(&text).map_err(|_| {
            BackendError::with("OpenCode returned non-JSON response", 502, "upstream_error")
        })
    }

    /// `watchEvents()`：全局唯一的事件流连接，断开 1s 后重连。
    pub fn watch_events(&self) {
        let mut slot = lock(&self.inner.events);
        if slot.is_some() {
            return;
        }
        let (controller, signal) = AbortSignal::channel();
        let backend = self.clone();
        tokio::spawn(async move {
            while !signal.is_aborted() {
                if let Err(error) = backend.stream_events(&signal).await {
                    if signal.is_aborted() {
                        return;
                    }
                    backend.log(&format!("Event stream error: {}", error.message()));
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = signal.cancelled() => {}
                }
            }
        });
        *slot = Some(controller);
    }

    /// `stopEvents()`。
    pub fn stop_events(&self) {
        if let Some(controller) = lock(&self.inner.events).take() {
            controller.abort();
        }
    }

    /// `streamEvents(signal)`：SSE 帧按 `\n` 增量切分，只消费 `data:` 行。
    async fn stream_events(&self, signal: &AbortSignal) -> Result<(), BackendError> {
        let call = self
            .inner
            .client
            .get(format!("{}{}", self.inner.base, "/event"))
            .headers(self.headers())
            .header(reqwest::header::ACCEPT, "text/event-stream");
        let response = tokio::select! {
            result = call.send() => result.map_err(|error| BackendError::plain(error.to_string()))?,
            _ = signal.cancelled() => return Ok(()),
        };
        if response.status().as_u16() != 200 {
            let status = response.status().as_u16();
            // JS 的 response.resume()：读完即弃。
            let _ = response.text().await;
            return Err(BackendError::with(
                format!("Event stream HTTP {status}"),
                502,
                "event_stream_error",
            ));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        loop {
            let chunk = tokio::select! {
                next = stream.next() => next,
                _ = signal.cancelled() => return Ok(()),
            };
            let Some(chunk) = chunk else {
                return Ok(()); // 上游结束，交给 watchEvents 重连。
            };
            let chunk = chunk.map_err(|error| BackendError::plain(error.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(index) = buffer.find('\n') {
                let line = buffer[..index].to_string();
                buffer.replace_range(..=index, "");
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                if let Ok(wrapper) = serde_json::from_str::<Value>(js_trim(data)) {
                    self.handle_event(&wrapper);
                }
            }
        }
    }

    /// `handleEvent(wrapper)`：维护审批/工具 part/用量缓存，并把进度转发给在途请求。
    fn handle_event(&self, wrapper: &Value) {
        let event = wrapper
            .get("payload")
            .filter(|payload| !payload.is_null())
            .unwrap_or(wrapper);
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        let properties = event.get("properties");

        let mut meta_for_activity: Option<ActiveMeta> = None;
        let mut stage: Option<Value> = None;
        let mut progress = Map::new();
        {
            let mut state = lock(&self.inner.state);
            if matches!(event_type, "permission.asked" | "permission.updated") {
                if let (Some(id), Some(properties)) = (
                    properties.and_then(|p| p.get("id")).and_then(Value::as_str),
                    properties,
                ) {
                    state.pending_approvals.insert(id.to_string(), properties.clone());
                }
            }
            if event_type == "permission.replied" {
                if let Some(request_id) =
                    properties.and_then(|p| p.get("requestID")).and_then(Value::as_str)
                {
                    state.pending_approvals.remove(request_id);
                }
            }
            let part = if event_type == "message.part.updated" {
                properties.and_then(|p| p.get("part"))
            } else {
                None
            };
            if let (Some(part), Some(call_id)) = (
                part,
                part.and_then(|part| part.get("callID")).and_then(Value::as_str),
            ) {
                if part.get("type") == Some(&json!("tool")) {
                    let entry = json!({
                        "tool": part.get("tool").cloned().unwrap_or(Value::Null),
                        "input": part
                            .get("state")
                            .and_then(|state| state.get("input"))
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    });
                    // JS Map 的重复 set 保持原插入位次，此处一致。
                    if let Some(existing) =
                        state.tool_parts.iter_mut().find(|(id, _)| id == call_id)
                    {
                        existing.1 = entry;
                    } else {
                        state.tool_parts.push_back((call_id.to_string(), entry));
                    }
                    while state.tool_parts.len() > 50 {
                        state.tool_parts.pop_front();
                    }
                }
            }
            if event_type == "message.updated" {
                let info = properties.and_then(|p| p.get("info"));
                let session_id = info.and_then(|info| info.get("sessionID")).and_then(Value::as_str);
                let tokens = info.and_then(|info| info.get("tokens"));
                if info.and_then(|info| info.get("role")) == Some(&json!("assistant"))
                    && truthy(tokens)
                {
                    if let (Some(session_id), Some(tokens)) = (session_id, tokens) {
                        if state.usage_by_session.contains_key(session_id) {
                            state
                                .usage_by_session
                                .insert(session_id.to_string(), Some(tokens.clone()));
                        }
                    }
                }
            }
            let session_id = properties.and_then(|p| p.get("sessionID"));
            if let Some(session_id) = session_id.and_then(Value::as_str) {
                meta_for_activity = state.active.get(session_id).cloned();
            }
            let Some(active) = &meta_for_activity else {
                return;
            };
            progress.insert("sessionID".to_string(), session_id_value(properties));
            progress.insert("model".to_string(), active.meta.get("model").unwrap_or(Value::Null));
            progress.insert("type".to_string(), json!(event_type));
            progress.insert("at".to_string(), json!(now_millis()));

            if event_type == "session.status" {
                let status = properties.and_then(|p| p.get("status"));
                if truthy(status) {
                    let status_type = status.and_then(|status| status.get("type"));
                    let stage = active.meta.get("stage");
                    let waiting_stage = stage.is_none()
                        || stage.as_ref().and_then(Value::as_str) == Some("retry");
                    if status_type == Some(&json!("retry"))
                        || (status_type == Some(&json!("busy")) && waiting_stage)
                    {
                        progress.insert(
                            "status".to_string(),
                            json!(if status_type == Some(&json!("busy")) {
                                "waiting"
                            } else {
                                "retry"
                            }),
                        );
                    }
                    if status_type == Some(&json!("retry")) {
                        for key in ["attempt", "message", "next"] {
                            if let Some(value) = status.and_then(|status| status.get(key)) {
                                progress.insert(key.to_string(), value.clone());
                            }
                        }
                    }
                }
            }
            if event_type == "session.error" {
                let error = properties.and_then(|p| p.get("error"));
                let message = error
                    .and_then(|error| error.get("data"))
                    .and_then(|data| data.get("message"))
                    .filter(|value| truthy(Some(*value)))
                    .cloned()
                    .or_else(|| error.and_then(|error| error.get("name")).cloned())
                    .unwrap_or_else(|| json!("上游错误"));
                progress.insert("error".to_string(), message);
            }
            if let Some(part) = part {
                let part_type = part.get("type").and_then(Value::as_str);
                if matches!(part_type, Some("text") | Some("reasoning"))
                    && truthy(part.get("text"))
                {
                    progress.insert("content".to_string(), json!(true));
                    progress.insert(
                        "status".to_string(),
                        json!(if part_type == Some("reasoning") {
                            "reasoning"
                        } else {
                            "receiving"
                        }),
                    );
                }
            }
            if event_type == "message.part.delta"
                && truthy(properties.and_then(|p| p.get("delta")))
            {
                progress.insert("content".to_string(), json!(true));
                progress.insert("status".to_string(), json!("receiving"));
            }
            if matches!(event_type, "permission.asked" | "permission.updated") {
                progress.insert("status".to_string(), json!("permission"));
            }
            if let Some(value) = progress.get("status") {
                if truthy(Some(value)) {
                    stage = Some(value.clone());
                }
            }
        }

        if let (Some(active), Some(stage)) = (&meta_for_activity, stage) {
            active.meta.set("stage", stage);
        }
        if let Some(active) = &meta_for_activity {
            active.activity.report(Value::Object(progress));
        }
    }

    fn session_value(meta: &SharedMeta, key: &str) -> Value {
        meta.get(key).unwrap_or(Value::Null)
    }

    /// `progress(meta, status, extra)`：桥接阶段进度（写入 meta.stage 并转发）。
    fn report_phase(meta: &SharedMeta, activity: &Activity, status: &str, extra: Option<Value>) {
        meta.set("stage", json!(status));
        let mut progress = json!({
            "sessionID": Self::session_value(meta, "sessionID"),
            "model": Self::session_value(meta, "model"),
            "type": "bridge.phase",
            "status": status,
        });
        if let (Some(extra), Some(target)) = (extra, progress.as_object_mut()) {
            if let Some(extra) = extra.as_object() {
                for (key, value) in extra {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        activity.report(progress);
    }

    /// `reject(permission, message, signal)`：拒绝一条审批（回复失败时由调用方吞掉）。
    async fn reject(
        &self,
        permission: &Value,
        message: &str,
        signal: &AbortSignal,
    ) -> Result<Value, BackendError> {
        let id = permission.get("id").and_then(Value::as_str).unwrap_or_default();
        let result = self
            .request(
                &format!("/permission/{}/reply", encode_uri_component(id)),
                "POST",
                Some(&json!({ "reply": "reject", "message": message })),
                Some(signal),
                Some(FIVE_SECONDS),
            )
            .await?;
        lock(&self.inner.state)
            .pending_approvals
            .remove(id);
        Ok(result)
    }

    /// `blockedAction(callID, signal)`：从事件缓存取回被拦调用的名字与参数（4×120ms）。
    async fn blocked_action(&self, call_id: &str, signal: &AbortSignal) -> Value {
        for _attempt in 0..4 {
            let part = lock(&self.inner.state)
                .tool_parts
                .iter()
                .find(|(id, _)| id == call_id)
                .map(|(_, part)| part.clone());
            if let Some(part) = part {
                if truthy(part.get("tool")) {
                    return part;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(120)) => {}
                _ = signal.cancelled() => {}
            }
        }
        json!({ "failure": "no event carried this call ID" })
    }

    /// `pendingPermissions(sessionID, signal)`：查询失败绝不等于放行。
    async fn pending_permissions(&self, session_id: &str, signal: &AbortSignal) -> Vec<Value> {
        let cached = |state: &BackendState| -> Vec<Value> {
            state
                .pending_approvals
                .values()
                .filter(|permission| permission.get("sessionID") == Some(&json!(session_id)))
                .cloned()
                .collect()
        };
        let result = self
            .request("/permission", "GET", None, Some(signal), Some(FIVE_SECONDS))
            .await;
        let pending = match result {
            Ok(value) => value,
            Err(error) => {
                if !signal.is_aborted() {
                    let label = error
                        .code
                        .clone()
                        .unwrap_or_else(|| if error.timed_out { "TimeoutError".to_string() } else { "Error".to_string() });
                    let status = error
                        .status
                        .map(|status| format!(" (HTTP {status})"))
                        .unwrap_or_default();
                    self.log(&format!(
                        "Permission monitor query failed: {label}{status}: {}",
                        error.message
                    ));
                }
                return cached(&lock(&self.inner.state));
            }
        };
        match pending {
            Value::Array(items) => items
                .into_iter()
                .filter(|permission| permission.get("sessionID") == Some(&json!(session_id)))
                .collect(),
            _ => {
                self.log("Permission monitor query failed: non-array response");
                cached(&lock(&self.inner.state))
            }
        }
    }

    /// `handoffUsage(sessionID, signal)`：中断前先读完最后一条 assistant 消息的 token 用量。
    async fn handoff_usage(&self, session_id: &str, signal: &AbortSignal) -> Option<Value> {
        let lookup = self
            .request(
                &format!(
                    "/session/{}/message?limit=1",
                    encode_uri_component(session_id)
                ),
                "GET",
                None,
                Some(signal),
                Some(FIVE_SECONDS),
            )
            .await;
        match lookup {
            Ok(Value::Array(messages)) => {
                let info = messages
                    .iter()
                    .rev()
                    .find(|message| {
                        message
                            .get("info")
                            .and_then(|info| info.get("role"))
                            == Some(&json!("assistant"))
                    })
                    .and_then(|message| message.get("info"));
                let tokens = info.and_then(|info| info.get("tokens"));
                if truthy(tokens) {
                    return tokens.cloned();
                }
            }
            Ok(_) => {}
            Err(_) => {
                if !signal.is_aborted() {
                    self.log("Usage lookup failed");
                }
            }
        }
        lock(&self.inner.state)
            .usage_by_session
            .get(session_id)
            .and_then(Option::as_ref)
            .cloned()
    }

    /// `handlePermission(...)`：拒绝一条原生审批，或把它交回外部客户端。
    /// 返回 `Some(handoff)` 表示动作已交接。
    async fn handle_permission(
        &self,
        permission: &Value,
        request: &PreparedRequest,
        signal: &AbortSignal,
        rejected: &Mutex<HashSet<String>>,
        meta: &SharedMeta,
        activity: &Activity,
    ) -> Result<Option<Value>, BackendError> {
        let call_id = permission
            .get("tool")
            .and_then(|tool| tool.get("callID"))
            .and_then(Value::as_str)
            .filter(|call_id| !call_id.is_empty())
            .map(str::to_string);
        if let Some(call_id) = &call_id {
            // 只在同步段持锁：JS 的 Set 无阻塞语义，Rust 里绝不能跨 await 持 std Mutex。
            let mut set = lock(rejected);
            if set.contains(call_id) {
                return Ok(None);
            }
            set.insert(call_id.clone());
        }
        let attempts = meta
            .get("nativeAttempts")
            .and_then(|value| value.as_i64())
            .unwrap_or(0)
            + 1;
        meta.set("nativeAttempts", json!(attempts));
        let mut permissions: Vec<Value> = meta
            .get("permissions")
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| items.to_vec())
            .unwrap_or_default();
        if permissions.len() < 5 {
            permissions.push(shrink_permission(permission, 400));
            meta.set("permissions", json!(permissions));
        }
        let action = match &call_id {
            Some(call_id) => self.blocked_action(call_id, signal).await,
            None => Value::Null,
        };
        let native = if truthy(action.get("tool")) {
            action.get("tool").cloned()
        } else if truthy(
            permission
                .get("metadata")
                .and_then(|metadata| metadata.get("command")),
        ) {
            Some(json!("bash"))
        } else {
            None
        };
        let input = handoff_input(&action, permission);
        let tools = allowed_tools(request);
        let handoff = native
            .as_ref()
            .and_then(|native| build_handoff(Some(native), &input, &tools));
        if handoff.is_none() {
            let detail = action
                .get("failure")
                .filter(|failure| truthy(Some(*failure)))
                .cloned()
                .unwrap_or_else(|| json!("arguments incomplete for the external schema"));
            meta.set(
                "handoffCheck",
                json!({
                    "native": native.clone().unwrap_or(Value::Null),
                    "offeredTools": request.tools().len(),
                    "detail": detail,
                }),
            );
            // 动作确实存在、只是没被表达出来：留给翻译器兜底。
            if let Some(native) = native.clone() {
                meta.set(
                    "handoffMiss",
                    json!({ "native": native, "input": input, "offeredTools": request.tools().len() }),
                );
            }
        }
        if let Some(handoff) = handoff {
            Self::report_phase(meta, activity, "handoff", None);
            let _ = self
                .reject(
                    permission,
                    "This native action is executed by the external client instead.",
                    signal,
                )
                .await;
            return Ok(Some(handoff));
        }
        let reason = if call_id.is_none() {
            "the approval request carries no call ID, so it cannot be matched to the external tool list"
        } else if !action.is_null() {
            "its arguments cannot be mapped onto an external tool schema supplied in this request"
        } else {
            "the call could not be read back from the session"
        };
        let label = match &native {
            Some(native) if truthy(Some(native)) => display_value(native),
            _ => {
                let filepath = permission
                    .get("metadata")
                    .and_then(|metadata| metadata.get("filepath"))
                    .and_then(Value::as_str);
                match filepath {
                    Some(filepath) => format!("a file operation on {filepath}"),
                    None => display_value(
                        &permission
                            .get("permission")
                            .cloned()
                            .unwrap_or(json!("native tool")),
                    ),
                }
            }
        };
        let feedback = reject_feedback(Some(&json!(label)), reason);
        let _ = self.reject(permission, &feedback, signal).await;
        Ok(None)
    }

    /// `models()`：拉取 /provider 并筛选免费模型。
    pub async fn models(&self) -> Result<Vec<Value>, BackendError> {
        let providers = self.request("/provider", "GET", None, None, None).await?;
        let models = free_models(&providers)
            .map_err(|error| BackendError::bridge(&error))?;
        if models.is_empty() {
            return Err(BackendError::plain(
                "No free text models found; existing list preserved",
            ));
        }
        Ok(models)
    }

    /// `translate(...)`：交给辅助模型的一次有界转写；探测路径不会走到这里。
    ///
    /// translate → complete_fn → complete → translate 构成异步递归环，rustc 无法为
    /// `async fn` 推断出有限的 future 类型；必须在这里做一次类型擦除打断环。
    #[allow(clippy::too_many_arguments)]
    fn translate<'a>(
        &'a self,
        request: &'a PreparedRequest,
        shape: &'a str,
        material: &'a Value,
        meta: &'a SharedMeta,
        activity: &'a Activity,
        blocked: Option<&'a Value>,
        signal: &'a AbortSignal,
    ) -> Pin<Box<dyn Future<Output = Option<Value>> + Send + 'a>> {
        Box::pin(self.translate_inner(request, shape, material, meta, activity, blocked, signal))
    }

    #[allow(clippy::too_many_arguments)]
    async fn translate_inner(
        &self,
        request: &PreparedRequest,
        shape: &str,
        material: &Value,
        meta: &SharedMeta,
        activity: &Activity,
        blocked: Option<&Value>,
        signal: &AbortSignal,
    ) -> Option<Value> {
        let deadline = AbortSignal::timeout(TRANSLATE_DEADLINE);
        let joined = AbortSignal::any(&[deadline.clone(), signal.clone()]);
        // 对应 JS 的 `translator(request.model?.id, shape)`：失败模型 id 为空串时不排除任何候选。
        let failed = request
            .model()
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let model = lock(&self.inner.translator)
            .as_ref()
            .map(|translator| translator(&failed, shape));
        let model = model.flatten();
        Self::report_phase(
            meta,
            activity,
            "repair",
            Some(json!({ "repairModel": model.clone().map_or(Value::Null, |m| json!(m)) })),
        );
        let repair_model = model.clone().unwrap_or_default();

        let inner_meta = SharedMeta::new(json!({ "model": repair_model }));
        let inner_activity = if activity.is_noop() {
            Activity::silent()
        } else {
            let outer_meta = meta.clone();
            let outer_activity = activity.clone();
            let repair_model = repair_model.clone();
            Activity::forwarding(signal, move |progress| {
                if progress.get("type") == Some(&json!("request.done")) {
                    return;
                }
                let mut mapped = Map::new();
                mapped.insert(
                    "sessionID".to_string(),
                    outer_meta.get("sessionID").unwrap_or(Value::Null),
                );
                mapped.insert(
                    "model".to_string(),
                    outer_meta.get("model").unwrap_or(Value::Null),
                );
                mapped.insert("type".to_string(), json!("bridge.repair"));
                mapped.insert("status".to_string(), json!("repair"));
                mapped.insert("repairModel".to_string(), json!(repair_model.clone()));
                if truthy(progress.get("content")) {
                    mapped.insert("content".to_string(), json!(true));
                }
                outer_activity.report(Value::Object(mapped));
            })
        };

        let backend = self.clone();
        let translate_meta = inner_meta.clone();
        let translate_activity = inner_activity.clone();
        let translate_signal = joined.clone();
        let mut complete_fn: CompleteFn =
            Box::new(move |inner_request: &Value| {
                let backend = backend.clone();
                let request = PreparedRequest::from_value(inner_request.clone());
                let meta = translate_meta.clone();
                let activity = translate_activity.clone();
                let signal = translate_signal.clone();
                Box::pin(async move {
                    backend
                        .complete(
                            request,
                            RequestContext {
                                meta: meta.clone(),
                                signal,
                                activity,
                            },
                        )
                        .await
                        .map_err(|error| to_bridge_error(&error))
                })
            });

        let allowed = allowed_tools(request);
        let mut request_value = request.as_value().clone();
        if let Some(map) = request_value.as_object_mut() {
            map.insert("tools".to_string(), json!(allowed));
        }

        let is_action = shape == "action";
        let decode_request = request.clone();
        let tools_for_action = allowed.clone();
        let mut validate_fn: ValidateFn =
            if is_action {
                Box::new(move |candidate: &Value| {
                    let action = validate_action(candidate, &tools_for_action)?;
                    decode(
                        &js_stringify(&json!({ "content": "", "calls": [action] })),
                        &decode_request,
                    )?;
                    Ok(action)
                })
            } else {
                let decode_request = decode_request.clone();
                Box::new(move |candidate: &Value| {
                    decode(&js_stringify(candidate), &decode_request)
                })
            };

        let mut scratch = json!({ "repaired": meta.get("repaired").unwrap_or(json!({})) });
        let log = self.inner.log.clone();
        let mut log_fn = move |line: &str| (log)(line);
        let translated = repair(RepairDeps {
            complete: &mut *complete_fn,
            translator: model,
            request: &request_value,
            shape,
            material,
            blocked,
            validate: &mut *validate_fn,
            meta: &mut scratch,
            log: &mut log_fn,
        })
        .await;
        if let Some(repaired) = scratch.get("repaired") {
            meta.set("repaired", repaired.clone());
        }
        translated
    }

    /// `complete(request, signal, meta)`：会话创建 → 权限监视 → 三轮信封协商 → 清理。
    pub async fn complete(
        &self,
        request: PreparedRequest,
        context: RequestContext,
    ) -> Result<Value, BackendError> {
        let meta = context.meta.clone();
        let activity = context.activity.clone();
        let signal = context.signal.clone();
        meta.set("steps", json!(0));
        meta.set("nativeAttempts", json!(0));
        meta.set("permissions", json!([]));

        let session = self
            .request(
                "/session",
                "POST",
                Some(&json!({ "title": "WB Bridge", "permission": permission_list() })),
                Some(&signal),
                None,
            )
            .await?;
        let session_id = session
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let route = format!("/session/{}", encode_uri_component(&session_id));
        meta.set("sessionID", json!(&session_id));
        {
            let mut state = lock(&self.inner.state);
            state
                .usage_by_session
                .insert(session_id.clone(), None);
            if !activity.is_noop() {
                state.active.insert(
                    session_id.clone(),
                    ActiveMeta {
                        meta: meta.clone(),
                        activity: activity.clone(),
                    },
                );
            }
        }
        self.watch_events();
        Self::report_phase(&meta, &activity, "waiting", None);

        let (guard, _guard_signal) = AbortSignal::channel();
        let guard_combined = AbortSignal::any(&[guard.signal(), signal.clone()]);
        let rejected = Arc::new(Mutex::new(HashSet::new()));

        // 权限监视循环（对应 JS 的 watch IIFE）：交接结果或 chat-only 拒绝会被竞态消费。
        let watch_handle = {
            let backend = self.clone();
            let watch_request = request.clone();
            let watch_meta = meta.clone();
            let watch_activity = activity.clone();
            let watch_rejected = rejected.clone();
            let watch_signal = guard_combined.clone();
            let watch_session = session_id.clone();
            tokio::spawn(async move {
                loop {
                    if watch_signal.is_aborted() {
                        return Ok(Value::Null);
                    }
                    let pending = backend
                        .pending_permissions(&watch_session, &watch_signal)
                        .await;
                    for permission in pending {
                        if watch_request.chat_only() {
                            return Err(BackendError::with(
                                "Chat-only model attempted native tool use; execution blocked",
                                502,
                                "native_tool_activity",
                            ));
                        }
                        let outcome = backend
                            .handle_permission(
                                &permission,
                                &watch_request,
                                &watch_signal,
                                &watch_rejected,
                                &watch_meta,
                                &watch_activity,
                            )
                            .await?;
                        if let Some(handoff) = outcome {
                            return Ok(json!({ "handoff": handoff }));
                        }
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                        _ = watch_signal.cancelled() => {}
                    }
                }
            })
        };
        let mut watch = Sticky {
            handle: watch_handle,
            cached: None,
        };

        let tools = allowed_tools(&request);
        let mut calls_schema = Map::new();
        calls_schema.insert("type".to_string(), json!("array"));
        if !request.parallel() {
            calls_schema.insert("maxItems".to_string(), json!(1));
        }
        if *request.choice() == json!("required") || request.forced().is_some() {
            calls_schema.insert("minItems".to_string(), json!(1));
        }
        if tools.is_empty() {
            calls_schema.insert("maxItems".to_string(), json!(0));
            calls_schema.insert("items".to_string(), json!({ "type": "object" }));
        } else {
            calls_schema.insert(
                "items".to_string(),
                json!({ "anyOf": tools.iter().map(|tool| {
                    let function = tool.get("function").cloned().unwrap_or(Value::Null);
                    json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "const": function.get("name").cloned().unwrap_or(Value::Null) },
                            "arguments": function.get("parameters").cloned().unwrap_or_else(|| json!({ "type": "object" })),
                        },
                        "required": ["name", "arguments"],
                        "additionalProperties": false,
                    })
                }).collect::<Vec<_>>() }),
            );
        }
        let system_text = match request.system() {
            Some(system) => system.to_string(),
            None => String::new(),
        };
        let mut payload = Map::new();
        payload.insert(
            "model".to_string(),
            json!({
                "providerID": "opencode",
                "modelID": request
                    .model()
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .get("opencode/".len()..)
                    .unwrap_or_default(),
            }),
        );
        if let Some(variant) = request.variant().filter(|value| truthy(Some(*value))) {
            payload.insert("variant".to_string(), variant.clone());
        }
        payload.insert(
            "agent".to_string(),
            json!(if request.chat_only() { "buddy-chat" } else { "buddy-bridge" }),
        );
        payload.insert(
            "system".to_string(),
            json!(if request.chat_only() {
                system_text.clone()
            } else {
                format!("{system_text}\nUse StructuredOutput to return this envelope. All other native tools are forbidden; do not perform the external actions yourself.")
            }),
        );
        if !request.chat_only() {
            payload.insert(
                "format".to_string(),
                json!({
                    "type": "json_schema",
                    "retryCount": 0,
                    "schema": {
                        "type": "object",
                        "properties": { "content": { "type": "string" }, "calls": Value::Object(calls_schema.clone()) },
                        "required": ["content", "calls"],
                        "additionalProperties": false,
                    },
                }),
            );
        }
        let mut parts = vec![json!({ "type": "text", "text": request.text() })];
        parts.extend(request.images().iter().cloned());
        payload.insert("parts".to_string(), json!(parts));
        let mut payload = Value::Object(payload);

        let model_id = request
            .model()
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut successful = false;

        let outcome = async {
            let mut action_retried = false;
            for attempt in 0..3usize {
                let steps = meta.get("steps").and_then(|v| v.as_i64()).unwrap_or(0) + 1;
                meta.set("steps", json!(steps));
                Self::report_phase(
                    &meta,
                    &activity,
                    if attempt == 0 { "waiting" } else { "correcting" },
                    None,
                );
                lock(&self.inner.state)
                    .usage_by_session
                    .insert(session_id.clone(), None);
                let message_route = format!("{route}/message");
                let response = tokio::select! {
                    raced = &mut watch => raced?,
                    raced = self.request(&message_route, "POST", Some(&payload), Some(&signal), None) => raced?,
                };
                Self::report_phase(&meta, &activity, "checking", None);
                let mut handoff = response
                    .get("handoff")
                    .filter(|value| truthy(Some(*value)))
                    .cloned();
                if handoff.is_none() {
                    // 响应刚落地的瞬间可能又冒出审批：必须逐条拒绝或交接，否则会被当成
                    // 意外的原生活动。
                    let late = self
                        .pending_permissions(&session_id, &guard_combined)
                        .await;
                    for permission in late {
                        if request.chat_only()
                            && permission.get("tool").is_some_and(|tool| !tool.is_null())
                        {
                            return Err(BackendError::with(
                                "Chat-only model attempted native tool use; execution blocked",
                                502,
                                "native_tool_activity",
                            ));
                        }
                        let outcome = self
                            .handle_permission(
                                &permission,
                                &request,
                                &guard_combined,
                                &rejected,
                                &meta,
                                &activity,
                            )
                            .await?;
                        if let Some(value) = outcome {
                            handoff = Some(value);
                            break;
                        }
                    }
                }
                if let Some(handoff) = handoff {
                    let _ = self
                        .request(
                            &format!("{route}/abort"),
                            "POST",
                            None,
                            None,
                            Some(FIVE_SECONDS),
                        )
                        .await;
                    meta.set("calls", json!(1));
                    meta.set("handoff", handoff.get("name").cloned().unwrap_or(Value::Null));
                    successful = true;
                    let usage = self.handoff_usage(&session_id, &signal).await;
                    return Ok(completion(
                        &model_id,
                        &json!({ "role": "assistant", "content": null,
                            "tool_calls": [{
                                "id": format!("call_{}", random_uuid().replace('-', "")),
                                "type": "function",
                                "function": {
                                    "name": handoff.get("name").cloned().unwrap_or(Value::Null),
                                    "arguments": js_stringify(
                                        handoff.get("arguments").unwrap_or(&Value::Null),
                                    ),
                                },
                            }],
                        }),
                        usage.as_ref(),
                    ));
                }

                let info = response.get("info");
                if let Some(error) = info.and_then(|info| info.get("error")) {
                    if !error.is_null()
                        && (request.chat_only()
                            || error.get("name") != Some(&json!("StructuredOutputError")))
                    {
                        let message = error
                            .get("data")
                            .and_then(|data| data.get("message"))
                            .filter(|value| truthy(Some(*value)))
                            .cloned()
                            .or_else(|| {
                                error
                                    .get("message")
                                    .filter(|value| truthy(Some(*value)))
                                    .cloned()
                            })
                            .or_else(|| {
                                error
                                    .get("name")
                                    .filter(|value| truthy(Some(*value)))
                                    .cloned()
                            })
                            .unwrap_or_else(|| json!("Model request failed"));
                        let status = error
                            .get("data")
                            .and_then(|data| data.get("statusCode"))
                            .and_then(Value::as_u64)
                            .map(|status| status as u16)
                            .unwrap_or(502);
                        return Err(BackendError::with(
                            display_value(&message),
                            status,
                            "model_error",
                        ));
                    }
                }
                let parts: Vec<&Value> = match response.get("parts") {
                    Some(Value::Array(items)) => items.iter().collect(),
                    _ => Vec::new(),
                };
                let unexpected = {
                    // 同步作用域内取锁：锁不得跨 await 存活（future 需保持 Send）。
                    let rejected_set = lock(&rejected);
                    parts.iter().any(|part| {
                        part.get("type") == Some(&json!("tool"))
                            && (request.chat_only()
                                || !["StructuredOutput", "invalid"]
                                    .contains(&part.get("tool").and_then(Value::as_str).unwrap_or("")))
                            && !(part
                                .get("callID")
                                .and_then(Value::as_str)
                                .is_some_and(|call_id| rejected_set.contains(call_id))
                                && part.get("state").and_then(|s| s.get("status"))
                                    == Some(&json!("error")))
                    })
                };
                if unexpected {
                    return Err(BackendError::with(
                        "Unexpected native tool activity; response rejected",
                        502,
                        "native_tool_activity",
                    ));
                }
                let structured_part = parts.iter().find(|part| {
                    part.get("type") == Some(&json!("tool"))
                        && part.get("tool") == Some(&json!("StructuredOutput"))
                        && part
                            .get("state")
                            .and_then(|state| state.get("status"))
                            == Some(&json!("completed"))
                        && part
                            .get("state")
                            .and_then(|state| state.get("input"))
                            .is_some_and(|input| truthy(Some(input)))
                });
                let envelope = info
                    .and_then(|info| info.get("structured"))
                    .filter(|value| !value.is_null())
                    .or_else(|| {
                        structured_part.and_then(|part| part.get("state")).and_then(|state| state.get("input")).filter(|value| !value.is_null())
                    });
                let text = match envelope {
                    Some(envelope) => js_stringify(envelope),
                    None => parts
                        .iter()
                        .filter(|part| part.get("type") == Some(&json!("text")))
                        .map(|part| part.get("text").and_then(Value::as_str).unwrap_or_default())
                        .collect::<String>(),
                };

                // 信封可能来自三种渠道：structured 字段、已完成的 StructuredOutput 调用、
                // 纯文本 —— 只认其一曾把正确答案误判为失败。
                let message: Result<Value, BackendError> = (|| {
                    if info.and_then(|info| info.get("finish")) == Some(&json!("length")) {
                        return Err(BackendError::with(
                            "Model output was truncated",
                            502,
                            "output_truncated",
                        ));
                    }
                    if js_trim(&text).is_empty() {
                        let unparsed = parts.iter().find(|part| {
                            part.get("type") == Some(&json!("tool"))
                                && part.get("tool") == Some(&json!("invalid"))
                        });
                        if request.chat_only() {
                            return Err(BackendError::with(
                                "Model returned no text",
                                502,
                                "empty_response",
                            ));
                        }
                        return Err(match unparsed {
                            Some(part) => {
                                let detail = part
                                    .get("state")
                                    .and_then(|state| state.get("input"))
                                    .and_then(|input| input.get("error"))
                                    .map(display_value)
                                    .unwrap_or_else(|| "no detail from the runtime".to_string());
                                BackendError::with(
                                    format!("模型交的调用参数不是合法 JSON：{detail}"),
                                    502,
                                    "invalid_model_output",
                                )
                            }
                            None => BackendError::with(
                                "模型没有返回信封：structured、已完成的 StructuredOutput 调用、文本 part 三者都为空",
                                502,
                                "invalid_model_output",
                            ),
                        });
                    }
                    if request.chat_only() {
                        return Ok(json!({ "role": "assistant", "content": text }));
                    }
                    decode(&text, &request).map_err(|error| BackendError::bridge(&error))
                })();
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        let code = error.code.clone().unwrap_or_default();
                        if request.chat_only()
                            || signal.is_aborted()
                            || !matches!(
                                code.as_str(),
                                "invalid_model_output" | "invalid_tool_call" | "output_truncated"
                            )
                        {
                            return Err(error);
                        }
                        let cut = code == "output_truncated";
                        let error_value = json!({ "code": code, "message": error.message });
                        if attempt == 0 {
                            payload["parts"] = json!([{ "type": "text", "text": if cut {
                                "Your previous response was cut off by the output limit before the envelope was complete. Send it again in a much more compact form: content holds the conclusion, calls hold only the essential arguments, and keep reasoning to a minimum.".to_string()
                            } else {
                                "Your previous response failed the adapter JSON format check. No external tool has been executed from that response. Return the intended answer or external tool proposal using StructuredOutput with exactly {\"content\":\"a string, empty if only calling tools\",\"calls\":[{\"name\":\"an allowed external tool name\",\"arguments\":{}}]}. Both fields are required; use [] when no tools are needed. Do not invoke native tools, repeat external searches, or claim actions have completed. Preserve the external conversation and its existing tool results.".to_string()
                            } }]);
                            continue;
                        }
                        if attempt == 1 && !truthy(meta.get("probe").as_ref()) {
                            let material =
                                raw_material(&response, &request_value_for_repair(&request), Some(&error_value));
                            let translated = self
                                .translate(&request, "envelope", &material, &meta, &activity, None, &signal)
                                .await;
                            if let Some(translated) = translated {
                                let calls = translated
                                    .get("tool_calls")
                                    .and_then(Value::as_array)
                                    .map(Vec::len)
                                    .unwrap_or(0);
                                meta.set("calls", json!(calls));
                                successful = true;
                                return Ok(completion(
                                    &model_id,
                                    &translated,
                                    info.and_then(|info| info.get("tokens")),
                                ));
                            }
                            let repaired = meta.get("repaired");
                            payload["parts"] = json!([{
                                "type": "text",
                                "text": resend_prompt(
                                    Some(&error_value),
                                    repaired.as_ref().and_then(|repaired| repaired.get("envelope")),
                                    None,
                                ),
                            }]);
                            continue;
                        }
                        return Err(error);
                    }
                };

                // 模型想本地执行、动作表表达不出来、最后用文本作答：把被拦动作翻译出来，
                // 按交接的形态返回。
                let has_calls = message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty());
                let handoff_miss = meta
                    .get("handoffMiss")
                    .filter(|value| truthy(Some(value)) && !value.is_null());
                if !has_calls && handoff_miss.is_some() && !truthy(meta.get("probe").as_ref()) && !action_retried
                {
                    let blocked = handoff_miss.clone();
                    let material = raw_material(&response, &request_value_for_repair(&request), None);
                    let rescued = self
                        .translate(&request, "action", &material, &meta, &activity, blocked.as_ref(), &signal)
                        .await;
                    if let Some(rescued) = rescued {
                        let _ = self
                            .request(&format!("{route}/abort"), "POST", None, None, Some(FIVE_SECONDS))
                            .await;
                        meta.set("calls", json!(1));
                        meta.set("handoff", rescued.get("name").cloned().unwrap_or(Value::Null));
                        successful = true;
                        let usage = match info.and_then(|info| info.get("tokens")) {
                            Some(tokens) if !tokens.is_null() => Some(tokens.clone()),
                            _ => self.handoff_usage(&session_id, &signal).await,
                        };
                        return Ok(completion(
                            &model_id,
                            &json!({ "role": "assistant", "content": null,
                                "tool_calls": [{
                                    "id": format!("call_{}", random_uuid().replace('-', "")),
                                    "type": "function",
                                    "function": {
                                        "name": rescued.get("name").cloned().unwrap_or(Value::Null),
                                        "arguments": js_stringify(
                                            rescued.get("arguments").unwrap_or(&Value::Null),
                                        ),
                                    },
                                }],
                            }),
                            usage.as_ref(),
                        ));
                    }
                    if attempt < 2 && !signal.is_aborted() {
                        action_retried = true;
                        let repaired = meta.get("repaired");
                        payload["parts"] = json!([{
                            "type": "text",
                            "text": resend_prompt(
                                None,
                                repaired.as_ref().and_then(|repaired| repaired.get("action")),
                                blocked.as_ref(),
                            ),
                        }]);
                        continue;
                    }
                }
                let calls = message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                meta.set("calls", json!(calls));
                successful = true;
                return Ok(completion(
                    &model_id,
                    &message,
                    info.and_then(|info| info.get("tokens")),
                ));
            }
            Err(BackendError::plain("complete loop terminated unexpectedly"))
        }
        .await;

        // finally：与 JS 完全同序（守卫 → 等待监视循环 → 用量/审批/活动清理 → 取消善后）。
        guard.abort();
        let _ = (&mut watch).await;
        let stop_events = {
            let mut state = lock(&self.inner.state);
            state.usage_by_session.remove(&session_id);
            let stop = state.usage_by_session.is_empty();
            let stale: Vec<String> = state
                .pending_approvals
                .iter()
                .filter(|(_, permission)| {
                    permission.get("sessionID") == Some(&json!(session_id))
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in stale {
                state.pending_approvals.remove(&id);
            }
            stop
        };
        if stop_events {
            self.stop_events();
        }
        if !activity.is_noop() {
            activity.report(json!({
                "sessionID": session_id,
                "model": meta.get("model").unwrap_or(Value::Null),
                "type": "request.done",
            }));
            lock(&self.inner.state).active.remove(&session_id);
        }
        if !successful {
            let _ = self
                .request(&format!("{route}/abort"), "POST", None, None, Some(FIVE_SECONDS))
                .await;
        }
        if let Err(error) = self.request(&route, "DELETE", None, None, Some(FIVE_SECONDS)).await {
            self.log(&format!("Session cleanup failed: {}", error.message));
        }
        outcome
    }
}

fn request_value_for_repair(request: &PreparedRequest) -> Value {
    // rawMaterial 的 conversation 只需要 prepared 对象自带的原文视图。
    request.as_value().clone()
}

fn display_value(value: &Value) -> String {
    display(Some(value))
}

fn session_id_value(properties: Option<&Value>) -> Value {
    properties
        .and_then(|properties| properties.get("sessionID"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// `BackendError` → `BridgeError`（编排层的探测路径需要同一套状态码/错误码归宿）。
pub fn to_bridge_error(error: &BackendError) -> BridgeError {
    BridgeError::with(
        error.message.clone(),
        error.status.unwrap_or(502),
        &error.code.clone().unwrap_or_else(|| "upstream_error".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_permissions_keeps_every_official_gate() {
        let permissions = native_permissions();
        assert_eq!(permissions["*"], json!("ask"));
        for denied in [
            "question",
            "websearch",
            "codesearch",
            "webfetch",
            "task",
            "plan_enter",
            "plan_exit",
            "todowrite",
        ] {
            assert_eq!(permissions[denied], json!("deny"), "{denied}");
        }
        let list = permission_list().as_array().cloned().unwrap();
        assert_eq!(list.len(), 9);
        assert_eq!(list[0], json!({ "permission": "*", "pattern": "*", "action": "ask" }));
    }

    #[test]
    fn shrink_permission_bounds_strings_but_keeps_shape() {
        let long = "x".repeat(401);
        let shrunk = shrink_permission(&json!({ "a": long, "b": [long] }), 400);
        let text = shrunk["a"].as_str().unwrap();
        assert!(text.ends_with("…[401 chars]"));
        assert_eq!(text.chars().count(), 400 + "…[401 chars]".chars().count());
        assert_eq!(shrink_permission(&json!({ "n": 7 }), 400), json!({ "n": 7 }));
        assert_eq!(shrink_permission(&json!(null), 400), json!(null));
    }

    #[test]
    fn free_models_keeps_only_zero_cost_text_models() {
        let providers = json!({ "all": [{ "id": "opencode", "models": {
            "free": {
                "name": "Free", "cost": { "input": 0, "output": 0 },
                "capabilities": { "output": { "text": true } },
                "limit": { "context": 8192 },
            },
            "paid": { "cost": { "input": 1, "output": 0 } },
            "cached": { "cost": { "input": 0, "output": 0, "cache": { "read": 1, "write": 0 } } },
            "no-text": { "cost": { "input": 0, "output": 0 }, "capabilities": { "output": { "text": false } } },
            "old": { "cost": { "input": 0, "output": 0 }, "status": "deprecated" },
            "no-name": { "cost": { "input": 0, "output": 0 } },
        } }] });
        let models = free_models(&providers).unwrap();
        let ids: Vec<&str> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["opencode/free", "opencode/no-name"]);
        assert_eq!(models[0]["name"], json!("Free"));
        assert_eq!(models[0]["context"], json!(8192));
        assert_eq!(models[0]["variants"], json!({}));
        assert_eq!(models[1]["name"], json!("no-name"));
        assert!(models[0].get("input").is_none());
    }

    #[test]
    fn free_models_requires_the_opencode_provider() {
        let error = free_models(&json!({ "all": [] })).unwrap_err();
        assert_eq!(error.message, "OpenCode provider missing");
    }

    #[test]
    fn allowed_tools_clears_on_none_and_filters_forced() {
        let tools = json!([
            { "function": { "name": "Read" } },
            { "function": { "name": "Write" } },
        ]);
        let mut none_case = json!({ "model": { "id": "opencode/m" }, "tools": tools.clone(), "choice": "none" });
        let none = PreparedRequest::from_value(none_case.clone());
        assert!(allowed_tools(&none).is_empty());
        none_case["choice"] = json!("auto");
        none_case["forced"] = json!("Write");
        let forced = PreparedRequest::from_value(none_case);
        let filtered = allowed_tools(&forced);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["function"]["name"], json!("Write"));
    }

    // -----------------------------------------------------------------------
    // complete()：axum 假 OpenCode 服务的 mock 测试
    // -----------------------------------------------------------------------

    use crate::protocol::prepare;
    use axum::extract::State;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockState {
        attempts: AtomicUsize,
        replies: Vec<Value>,
        permissions: Vec<Value>,
    }

    async fn mock_handler(
        State(state): State<Arc<MockState>>,
        req: axum::extract::Request,
    ) -> axum::Json<Value> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        if method == axum::http::Method::POST && path == "/session" {
            return axum::Json(json!({ "id": "ses_mock" }));
        }
        if method == axum::http::Method::GET && path == "/permission" {
            return axum::Json(Value::Array(state.permissions.clone()));
        }
        if method == axum::http::Method::POST && path.ends_with("/message") {
            let idx = state
                .attempts
                .fetch_add(1, Ordering::SeqCst)
                .min(state.replies.len().saturating_sub(1));
            return axum::Json(state.replies.get(idx).cloned().unwrap_or_else(|| json!({})));
        }
        if method == axum::http::Method::GET && path.starts_with("/session/") {
            // handoffUsage 的 GET /session/:id/message?limit=1。
            return axum::Json(json!([]));
        }
        axum::Json(json!({}))
    }

    /// 起一个只覆盖 Backend 会调用的路由的假服务，返回 base URL 与服务任务。
    async fn start_mock(replies: Vec<Value>, permissions: Vec<Value>) -> (String, tokio::task::JoinHandle<()>) {
        let state = Arc::new(MockState {
            attempts: AtomicUsize::new(0),
            replies,
            permissions,
        });
        let app = axum::Router::new()
            .fallback(axum::routing::any(mock_handler))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), handle)
    }

    fn mock_model(chat_only: bool) -> Value {
        let mut model = json!({ "id": "opencode/mock", "name": "Mock" });
        if chat_only {
            model["chatOnly"] = json!(true);
        }
        model
    }

    fn mock_body() -> Value {
        json!({ "model": "opencode/mock", "messages": [{ "role": "user", "content": "hi" }] })
    }

    fn good_envelope(content: &str) -> Value {
        json!({
            "info": {
                "role": "assistant",
                "structured": { "content": content, "calls": [] },
                "tokens": { "input": 3, "output": 2 },
            },
            "parts": [],
        })
    }

    async fn run_complete(
        base: &str,
        model: Value,
    ) -> (Result<Value, BackendError>, SharedMeta) {
        let backend = Backend::new(base, "pw", |_| {});
        let prepared = prepare(&mock_body(), std::slice::from_ref(&model)).unwrap();
        let meta = SharedMeta::new(json!({ "model": model }));
        let (_controller, signal) = AbortSignal::channel();
        let result = backend
            .complete(
                prepared,
                RequestContext {
                    meta: meta.clone(),
                    signal,
                    activity: Activity::silent(),
                },
            )
            .await;
        (result, meta)
    }

    #[tokio::test]
    async fn complete_returns_decoded_envelope_and_records_usage() {
        let (base, server) = start_mock(vec![good_envelope("Hello!")], vec![]).await;
        let (result, meta) = run_complete(&base, mock_model(false)).await;
        server.abort();

        let response = result.expect("mock 信封应成功");
        assert_eq!(response["choices"][0]["message"]["content"], json!("Hello!"));
        assert_eq!(response["choices"][0]["finish_reason"], json!("stop"));
        assert!(response["usage"]["total_tokens"].is_number());
        assert_eq!(meta.get("sessionID"), Some(json!("ses_mock")));
        assert_eq!(meta.get("calls"), Some(json!(0)));
        assert_eq!(meta.get("steps"), Some(json!(1)));
    }

    #[tokio::test]
    async fn complete_retries_format_failure_then_translates_once() {
        // 前两次返回空信封触发 invalid_model_output；attempt 1 走 translate
        // （无翻译器 → 记录 repaired.envelope），第三次成功。
        let empty = json!({ "info": { "role": "assistant" }, "parts": [] });
        let (base, server) = start_mock(
            vec![empty.clone(), empty, good_envelope("Recovered")],
            vec![],
        )
        .await;
        let (result, meta) = run_complete(&base, mock_model(false)).await;
        server.abort();

        let response = result.expect("第三次尝试应成功");
        assert_eq!(response["choices"][0]["message"]["content"], json!("Recovered"));
        assert_eq!(meta.get("steps"), Some(json!(3)));
        assert_eq!(
            meta.get("repaired").unwrap_or(Value::Null)["envelope"]["reason"],
            json!("no translator available")
        );
    }

    #[tokio::test]
    async fn complete_blocks_chat_only_native_permission() {
        // 待审批里有原生工具：chatOnly 请求无论先赢哪一路都必须报 native_tool_activity。
        let permissions = vec![json!({
            "id": "per_1",
            "sessionID": "ses_mock",
            "tool": "Bash",
            "callID": "call_1",
        })];
        let (base, server) = start_mock(vec![good_envelope("nope")], permissions).await;
        let (result, _meta) = run_complete(&base, mock_model(true)).await;
        server.abort();

        let error = result.expect_err("chatOnly 遇到原生审批必须失败");
        assert_eq!(error.code.as_deref(), Some("native_tool_activity"));
        assert_eq!(error.status, Some(502));
    }
}
