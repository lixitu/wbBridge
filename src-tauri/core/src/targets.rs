//! 多目标写入：检测用户环境里实际存在哪些 IDE 插件，并把模型发布分发给它们。
//!
//! 「已安装」的判定只认**配置文件真实存在**（定位函数返回 `Some`，内部已过
//! [`crate::workbuddy_config::validate_models_file`]）——不猜进程、不猜注册表、不猜包管理器
//! 数据库。探测不到的目标一律报「未检测到」，绝不静默回退。两个目标的定位与校验分别
//! 由对照模块提供：[`crate::workbuddy_config`]（既有行为，原样保留）与
//! [`crate::codebuddy_config`]（与其逐行对照的实现）。
//!
//! 合并写入共用 [`crate::sync`]：同一套 OWNER 归属标记、冲突不覆盖、文件锁、二次读取与
//! 原子替换——CodeBuddy 不是第二套写入实现，只是第二个写入目标。唯一的按目标差异是
//! **外层形态**（[`Target::document_shape`]）：WorkBuddy 的 `models.json` 顶层是裸数组，
//! CodeBuddy 的那份是 `{ "models": […] }` 对象；写错形态会让插件读不出任何模型。

use crate::codebuddy_config;
use crate::sync::{DocumentShape, SyncOutcome};
use crate::workbuddy_config;
use crate::Env;
use serde_json::Value;

/// 写入目标。`WorkBuddy` 是既有行为；`CodeBuddy` 是本次扩展的第二个目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    WorkBuddy,
    CodeBuddy,
}

impl Target {
    /// 全部写入目标，顺序即分发顺序（WorkBuddy 在前，保持既有优先级不变）。
    pub const ALL: [Target; 2] = [Target::WorkBuddy, Target::CodeBuddy];

    /// 状态展示名（与面板文案共用一份措辞来源）。
    pub fn label(self) -> &'static str {
        match self {
            Target::WorkBuddy => "WorkBuddy",
            Target::CodeBuddy => "CodeBuddy",
        }
    }

    /// `status.json` 里的配置路径字段名。
    pub fn models_file_field(self) -> &'static str {
        match self {
            Target::WorkBuddy => "modelsFile",
            Target::CodeBuddy => "codeBuddyModelsFile",
        }
    }

    /// `sync.targets` 里的键名。
    pub fn sync_key(self) -> &'static str {
        match self {
            Target::WorkBuddy => "workBuddy",
            Target::CodeBuddy => "codeBuddy",
        }
    }

    /// 该目标 `models.json` 的外层形态。
    ///
    /// 以两边真实文件为凭（2026-10-10）：WorkBuddy 顶层是**裸数组**，CodeBuddy 顶层是
    /// `{ "models": […] }` 对象。给 CodeBuddy 写裸数组时它一个模型都读不出（Windows 上实测到），
    /// 所以这里按目标固定，而不是「文档原来什么形态就写回什么形态」。
    pub fn document_shape(self) -> DocumentShape {
        match self {
            Target::WorkBuddy => DocumentShape::Preserve,
            Target::CodeBuddy => DocumentShape::ModelsObject,
        }
    }

    /// 目标自己的环境变量覆盖键（用于状态里提示用户如何显式指定）。
    pub fn models_file_env(self) -> &'static str {
        match self {
            Target::WorkBuddy => "BUDDY_MODELS_FILE",
            Target::CodeBuddy => "BUDDY_CODEBUDDY_MODELS_FILE",
        }
    }
}

/// 目标化定位：WorkBuddy 走 `workbuddy_config::resolve_models_file`（行为逐字节不变），
/// CodeBuddy 走 `codebuddy_config::resolve_models_file`（与其逐行对照）。
pub fn resolve_target_models_file(
    target: Target,
    saved: Option<&str>,
    env: &Env,
    home: &str,
) -> Option<String> {
    match target {
        Target::WorkBuddy => workbuddy_config::resolve_models_file(saved, env, home),
        Target::CodeBuddy => codebuddy_config::resolve_models_file(saved, env, home),
    }
}

/// 导入链的对称入口：校验用户手动选择的 `models.json`（面板「导入」动作）。
///
/// 两个目标的配置文件**同名**，且读取侧都接受「裸数组」与 `{ "models": […] }` 两种外层形态，
/// 因此共用同一校验器；**写出**时各自按 `Target::document_shape` 固定形态。包装层存在的意义是让
/// `orchestration.rs` 对两个目标都只经本模块引用（发现链 `resolve_target_models_file` 与导入链同构），
/// 并在 CodeBuddy 将来获得自己的导入动作时无需再动调用方。与发现链不同，这里
/// **保留原始错误**（路径 / 格式提示是面板可见的契约文案），绝不做 `.ok()` 吞错。
pub fn validate_selected_models_file(
    target: Target,
    file: &str,
) -> Result<String, workbuddy_config::ConfigError> {
    match target {
        Target::WorkBuddy | Target::CodeBuddy => workbuddy_config::validate_models_file(file),
    }
}

/// 遍历全部目标并定位其配置文件。返回 `(目标, 定位结果)` 的有序列表——
/// `None` 就是「未检测到该插件」，调用方按目标分发时跳过并说明原因。
pub fn detect_targets(saved: Option<&str>, env: &Env, home: &str) -> Vec<(Target, Option<String>)> {
    Target::ALL
        .iter()
        .map(|target| (*target, resolve_target_models_file(*target, saved, env, home)))
        .collect()
}

/// 目标未被检测到时的原因文案。
pub fn missing_reason(target: Target) -> &'static str {
    match target {
        Target::WorkBuddy => {
            "未检测到 WorkBuddy 的 models.json（可点击导入并选择 models.json；首次使用请先在 WorkBuddy 保存一个自定义模型。）"
        }
        Target::CodeBuddy => codebuddy_config::MISSING_MESSAGE,
    }
}

/// `sync_published` 的分发结果聚合（纯函数，不碰 IO）。
///
/// - 顶层 `count` = 各成功目标条数之和；
/// - 顶层 `error` 只在「**至少一个目标被定位且全部定位目标都失败**」时出现——未检测到
///   不算失败（该目标不存在，不是错误）；部分失败仍有写入成功时也不算顶层失败；
/// - 顶层 `changed` 恒为布尔（任一目标有真实写入才为 `true`）；`time` 由调用方统一补。
pub fn aggregate_sync(outcomes: Vec<(Target, Option<Result<SyncOutcome, String>>)>) -> Value {
    let mut targets = serde_json::Map::new();
    let mut count = 0u64;
    let mut any_changed = false;
    let mut located = 0usize;
    let mut failed = 0usize;

    for (target, outcome) in &outcomes {
        let mut entry = serde_json::Map::new();
        match outcome {
            // 未定位：目标不存在，如实标注原因，不计入失败。
            None => {
                entry.insert("status".to_string(), Value::String("missing".to_string()));
                entry.insert(
                    "reason".to_string(),
                    Value::String(missing_reason(*target).to_string()),
                );
            }
            Some(Ok(outcome)) => {
                located += 1;
                count += outcome.count;
                any_changed |= outcome.changed;
                entry.insert("status".to_string(), Value::String("ok".to_string()));
                entry.insert(
                    "count".to_string(),
                    crate::json::number_from_f64(outcome.count as f64),
                );
                entry.insert("changed".to_string(), Value::Bool(outcome.changed));
            }
            Some(Err(message)) => {
                located += 1;
                failed += 1;
                entry.insert("status".to_string(), Value::String("error".to_string()));
                entry.insert("error".to_string(), Value::String(message.clone()));
            }
        }
        targets.insert(target.sync_key().to_string(), Value::Object(entry));
    }

    let mut sync = serde_json::Map::new();
    sync.insert("targets".to_string(), Value::Object(targets));
    sync.insert("count".to_string(), crate::json::number_from_f64(count as f64));
    // 顶层 changed 恒为布尔：面板的「配置已是最新」判断依赖它，缺键会被当成「有写入」。
    sync.insert("changed".to_string(), Value::Bool(any_changed));
    if located > 0 && failed == located {
        // 全部定位目标失败时保留旧版「顶层 error」形状，面板与 import_models 的错误判断不用分叉。
        let errors: Vec<String> = outcomes
            .iter()
            .filter_map(|(target, outcome)| match outcome {
                Some(Err(message)) => Some(format!("{}: {}", target.label(), message)),
                _ => None,
            })
            .collect();
        sync.insert("error".to_string(), Value::String(errors.join("; ")));
    }
    Value::Object(sync)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "wbbridge-targets-{}-{}-{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_nanos())
                    .unwrap_or_default()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("临时目录可建");
            Scratch { root }
        }

        fn write(&self, relative: &str, content: &str) -> String {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("父目录可建");
            }
            fs::write(&path, content).expect("文件可写");
            path.to_string_lossy().to_string()
        }

        fn home(&self) -> String {
            self.root.to_string_lossy().to_string()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn env_of(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<HashMap<_, _>>()
    }

    fn outcome(count: u64, changed: bool) -> Result<SyncOutcome, String> {
        Ok(SyncOutcome { changed, count })
    }

    #[test]
    fn workbuddy_detection_is_unchanged_and_codebuddy_is_independent() {
        // 只装 WorkBuddy：WorkBuddy 命中默认位置（既有行为），CodeBuddy 报未检测到。
        let scratch = Scratch::new("wb-only");
        let home = scratch.home();
        let wb = scratch.write(".workbuddy/models.json", "[]");
        assert_eq!(
            detect_targets(None, &env_of(&[]), &home),
            vec![(Target::WorkBuddy, Some(wb)), (Target::CodeBuddy, None)]
        );

        // 只装 CodeBuddy：只检出 CodeBuddy，WorkBuddy 报未检测到。
        let scratch = Scratch::new("cb-only");
        let home = scratch.home();
        let cb = scratch.write(".codebuddy/models.json", "[]");
        assert_eq!(
            detect_targets(None, &env_of(&[]), &home),
            vec![(Target::WorkBuddy, None), (Target::CodeBuddy, Some(cb))]
        );
    }

    #[test]
    fn both_plugins_detected_in_order() {
        let scratch = Scratch::new("both");
        let home = scratch.home();
        let wb = scratch.write(".workbuddy/models.json", "[]");
        let cb = scratch.write(".codebuddy/models.json", "[]");
        assert_eq!(
            detect_targets(None, &env_of(&[]), &home),
            vec![(Target::WorkBuddy, Some(wb)), (Target::CodeBuddy, Some(cb))]
        );
    }

    #[test]
    fn codebuddy_env_override_wins_over_default_discovery() {
        let scratch = Scratch::new("cb-env");
        let home = scratch.home();
        let default_file = scratch.write(".codebuddy/models.json", "[]");
        let explicit = scratch.write("other/models.json", "[]");

        let env = env_of(&[("BUDDY_CODEBUDDY_MODELS_FILE", &explicit)]);
        assert_eq!(
            detect_targets(None, &env, &home)
                .into_iter()
                .nth(1)
                .and_then(|(_, file)| file),
            Some(explicit)
        );

        // 空串按 JS `||` 语义视作假值：回落默认发现。
        let env = env_of(&[("BUDDY_CODEBUDDY_MODELS_FILE", "")]);
        assert_eq!(
            detect_targets(None, &env, &home)
                .into_iter()
                .nth(1)
                .and_then(|(_, file)| file),
            Some(default_file)
        );
    }

    #[test]
    fn broken_codebuddy_file_is_reported_missing_not_falling_back() {
        let scratch = Scratch::new("cb-broken");
        let home = scratch.home();
        // 形状不对的文件绝不能被当成「未安装」并回退到别的位置；「未检测到」就是结论。
        scratch.write(".codebuddy/models.json", "{\"models\":{}}");
        scratch.write(".workbuddy/models.json", "[]");
        let detected = detect_targets(None, &env_of(&[]), &home);
        assert_eq!(detected[1], (Target::CodeBuddy, None));
        assert!(detected[0].1.is_some(), "WorkBuddy 不受 CodeBuddy 状态影响");
    }

    #[test]
    fn empty_plugin_directories_are_detected_after_creating_models_file() {
        // 两个插件目录都在、但都没保存过 models.json：补建空配置后两个目标都应被检出。
        let scratch = Scratch::new("dirs-only");
        let home = scratch.home();
        fs::create_dir_all(scratch.root.join(".workbuddy")).expect("目录可建");
        fs::create_dir_all(scratch.root.join(".codebuddy")).expect("目录可建");
        let wb = scratch.root.join(".workbuddy/models.json").to_string_lossy().to_string();
        let cb = scratch.root.join(".codebuddy/models.json").to_string_lossy().to_string();

        assert_eq!(
            detect_targets(None, &env_of(&[]), &home),
            vec![(Target::WorkBuddy, Some(wb.clone())), (Target::CodeBuddy, Some(cb.clone()))]
        );
        assert_eq!(
            fs::read_to_string(&wb).expect("可读"),
            crate::workbuddy_config::EMPTY_MODELS_FILE_TEXT
        );
        // 🔴 两边补建的形态不同：WorkBuddy 是裸数组，CodeBuddy 是 `{ "models": [] }` 对象。
        // 给 CodeBuddy 补建裸数组会让插件一个模型都读不出来。
        assert_eq!(
            fs::read_to_string(&cb).expect("可读"),
            crate::codebuddy_config::EMPTY_MODELS_FILE_TEXT
        );
    }

    #[test]
    fn validate_selected_file_dispatches_to_the_shared_validator() {
        // 导入链包装：合法文件原样通过；坏形状保留原始契约文案（不被 .ok() 吞掉）。
        let scratch = Scratch::new("validate-selected");
        let ok = scratch.write(".codebuddy/models.json", "[]");
        assert_eq!(
            validate_selected_models_file(Target::CodeBuddy, &ok).expect("合法文件应通过"),
            ok
        );
        let bad = scratch.write("shape/models.json", "{\"models\":{}}");
        let error =
            validate_selected_models_file(Target::WorkBuddy, &bad).expect_err("坏形状必须报错");
        assert_eq!(error.to_string(), crate::workbuddy_config::INVALID_FORMAT_MESSAGE);
    }

    #[test]
    fn aggregate_counts_only_successful_targets() {
        // 两个目标都成功：count 相加，changed 取或。
        let both = vec![
            (Target::WorkBuddy, Some(outcome(3, true))),
            (Target::CodeBuddy, Some(outcome(2, false))),
        ];
        let value = aggregate_sync(both);
        assert_eq!(value["count"], json!(5));
        assert_eq!(value["targets"]["workBuddy"]["status"], json!("ok"));
        assert_eq!(value["targets"]["codeBuddy"]["status"], json!("ok"));
        assert_eq!(value["targets"]["workBuddy"]["count"], json!(3));
        assert_eq!(value["targets"]["codeBuddy"]["changed"], json!(false));
        assert_eq!(value["changed"], json!(true));
        assert!(value.get("error").is_none(), "无失败不得出现顶层 error");

        // 未检测到 ≠ 失败：不产生顶层 error，也不计入 count。
        let missing = vec![
            (Target::WorkBuddy, Some(outcome(0, false))),
            (Target::CodeBuddy, None),
        ];
        let value = aggregate_sync(missing);
        assert_eq!(value["targets"]["codeBuddy"]["status"], json!("missing"));
        assert_eq!(value["targets"]["codeBuddy"]["reason"], json!(codebuddy_config::MISSING_MESSAGE));
        assert_eq!(value["targets"]["workBuddy"]["status"], json!("ok"));
        assert_eq!(value["changed"], json!(false), "无写入时 changed 必须是显式 false");
        assert!(value.get("error").is_none());

        // 一个成功一个失败：count 只算成功的，不产生顶层 error（有写入成功就不算顶层失败）。
        let partial = vec![
            (Target::WorkBuddy, Some(outcome(2, true))),
            (Target::CodeBuddy, Some(Err("boom".to_string()))),
        ];
        let value = aggregate_sync(partial);
        assert_eq!(value["count"], json!(2));
        assert_eq!(value["targets"]["codeBuddy"]["status"], json!("error"));
        assert_eq!(value["targets"]["codeBuddy"]["error"], json!("boom"));
        assert!(value.get("error").is_none(), "部分失败不算顶层失败");

        // 全部定位目标都失败：保留旧版顶层 error 形状。
        let all_failed = vec![
            (Target::WorkBuddy, Some(Err("e1".to_string()))),
            (Target::CodeBuddy, Some(Err("e2".to_string()))),
        ];
        let value = aggregate_sync(all_failed);
        assert_eq!(value["error"], json!("WorkBuddy: e1; CodeBuddy: e2"));
        assert_eq!(value["count"], json!(0));
        assert_eq!(value["changed"], json!(false));
    }

    #[test]
    fn target_metadata_fields_are_stable_contract_names() {
        // 对外契约名集中自检：status 字段名、sync 键名、env 键名都不得随手改名。
        assert_eq!(Target::WorkBuddy.models_file_field(), "modelsFile");
        assert_eq!(Target::CodeBuddy.models_file_field(), "codeBuddyModelsFile");
        assert_eq!(Target::WorkBuddy.sync_key(), "workBuddy");
        assert_eq!(Target::CodeBuddy.sync_key(), "codeBuddy");
        assert_eq!(Target::WorkBuddy.models_file_env(), "BUDDY_MODELS_FILE");
        assert_eq!(Target::CodeBuddy.models_file_env(), "BUDDY_CODEBUDDY_MODELS_FILE");
        assert_eq!(Target::ALL, [Target::WorkBuddy, Target::CodeBuddy]);
        // 写出形态按目标固定：WorkBuddy 保留文档原形态（裸数组），CodeBuddy 必须写对象形态。
        // 断言用字面量而非互相比较，避免两个枚举值被顺手改成同一档时自检失效。
        assert_eq!(Target::WorkBuddy.document_shape(), DocumentShape::Preserve);
        assert_eq!(Target::CodeBuddy.document_shape(), DocumentShape::ModelsObject);
        // 补建的初始内容必须与该目标的写出形态一致，否则「检测到时补建」与「首次发布」会给出
        // 两种不同的文件结构，插件读到空配置的那次仍然是不可用形态。
        let wb_seed: serde_json::Value =
            serde_json::from_str(crate::workbuddy_config::EMPTY_MODELS_FILE_TEXT).expect("可解析");
        let cb_seed: serde_json::Value =
            serde_json::from_str(crate::codebuddy_config::EMPTY_MODELS_FILE_TEXT).expect("可解析");
        assert!(wb_seed.is_array());
        assert!(cb_seed.get("models").map(|v| v.is_array()) == Some(true));
    }
}
