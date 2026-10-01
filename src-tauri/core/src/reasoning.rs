//! `core/src/reasoning.js` 的 Rust 等价实现：OpenCode variant ↔ reasoning effort 映射。
//!
//! 原注释：只对外暴露能映射到真实 OpenCode variant 的 effort 控制项。

use crate::json::truthy;
use serde_json::{json, Map, Value};

/// OpenCode 认可的 reasoning effort 取值（顺序与原实现一致）。
pub const EFFORT_LEVELS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// `reasoningEfforts(model)`：`{[reasoningEffort]: variant}`。
///
/// 只保留未禁用、且 `reasoningEffort` 是合法 effort 的 variant；同一 effort 出现多次时
/// 后者覆盖前者，但键的位置保持首次出现的位置——这与 JS `Object.fromEntries` 一致。
pub fn reasoning_efforts(model: &Value) -> Map<String, Value> {
    let mut efforts = Map::new();
    if !truthy(model.get("reasoning")) {
        return efforts;
    }
    if let Some(Value::Object(variants)) = model.get("variants") {
        for (variant, options) in variants {
            if truthy(options.get("disabled")) {
                continue;
            }
            if let Some(Value::String(effort)) = options.get("reasoningEffort") {
                if EFFORT_LEVELS.contains(&effort.as_str()) {
                    efforts.insert(effort.clone(), Value::String(variant.clone()));
                }
            }
        }
    }
    efforts
}

/// `workBuddyReasoning(model)`：派生 WorkBuddy 侧的 reasoning 能力声明。
pub fn work_buddy_reasoning(model: &Value) -> Value {
    let efforts = reasoning_efforts(model);
    let supported_efforts: Vec<String> = efforts.keys().cloned().collect();
    let enabled: Vec<String> = supported_efforts
        .iter()
        .filter(|effort| effort.as_str() != "none")
        .cloned()
        .collect();

    if !truthy(model.get("reasoning")) {
        return json!({ "supportsReasoning": false });
    }
    if enabled.is_empty() {
        return json!({
            "supportsReasoning": true,
            "onlyReasoning": true,
            "reasoning": { "supportedEfforts": [], "canDisableThinking": false },
        });
    }
    let default_effort = if enabled.iter().any(|effort| effort == "medium") {
        "medium"
    } else {
        enabled[0].as_str()
    };
    json!({
        "supportsReasoning": true,
        "onlyReasoning": !supported_efforts.iter().any(|effort| effort == "none"),
        "reasoning": {
            "supportedEfforts": supported_efforts,
            "defaultEffort": default_effort,
            "canDisableThinking": supported_efforts.iter().any(|effort| effort == "none"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_model_without_reasoning_advertises_nothing() {
        assert_eq!(reasoning_efforts(&json!({})), Map::new());
        assert_eq!(work_buddy_reasoning(&json!({})), json!({ "supportsReasoning": false }));
    }

    #[test]
    fn only_enabled_and_known_efforts_are_mapped() {
        let model = json!({
            "reasoning": true,
            "variants": {
                "low": { "reasoningEffort": "low" },
                "high": { "reasoningEffort": "high" },
                "off": { "reasoningEffort": "none" },
                "broken": { "reasoningEffort": "ultra" },
                "disabled": { "reasoningEffort": "max", "disabled": true },
                "unnamed": {}
            }
        });
        let efforts = reasoning_efforts(&model);
        assert_eq!(efforts.len(), 3);
        assert_eq!(efforts.get("low"), Some(&json!("low")));
        assert_eq!(efforts.get("high"), Some(&json!("high")));
        assert_eq!(efforts.get("none"), Some(&json!("off")));
        assert!(!efforts.contains_key("ultra"));
        assert!(!efforts.contains_key("max"));
    }

    #[test]
    fn later_variant_wins_for_a_duplicated_effort() {
        let model = json!({
            "reasoning": true,
            "variants": { "a": { "reasoningEffort": "low" }, "b": { "reasoningEffort": "low" } }
        });
        let efforts = reasoning_efforts(&model);
        assert_eq!(efforts.len(), 1);
        assert_eq!(efforts.get("low"), Some(&json!("b")));
    }

    #[test]
    fn default_effort_falls_back_to_the_first_enabled_level() {
        let model = json!({
            "reasoning": true,
            "variants": {
                "v1": { "reasoningEffort": "high" },
                "v2": { "reasoningEffort": "xhigh" }
            }
        });
        let declared = work_buddy_reasoning(&model);
        assert_eq!(declared["reasoning"]["defaultEffort"], json!("high"));
        assert_eq!(declared["onlyReasoning"], json!(true));
        assert_eq!(declared["reasoning"]["canDisableThinking"], json!(false));
    }

    #[test]
    fn medium_is_the_preferred_default_and_none_enables_disabling() {
        let model = json!({
            "reasoning": true,
            "variants": {
                "off": { "reasoningEffort": "none" },
                "mid": { "reasoningEffort": "medium" },
                "high": { "reasoningEffort": "high" }
            }
        });
        let declared = work_buddy_reasoning(&model);
        assert_eq!(declared["reasoning"]["defaultEffort"], json!("medium"));
        assert_eq!(declared["onlyReasoning"], json!(false));
        assert_eq!(declared["reasoning"]["canDisableThinking"], json!(true));
    }

    #[test]
    fn a_reasoning_model_without_enabled_variants_is_only_reasoning() {
        let model = json!({ "reasoning": true, "variants": { "off": { "reasoningEffort": "none" } } });
        assert_eq!(
            work_buddy_reasoning(&model),
            json!({
                "supportsReasoning": true,
                "onlyReasoning": true,
                "reasoning": { "supportedEfforts": [], "canDisableThinking": false },
            })
        );
    }
}
