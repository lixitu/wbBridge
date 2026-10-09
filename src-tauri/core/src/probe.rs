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
    if tool_call_unsupported(&error.message) {
        return true;
    }
    // 对应 Node 的 /only.{0,10}auto.{0,40}supported.{0,20}tool_choice/i
    // 使用 regex 大小写不敏感匹配
    regex::Regex::new(r"(?i)only.{0,10}auto.{0,40}supported.{0,20}tool_choice")
        .map(|re| re.is_match(&error.message))
        .unwrap_or(false)
}

/// 上游明说「这个模型不支持函数/工具调用」：SiliconFlow 回的是
/// `Bad Request: Function call is not supported for this model.`。
///
/// 这不是「模型坏了」而是「只能对话」——恰好是 chatOnly 通道服务的形态。探测请求带工具目录，
/// 而目录里 `capabilities.toolcall` 是 models.dev 的**声明**值，声明为真、平台实际不给调用时
/// 就落到这里；`probe_single_model` 对声明为假的模型直接给 `invalid_tool_call`（同一个归宿），
/// 本函数补的是声明与实况的差集那一段。
///
/// 判定要求「调用类名词」与「不支持」措辞**同时**出现且相邻（间隔 ≤40 字符），并且调用类名词
/// 必须是 `function/tool` + `call/calling/use` 的复合形态：单独的 `functions` / `tools` 太宽，
/// 会把 `invalid api key`、`429`、`context length exceeded`、地区拒绝这类与本判断无关的文案误伤。
/// 真误判了代价也很小：降级路径 `chat_only_attempt` 会向该模型发一次真实的纯对话请求，
/// 通不过就照原样记失败，不会凭文案就把模型发布出去。
pub fn tool_call_unsupported(message: &str) -> bool {
    const CALL: &str = r"(?:function|tool)[_ -]?(?:calls?|calling|use)";
    const REFUSED: &str = r"not\s+(?:supported|allowed|enabled|available|implemented)|unsupported|does\s+not\s+(?:support|allow)|no\s+(?:support|implementation)";
    let pattern = format!(r"(?is)({CALL}).{{0,40}}?({REFUSED})|({REFUSED}).{{0,40}}?({CALL})");
    regex::Regex::new(&pattern)
        .map(|re| re.is_match(message))
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
///
/// 地区拒绝与「模型已下线」走同一条文案改写：上游（OpenCode 的 zen 网关与其背后的提供方）给的
/// 是英文原文，面板直接显示它会让用户以为「模型坏了」或「是我哪里配错了」，而真正的原因
/// （换网络出口 / 提供方撤架）与可操作入口都没说出来。
/// 只改文案：`code`/`status` 原样保留，两者都本就不在 `RETRYABLE_PROBE` 里，
/// 因此不重试、不发布、也不牵连其它已发布模型。
pub fn probe_failure(cause: BridgeError, timed_out: bool) -> BridgeError {
    if timed_out {
        let mut error = BridgeError::with("Model probe timed out", 504, "timeout");
        error.code = "timeout".to_string();
        return error;
    }
    let Some(text) = region_unavailable_message(&cause.message)
        .or_else(|| deprecated_model_message(&cause.message))
    else {
        return cause;
    };
    BridgeError {
        message: text,
        ..cause
    }
}

/// 判定并改写上文的「提供方已下线该模型」。命中返回可直接展示的新文案，否则 `None`。
///
/// 目录里的 `status` 只在**等于** `"deprecated"` 时被 `free_models_in` 过滤掉（backend.rs:131），
/// 而 models.dev 的声明落后于提供方的实际撤架，于是这类模型仍会进入探测队列并在这里失败。
/// 判定要求「下线措辞」与「模型/提供方类主语」同时出现（不限次序，间隔 ≤60 字符），
/// 避免把与模型无关的弃用提示（例如某个内部字段 deprecated）说成下线。
pub fn deprecated_model_message(message: &str) -> Option<String> {
    const RETIRED: &str = r"deprecat(?:ed|ating|ion)";
    const SUBJECT: &str = r"(?:model|provider|endpoint|version)s?";
    let pattern = format!(r"(?is)({RETIRED}).{{0,60}}?({SUBJECT})|({SUBJECT}).{{0,60}}?({RETIRED})");
    let re = regex::Regex::new(&pattern).ok()?;
    if !re.is_match(message) {
        return None;
    }
    Some(format!(
        "提供方已下线该模型（原文：{message}）。这不是 Key 或本工具的问题：它不会被发布，也不会重复消耗探测额度。"
    ))
}

/// 判定并改写上文的地区拒绝文案。命中返回可直接展示的新文案，否则 `None`。
///
/// 判定要求「不可用/被拒」与「国家/地区」两类词**同时**出现（不限次序，间隔 ≤60 字符），
/// 单靠一个词会误伤：`429`、`invalid api key`、`context length exceeded` 都不该被说成地区问题。
pub fn region_unavailable_message(message: &str) -> Option<String> {
    // OpenAI 系提供方在 error.code 里给的是这个机器可读值，文案形态多变但这一串稳定。
    let known_code = message
        .to_ascii_lowercase()
        .contains("unsupported_country_region_territory");
    if !known_code {
        const REFUSED: &str =
            r"not\s+(?:available|allowed|supported|permitted)|unsupported|unavailable|restricted|blocked|denied";
        const GEO: &str = r"country|region|territory|location|geograph";
        let pattern = format!(r"(?is)({REFUSED}).{{0,60}}?({GEO})|({GEO}).{{0,60}}?({REFUSED})");
        let re = regex::Regex::new(&pattern).ok()?;
        if !re.is_match(message) {
            return None;
        }
    }
    Some(format!(
        "上游按出口 IP 判定该模型在你所在的国家/地区不可用（原文：{message}）。换出口才可能通过：可在侧栏「运行设置」打开「使用系统代理」，再重新检测。"
    ))
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
    fn a_model_without_function_call_support_degrades_to_chat_only() {
        // SiliconFlow 实测文案：目录声明 toolcall=true、平台实际不给调用，只能按纯对话发布。
        for message in [
            "Bad Request: Function call is not supported for this model.",
            "this model does not support tool calls",
            "Tool use is unsupported here",
            "function calls are not supported by the selected model",
        ] {
            let error = BridgeError::with(message, 400, "model_error");
            assert!(format_unsupported(&error), "{message}");
            // 降级不改变归类以外的任何东西：这句不该被当成抖动重试。
            assert!(!should_retry(&error.code), "{message}");
        }
    }

    #[test]
    fn an_unrelated_upstream_failure_is_never_read_as_a_function_call_gap() {
        for message in [
            "This model is not available in your country",
            "invalid api key",
            "429 Too Many Requests",
            "context length exceeded, try a shorter prompt",
            "模型只返回了文本，没有产生任何动作",
            "Chat-only model attempted native tool use; execution blocked",
            "tool_choice must be one of auto or none",
        ] {
            assert!(!tool_call_unsupported(message), "{message}");
        }
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

    /// 地区拒绝必须说清「谁拒的、为什么、怎么换出口」，而原来的 code/status 一位都不动 ——
    /// `upstream_error` 不在重试集合里，这一点决定了它不会被反复烧探测额度。
    #[test]
    fn probe_failure_turns_a_region_refusal_into_an_actionable_message() {
        let cause = BridgeError::with("This model is not available in your country", 403, "upstream_error");
        let message = cause.message.clone();
        let error = probe_failure(cause, false);
        assert_eq!(error.code, "upstream_error", "错误码不得被改写");
        assert_eq!(error.status, 403, "状态码不得被改写");
        assert!(error.message.contains("国家/地区"), "{}", error.message);
        assert!(
            error.message.contains("使用系统代理"),
            "必须给出唯一的可操作入口：{}",
            error.message
        );
        assert!(
            error.message.contains(&message),
            "上游原文要留在文案里便于对账：{}",
            error.message
        );
        assert!(!should_retry(&error.code), "地区拒绝不得触发重试");
    }

    /// 「被拒」与「地区」两类词必须同时在场才算地区拒绝，否则 429、坏 Key、超上下文都会被
    /// 说成地区问题，把用户支去做无用功。
    #[test]
    fn only_a_real_region_refusal_is_rewritten() {
        for message in [
            "This model is not available in your country",
            "requests from your region are blocked",
            "Claude is unavailable in your location",
            "Error code: unsupported_country_region_territory",
        ] {
            assert!(
                region_unavailable_message(message).is_some(),
                "应判为地区拒绝：{message}"
            );
        }
        for message in [
            "429 Too Many Requests",
            "invalid api key",
            "context length exceeded",
            "模型只返回了文本，没有产生任何动作",
            "The model is not available right now, please retry later",
            "This model supports only auto tool_choice",
        ] {
            assert!(
                region_unavailable_message(message).is_none(),
                "不该被判为地区拒绝：{message}"
            );
        }
    }

    #[test]
    fn a_retired_upstream_model_gets_a_readable_reason_without_changing_its_verdict() {
        for message in [
            "Model exo-free has been deprecated.",
            "this model is deprecated and no longer served",
            "The provider deprecated this endpoint",
        ] {
            assert!(
                deprecated_model_message(message).is_some(),
                "应判为提供方已下线：{message}"
            );
        }
        // 只改文案：code/status 原样保留，且这类失败本就不重试、不发布。
        let original = BridgeError::with("Model exo-free has been deprecated.", 400, "model_error");
        let rewritten = probe_failure(original.clone(), false);
        assert!(rewritten.message.contains("提供方已下线该模型"), "{}", rewritten.message);
        assert_eq!(rewritten.code, original.code);
        assert_eq!(rewritten.status, original.status);
        assert!(!should_retry(&rewritten.code));
    }

    #[test]
    fn an_unrelated_deprecation_note_is_not_read_as_a_retired_model() {
        for message in [
            "429 Too Many Requests",
            "invalid api key",
            "context length exceeded",
            "模型只返回了文本，没有产生任何动作",
            "This model is not available in your country",
            "This config field is deprecated, use the new one",
            "Bad Request: Function call is not supported for this model.",
        ] {
            assert!(
                deprecated_model_message(message).is_none(),
                "不该被判为模型已下线：{message}"
            );
        }
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
