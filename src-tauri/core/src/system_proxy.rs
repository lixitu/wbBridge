//! `core/src/system-proxy.js` 的 Rust 等价实现（方案B 阶段二）。
//!
//! 读取操作系统的手动代理配置，产出给子进程用的 `HTTP_PROXY`/`HTTPS_PROXY` 环境变量表。
//!
//! | JS | Rust |
//! |---|---|
//! | `parseSystemProxy(text)` | [`parse_system_proxy`]（纯函数） |
//! | `parseWindowsProxy(settings)` | [`parse_windows_proxy`]（纯函数） |
//! | `systemProxyEnvironment(enabled)` | [`system_proxy_environment`]（tarpaulin 之外只读执行 `scutil --proxy` / `powershell.exe`） |
//! | `exec(cmd, args, {timeout: 5000})` | [`COMMAND_TIMEOUT_MS`]：tokio 子进程 + 超时即杀 |
//!
//! 分支选择被单独抽成纯函数 [`environment_from_output`]，这样「平台分派 + 文本解析」可以
//! 在不开子进程的情况下被单测和对拍覆盖；真正开进程的只剩 `run_command` 一层。
//!
//! 已知边界（与 JS 的差异，均已记录）：
//! - 仅当 `enabled` 为真时才可能开子进程；`enabled` 为假时两个实现都不触发 IO；
//! - `windowsHide: true` 在 tokio 侧无对应项（Windows 上会短暂出现控制台窗口），不影响取值；
//! - Windows 地址解析里 `new URL('http://' + value)` 的 IDNA/punycode 归一化未实现，
//!   非 ASCII 代理主机名不在本项目数据范围内；IPv6 只做小写与方括号保留，不做压缩归一化。

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{Map, Value};
use tokio::process::Command;

/// 与 JS `NO_PROXY` / `no_proxy` 完全一致的取值（不带 `0.0.0.0`）。
pub const NO_PROXY: &str = "localhost,127.0.0.1,::1";

/// 子进程超时（对应 `exec(..., { timeout: 5000 })`）。
pub const COMMAND_TIMEOUT_MS: u64 = 5000;

const DARWIN_HTTPS_REQUIRED: &str =
    "请先在 macOS 中启用 HTTPS 系统代理；暂不支持仅 SOCKS 或 PAC 配置";
const WINDOWS_MANUAL_REQUIRED: &str = "请先启用 Windows 手动系统代理；暂不支持仅 PAC 配置";
const WINDOWS_HOST_INVALID: &str = "Windows 系统代理地址无效";
const WINDOWS_PORT_INVALID: &str = "Windows 系统代理端口无效";
const UNSUPPORTED_PLATFORM: &str = "此系统暂不支持读取系统代理";
const SYSTEM_PROXY_INVALID: &str = "系统代理地址无效";

/// 与 JS 版普通 `Error` 对应的失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyError {
    /// 面向调用方的消息。
    pub message: String,
}

impl ProxyError {
    /// 构造一个失败。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ProxyError {}

/// JS 侧普通 `Error` 在服务层的归宿：502 `upstream_error`。
impl From<ProxyError> for crate::protocol::BridgeError {
    fn from(error: ProxyError) -> Self {
        crate::protocol::BridgeError::with(error.message, 502, "upstream_error")
    }
}

/// JS `\s` 的最小可用替身（White_Space 属性；不含 U+FEFF，见模块注释）。
fn is_space(character: char) -> bool {
    character.is_whitespace()
}

fn environment(https: &str, http: &str) -> Value {
    let mut object = Map::new();
    object.insert("HTTP_PROXY".to_string(), Value::String(http.to_string()));
    object.insert("HTTPS_PROXY".to_string(), Value::String(https.to_string()));
    object.insert("http_proxy".to_string(), Value::String(http.to_string()));
    object.insert("https_proxy".to_string(), Value::String(https.to_string()));
    object.insert("NO_PROXY".to_string(), Value::String(NO_PROXY.to_string()));
    object.insert("no_proxy".to_string(), Value::String(NO_PROXY.to_string()));
    Value::Object(object)
}

fn disabled_environment() -> Value {
    let mut object = Map::new();
    object.insert("NO_PROXY".to_string(), Value::String(NO_PROXY.to_string()));
    object.insert("no_proxy".to_string(), Value::String(NO_PROXY.to_string()));
    Value::Object(object)
}

/// `value(name)`：按 `^\s*<name>\s*:\s*(.*?)\s*$`（多行）抓取 `scutil --proxy` 的一行。
fn proxy_value<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let pattern = format!(r"(?m)^\s*{}\s*:\s*(.*?)\s*$", regex::escape(name));
    let compiled = {
        let mut guard = cache.lock().expect("正则缓存锁");
        guard
            .entry(name.to_string())
            .or_insert_with(|| Regex::new(&pattern).expect("合法正则"))
            .clone()
    };
    compiled
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str())
}

/// `address(kind)`：`HTTP`/`HTTPS` 条目的 host:port 规范化为 `http://host:port`。
fn macos_address(text: &str, kind: &str) -> Result<Option<String>, ProxyError> {
    if proxy_value(text, &format!("{kind}Enable")) != Some("1") {
        return Ok(None);
    }
    let host = proxy_value(text, &format!("{kind}Proxy"));
    let port = js_number(proxy_value(text, &format!("{kind}Port")).map(Value::from));
    let host = host.unwrap_or("");
    if host.is_empty()
        || host
            .chars()
            .any(|character| is_space(character) || matches!(character, '/' | '@' | '?' | '#'))
        || !is_integer(port)
        || !(1.0..=65535.0).contains(&port)
    {
        return Err(ProxyError::new(SYSTEM_PROXY_INVALID));
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Ok(Some(format!("http://{host}:{}", port as i64)))
}

/// `parseSystemProxy(text)`。
pub fn parse_system_proxy(text: &str) -> Result<Value, ProxyError> {
    let http = macos_address(text, "HTTP")?;
    let https = match macos_address(text, "HTTPS")? {
        Some(https) => https,
        None => return Err(ProxyError::new(DARWIN_HTTPS_REQUIRED)),
    };
    let http = http.unwrap_or_else(|| https.clone());
    Ok(environment(&https, &http))
}

/// JS `Number.isInteger`。
fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

/// JS `Number(x)`（标量/数组都覆盖，对象一律 NaN）。
pub fn js_number(value: Option<Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => {
            if flag {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => js_number_from_str(&text),
        Some(Value::Array(items)) => match items.len() {
            0 => 0.0,
            1 => js_number(items.into_iter().next()),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// JS `Number(string)`：空串为 0，支持 `0x`/`0b`/`0o` 与 `Infinity`，其余非法为 NaN。
pub fn js_number_from_str(text: &str) -> f64 {
    let trimmed = text.trim_matches(is_space);
    if trimmed.is_empty() {
        return 0.0;
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let lowered = trimmed.to_ascii_lowercase();
    if matches!(
        lowered.as_str(),
        "inf" | "+inf" | "-inf" | "infinity" | "+infinity" | "-infinity"
    ) {
        // Rust 的 `f64::from_str` 接受这些写法，JS 的 `Number` 不接受（除首字母大写的 Infinity）。
        return f64::NAN;
    }
    for (prefix, radix) in [("0x", 16u32), ("0X", 16), ("0b", 2), ("0B", 2), ("0o", 8), ("0O", 8)] {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix)
                .map(|value| value as f64)
                .unwrap_or(f64::NAN);
        }
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

/// JS `||`：返回第一个真值。
fn first_truthy<'a>(left: Option<&'a str>, right: Option<&'a str>) -> Option<&'a str> {
    match left {
        Some(text) if !text.is_empty() => Some(text),
        _ => match right {
            Some(text) if !text.is_empty() => Some(text),
            _ => None,
        },
    }
}

/// `parseWindowsProxy(settings)`。
pub fn parse_windows_proxy(settings: &Value) -> Result<Value, ProxyError> {
    if !truthy_number(js_number(settings.get("ProxyEnable").cloned())) {
        return Err(ProxyError::new(WINDOWS_MANUAL_REQUIRED));
    }
    if !crate::json::truthy(settings.get("ProxyServer")) {
        return Err(ProxyError::new(WINDOWS_MANUAL_REQUIRED));
    }
    let raw = crate::json::display(settings.get("ProxyServer"));
    let entries: Vec<&str> = raw
        .trim_matches(is_space)
        .split(';')
        .filter(|entry| !entry.is_empty())
        .collect();
    let split = entries.iter().any(|entry| entry.contains('='));
    let (http_value, https_value) = if split {
        let mut map: Vec<(String, Option<String>)> = Vec::new();
        for entry in &entries {
            let trimmed = entry.trim_matches(is_space);
            let mut parts = trimmed.split('=');
            let key = parts.next().unwrap_or_default().to_string();
            let value = parts.next().map(|value| value.to_string());
            map.retain(|(existing, _)| existing != &key);
            map.push((key, value));
        }
        let lookup = |name: &str| {
            map.iter()
                .find(|(key, _)| key == name)
                .and_then(|(_, value)| value.clone())
        };
        (lookup("http"), lookup("https"))
    } else {
        let single = entries.first().map(|entry| entry.to_string());
        (single.clone(), single)
    };
    let https = windows_address(first_truthy(https_value.as_deref(), http_value.as_deref()))?;
    let http = windows_address(first_truthy(http_value.as_deref(), https_value.as_deref()))?;
    Ok(environment(&https, &http))
}

fn truthy_number(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

/// JS `value.match(/:(\d+)$/)?.[1]` 后的 `Number(...)`。
fn trailing_port(value: &str) -> f64 {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let compiled = PATTERN.get_or_init(|| Regex::new(r":(\d+)$").expect("合法正则"));
    match compiled.captures(value).and_then(|captures| captures.get(1)) {
        Some(digits) => js_number_from_str(digits.as_str()),
        None => f64::NAN,
    }
}

/// Windows 分支里的 `address(value)`：`new URL('http://' + value).origin` + 端口校验。
fn windows_address(value: Option<&str>) -> Result<String, ProxyError> {
    let value = match value {
        Some(text) if !text.is_empty() => text,
        _ => return Err(ProxyError::new(WINDOWS_HOST_INVALID)),
    };
    if value
        .chars()
        .any(|character| is_space(character) || matches!(character, '/' | '@' | '?' | '#'))
    {
        return Err(ProxyError::new(WINDOWS_HOST_INVALID));
    }
    let origin = url_origin(value).ok_or_else(|| ProxyError::new(WINDOWS_HOST_INVALID))?;
    let port = trailing_port(value);
    if !is_integer(port) || !(1.0..=65535.0).contains(&port) {
        return Err(ProxyError::new(WINDOWS_PORT_INVALID));
    }
    Ok(origin)
}

/// `new URL('http://' + value).origin` 的最小实现（http 专属 scheme）。
///
/// 覆盖：主机名小写、百分号解码、IPv4、方括号 IPv6、默认端口 80 省略、端口非法即失败。
/// 未覆盖：IDNA/punycode、IPv6 压缩归一化（见模块注释）。
fn url_origin(value: &str) -> Option<String> {
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        let inner = &rest[..end];
        let after = &rest[end + 1..];
        if inner.is_empty() || !inner.chars().all(|character| {
            character.is_ascii_hexdigit() || matches!(character, ':' | '.' | 'v' | 'V')
        }) {
            return None;
        }
        let port = match after.strip_prefix(':') {
            None if after.is_empty() => None,
            None => return None,
            // `host:` 这种空端口在 WHATWG 里等同于「没有端口」，不构成解析失败。
            Some("") => None,
            Some(digits) => {
                if !digits.chars().all(|character| character.is_ascii_digit()) {
                    return None;
                }
                Some(parse_port(digits)?)
            }
        };
        (format!("[{}]", inner.to_ascii_lowercase()), port)
    } else {
        let mut authority = value.splitn(2, ':');
        let host = authority.next()?;
        let port = match authority.next() {
            Some(digits) if !digits.is_empty() => {
                if !digits.chars().all(|character| character.is_ascii_digit()) {
                    return None;
                }
                Some(parse_port(digits)?)
            }
            _ => None,
        };
        (decode_host(host)?, port)
    };
    if host.is_empty() {
        return None;
    }
    match port {
        None | Some(80) => Some(format!("http://{host}")),
        Some(port) => Some(format!("http://{host}:{port}")),
    }
}

/// 端口串 → 数字；超过 65535 视为 URL 解析失败（与 WHATWG 一致）。
fn parse_port(digits: &str) -> Option<u32> {
    let port: u64 = digits.parse().ok()?;
    if port > 65535 {
        return None;
    }
    Some(port as u32)
}

/// 主机百分号解码 + ASCII 小写；出现非法字符或非法转义时返回 `None`（URL 解析失败）。
fn decode_host(host: &str) -> Option<String> {
    let mut decoded = String::new();
    let mut chars = host.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' {
            let high = chars.next()?;
            let low = chars.next()?;
            let byte = u8::from_str_radix(&format!("{high}{low}"), 16).ok()?;
            decoded.push(byte as char);
            continue;
        }
        if matches!(
            character,
            '<' | '>' | '"' | '`' | '{' | '}' | '|' | '\\' | '^' | '[' | ']'
        ) {
            return None;
        }
        decoded.push(character);
    }
    if decoded.is_empty() {
        return None;
    }
    Some(decoded.to_ascii_lowercase())
}

/// 平台分派 + 解析（纯函数，`output` 为子进程 stdout 或注册表 JSON 文本）。
pub fn environment_from_output(
    enabled: bool,
    platform: &str,
    output: &str,
) -> Result<Value, ProxyError> {
    if !enabled {
        return Ok(disabled_environment());
    }
    if platform == "win32" {
        let settings: Value = crate::json::parse_json(output)
            .map_err(|error| ProxyError::new(format!("{WINDOWS_MANUAL_REQUIRED}（{error}）")))?;
        return parse_windows_proxy(&settings);
    }
    if platform != "darwin" {
        return Err(ProxyError::new(UNSUPPORTED_PLATFORM));
    }
    parse_system_proxy(output)
}

/// `systemProxyEnvironment(enabled)`。
pub async fn system_proxy_environment(enabled: bool) -> Result<Value, ProxyError> {
    if !enabled {
        return Ok(disabled_environment());
    }
    let platform = runtime_platform();
    if platform == "win32" {
        let script = "Get-ItemProperty -LiteralPath 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' | Select-Object ProxyEnable,ProxyServer | ConvertTo-Json -Compress";
        let output = run_command(
            "powershell.exe",
            &[
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                script.to_string(),
            ],
        )
        .await?;
        return environment_from_output(true, platform, &output);
    }
    if platform != "darwin" {
        return Err(ProxyError::new(UNSUPPORTED_PLATFORM));
    }
    let output = run_command("/usr/sbin/scutil", &["--proxy".to_string()]).await?;
    environment_from_output(true, platform, &output)
}

fn runtime_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    }
}

/// `exec(command, args, { timeout: 5000 })`：超时即杀子进程。
async fn run_command(command: &str, args: &[String]) -> Result<String, ProxyError> {
    let child = Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ProxyError::new(format!("{command} 执行失败：{error}")))?;
    match tokio::time::timeout(Duration::from_millis(COMMAND_TIMEOUT_MS), child.wait_with_output())
        .await
    {
        Ok(Ok(output)) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Ok(Err(error)) => Err(ProxyError::new(format!("{command} 执行失败：{error}"))),
        Err(_) => Err(ProxyError::new(format!("{command} 执行超时"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCUTIL: &str = "HTTPEnable : 1\nHTTPPort : 7890\nHTTPProxy : 127.0.0.1\nHTTPSEnable : 1\nHTTPSPort : 7890\nHTTPSProxy : 127.0.0.1\n";

    #[test]
    fn parses_macos_proxy() {
        let parsed = parse_system_proxy(SCUTIL).expect("解析成功");
        assert_eq!(parsed["HTTP_PROXY"], json!("http://127.0.0.1:7890"));
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://127.0.0.1:7890"));
        assert_eq!(parsed["http_proxy"], json!("http://127.0.0.1:7890"));
        assert_eq!(parsed["https_proxy"], json!("http://127.0.0.1:7890"));
        assert_eq!(parsed["NO_PROXY"], json!(NO_PROXY));
        assert_eq!(parsed["no_proxy"], json!(NO_PROXY));
        let keys: Vec<&String> = parsed.as_object().expect("对象").keys().collect();
        assert_eq!(
            keys,
            vec![
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "http_proxy",
                "https_proxy",
                "NO_PROXY",
                "no_proxy"
            ]
        );
    }

    #[test]
    fn http_entry_falls_back_to_https() {
        let text = "HTTPSEnable : 1\nHTTPSPort : 8080\nHTTPSProxy : proxy.local\nSOCKSEnable : 1\n";
        let parsed = parse_system_proxy(text).expect("解析成功");
        assert_eq!(parsed["HTTP_PROXY"], json!("http://proxy.local:8080"));
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://proxy.local:8080"));
    }

    #[test]
    fn requires_https_entry() {
        let text = "HTTPEnable : 1\nHTTPPort : 8080\nHTTPProxy : 127.0.0.1\nSOCKSEnable : 1\n";
        let error = parse_system_proxy(text).expect_err("缺少 HTTPS");
        assert_eq!(error.message, DARWIN_HTTPS_REQUIRED);
    }

    #[test]
    fn rejects_invalid_macos_address() {
        for text in [
            "HTTPSEnable : 1\nHTTPSPort : 0\nHTTPSProxy : 127.0.0.1\n",
            "HTTPSEnable : 1\nHTTPSPort : 70000\nHTTPSProxy : 127.0.0.1\n",
            "HTTPSEnable : 1\nHTTPSPort : 8o80\nHTTPSProxy : 127.0.0.1\n",
            "HTTPSEnable : 1\nHTTPSPort : 8080\nHTTPSProxy : prox y\n",
            "HTTPSEnable : 1\nHTTPSPort : 8080\nHTTPSProxy : http://127.0.0.1\n",
            "HTTPSEnable : 1\nHTTPSPort : 8080\n",
        ] {
            let error = parse_system_proxy(text).expect_err("地址无效");
            assert_eq!(error.message, SYSTEM_PROXY_INVALID, "{text}");
        }
    }

    #[test]
    fn brackets_ipv6_hosts() {
        let text = "HTTPSEnable : 1\nHTTPSPort : 1080\nHTTPSProxy : ::1\n";
        let parsed = parse_system_proxy(text).expect("解析成功");
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://[::1]:1080"));
        let text = "HTTPSEnable : 1\nHTTPSPort : 1080\nHTTPSProxy : [fe80::1]\n";
        let parsed = parse_system_proxy(text).expect("解析成功");
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://[fe80::1]:1080"));
    }

    #[test]
    fn trims_and_reads_multiline_values() {
        let text = "  HTTPSEnable: 1  \n  HTTPSPort :   3128   \n  HTTPSProxy :  10.0.0.2   \n";
        let parsed = parse_system_proxy(text).expect("解析成功");
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://10.0.0.2:3128"));
    }

    #[test]
    fn parses_windows_single_server() {
        let parsed = parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "127.0.0.1:8080" }))
            .expect("解析成功");
        assert_eq!(parsed["HTTP_PROXY"], json!("http://127.0.0.1:8080"));
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://127.0.0.1:8080"));
        assert_eq!(parsed["NO_PROXY"], json!(NO_PROXY));
    }

    #[test]
    fn parses_windows_per_scheme_servers() {
        let parsed = parse_windows_proxy(&json!({
            "ProxyEnable": "1",
            "ProxyServer": "http=10.1.1.1:80;https=secure.local:8443"
        }))
        .expect("解析成功");
        // http=…:80 命中默认端口，会被 URL 归一化掉
        assert_eq!(parsed["HTTP_PROXY"], json!("http://10.1.1.1"));
        assert_eq!(parsed["HTTPS_PROXY"], json!("http://secure.local:8443"));
    }

    #[test]
    fn windows_requires_enable_and_server() {
        let error = parse_windows_proxy(&json!({ "ProxyEnable": 0, "ProxyServer": "a:1" }))
            .expect_err("未启用");
        assert_eq!(error.message, WINDOWS_MANUAL_REQUIRED);
        let error = parse_windows_proxy(&json!({ "ProxyEnable": 1 })).expect_err("无服务器");
        assert_eq!(error.message, WINDOWS_MANUAL_REQUIRED);
        let error = parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "" }))
            .expect_err("空服务器");
        assert_eq!(error.message, WINDOWS_MANUAL_REQUIRED);
    }

    #[test]
    fn windows_rejects_bad_host_and_port() {
        let bad_host = parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "a b:80" }))
            .expect_err("地址无效");
        assert_eq!(bad_host.message, WINDOWS_HOST_INVALID);
        let bad_url =
            parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "a:b:c" })).expect_err("URL 无效");
        assert_eq!(bad_url.message, WINDOWS_HOST_INVALID);
        let no_port =
            parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "host:" })).expect_err("端口无效");
        assert_eq!(no_port.message, WINDOWS_PORT_INVALID);
        let zero =
            parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "host:0" })).expect_err("端口越界");
        assert_eq!(zero.message, WINDOWS_PORT_INVALID);
        let big = parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": "host:70000" }))
            .expect_err("URL 端口越界");
        assert_eq!(big.message, WINDOWS_HOST_INVALID);
    }

    #[test]
    fn windows_handles_empty_entries() {
        let error = parse_windows_proxy(&json!({ "ProxyEnable": 1, "ProxyServer": ";;" }))
            .expect_err("无可用条目");
        assert_eq!(error.message, WINDOWS_HOST_INVALID);
    }

    #[test]
    fn disabled_environment_has_only_no_proxy() {
        let value = environment_from_output(false, "darwin", "").expect("禁用分支");
        assert_eq!(value, json!({ "NO_PROXY": NO_PROXY, "no_proxy": NO_PROXY }));
        // 禁用分支不读平台，任何系统都返回同一结果
        let value = environment_from_output(false, "linux", "").expect("禁用分支");
        assert_eq!(value, json!({ "NO_PROXY": NO_PROXY, "no_proxy": NO_PROXY }));
    }

    #[test]
    fn unsupported_platform_is_reported() {
        let error = environment_from_output(true, "linux", "").expect_err("不支持");
        assert_eq!(error.message, UNSUPPORTED_PLATFORM);
    }

    #[test]
    fn platform_dispatch_reads_output() {
        let value = environment_from_output(true, "darwin", SCUTIL).expect("darwin");
        assert_eq!(value["HTTPS_PROXY"], json!("http://127.0.0.1:7890"));
        let value = environment_from_output(
            true,
            "win32",
            "{\"ProxyEnable\":1,\"ProxyServer\":\"127.0.0.1:8080\"}",
        )
        .expect("win32");
        assert_eq!(value["HTTPS_PROXY"], json!("http://127.0.0.1:8080"));
        let error =
            environment_from_output(true, "win32", "{oops").expect_err("注册表输出非法 JSON");
        assert!(error.message.starts_with(WINDOWS_MANUAL_REQUIRED));
    }

    #[test]
    fn js_number_matches_javascript() {
        assert!(js_number(None).is_nan());
        assert_eq!(js_number(Some(json!(null))), 0.0);
        assert_eq!(js_number(Some(json!(true))), 1.0);
        assert_eq!(js_number(Some(json!("0x50"))), 80.0);
        assert_eq!(js_number(Some(json!(""))), 0.0);
        assert_eq!(js_number(Some(json!("  "))), 0.0);
        assert_eq!(js_number(Some(json!("12.5"))), 12.5);
        assert_eq!(js_number(Some(json!("1e3"))), 1000.0);
        assert!(js_number(Some(json!("Infinity"))).is_infinite());
        assert!(js_number(Some(json!("infinity"))).is_nan());
        assert!(js_number(Some(json!("8o80"))).is_nan());
        assert!(js_number(Some(json!("item"))).is_nan());
        assert_eq!(js_number(Some(json!(["7"]))), 7.0);
        assert_eq!(js_number(Some(json!([]))), 0.0);
        assert!(js_number(Some(json!([1, 2]))).is_nan());
        assert!(js_number(Some(json!({ "a": 1 }))).is_nan());
    }

    #[test]
    fn proxy_error_maps_to_upstream_error() {
        let error = crate::protocol::BridgeError::from(ProxyError::new("boom"));
        assert_eq!(error.status, 502);
        assert_eq!(error.code, "upstream_error");
    }
}
