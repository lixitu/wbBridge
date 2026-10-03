//! `core/src/model-status.js` 的 Rust 等价实现：失败归类、请求观测元信息、客户端模型 ID。
//!
//! 原注释：Provider 不暴露可靠的剩余额度 API，因此只能按错误文本与状态码归类；
//! `withRequestMeta` 记录的是「本次工作项上观察到什么」（工具调用数、被拦截的原生尝试、
//! 步数、已移交的动作），是观测而不是判定——能力标签只来自探测。

use crate::json::{display, is_integer, truthy};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// 一条模型/请求结果的归类。
pub const CATEGORY_AVAILABLE: &str = "available";
pub const CATEGORY_QUOTA: &str = "quota";
pub const CATEGORY_RATE_LIMIT: &str = "rate_limit";
pub const CATEGORY_ACCESS: &str = "access";
pub const CATEGORY_TIMEOUT: &str = "timeout";
pub const CATEGORY_ERROR: &str = "error";

fn quota_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)insufficient[_ ]quota|quota.{0,30}(exceed|exhaust|deplet)|out of credits|insufficient.{0,20}(credit|balance)|额度.{0,10}(不足|用尽)",
        )
        .expect("quota 正则必须是合法表达式")
    })
}

fn rate_limit_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)rate.?limit|too many requests").expect("rate limit 正则必须是合法表达式")
    })
}

fn timeout_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"(?i)timeout|timed out").expect("timeout 正则必须是合法表达式"))
}

/// `modelResult(ok, message, status, code)`：归类失败原因，并注入当前时间。
pub fn model_result(ok: bool, message: &str, status: Option<&Value>, code: Option<&str>) -> Value {
    model_result_at(&now_iso8601(), ok, message, status, code)
}

/// 与 [`model_result`] 相同，但时间戳由调用方注入（便于测试与对拍）。
pub fn model_result_at(
    time: &str,
    ok: bool,
    message: &str,
    status: Option<&Value>,
    code: Option<&str>,
) -> Value {
    let mut category = CATEGORY_AVAILABLE;
    if !ok {
        if quota_pattern().is_match(message) {
            category = CATEGORY_QUOTA;
        } else if status.and_then(|value| value.as_i64()) == Some(429) || rate_limit_pattern().is_match(message) {
            category = CATEGORY_RATE_LIMIT;
        } else if matches!(status.and_then(|value| value.as_i64()), Some(401) | Some(403)) {
            category = CATEGORY_ACCESS;
        } else if timeout_pattern().is_match(message) {
            category = CATEGORY_TIMEOUT;
        } else {
            category = CATEGORY_ERROR;
        }
    }

    let mut result = Map::new();
    result.insert("ok".to_string(), Value::Bool(ok));
    result.insert("category".to_string(), Value::String(category.to_string()));
    result.insert("time".to_string(), Value::String(time.to_string()));
    if !message.is_empty() {
        result.insert("error".to_string(), Value::String(message.to_string()));
    }
    if truthy(status) {
        result.insert("status".to_string(), status.cloned().unwrap_or(Value::Null));
    }
    if truthy(code.map(Value::from).as_ref()) {
        result.insert("code".to_string(), Value::String(code.unwrap_or_default().to_string()));
    }
    Value::Object(result)
}

/// `withRequestMeta(result, meta)`：把本次请求的观测值回填到结果对象上。
pub fn with_request_meta(result: &mut Value, meta: &Value) {
    if !meta.is_object() || !result.is_object() {
        return;
    }
    let Some(result) = result.as_object_mut() else {
        return;
    };
    for key in ["calls", "nativeAttempts", "steps"] {
        if is_integer(meta.get(key)) {
            result.insert(key.to_string(), meta.get(key).cloned().unwrap_or(Value::Null));
        }
    }
    if let Some(Value::String(handoff)) = meta.get("handoff") {
        result.insert("handoff".to_string(), Value::String(handoff.clone()));
    }
    if truthy(meta.get("handoffCheck")) {
        result.insert(
            "handoffCheck".to_string(),
            meta.get("handoffCheck").cloned().unwrap_or(Value::Null),
        );
    }
    if truthy(meta.get("repaired")) {
        result.insert(
            "repaired".to_string(),
            meta.get("repaired").cloned().unwrap_or(Value::Null),
        );
    }
}

/// `clientModelID(model)`：给 WorkBuddy 展示的模型 ID。
///
/// 前缀不再是写死的 `OC`：命名空间取自 `free_models_in` 生成的全限定 id（见 `split_namespace`）。
/// OpenCode 继续用 `OC · `，所以存量 client id 一个字节都不变；无 `id` 或无命名空间的模型
/// 也走这条回落（对拍夹具里的入参正是这种形态）。
pub fn client_model_id(model: &Value) -> String {
    format!("{} · {}", client_prefix(model), display(model.get("name")))
}

/// 免费模型的上游命名空间：OpenCode 是本工具的老住户，也是 id 缺失时的回落值。
pub const OPENCODE_NAMESPACE: &str = "opencode";

/// client id 的展示前缀里，OpenCode 沿用历史的 `OC`（改名等于把所有已发布模型的 ID 换掉）。
const OPENCODE_CLIENT_PREFIX: &str = "OC";

/// `{namespace}/{key}`：目录里全限定模型 id 的唯一拼法。
pub fn join_namespace(namespace: &str, key: &str) -> String {
    format!("{namespace}/{key}")
}

/// `split_namespace`：按**第一个** `/` 拆开全限定 id，返回 `(命名空间, 上游模型 key)`。
/// 无 `/`（含空串）时命名空间为空串，由调用侧决定回落；key 里再出现的 `/` 原样保留，
/// 与旧的「截掉 `opencode/` 定长前缀」逐字节等价。
pub fn split_namespace(id: &str) -> (&str, &str) {
    match id.split_once('/') {
        Some((namespace, model_id)) => (namespace, model_id),
        None => ("", id),
    }
}

/// 展示前缀：**只有**注册表里的平台换用其 `label`，其余一律 `OC`。
///
/// 这不是偷懒：`opencode` 与「无命名空间」之外的命名空间在今天的调用面里根本不可能出现
/// （`free_models_in` 只以 `OPENCODE_NAMESPACE` 或注册表 id 被调用，见 Stage 3），而测试与
/// 对拍夹具里确实存在 `vendor/gpt` 这类合成 id —— 让它们改前缀就是行为变化。
fn client_prefix(model: &Value) -> &str {
    let (namespace, _) = split_namespace(
        model.get("id").and_then(Value::as_str).unwrap_or_default(),
    );
    crate::providers::find(namespace)
        .map(|provider| provider.label)
        .unwrap_or(OPENCODE_CLIENT_PREFIX)
}

const MILLIS_PER_SECOND: u128 = 1_000;

/// 当前时间的 ISO-8601 表示（对应 `new Date().toISOString()`）。
pub fn now_iso8601() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    iso8601_from_millis(elapsed.as_millis() as i64)
}

/// 由 Unix 毫秒时间戳生成 `YYYY-MM-DDTHH:MM:SS.mmmZ`。
pub fn iso8601_from_millis(millis: i64) -> String {
    let seconds = millis.div_euclid(MILLIS_PER_SECOND as i64);
    let millis_part = millis.rem_euclid(MILLIS_PER_SECOND as i64);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis_part:03}Z"
    )
}

/// Howard Hinnant 的 civil_from_days：把「1970-01-01 起的天数」还原成公历日期。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// 便捷函数：把 `&str` 转成用于 `status` 字段的 JSON 数字。
pub fn status_value(status: i64) -> Value {
    json!(status)
}

/// 便捷函数：构造一个 `String` 值。
pub fn text<T: Into<String>>(value: T) -> Value {
    Value::String(value.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn failures_are_categorised_like_javascript() {
        let status = json!(429);
        let quota = model_result_at("<t>", false, "Insufficient quota for this model", None, None);
        assert_eq!(quota["category"], json!("quota"));
        let rate = model_result_at("<t>", false, "", Some(&status), None);
        assert_eq!(rate["category"], json!("rate_limit"));
        let access = model_result_at("<t>", false, "nope", Some(&json!(401)), None);
        assert_eq!(access["category"], json!("access"));
        let timeout = model_result_at("<t>", false, "upstream timed out", None, None);
        assert_eq!(timeout["category"], json!("timeout"));
        let generic = model_result_at("<t>", false, "boom", None, None);
        assert_eq!(generic["category"], json!("error"));
        let ok = model_result_at("<t>", true, "", None, None);
        assert_eq!(ok["category"], json!("available"));
    }

    #[test]
    fn chinese_quota_phrasing_is_recognised() {
        let result = model_result_at("<t>", false, "当前账户额度已不足，请充值", None, None);
        assert_eq!(result["category"], json!("quota"));
    }

    #[test]
    fn optional_fields_follow_truthiness() {
        let bare = model_result_at("<t>", false, "", None, None);
        assert_eq!(bare, json!({ "ok": false, "category": "error", "time": "<t>" }));
        let with_extra = model_result_at("<t>", false, "boom", Some(&json!(500)), Some("E_NET"));
        assert_eq!(with_extra["status"], json!(500));
        assert_eq!(with_extra["code"], json!("E_NET"));
        assert_eq!(with_extra["error"], json!("boom"));
    }

    #[test]
    fn request_meta_only_copies_documented_shapes() {
        let mut result = json!({ "ok": true });
        with_request_meta(
            &mut result,
            &json!({
                "calls": 2,
                "nativeAttempts": 1.5,
                "steps": 3,
                "handoff": "bash",
                "handoffCheck": { "ok": false },
                "repaired": { "envelope": { "ok": true } },
                "unknown": "ignored"
            }),
        );
        assert_eq!(result["calls"], json!(2));
        assert!(result.get("nativeAttempts").is_none(), "1.5 不是整数，不应回填");
        assert_eq!(result["steps"], json!(3));
        assert_eq!(result["handoff"], json!("bash"));
        assert_eq!(result["handoffCheck"], json!({ "ok": false }));
        assert!(result.get("unknown").is_none());
    }

    #[test]
    fn request_meta_ignores_non_objects() {
        let mut result = json!({ "ok": true });
        with_request_meta(&mut result, &json!("nope"));
        with_request_meta(&mut result, &json!(null));
        assert_eq!(result, json!({ "ok": true }));
    }

    #[test]
    fn client_model_id_uses_the_middle_dot_prefix() {
        assert_eq!(client_model_id(&json!({ "name": "GPT-5" })), "OC · GPT-5");
        assert_eq!(client_model_id(&json!({})), "OC · undefined");
    }

    /// Stage 2 的零回归承诺：命名空间参数化之后，除注册表平台之外的输出必须与写死 `OC · ` 时
    /// 逐字节一致（含 `vendor/...` 这类测试用的合成命名空间）。
    #[test]
    fn client_prefix_follows_the_namespace_but_never_renames_opencode() {
        assert_eq!(
            client_model_id(&json!({ "id": "opencode/big-pickle", "name": "Big Pickle" })),
            "OC · Big Pickle"
        );
        assert_eq!(
            client_model_id(&json!({ "id": "zhipuai/glm-4.5-air", "name": "GLM" })),
            "智谱 · GLM"
        );
        assert_eq!(
            client_model_id(&json!({ "id": "modelscope/foo", "name": "Foo" })),
            "ModelScope · Foo"
        );
        // 合成 / 未知命名空间不得改前缀：`sync.rs` 的既有测试与对拍夹具依赖这一点。
        assert_eq!(
            client_model_id(&json!({ "id": "vendor/gpt", "name": "gpt" })),
            "OC · gpt"
        );
        // 无斜杠 / 空 id / 非字符串 id 都回落到 OC，与参数化之前一致。
        assert_eq!(client_model_id(&json!({ "id": "bare-key", "name": "B" })), "OC · B");
        assert_eq!(client_model_id(&json!({ "id": "", "name": "E" })), "OC · E");
        assert_eq!(client_model_id(&json!({ "id": 7, "name": "N" })), "OC · N");
    }

    /// `free_models_in` 是这些 id 的唯一生产者；拆合必须无损，且对 `opencode/` 前缀与旧的
    /// 「截掉定长 9 字节」写法逐个同值（旧写法是 `id.get("opencode/".len()..)`）。
    #[test]
    fn namespace_split_and_join_round_trip_and_match_the_old_fixed_length_strip() {
        // 旧写法：`id.get("opencode/".len()..)`，即按字节截掉定长前缀。
        fn legacy(id: &str) -> &str {
            id.get(OPENCODE_NAMESPACE.len() + 1..).unwrap_or_default()
        }
        let ids = [
            "opencode/big-pickle",
            "opencode/nemotron-3.5-lightning-free",
            "opencode/space-bunny-free",
            "opencode/mimo-v2.6-flash-free",
            "opencode/mock",
            // key 自带斜杠时两种写法也必须同值。
            "opencode/a/b",
        ];
        for id in ids {
            let (namespace, model_id) = split_namespace(id);
            assert_eq!(namespace, OPENCODE_NAMESPACE);
            assert_eq!(model_id, legacy(id), "{id} 的拆分与旧截串不同值");
            assert_eq!(join_namespace(namespace, model_id), id);
        }
        assert_eq!(split_namespace("zhipuai/glm"), ("zhipuai", "glm"));
        assert_eq!(split_namespace("no-slash"), ("", "no-slash"));
        assert_eq!(split_namespace(""), ("", ""));
    }

    #[test]
    fn iso8601_matches_known_instants() {
        assert_eq!(iso8601_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601_from_millis(1_000), "1970-01-01T00:00:01.000Z");
        // 2026-09-30T00:00:00Z
        assert_eq!(iso8601_from_millis(1_790_726_400_000), "2026-09-30T00:00:00.000Z");
        assert_eq!(iso8601_from_millis(1_790_726_400_123), "2026-09-30T00:00:00.123Z");
    }
}
