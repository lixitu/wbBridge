//! `workbuddy_config.rs` 的 CodeBuddy 对照实现：`models.json` 的发现与定位。
//!
//! CodeBuddy 的自定义模型配置与 WorkBuddy **同名同形**（`models.json`，顶层是数组或
//! `models` 是数组），因此路径校验与错误提示直接复用 [`crate::workbuddy_config`]，
//! 本模块只补齐 CodeBuddy 自己的定位优先级——与 WorkBuddy 版逐行对照：
//!
//! | 优先级 | WorkBuddy（`workbuddy_config.rs`） | CodeBuddy（本模块） |
//! |---|---|---|
//! | 1. 显式文件 | `BUDDY_MODELS_FILE` | `BUDDY_CODEBUDDY_MODELS_FILE` |
//! | 2. 已保存值 | `settings.json` 的 `workBuddyModelsFile` | 同形态的 `codeBuddyModelsFile`（当前尚无面板入口，调用方传 `None`） |
//! | 3. 配置目录 | `WORKBUDDY_CONFIG_DIR`（trim，空串视作假值） | `CODEBUDDY_CONFIG_DIR` |
//! | 4. 数据目录名 | `WORKBUDDY_DATA_FOLDER_NAME`（trim，默认 `.workbuddy`） | `CODEBUDDY_DATA_FOLDER_NAME`（默认 `.codebuddy`） |
//!
//! 与 WorkBuddy 版共享同一条铁律：显式或记忆的位置失效时**绝不静默回退**到默认位置，
//! 只返回 `None`（对应 JS 的 `catch { return null }`）。「是否已安装」由调用方
//! （`targets.rs`）以「定位结果是否为 `Some`」判定，本模块不做任何猜测性回退。

use crate::json::js_trim;
use crate::platform::join_host;
use crate::workbuddy_config::{ensure_models_file, validate_models_file, MODELS_FILE_NAME};
use crate::Env;

/// 未配置 `CODEBUDDY_DATA_FOLDER_NAME` 时的默认数据目录名。
pub const DEFAULT_DATA_FOLDER: &str = ".codebuddy";

/// CodeBuddy 目标未被检测到时的状态说明（`sync.targets.codeBuddy.reason`）。
pub const MISSING_MESSAGE: &str =
    "未检测到 CodeBuddy 的 models.json（默认位置 ~/.codebuddy/models.json；可用 BUDDY_CODEBUDDY_MODELS_FILE 指定）";

/// CodeBuddy 版 `resolveModelsFile({ saved, env, home })`：按 `env > saved > 平台默认` 发现配置文件。
///
/// 与 WorkBuddy 版一致地把空字符串视作假值（`||` 语义），并对目录类变量先 `trim()`；
/// 候选文件必须通过 [`validate_models_file`] 才返回，否则 `None`。
///
/// 默认位置同样先过 [`ensure_models_file`]（目录在、`models.json` 不在则补建空配置），
/// 与 WorkBuddy 版逐行对照；显式指定的位置不补建。
pub fn resolve_models_file(saved: Option<&str>, env: &Env, home: &str) -> Option<String> {
    let explicit = env
        .get("BUDDY_CODEBUDDY_MODELS_FILE")
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let file = explicit
        .or_else(|| saved.filter(|value| !value.is_empty()).map(|value| value.to_string()))
        .unwrap_or_else(|| {
            let directory = env
                .get("CODEBUDDY_CONFIG_DIR")
                .map(|value| js_trim(value).to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    let folder = env
                        .get("CODEBUDDY_DATA_FOLDER_NAME")
                        .map(|value| js_trim(value).to_string())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| DEFAULT_DATA_FOLDER.to_string());
                    join_host(&[home, &folder])
                });
            let file = join_host(&[&directory, MODELS_FILE_NAME]);
            // 与 WorkBuddy 版同款：目录已在、models.json 缺失就补建空配置；目录不在则不动。
            ensure_models_file(&file);
            file
        });

    validate_models_file(&file).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    /// 与 `workbuddy_config.rs` 测试同款的临时沙箱。
    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "wbbridge-codebuddy-config-{}-{}-{}",
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

        fn path(&self, relative: &str) -> String {
            self.root.join(relative).to_string_lossy().to_string()
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

    #[test]
    fn resolve_prefers_env_then_saved_then_default() {
        let scratch = Scratch::new("resolve");
        let env_file = scratch.write("env/models.json", "[]");
        let saved_file = scratch.write("saved/models.json", "[]");
        let default_file = scratch.write(".codebuddy/models.json", "[]");
        let home = scratch.home();

        let from_env = resolve_models_file(
            Some(&saved_file),
            &env_of(&[("BUDDY_CODEBUDDY_MODELS_FILE", &env_file)]),
            &home,
        );
        assert_eq!(from_env.as_deref(), Some(env_file.as_str()));

        let from_saved = resolve_models_file(Some(&saved_file), &env_of(&[]), &home);
        assert_eq!(from_saved.as_deref(), Some(saved_file.as_str()));

        let from_default = resolve_models_file(None, &env_of(&[]), &home);
        assert_eq!(from_default.as_deref(), Some(default_file.as_str()));
    }

    #[test]
    fn resolve_uses_config_dir_and_data_folder_overrides() {
        let scratch = Scratch::new("override");
        let config_file = scratch.write("config/models.json", "[]");
        let folder_file = scratch.write("custom-folder/models.json", "[]");
        let home = scratch.home();

        let from_config_dir = resolve_models_file(
            None,
            &env_of(&[("CODEBUDDY_CONFIG_DIR", &format!("  {}  ", scratch.path("config")))]),
            &home,
        );
        assert_eq!(from_config_dir.as_deref(), Some(config_file.as_str()));

        let from_folder = resolve_models_file(
            None,
            &env_of(&[("CODEBUDDY_DATA_FOLDER_NAME", "custom-folder")]),
            &home,
        );
        assert_eq!(from_folder.as_deref(), Some(folder_file.as_str()));
    }

    #[test]
    fn empty_env_values_fall_through_like_js_falsy() {
        let scratch = Scratch::new("falsy");
        let home = scratch.home();
        let default_file = scratch.write(".codebuddy/models.json", "[]");

        // 空字符串按 `||` 语义视作假值：显式文件与配置目录都回落到默认发现。
        let resolved = resolve_models_file(
            None,
            &env_of(&[
                ("BUDDY_CODEBUDDY_MODELS_FILE", ""),
                ("CODEBUDDY_CONFIG_DIR", "   "),
            ]),
            &home,
        );
        assert_eq!(resolved.as_deref(), Some(default_file.as_str()));
    }

    #[test]
    fn resolve_never_falls_back_when_the_remembered_location_is_invalid() {
        let scratch = Scratch::new("no-fallback");
        let home = scratch.home();
        scratch.write(".codebuddy/models.json", "[]");

        let result = resolve_models_file(
            Some(&scratch.path("broken/models.json")),
            &env_of(&[("BUDDY_CODEBUDDY_MODELS_FILE", "")]),
            &home,
        );
        assert_eq!(result, None, "显式位置失效时必须返回 None，而不是回退到默认位置");
    }

    #[test]
    fn default_discovery_creates_missing_models_file_when_directory_exists() {
        let scratch = Scratch::new("ensure-dir");
        let home = scratch.home();
        fs::create_dir_all(scratch.path(".codebuddy")).expect("目录可建");
        let expected = scratch.path(".codebuddy/models.json");

        // 与 WorkBuddy 版同款：目录已在、models.json 缺失 → 补建空配置并定位成功。
        assert_eq!(
            resolve_models_file(None, &env_of(&[]), &home).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            fs::read_to_string(&expected).expect("补建的文件可读"),
            crate::workbuddy_config::EMPTY_MODELS_FILE_TEXT
        );

        // 目录不存在时什么都不做：那属于「未安装」，不是「没配置文件」。
        let scratch = Scratch::new("ensure-none");
        assert_eq!(resolve_models_file(None, &env_of(&[]), &scratch.home()), None);
        assert!(!PathBuf::from(scratch.path(".codebuddy/models.json")).exists());
        assert!(!PathBuf::from(scratch.path(".codebuddy")).exists());
    }
}
