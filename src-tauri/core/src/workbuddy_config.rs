//! `core/src/workbuddy-config.js` 的 Rust 等价实现：`models.json` 的发现与格式校验。
//!
//! 原注释强调：**记忆或显式指定的位置绝不能静默回退到另一个 profile**。因此
//! [`resolve_models_file`] 只在候选文件真的通过 [`validate_models_file`] 时才返回路径，
//! 否则返回 `None`（对应 JS 的 `catch { return null }`）。

use crate::json::{js_trim, parse_json, Env};
use crate::platform::{basename_host, is_absolute_host, join_host};
use serde_json::Value;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// 目标配置文件名（比较时不区分大小写）。
pub const MODELS_FILE_NAME: &str = "models.json";

/// 未配置 `WORKBUDDY_DATA_FOLDER_NAME` 时的默认数据目录名。
pub const DEFAULT_DATA_FOLDER: &str = ".workbuddy";

/// 路径不合法时的用户可见提示。
pub const INVALID_PATH_MESSAGE: &str = "请选择 WorkBuddy 的 models.json 配置文件";

/// 文件内容格式不受支持时的用户可见提示。
pub const INVALID_FORMAT_MESSAGE: &str = "文件不是支持的 WorkBuddy 模型配置格式";

/// 补建 `models.json` 时写入的初始内容：**空**的数组形态配置。
///
/// 与写入端对「文件不存在」的认定保持一致（`sync_models` 读不到文件时把文档当作 `[]`），
/// 同时它也是 [`validate_models_file`] 支持的两种形状之一——补建出来的文件必须能立刻
/// 通过校验，否则补建没有意义。
pub const EMPTY_MODELS_FILE_TEXT: &str = "[]\n";

/// `validateModelsFile` 可能抛出的错误。
#[derive(Debug)]
pub enum ConfigError {
    /// 读取文件失败（对应 JS 里 `fs.readFile` 抛出的 errno 错误）。
    Io(io::Error),
    /// JSON 解析失败（对应 JS 的 `JSON.parse` 语法错误）。
    Json(serde_json::Error),
    /// 路径或内容不符合要求，携带与 JS 完全一致的中文提示。
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(error) => write!(formatter, "{error}"),
            ConfigError::Json(error) => write!(formatter, "{error}"),
            ConfigError::Invalid(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// `validateModelsFile(file)`：校验路径与内容，成功时原样返回该路径。
///
/// 判定顺序与原实现一致：先看路径（必须是绝对路径，且 basename 忽略大小写等于 `models.json`），
/// 再看内容（顶层是数组，**或**顶层对象的 `models` 是数组）。
pub fn validate_models_file(file: &str) -> Result<String, ConfigError> {
    if !is_absolute_host(file) || basename_host(file).to_lowercase() != MODELS_FILE_NAME {
        return Err(ConfigError::Invalid(INVALID_PATH_MESSAGE.to_string()));
    }
    let text = std::fs::read_to_string(file).map_err(ConfigError::Io)?;
    let document: Value = parse_json(&text).map_err(ConfigError::Json)?;
    let supported = document.is_array()
        || document
            .get("models")
            .map(|models| models.is_array())
            .unwrap_or(false);
    if !supported {
        return Err(ConfigError::Invalid(INVALID_FORMAT_MESSAGE.to_string()));
    }
    Ok(file.to_string())
}

/// 「插件目录已在、但 `models.json` 还不存在」时补建一个空配置（返回是否真的新建了文件）。
///
/// 只在**发现链的默认位置**上调用：`env` / `saved` 显式指定的位置不在此列——那里失效必须
/// 继续返回 `None`（既有铁律，有单测钉住）。三道顺序闸门：
/// 1. 文件已存在 → 立刻返回，绝不以任何方式改写用户内容；
/// 2. 父目录不存在 → 也不建目录（目录不在＝插件没装，替它建目录会凭空造出「已安装」的假象）；
/// 3. 任何 IO 失败（权限不足、只读盘）→ 返回 `false`，由调用方按既有路径继续判定——
///    检测链绝不因补建失败而报错，也绝不把「补建失败」说成「插件已安装」。
pub fn ensure_models_file(file: &str) -> bool {
    let path = PathBuf::from(file);
    if path.exists() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    if !parent.is_dir() {
        return false;
    }
    let Ok(mut handle) = create_new_private(&path) else {
        return false;
    };
    handle.write_all(EMPTY_MODELS_FILE_TEXT.as_bytes()).is_ok()
}

/// `flag: 'wx'`（存在即失败）+ `mode: 0o600`：绝不以截断方式打开已存在的文件。
fn create_new_private(path: &Path) -> io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

/// `resolveModelsFile({ saved, env, home })`：按 `env > saved > 平台默认` 发现配置文件。
///
/// 与 JS 一致地把空字符串视作假值（`||` 语义），并对
/// `WORKBUDDY_CONFIG_DIR` / `WORKBUDDY_DATA_FOLDER_NAME` 先做 `trim()`。
///
/// 走到默认位置时会顺带 [`ensure_models_file`]：目录已在、`models.json` 缺失就补建一个空
/// 配置（装了插件但还没保存过自定义模型的情形）。显式指定（`env` / `saved`）的位置
/// **不补建**——那里失效必须照旧返回 `None`。
pub fn resolve_models_file(saved: Option<&str>, env: &Env, home: &str) -> Option<String> {
    let explicit = env
        .get("BUDDY_MODELS_FILE")
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let file = explicit
        .or_else(|| saved.filter(|value| !value.is_empty()).map(|value| value.to_string()))
        .unwrap_or_else(|| {
            let directory = env
                .get("WORKBUDDY_CONFIG_DIR")
                .map(|value| js_trim(value).to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    let folder = env
                        .get("WORKBUDDY_DATA_FOLDER_NAME")
                        .map(|value| js_trim(value).to_string())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| DEFAULT_DATA_FOLDER.to_string());
                    join_host(&[home, &folder])
                });
            let file = join_host(&[&directory, MODELS_FILE_NAME]);
            // 插件目录已存在、只是还没保存过自定义模型：补建一个空配置，让它可被检测到。
            // 目录不存在时 `ensure_models_file` 什么也不做——那属于「未安装」，不是「没配置文件」。
            ensure_models_file(&file);
            file
        });

    validate_models_file(&file).ok()
}

/// 便捷函数：把校验结果转成 `PathBuf`。
pub fn resolve_models_path(saved: Option<&str>, env: &Env, home: &str) -> Option<PathBuf> {
    resolve_models_file(saved, env, home).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "wbbridge-config-{}-{}-{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_nanos())
                    .unwrap_or_default()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Scratch { root }
        }

        fn write(&self, relative: &str, content: &str) -> String {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, content).unwrap();
            path.to_string_lossy().to_string()
        }

        fn path(&self, relative: &str) -> String {
            self.root.join(relative).to_string_lossy().to_string()
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
    fn accepts_array_and_object_shape() {
        let scratch = Scratch::new("accept");
        let array_file = scratch.write("array/models.json", "[]");
        assert_eq!(validate_models_file(&array_file).unwrap(), array_file);
        let object_file = scratch.write("object/models.json", "{\"models\":[]}");
        assert_eq!(validate_models_file(&object_file).unwrap(), object_file);
    }

    #[test]
    fn rejects_wrong_name_relative_path_and_wrong_shape() {
        let scratch = Scratch::new("reject");
        let wrong_name = scratch.write("array/other.json", "[]");
        assert_eq!(
            validate_models_file(&wrong_name).unwrap_err().to_string(),
            INVALID_PATH_MESSAGE
        );
        assert_eq!(
            validate_models_file("relative/models.json").unwrap_err().to_string(),
            INVALID_PATH_MESSAGE
        );
        let wrong_shape = scratch.write("shape/models.json", "{\"models\":{}}");
        assert_eq!(
            validate_models_file(&wrong_shape).unwrap_err().to_string(),
            INVALID_FORMAT_MESSAGE
        );
    }

    #[test]
    fn missing_file_is_an_io_error_not_a_format_error() {
        let scratch = Scratch::new("missing");
        let error = validate_models_file(&scratch.path("nope/models.json")).unwrap_err();
        assert!(matches!(error, ConfigError::Io(_)));
    }

    #[test]
    fn resolve_prefers_env_then_saved_then_default() {
        let scratch = Scratch::new("resolve");
        let env_file = scratch.write("env/models.json", "[]");
        let saved_file = scratch.write("saved/models.json", "[]");
        let default_file = scratch.write(".workbuddy/models.json", "[]");

        let from_env = resolve_models_file(
            Some(&saved_file),
            &env_of(&[("BUDDY_MODELS_FILE", &env_file)]),
            &scratch.root.to_string_lossy(),
        );
        assert_eq!(from_env.as_deref(), Some(env_file.as_str()));

        let from_saved =
            resolve_models_file(Some(&saved_file), &env_of(&[]), &scratch.root.to_string_lossy());
        assert_eq!(from_saved.as_deref(), Some(saved_file.as_str()));

        let from_default = resolve_models_file(None, &env_of(&[]), &scratch.root.to_string_lossy());
        assert_eq!(from_default.as_deref(), Some(default_file.as_str()));
    }

    #[test]
    fn resolve_uses_config_dir_and_data_folder_overrides() {
        let scratch = Scratch::new("override");
        let config_file = scratch.write("config/models.json", "[]");
        let folder_file = scratch.write("custom-folder/models.json", "[]");
        let home = scratch.root.to_string_lossy().to_string();

        let from_config_dir = resolve_models_file(
            None,
            &env_of(&[("WORKBUDDY_CONFIG_DIR", &format!("  {}  ", scratch.path("config")))]),
            &home,
        );
        assert_eq!(from_config_dir.as_deref(), Some(config_file.as_str()));

        let from_folder = resolve_models_file(
            None,
            &env_of(&[("WORKBUDDY_DATA_FOLDER_NAME", "custom-folder")]),
            &home,
        );
        assert_eq!(from_folder.as_deref(), Some(folder_file.as_str()));
    }

    #[test]
    fn resolve_never_falls_back_when_the_remembered_location_is_invalid() {
        let scratch = Scratch::new("no-fallback");
        let home = scratch.root.to_string_lossy().to_string();
        scratch.write(".workbuddy/models.json", "[]");

        let result = resolve_models_file(
            Some(&scratch.path("broken/models.json")),
            &env_of(&[("BUDDY_MODELS_FILE", "")]),
            &home,
        );
        assert_eq!(result, None, "显式位置失效时必须返回 null，而不是回退到默认 profile");
    }

    #[test]
    fn ensure_models_file_creates_an_empty_config_when_directory_exists() {
        let scratch = Scratch::new("ensure-dir");
        let home = scratch.root.to_string_lossy().to_string();
        fs::create_dir_all(scratch.path(".workbuddy")).expect("目录可建");
        let expected = scratch.path(".workbuddy/models.json");

        // 目录已在、配置文件缺失：补建后即可被检测到（装了插件但还没保存过自定义模型）。
        assert_eq!(
            resolve_models_file(None, &env_of(&[]), &home).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            fs::read_to_string(&expected).expect("补建的文件可读"),
            EMPTY_MODELS_FILE_TEXT
        );

        // 已有内容的文件绝不被补建逻辑改写：补建只发生在文件缺失时。
        fs::write(&expected, "[{\"id\":\"mine\"}]").expect("可写");
        assert_eq!(
            resolve_models_file(None, &env_of(&[]), &home).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(fs::read_to_string(&expected).expect("可读"), "[{\"id\":\"mine\"}]");
    }

    #[test]
    fn ensure_models_file_invents_neither_a_directory_nor_an_explicit_file() {
        let scratch = Scratch::new("ensure-none");
        let home = scratch.root.to_string_lossy().to_string();
        let default_file = scratch.path(".workbuddy/models.json");

        // 目录不在 = 插件没装：不得建目录、不得建文件，结论仍是「未检测到」。
        assert_eq!(resolve_models_file(None, &env_of(&[]), &home), None);
        assert!(!PathBuf::from(&default_file).exists(), "不得凭空造出 models.json");
        assert!(!PathBuf::from(scratch.path(".workbuddy")).exists(), "不得凭空造出插件目录");

        // 显式（saved）位置失效时同样不补建——既有铁律不受本次优化影响。
        let saved = scratch.path("saved/models.json");
        fs::create_dir_all(scratch.path("saved")).expect("目录可建");
        assert_eq!(resolve_models_file(Some(&saved), &env_of(&[]), &home), None);
        assert!(!PathBuf::from(&saved).exists(), "显式位置不得被补建");
    }
}
