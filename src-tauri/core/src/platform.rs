//! `core/src/platform.js` 的 Rust 等价实现。
//!
//! JS 版依赖 `node:os` / `node:path`：数据目录按平台派生，运行时包名按平台/架构拼装。
//! 这里用纯字符串拼接复刻 `path.posix.join` / `path.win32.join` 在项目实际用到的语义
//! （忽略空串、按平台分隔符连接），不引入额外 crate。

use crate::json::Env;
use std::path::{Path, PathBuf};

/// 数据目录的固定末级目录名。
pub const DATA_DIR_NAME: &str = "Buddy Bridge";

/// 编译目标对应的 Node `process.platform` 取值。
pub fn host_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "linux"
    }
}

/// 编译目标对应的 Node `process.arch` 取值。
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

/// 平台对应的路径分隔符。
pub fn separator(platform: &str) -> char {
    if platform == "win32" {
        '\\'
    } else {
        '/'
    }
}

/// 复刻 `path.posix.join` / `path.win32.join`：忽略空片段，按平台分隔符连接。
pub fn join(platform: &str, parts: &[&str]) -> String {
    let separator = separator(platform);
    let mut joined = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if joined.is_empty() {
            joined.push_str(part);
            continue;
        }
        if joined.ends_with(separator) || joined.ends_with('/') || joined.ends_with('\\') {
            joined.push_str(part);
        } else {
            joined.push(separator);
            joined.push_str(part);
        }
    }
    joined
}

/// 使用当前系统路径语义连接路径（对应 JS 里直接使用原生 `path.join` 的位置）。
pub fn join_host(parts: &[&str]) -> String {
    join(host_platform(), parts)
}

/// 平台感知的 `path.isAbsolute`。
pub fn is_absolute(platform: &str, candidate: &str) -> bool {
    if platform == "win32" {
        let bytes: Vec<char> = candidate.chars().collect();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == ':'
            && (bytes[2] == '\\' || bytes[2] == '/');
        let unc = candidate.starts_with("\\\\");
        drive || unc
    } else {
        candidate.starts_with('/')
    }
}

/// 平台感知的 `path.basename`。
pub fn basename(platform: &str, candidate: &str) -> String {
    let separators: &[char] = if platform == "win32" {
        &['\\', '/']
    } else {
        &['/']
    };
    candidate
        .split(|c| separators.contains(&c))
        .next_back()
        .unwrap_or("")
        .to_string()
}

/// 平台感知的 `path.isAbsolute`，使用当前系统语义。
pub fn is_absolute_host(candidate: &str) -> bool {
    is_absolute(host_platform(), candidate)
}

/// 平台感知的 `path.basename`，使用当前系统语义。
pub fn basename_host(candidate: &str) -> String {
    basename(host_platform(), candidate)
}

/// `dataDirectory()`：默认取当前平台、当前环境变量与当前用户主目录。
pub fn data_directory() -> String {
    let env: Env = std::env::vars().collect();
    let home = home_directory();
    data_directory_with(host_platform(), &env, &home)
}

/// `dataDirectory(platform, env, home)`。
pub fn data_directory_with(platform: &str, env: &Env, home: &str) -> String {
    if platform == "darwin" {
        return join(platform, &[home, "Library", "Application Support", DATA_DIR_NAME]);
    }
    if platform == "win32" {
        let base = match env.get("APPDATA").filter(|value| !value.is_empty()) {
            Some(appdata) => appdata.clone(),
            None => join(platform, &[home, "AppData", "Roaming"]),
        };
        return join(platform, &[&base, DATA_DIR_NAME]);
    }
    let base = match env.get("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(config) => config.clone(),
        None => join(platform, &[home, ".config"]),
    };
    join(platform, &[&base, DATA_DIR_NAME])
}

/// `runtimePackage(platform, arch)` 的返回值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePackage {
    /// npm 包名，例如 `opencode-darwin-arm64`。
    pub name: String,
    /// 包内可执行文件名，例如 `opencode` / `opencode.exe`。
    pub binary: String,
}

/// `runtimePackage(platform, arch)`：不支持的系统或架构返回 `不支持的系统或架构：{platform}/{arch}`。
pub fn runtime_package(platform: &str, arch: &str) -> Result<RuntimePackage, String> {
    if !["darwin", "win32", "linux"].contains(&platform) || !["x64", "arm64"].contains(&arch) {
        return Err(format!("不支持的系统或架构：{platform}/{arch}"));
    }
    let os = if platform == "win32" { "windows" } else { platform };
    Ok(RuntimePackage {
        name: format!("opencode-{os}-{arch}"),
        binary: if platform == "win32" {
            "opencode.exe".to_string()
        } else {
            "opencode".to_string()
        },
    })
}

/// `os.homedir()`：优先 `$HOME`，退回用户目录查询，再退回当前目录。
pub fn home_directory() -> String {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return home;
        }
    }
    if let Some(home) = home_dir_from_passwd() {
        return home;
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

#[cfg(unix)]
fn home_dir_from_passwd() -> Option<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("printf %s \"$HOME\"")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(not(unix))]
fn home_dir_from_passwd() -> Option<String> {
    std::env::var("USERPROFILE").ok().filter(|value| !value.is_empty())
}

/// 便捷函数：把路径文本转成 `Path` 引用（仅用于避免调用方重复写 `Path::new`）。
pub fn as_path(value: &str) -> &Path {
    Path::new(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<HashMap<_, _>>()
    }

    #[test]
    fn macos_data_directory_matches_javascript() {
        assert_eq!(
            data_directory_with("darwin", &env_of(&[]), "/Users/test"),
            "/Users/test/Library/Application Support/Buddy Bridge"
        );
    }

    #[test]
    fn windows_data_directory_prefers_appdata() {
        assert_eq!(
            data_directory_with(
                "win32",
                &env_of(&[("APPDATA", "C:\\Users\\测试\\AppData\\Roaming")]),
                "C:\\Users\\测试"
            ),
            "C:\\Users\\测试\\AppData\\Roaming\\Buddy Bridge"
        );
        assert_eq!(
            data_directory_with("win32", &env_of(&[]), "C:\\Users\\Test"),
            "C:\\Users\\Test\\AppData\\Roaming\\Buddy Bridge"
        );
    }

    #[test]
    fn linux_data_directory_prefers_xdg_config_home() {
        assert_eq!(
            data_directory_with("linux", &env_of(&[("XDG_CONFIG_HOME", "/tmp/config")]), "/home/test"),
            "/tmp/config/Buddy Bridge"
        );
        assert_eq!(
            data_directory_with("linux", &env_of(&[]), "/home/test"),
            "/home/test/.config/Buddy Bridge"
        );
    }

    #[test]
    fn runtime_package_covers_every_supported_triple() {
        assert_eq!(
            runtime_package("win32", "x64").unwrap(),
            RuntimePackage {
                name: "opencode-windows-x64".to_string(),
                binary: "opencode.exe".to_string()
            }
        );
        assert_eq!(runtime_package("win32", "arm64").unwrap().name, "opencode-windows-arm64");
        assert_eq!(runtime_package("darwin", "arm64").unwrap().binary, "opencode");
        assert_eq!(runtime_package("linux", "x64").unwrap().name, "opencode-linux-x64");
    }

    #[test]
    fn unsupported_triples_report_chinese_error() {
        let error = runtime_package("win32", "ia32").unwrap_err();
        assert!(error.contains("不支持"));
        assert!(error.contains("win32/ia32"));
        assert!(runtime_package("solaris", "x64").is_err());
    }

    #[test]
    fn path_helpers_follow_platform_semantics() {
        assert!(is_absolute("win32", "C:\\x"));
        assert!(!is_absolute("win32", "/x"));
        assert!(is_absolute("darwin", "/x"));
        assert_eq!(basename("win32", "C:\\a\\models.json"), "models.json");
        assert_eq!(basename("darwin", "/a/models.json"), "models.json");
        assert_eq!(join("darwin", &["/x/", "models.json"]), "/x/models.json");
        assert_eq!(join("win32", &["C:\\u\\", "Buddy Bridge"]), "C:\\u\\Buddy Bridge");
    }
}
