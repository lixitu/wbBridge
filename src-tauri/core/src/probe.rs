//! `core/src/probe.js` 的 Rust 等价实现 —— 模型探测协议与判定。
//!
//! 行为与 Node 版逐条对齐：
//! - 探测请求携带多个工具与自由 `tool_choice`，模拟真实 WorkBuddy 流量；
//! - `judgeProbe` 只在恰好一个调用、名称为 `Read`、且 `file_path` 含 token 时判通过；
//! - `formatUnsupported` 区分格式类失败（降级为 chatOnly）与执行类失败（直接失败）；
//! - `RETRYABLE_PROBE` 只对语义误判重试一次，格式失败与超时不重试；
//! - 每个模型各自持有 60s 的 deadline（首次尝试与重试共用同一个，不重置）。

use crate::protocol::BridgeError;
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

/// 单个模型的探测超时预算（毫秒），对应 `PROBE_TIMEOUT = 60000`。
/// 每个模型各有一份，重试用的是同一个 deadline 的剩余时间，不是重新计时。
pub const PROBE_TIMEOUT_MS: u64 = 60_000;

/// 探测请求使用的工具目录，对应 `PROBE_TOOLS`。
pub fn probe_tools() -> Value {
    json!([
        { "type": "function", "function": { "name": "Read",
            "description": "Read a file from the external working directory.",
            "parameters": { "type": "object", "properties": { "file_path": { "type": "string", "description": "Absolute path of the file to read" } }, "required": ["file_path"] } } },
        { "type": "function", "function": { "name": "Write",
            "description": "Write a file in the external working directory.",
            "parameters": { "type": "object", "properties": { "file_path": { "type": "string" }, "content": { "type": "string" } }, "required": ["file_path", "content"] } } },
        { "type": "function", "function": { "name": "Bash",
            "description": "Run a shell command on the external machine.",
            "parameters": { "type": "object", "properties": { "command": { "type": "string" }, "description": { "type": "string" } }, "required": ["command"] } } },
        { "type": "function", "function": { "name": "Glob",
            "description": "List files matching a pattern.",
            "parameters": { "type": "object", "properties": { "pattern": { "type": "string" }, "path": { "type": "string" } }, "required": ["pattern"] } } },
        { "type": "function", "function": { "name": "WebSearch",
            "description": "Search the web.",
            "parameters": { "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] } } },
    ])
}

/// `probeBody(model, token)`：构造一次探测请求体。
/// `model` 为 `{ id, … }` 对象，`token` 为 16 位十六进制随机串（每次不同）。
pub fn probe_body(model: &Value, token: &str) -> Value {
    json!({
        "model": model.get("id").and_then(Value::as_str).unwrap_or_default(),
        "messages": [{ "role": "user", "content": format!("Read /external/probe-{token}.txt and report its contents.") }],
        "tools": probe_tools(),
        "parallel_tool_calls": false,
    })
}

/// `judgeProbe(response, token)`：校验 tool_calls 结构。
///
/// 通过条件：恰好 1 个调用、名称为 `Read`、且 `file_path` 参数含 `token`。
/// 失败时抛 `BridgeError`，`code` 为 `no_action`（无调用）或 `probe_mismatch`（调用不符）。
pub fn judge_probe(response: &Value, token: &str) -> Result<Value, BridgeError> {
    let calls = response
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if calls.is_empty() {
        return Err(BridgeError::with(
            "模型只返回了文本，没有产生任何动作",
            502,
            "no_action",
        ));
    }

    let first = &calls[0];
    let args: Value = first
        .get("function")
        .and_then(|f| f.get("arguments"))
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .parse()
        .unwrap_or(json!({}));

    let name = first.get("function").and_then(|f| f.get("name")).and_then(Value::as_str).unwrap_or_default();
    let file_path = args.get("file_path").and_then(Value::as_str).unwrap_or_default();

    if calls.len() != 1 || name != "Read" || !file_path.contains(token) {
        let received = calls
            .iter()
            .map(|c| c.get("function").and_then(|f| f.get("name")).and_then(Value::as_str).unwrap_or("未命名"))
            .collect::<Vec<_>>()
            .join("、");
        return Err(BridgeError::with(
            format!("模型返回的动作与探测请求不符（收到 {received}）"),
            502,
            "probe_mismatch",
        ));
    }

    Ok(first.clone())
}

/// `formatUnsupported(error)`：判断是否为格式类失败（应降级为 chatOnly 而非直接失败）。
pub fn format_unsupported(error: &BridgeError) -> bool {
    if ["invalid_model_output", "invalid_tool_call"].contains(&error.code.as_str()) {
        return true;
    }
    // 对应 Node 的 /only.{0,10}auto.{0,40}supported.{0,20}tool_choice/i
    // 使用 regex 大小写不敏感匹配
    regex::Regex::new(r"(?i)only.{0,10}auto.{0,40}supported.{0,20}tool_choice")
        .map(|re| re.is_match(&error.message))
        .unwrap_or(false)
}

/// `RETRYABLE_PROBE`：只对语义类失败重试（一次），格式失败和超时不重试。
pub fn retryable_probe_codes() -> HashSet<&'static str> {
    ["probe_mismatch", "no_action"].into_iter().collect()
}

/// 生成探测 token（16 位十六进制，对应 Node 的 `randomBytes(8).toString('hex')`）。
pub fn probe_token() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_string()
}

/// `probeFailure(cause, timedOut)`：超时失败时统一改写为 `TimeoutError`。
pub fn probe_failure(cause: BridgeError, timed_out: bool) -> BridgeError {
    if !timed_out {
        return cause;
    }
    let mut error = BridgeError::with("Model probe timed out", 504, "timeout");
    error.code = "timeout".to_string();
    error
}

/// 是否应重试：对应 `RETRYABLE_PROBE.has(error.code)`。
pub fn should_retry(code: &str) -> bool {
    retryable_probe_codes().contains(code)
}

/// `probeModel({ complete, retries = 1 })`：最多尝试 `retries + 1` 次。
///
/// `complete(token)` 对应 JS 注入的 `complete` 回调；探测方在外层用**共享的批内 deadline**
/// 包裹它，因此重试与首次尝试共用同一预算（「A retry shares the single deadline」）。
/// 只对语义类失败（`probe_mismatch` / `no_action`）重试：格式失败走 chatOnly 通道，
/// 超时反映的是负载而不是抖动，都不该再花一次预算。
pub async fn probe_model<F, Fut>(complete: F, retries: u32) -> Result<Value, BridgeError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Value, BridgeError>> + Send,
{
    let mut attempt = 0;
    loop {
        let token = probe_token();
        match complete(token.clone()).await {
            Ok(response) => match judge_probe(&response, &token) {
                Ok(_) => return Ok(response),
                Err(error) => {
                    if attempt >= retries || !should_retry(&error.code) {
                        return Err(error);
                    }
                }
            },
            Err(error) => {
                if attempt >= retries || !should_retry(&error.code) {
                    return Err(error);
                }
            }
        }
        attempt += 1;
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_response(token: &str) -> Value {
        json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "Read",
                            "arguments": format!(r#"{{"file_path":"/external/probe-{token}.txt"}}"#)
                        }
                    }]
                }
            }]
        })
    }

    #[test]
    fn judge_probe_accepts_correct_call() {
        let token = "abc123";
        let result = judge_probe(&ok_response(token), token);
        assert!(result.is_ok(), "应通过：{:?}", result.err());
    }

    #[test]
    fn judge_probe_rejects_empty_calls() {
        let response = json!({ "choices": [{ "message": { "tool_calls": [] } }] });
        let error = judge_probe(&response, "xyz").expect_err("应失败");
        assert_eq!(error.code, "no_action");
    }

    #[test]
    fn judge_probe_rejects_wrong_tool_name() {
        let response = json!({
            "choices": [{ "message": { "tool_calls": [{
                "function": { "name": "Write", "arguments": r#"{"file_path":"probe-abc.txt"}"# }
            }] } }]
        });
        let error = judge_probe(&response, "abc").expect_err("应失败");
        assert_eq!(error.code, "probe_mismatch");
    }

    #[test]
    fn format_unsupported_matches_codes() {
        assert!(format_unsupported(&BridgeError::with("x", 502, "invalid_model_output")));
        assert!(format_unsupported(&BridgeError::with("x", 502, "invalid_tool_call")));
        assert!(!format_unsupported(&BridgeError::with("x", 502, "native_tool_activity")));
    }

    #[test]
    fn format_unsupported_matches_regex() {
        // 消息需含 only → (≤10) → auto → (≤40) → supported → (≤20) → tool_choice 子串序列
        let error = BridgeError::with(
            "only auto tool_choice is the only output format supported for tool_choice",
            502,
            "unknown",
        );
        assert!(format_unsupported(&error));
    }

    #[test]
    fn should_retry_only_for_probe_mismatch_and_no_action() {
        assert!(should_retry("probe_mismatch"));
        assert!(should_retry("no_action"));
        assert!(!should_retry("invalid_model_output"));
        assert!(!should_retry("timeout"));
    }

    #[test]
    fn probe_body_has_required_shape() {
        let model = json!({ "id": "opencode/gpt-4o" });
        let body = probe_body(&model, "deadbeef");
        assert_eq!(body["model"], "opencode/gpt-4o");
        assert_eq!(body["parallel_tool_calls"], false);
        assert!(body["messages"].as_array().unwrap().len() == 1);
        assert!(body["tools"].as_array().unwrap().len() == 5);
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("deadbeef"));
    }

    #[test]
    fn probe_failure_on_timeout_replaces_error() {
        let original = BridgeError::with("some error", 502, "model_error");
        let timed = probe_failure(original.clone(), true);
        assert_eq!(timed.code, "timeout");
        assert_eq!(timed.status, 504);
        let not_timed = probe_failure(original.clone(), false);
        assert_eq!(not_timed.code, "model_error");
    }

    #[test]
    fn probe_token_is_16_hex_chars() {
        let token = probe_token();
        assert_eq!(token.len(), 16);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn probe_model_retries_once_then_succeeds() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = AtomicUsize::new(0);
        let result = probe_model(
            |token| {
                let attempts = &attempts;
                async move {
                    let call = attempts.fetch_add(1, Ordering::SeqCst);
                    if call == 0 {
                        return Err(BridgeError::with("no action", 502, "no_action"));
                    }
                    Ok::<_, BridgeError>(ok_response(&token))
                }
            },
            1,
        )
        .await;
        assert!(result.is_ok(), "{:?}", result.err());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn probe_model_does_not_retry_format_or_last_error() {
        let format_failure = probe_model(
            |_| async { Err(BridgeError::with("bad envelope", 502, "invalid_model_output")) },
            1,
        )
        .await
        .expect_err("格式失败不应重试");
        assert_eq!(format_failure.code, "invalid_model_output");

        let exhausted = probe_model(
            |_| async { Err(BridgeError::with("no action", 502, "no_action")) },
            1,
        )
        .await
        .expect_err("重试用尽后应抛出最后一次错误");
        assert_eq!(exhausted.code, "no_action");
    }
}
