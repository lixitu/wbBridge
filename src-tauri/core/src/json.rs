//! `core/src/json.js` 的 Rust 等价实现，外加一批「让 Rust 逻辑与 JS 语义逐条对齐」的辅助函数。
//!
//! JS 里大量使用真值判断、`??`、`typeof`、`String()` 这类隐式语义，移植时如果直接用 Rust 的
//! `Option`/`bool` 推断，会在边界输入上与原实现分叉。这里把它们集中实现，供其他模块复用。

use serde_json::Value;
use std::collections::HashMap;

/// 环境变量的最小抽象，对应 JS 中以对象形式使用的 `process.env`。
pub type Env = HashMap<String, String>;

/// JS `String.prototype.trim()`：去掉首尾空白。
///
/// Rust 的 `char::is_whitespace()` 不把 BOM（U+FEFF）视为空白，而 ECMAScript 的
/// `WhiteSpace` 包含它，所以这里显式补上，避免 `env.X?.trim()` 这类调用出现分歧。
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// `core/src/json.js`：剥掉 UTF-8 BOM 后再解析 JSON。
pub fn parse_json(text: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text))
}

/// JS 真值判断：`undefined` / `null` / `false` / `0` / `NaN` / `""` 为假，其余为真。
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(inner)) => *inner,
        Some(Value::Number(number)) => number.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(text)) => !text.is_empty(),
        _ => true,
    }
}

/// JS `String(value)`：`undefined` → `"undefined"`，`null` → `"null"`。
pub fn display(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(inner)) => inner.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => display(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

/// JS `typeof`：注意 `null` 与数组都是 `"object"`。
pub fn type_of(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "object",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) | Some(Value::Object(_)) => "object",
    }
}

/// JS `a === b`。缺失的键等同于 `undefined`，因此「两边都缺」判定为相等。
pub fn strict_eq(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// JS `??`（空值合并）：左侧缺失或为 `null` 时取右侧。
pub fn coalesce<'a>(left: Option<&'a Value>, right: Option<&'a Value>) -> Option<&'a Value> {
    match left {
        Some(value) if !value.is_null() => Some(value),
        _ => right,
    }
}

/// JS `value == null`：缺失或 `null`。
pub fn is_nullish(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

/// JS `Object.keys(value).length`：字符串按码点计数（近似 JS 的 UTF-16 长度），其余标量视为 0。
pub fn key_count(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Object(map)) => map.len(),
        Some(Value::Array(items)) => items.len(),
        Some(Value::String(text)) => text.chars().count(),
        _ => 0,
    }
}

/// JS `Number.isInteger(value)`。
pub fn is_integer(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Number(number)) => match number.as_f64() {
            Some(f) => f.is_finite() && f.fract() == 0.0,
            None => false,
        },
        _ => false,
    }
}

/// `JSON.stringify` 的便捷封装：序列化失败时退化为 `"null"`。
pub fn stringify(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// `JSON.stringify(value, null, 2)` 的等价实现（阶段二 `sync.js` 写 `models.json` 用）。
///
/// 与 [`js_stringify`] 同样先做数字规范化，区别只在于缩进：`sync.js` 落盘的是两空格缩进的
/// 文本（外加一个换行），对拍时会逐字节比较文件内容，所以缩进与数字格式都必须一致。
pub fn js_stringify_pretty(value: &Value) -> String {
    let normalized = normalize_numbers(value);
    let mut buffer: Vec<u8> = Vec::new();
    match serde_json::to_writer_pretty(&mut buffer, &normalized) {
        Ok(()) => String::from_utf8(buffer).unwrap_or_else(|_| "null".to_string()),
        Err(_) => "null".to_string(),
    }
}

/// 递归把「值恰好为整数的浮点」规范成整数（`3.0` → `3`），其余保持原样。
fn normalize_numbers(value: &Value) -> Value {
    match value {
        Value::Number(number) => {
            if number.is_f64() {
                if let Some(f) = number.as_f64() {
                    if f.is_finite() && f.fract() == 0.0 {
                        return number_from_f64(f);
                    }
                }
            }
            value.clone()
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize_numbers).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), normalize_numbers(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `JSON.stringify` 的等价实现：先按 JS 的数字格式递归规范化，再序列化。
///
/// `serde_json` 会把 `3.0` 原样序列化成 `3.0`，而 JS 的 `JSON.stringify(3.0)` 是 `3`。
/// 凡是「JS 侧调用 `JSON.stringify`」的地方（对话正文、工具参数、SSE 帧、修复材料）都必须走这里，
/// 否则 JS↔Rust 对拍会出现纯格式差异。
///
/// 已知边界：超过 2^53 的浮点与 `1e21` 这类极端指数的字符串形式两边仍可能不同（JS 会写成 `1e+21`），
/// 该区间不在本项目的数据范围内。
pub fn js_stringify(value: &Value) -> String {
    stringify(&normalize_numbers(value))
}

/// 把 JS 侧常见的数值（可能来自加法）规范成不带小数的 JSON 数字。
///
/// `serde_json` 的 `f64` 会序列化成 `5.0`，而 JS 的 `JSON.stringify(5)` 是 `5`；
/// 数值为整数时统一转成 `i64`，避免对拍时出现纯格式差异。
pub fn number_from_f64(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_json_strips_only_a_leading_bom() {
        assert_eq!(parse_json("\u{feff}[]").unwrap(), json!([]));
        assert!(parse_json("\u{feff}\u{feff}[]").is_err());
        assert_eq!(parse_json("[]").unwrap(), json!([]));
        assert!(parse_json("\u{feff}{\"a\":1}").is_ok());
    }

    #[test]
    fn truthy_follows_javascript() {
        assert!(!truthy(None));
        assert!(!truthy(Some(&json!(null))));
        assert!(!truthy(Some(&json!(false))));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(Some(&json!(""))));
        assert!(truthy(Some(&json!(true))));
        assert!(truthy(Some(&json!(1))));
        assert!(truthy(Some(&json!("x"))));
        assert!(truthy(Some(&json!({}))));
        assert!(truthy(Some(&json!([]))));
    }

    #[test]
    fn display_and_type_of_match_javascript() {
        assert_eq!(display(None), "undefined");
        assert_eq!(display(Some(&json!(null))), "null");
        assert_eq!(display(Some(&json!(true))), "true");
        assert_eq!(display(Some(&json!(12))), "12");
        assert_eq!(display(Some(&json!("OC"))), "OC");
        assert_eq!(display(Some(&json!({ "a": 1 }))), "[object Object]");
        assert_eq!(type_of(Some(&json!(null))), "object");
        assert_eq!(type_of(Some(&json!([]))), "object");
        assert_eq!(type_of(None), "undefined");
        assert_eq!(type_of(Some(&json!("s"))), "string");
    }

    #[test]
    fn strict_eq_treats_missing_keys_as_undefined() {
        assert!(strict_eq(None, None));
        assert!(!strict_eq(None, Some(&json!(null))));
        assert!(strict_eq(Some(&json!("a")), Some(&json!("a"))));
        assert!(!strict_eq(Some(&json!(1)), Some(&json!("1"))));
    }

    #[test]
    fn coalesce_skips_null_and_missing() {
        let empty = json!({});
        let fallback = json!("fallback");
        assert_eq!(coalesce(empty.get("missing"), Some(&fallback)), Some(&fallback));
        assert_eq!(coalesce(Some(&json!(null)), Some(&fallback)), Some(&fallback));
        assert_eq!(coalesce(Some(&json!(0)), Some(&fallback)), Some(&json!(0)));
    }

    #[test]
    fn js_trim_removes_bom_and_whitespace() {
        assert_eq!(js_trim("\u{feff}  a  "), "a");
        assert_eq!(js_trim("   "), "");
    }

    #[test]
    fn number_from_f64_avoids_trailing_zero() {
        assert_eq!(stringify(&number_from_f64(5.0)), "5");
        assert_eq!(stringify(&number_from_f64(5.5)), "5.5");
    }

    #[test]
    fn js_stringify_normalizes_integral_floats_recursively() {
        let value = serde_json::from_str::<Value>(r#"{"a":3.0,"b":[1.0,2.5],"c":{"d":-0.0}}"#).unwrap();
        assert_eq!(js_stringify(&value), r#"{"a":3,"b":[1,2.5],"c":{"d":0}}"#);
        // 非浮点数字不受影响：u64 极值不会被折成 f64。
        let big = serde_json::from_str::<Value>(r#"{"n":18446744073709551615}"#).unwrap();
        assert_eq!(js_stringify(&big), r#"{"n":18446744073709551615}"#);
        assert_eq!(js_stringify(&json!("普通字符串")), "\"普通字符串\"");
    }
}
