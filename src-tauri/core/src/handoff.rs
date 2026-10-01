//! `core/src/handoff.js` 的 Rust 等价实现：原生工具调用 → 外部工具 schema 的映射与校验。
//!
//! 原注释要点：
//! - 外部客户端拥有执行权。模型试图本地执行时，把被拦截的动作作为外部调用移交，
//!   而不是让模型复述——复述恰好是模型最容易失败的步骤，且要多花一整轮上游开销。
//! - **WorkBuddy 随每个请求发来的工具列表才是权威**。只填写目标 schema 里存在的键，
//!   且每个 required 键都必须可满足，否则不产生移交，退回纠正式拒绝。

use crate::json::{display, truthy};
use crate::protocol::BridgeError;
use serde_json::{json, Map, Value};

/// 一个原生字段到目标工具字段名的映射（可有多个可接受拼写）。
struct FieldMap {
    key: &'static str,
    targets: &'static [&'static str],
}

/// 一类原生工具到外部工具的映射表。
struct Category {
    native: &'static [&'static str],
    targets: &'static [&'static str],
    fields: &'static [FieldMap],
}

/// 与 JS 版 `CATEGORIES` 逐条对应的映射表。
const CATEGORIES: &[Category] = &[
    Category {
        native: &["bash", "shell"],
        targets: &["Bash", "PowerShell"],
        fields: &[
            FieldMap { key: "command", targets: &["command"] },
            FieldMap { key: "description", targets: &["description"] },
            FieldMap { key: "timeout", targets: &["timeout"] },
        ],
    },
    Category {
        native: &["read"],
        targets: &["Read"],
        fields: &[
            FieldMap { key: "filePath", targets: &["file_path"] },
            FieldMap { key: "file_path", targets: &["file_path"] },
            FieldMap { key: "path", targets: &["file_path"] },
            FieldMap { key: "offset", targets: &["offset"] },
            FieldMap { key: "limit", targets: &["limit"] },
        ],
    },
    Category {
        native: &["write"],
        targets: &["Write"],
        fields: &[
            FieldMap { key: "filePath", targets: &["file_path"] },
            FieldMap { key: "file_path", targets: &["file_path"] },
            FieldMap { key: "path", targets: &["file_path"] },
            FieldMap { key: "content", targets: &["content"] },
        ],
    },
    Category {
        native: &["edit", "multiedit", "multi_edit", "patch", "apply_patch"],
        targets: &["Edit", "MultiEdit"],
        fields: &[
            FieldMap { key: "filePath", targets: &["file_path"] },
            FieldMap { key: "file_path", targets: &["file_path"] },
            FieldMap { key: "path", targets: &["file_path"] },
            FieldMap { key: "oldString", targets: &["old_string"] },
            FieldMap { key: "old_string", targets: &["old_string"] },
            FieldMap { key: "newString", targets: &["new_string"] },
            FieldMap { key: "new_string", targets: &["new_string"] },
            FieldMap { key: "replaceAll", targets: &["replace_all"] },
            FieldMap { key: "replace_all", targets: &["replace_all"] },
            FieldMap { key: "edits", targets: &["edits"] },
        ],
    },
    Category {
        native: &["glob"],
        targets: &["Glob", "LS"],
        fields: &[
            FieldMap { key: "pattern", targets: &["pattern"] },
            FieldMap { key: "path", targets: &["path", "directory"] },
            FieldMap { key: "include", targets: &["include"] },
        ],
    },
    Category {
        native: &["grep"],
        targets: &["Grep", "Search"],
        fields: &[
            FieldMap { key: "pattern", targets: &["pattern"] },
            FieldMap { key: "path", targets: &["path", "directory"] },
            FieldMap { key: "include", targets: &["include"] },
            FieldMap { key: "output_mode", targets: &["output_mode"] },
        ],
    },
    Category {
        native: &["skill"],
        targets: &["Skill"],
        fields: &[
            FieldMap { key: "name", targets: &["name", "skill"] },
            FieldMap { key: "skill", targets: &["skill", "name"] },
            FieldMap { key: "args", targets: &["args", "arguments"] },
            FieldMap { key: "arguments", targets: &["arguments", "args"] },
        ],
    },
];

/// 取工具的 `function` 子对象；同时要求 `name` 是小写后等于 `target` 的字符串。
fn find_tool_spec<'a>(tools: &'a [Value], target: &str) -> Option<&'a Value> {
    tools
        .iter()
        .filter_map(|tool| tool.get("function"))
        .find(|function| match function.get("name").and_then(|name| name.as_str()) {
            Some(name) => name.to_lowercase() == target.to_lowercase(),
            None => false,
        })
}

/// `buildHandoff({ native, input, tools })`。
pub fn build_handoff(native: Option<&Value>, input: &Value, tools: &[Value]) -> Option<Value> {
    let name = match native {
        None | Some(Value::Null) => String::new(),
        Some(value) => display(Some(value)),
    }
    .to_lowercase();

    let category = CATEGORIES
        .iter()
        .find(|category| category.native.contains(&name.as_str()))?;

    for target in category.targets {
        let Some(spec) = find_tool_spec(tools, target) else {
            continue;
        };
        let Some(Value::Object(properties)) = spec.get("parameters").and_then(|parameters| parameters.get("properties"))
        else {
            continue;
        };
        let Some(input) = input.as_object() else {
            continue;
        };

        let mut arguments = Map::new();
        for (key, value) in input {
            let Some(candidates) = category
                .fields
                .iter()
                .find(|field| field.key == key.as_str())
                .map(|field| field.targets)
            else {
                continue;
            };
            for mapped in candidates {
                if arguments.contains_key(*mapped) || !properties.contains_key(*mapped) {
                    continue;
                }
                arguments.insert((*mapped).to_string(), value.clone());
                break;
            }
        }

        if arguments.is_empty() {
            continue;
        }
        let required = spec
            .get("parameters")
            .and_then(|parameters| parameters.get("required"))
            .and_then(|required| required.as_array())
            .cloned()
            .unwrap_or_default();
        let satisfiable = required.iter().all(|key| {
            let value = key.as_str().and_then(|key| arguments.get(key));
            match value {
                Some(Value::Null) | None => false,
                Some(Value::String(text)) => !text.is_empty(),
                Some(_) => true,
            }
        });
        if !satisfiable {
            continue;
        }
        return Some(json!({
            "name": spec.get("name").cloned().unwrap_or(Value::Null),
            "arguments": Value::Object(arguments),
        }));
    }
    None
}

/// `handoffInput(action, permission)`：合并工具调用参数与审批元数据里的参数。
///
/// 调用还在等待审批时，OpenCode 通过审批元数据而不是 tool part 报告参数（后者在调用真正
/// 运行前输入为空）。tool part 有参数时以它为准，元数据只补齐缺失的部分。
pub fn handoff_input(action: &Value, permission: &Value) -> Value {
    let mut input = match action.get("input") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    let metadata = permission.get("metadata").and_then(|value| value.as_object());

    let mut fallback: Vec<(&str, Option<&Value>)> = Vec::new();
    fallback.push(("command", metadata.and_then(|map| map.get("command"))));

    let file_path = ["filepath", "filePath", "path"]
        .iter()
        .find_map(|key| metadata.and_then(|map| map.get(*key)).filter(|value| !value.is_null()));
    fallback.push(("filePath", file_path));

    let pattern = metadata
        .and_then(|map| map.get("pattern"))
        .filter(|value| !value.is_null())
        .or_else(|| {
            metadata
                .and_then(|map| map.get("patterns"))
                .and_then(|patterns| patterns.as_array())
                .and_then(|patterns| patterns.first())
        });
    fallback.push(("pattern", pattern));

    for (key, value) in fallback {
        if input.contains_key(key) {
            continue;
        }
        if let Some(Value::String(text)) = value {
            if !crate::json::js_trim(text).is_empty() {
                input.insert(key.to_string(), value.cloned().unwrap_or(Value::Null));
            }
        }
    }
    Value::Object(input)
}

/// `rejectFeedback(native, reason)`：拒绝必须点名工具与原因。
pub fn reject_feedback(native: Option<&Value>, reason: &str) -> String {
    let native_text = match native {
        None | Some(Value::Null) => String::new(),
        Some(value) => display(Some(value)),
    };
    format!(
        "Native tool \"{native_text}\" was blocked: {reason}. Native execution is forbidden; the external client owns execution. \
Return the requested external action inside the calls array using StructuredOutput. The external client will execute it and supply results. Do not call any other native tools."
    )
}

/// `validateAction(candidate, tools)`：用接收方 schema 校验翻译后的动作。
///
/// 与原实现同规则，因此翻译永远不能放宽「可以被执行」的范围。
pub fn validate_action(candidate: &Value, tools: &[Value]) -> Result<Value, BridgeError> {
    if !candidate.is_object() {
        return Err(BridgeError::with(
            "Translated action is not an object",
            502,
            "invalid_tool_call",
        ));
    }
    let name = candidate.get("name");
    let spec = tools
        .iter()
        .filter_map(|tool| tool.get("function"))
        .find(|function| match function.get("name").and_then(|name| name.as_str()) {
            Some(tool_name) => {
                truthy(Some(function.get("name").unwrap_or(&Value::Null)))
                    && tool_name.to_lowercase() == display(name).to_lowercase()
            }
            None => false,
        });
    let Some(spec) = spec else {
        return Err(BridgeError::with(
            "Translated action names a tool the receiver did not offer",
            502,
            "invalid_tool_call",
        ));
    };

    let arguments = match candidate.get("arguments") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(value) => value.clone(),
    };
    let Some(arguments_map) = arguments.as_object() else {
        return Err(BridgeError::with(
            "Translated action arguments are not an object",
            502,
            "invalid_tool_call",
        ));
    };

    let properties = spec
        .get("parameters")
        .and_then(|parameters| parameters.get("properties"))
        .and_then(|properties| properties.as_object());
    for key in arguments_map.keys() {
        let declared = properties.map(|properties| properties.contains_key(key)).unwrap_or(false);
        if !declared {
            return Err(BridgeError::with(
                format!("Translated action sets an argument the receiver does not declare: {key}"),
                502,
                "invalid_tool_call",
            ));
        }
    }

    let required = spec
        .get("parameters")
        .and_then(|parameters| parameters.get("required"))
        .and_then(|required| required.as_array())
        .cloned()
        .unwrap_or_default();
    for key in &required {
        let value = key.as_str().and_then(|key| arguments_map.get(key));
        let satisfied = match value {
            Some(Value::Null) | None => false,
            Some(Value::String(text)) => !text.is_empty(),
            Some(_) => true,
        };
        if !satisfied {
            return Err(BridgeError::with(
                "Translated action misses a required argument",
                502,
                "invalid_tool_call",
            ));
        }
    }

    Ok(json!({
        "name": spec.get("name").cloned().unwrap_or(Value::Null),
        "arguments": arguments,
    }))
}

/// 内部使用：原生工具名是否落入映射表（用于面板统计）。
pub fn has_category(native: &str) -> bool {
    let lowered = native.to_lowercase();
    CATEGORIES.iter().any(|category| category.native.contains(&lowered.as_str()))
}

/// 内部辅助（仅测试用）：`strict_eq` 的再导出，避免测试重复 import 路径。
#[cfg(test)]
pub(crate) fn same_name(left: Option<&Value>, right: Option<&Value>) -> bool {
    crate::json::strict_eq(left, right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read_tool() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "Read",
                "parameters": {
                    "type": "object",
                    "properties": { "file_path": { "type": "string" }, "offset": { "type": "number" } },
                    "required": ["file_path"]
                }
            }
        })
    }

    fn write_tool() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "Write",
                "parameters": {
                    "type": "object",
                    "properties": { "file_path": { "type": "string" }, "content": { "type": "string" } },
                    "required": ["file_path", "content"]
                }
            }
        })
    }

    #[test]
    fn maps_native_read_to_external_read() {
        let handoff = build_handoff(
            Some(&json!("read")),
            &json!({ "filePath": "/tmp/a.txt", "offset": 5, "unknown": 1 }),
            &[read_tool()],
        )
        .unwrap();
        assert_eq!(handoff["name"], json!("Read"));
        assert_eq!(handoff["arguments"], json!({ "file_path": "/tmp/a.txt", "offset": 5 }));
    }

    #[test]
    fn unknown_native_tool_produces_no_handoff() {
        assert!(build_handoff(Some(&json!("webfetch")), &json!({ "url": "x" }), &[read_tool()]).is_none());
        assert!(build_handoff(None, &json!({}), &[read_tool()]).is_none());
    }

    #[test]
    fn a_missing_required_argument_blocks_the_handoff() {
        let handoff = build_handoff(
            Some(&json!("write")),
            &json!({ "filePath": "/tmp/a.txt" }),
            &[write_tool()],
        );
        assert!(handoff.is_none(), "缺少 content 时不应产生移交");
    }

    #[test]
    fn the_first_matching_target_wins() {
        let bash = json!({
            "type": "function",
            "function": {
                "name": "Bash",
                "parameters": { "properties": { "command": {} }, "required": ["command"] }
            }
        });
        let powershell = json!({
            "type": "function",
            "function": {
                "name": "PowerShell",
                "parameters": { "properties": { "command": {} }, "required": ["command"] }
            }
        });
        let handoff = build_handoff(
            Some(&json!("shell")),
            &json!({ "command": "ls" }),
            &[powershell, bash],
        )
        .unwrap();
        assert_eq!(
            handoff["name"],
            json!("Bash"),
            "按目标声明顺序取第一个可满足者，与传入工具列表的顺序无关"
        );
    }

    #[test]
    fn the_first_declared_spelling_of_a_multi_target_field_wins() {
        let tool = json!({
            "type": "function",
            "function": {
                "name": "Glob",
                "parameters": { "properties": { "directory": {} }, "required": ["directory"] }
            }
        });
        let handoff = build_handoff(Some(&json!("glob")), &json!({ "path": "/tmp" }), &[tool]).unwrap();
        assert_eq!(handoff["arguments"], json!({ "directory": "/tmp" }));
    }

    #[test]
    fn handoff_input_prefers_the_tool_part_and_fills_the_rest_from_metadata() {
        let merged = handoff_input(
            &json!({ "input": { "command": "ls" } }),
            &json!({ "metadata": { "filepath": "/tmp/a.txt", "pattern": "*.rs", "unknown": "x" } }),
        );
        assert_eq!(
            merged,
            json!({ "command": "ls", "filePath": "/tmp/a.txt", "pattern": "*.rs" })
        );
    }

    #[test]
    fn handoff_input_ignores_blank_and_non_string_metadata() {
        let merged = handoff_input(
            &json!({ "input": {} }),
            &json!({ "metadata": { "command": "   ", "filePath": "/tmp/b.txt", "patterns": ["p1", "p2"] } }),
        );
        assert_eq!(merged, json!({ "filePath": "/tmp/b.txt", "pattern": "p1" }));
    }

    #[test]
    fn reject_feedback_names_the_tool_and_the_rule() {
        let text = reject_feedback(Some(&json!("bash")), "no native execution");
        assert!(text.starts_with("Native tool \"bash\" was blocked: no native execution."));
        assert!(text.contains("StructuredOutput"));
    }

    #[test]
    fn validate_action_accepts_a_translation_that_fits_the_schema() {
        let validated = validate_action(&json!({ "name": "write", "arguments": { "file_path": "/tmp/a", "content": "x" } }), &[write_tool()]).unwrap();
        assert_eq!(validated["name"], json!("Write"), "返回接收方的原始工具名");
    }

    #[test]
    fn validate_action_rejects_every_documented_violation() {
        let undeclared = validate_action(
            &json!({ "name": "Write", "arguments": { "file_path": "/tmp/a", "content": "x", "extra": 1 } }),
            &[write_tool()],
        )
        .unwrap_err();
        assert_eq!(undeclared.code, "invalid_tool_call");
        assert_eq!(undeclared.status, 502);
        assert!(undeclared.message.contains("does not declare: extra"));

        assert!(validate_action(&json!({ "name": "Edit", "arguments": {} }), &[write_tool()]).is_err());
        assert!(validate_action(&json!({ "name": "Write", "arguments": [] }), &[write_tool()]).is_err());
        assert!(validate_action(&json!({ "arguments": {} }), &[write_tool()]).is_err());
        assert!(validate_action(&json!([1, 2]), &[write_tool()]).is_err());

        let missing = validate_action(&json!({ "name": "Write", "arguments": { "file_path": "/tmp/a" } }), &[write_tool()]).unwrap_err();
        assert_eq!(missing.message, "Translated action misses a required argument");
    }

    #[test]
    fn has_category_is_case_insensitive() {
        assert!(has_category("MULTIEDIT"));
        assert!(has_category("apply_patch"));
        assert!(!has_category("webfetch"));
    }

    #[test]
    fn same_name_helper_marks_equal_tool_names() {
        assert!(same_name(Some(&json!("Read")), Some(&json!("Read"))));
        assert!(!same_name(None, Some(&json!("Read"))));
    }
}
