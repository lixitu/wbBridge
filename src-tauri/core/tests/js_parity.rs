//! JS↔Rust 对拍测试。
//!
//! **迁移后（无 Node 环境）的默认形态**：用例的期望值是 `tests/fixtures/*.json` 里每个用例的
//! `expected` 字段 —— 迁移前用 Node 真实加载 `core/src/*.js` 录制的 JS 真相快照（已按下面的
//! 规范化规则抹平随机值）。本测试只把 Rust 实现与冻结快照逐条比较，**不启动 Node**。
//!
//! 需要重新录制时（例如改动纯逻辑模块并想刷新黄金快照），把归档在
//! `../../../backup/wbBridge-node-20261001/`（仓库根的上一级 `…/trexwb/backup/`）的 `core/`
//! 与 `core-rs-tests-js/`（即 `tests/js/`）放回原位，再运行
//! `WB_PARITY_RECORD=1 cargo test --test js_parity`；描述器会用同一份输入跑一遍 JS，
//! 把输出写回 fixture。
//!
//! - 用例数据放在 `tests/fixtures/*.json`（本 crate 内），沙箱目录写在 `target/tmp/js-parity/`；
//! - 「Rust 与快照不一致」即失败，并打印完整的期望/实际对照。
//!
//! 约定（与描述器一一对应）：
//! - 用例里的 `$BASE` 会被替换成该用例专属的沙箱目录，两侧看到同一批路径；
//!   两侧输出里出现的沙箱绝对路径都会在比较前还原成 `$BASE`，快照因此不绑定机器与目录布局；
//! - `Date` 被冻结在 `FIXED_ISO`，`model-status` 的时间戳与 `protocol.completion` 的
//!   `id` / `created`（随机/时间派生）在比较前被规范化；
//! - `repair.repair` 诊断里的 `ms`（耗时）同样被规范化。

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Map, Value};

use wbbridge_core::{
    atomic, handoff, json as json_lib, model_status, platform, protocol, reasoning, repair, sync,
    system_proxy, workbuddy_config,
};

/// 与 `tests/js/describe.mjs` 的 `FIXED_NOW` 一致；两侧时间被冻结到同一点。
const FIXED_ISO: &str = "2026-09-30T00:00:00.000Z";
/// `Date.parse(FIXED_ISO)`：`protocol.completion` 的 `created` 用它对齐。
const FIXED_MILLIS: i64 = 1_790_726_400_000;

#[test]
fn json_module_matches_javascript() {
    run_fixture("json");
}

#[test]
fn platform_module_matches_javascript() {
    run_fixture("platform");
}

#[test]
fn workbuddy_config_module_matches_javascript() {
    run_fixture("workbuddy_config");
}

#[test]
fn reasoning_module_matches_javascript() {
    run_fixture("reasoning");
}

#[test]
fn model_status_module_matches_javascript() {
    run_fixture("model_status");
}

#[test]
fn handoff_module_matches_javascript() {
    run_fixture("handoff");
}

#[test]
fn atomic_module_matches_javascript() {
    run_fixture("atomic");
}

#[test]
fn protocol_module_matches_javascript() {
    run_fixture("protocol");
}

#[test]
fn repair_module_matches_javascript() {
    run_fixture("repair");
}

#[test]
fn sync_module_matches_javascript() {
    run_fixture("sync");
}

#[test]
fn system_proxy_module_matches_javascript() {
    run_fixture("system_proxy");
}

// ---------------------------------------------------------------------------
// 用例驱动
// ---------------------------------------------------------------------------

/// 跑完一个 fixture 文件里的全部用例，任一差异都会让测试失败并打印对照。
fn run_fixture(module: &str) {
    let path = fixture_path(module);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 fixture {} 失败：{error}", path.display()));
    let mut fixture: Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} 不是合法 JSON：{error}", path.display()));
    let case_count = fixture
        .get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{} 缺少 cases 数组", path.display()))
        .len();
    assert!(case_count > 0, "{} 没有任何用例", path.display());
    let cases = fixture
        .get("cases")
        .and_then(Value::as_array)
        .expect("cases 数组存在");

    let file_tools = fixture.get("tools").cloned();
    let file_models = fixture.get("models").cloned();
    let mut failures: Vec<String> = Vec::new();
    let mut recorded: HashMap<usize, Value> = HashMap::new();

    for (index, case) in cases.iter().enumerate() {
        let name = case
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("(未命名用例)");
        let op = case
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("「{name}」缺少 op"));

        let sandbox = sandbox_root().join(format!("{module}-{index}"));
        if sandbox.exists() {
            let _ = std::fs::remove_dir_all(&sandbox);
        }
        std::fs::create_dir_all(&sandbox)
            .unwrap_or_else(|error| panic!("创建沙箱 {} 失败：{error}", sandbox.display()));
        let base = sandbox.to_string_lossy().replace('\\', "/");

        write_case_files(case, &sandbox, &base);

        let mut input = substitute(case.get("input"), &base).unwrap_or_else(|| json!({}));
        if !input.is_object() {
            input = json!({});
        }
        inject_input(
            &mut input,
            op,
            tools_for(case, file_tools.as_ref(), &base),
            models_for(case, file_models.as_ref(), &base),
        );

        // 期望值来自 fixture 里录好的 `expected`（JS 真相快照）；只有显式开启录制模式才会
        // 启动 Node 去跑 `core/src/*.js`（归档后需要先把 JS 源码放回原位）。
        let recording = std::env::var("WB_PARITY_RECORD").map(|value| value == "1").unwrap_or(false);
        let expected = if recording {
            let mut value = call_javascript(op, &input, &sandbox, name);
            normalize(op, &mut value);
            mask_sandbox_paths(&mut value, &base);
            recorded.insert(index, value.clone());
            value
        } else {
            let mut value = case.get("expected").cloned().unwrap_or_else(|| {
                panic!(
                    "用例「{name}」(op = {op}) 缺少 expected 快照；如需重新录制，请恢复 Node 源码后运行 \
                     `WB_PARITY_RECORD=1 cargo test --test js_parity`"
                )
            });
            mask_sandbox_paths(&mut value, &base);
            value
        };
        // `sync.*` 会真的改动沙箱（写 models.json、留 .bak、加锁再解锁），跑完 JS 后
        // 必须把沙箱还原成用例声明的初始状态，否则 Rust 读到的是 JS 的产物而不是用例输入，
        // 「无变化判定」「.bak 内容」这类断言会整片失真。
        if op.starts_with("sync.") {
            reset_case_files(case, &sandbox, &base);
        }
        let actual = match rust_output(op, &input) {
            Ok(value) => value,
            Err(message) => json!({ "error": { "message": message } }),
        };
        let mut actual = actual;
        normalize(op, &mut actual);
        mask_sandbox_paths(&mut actual, &base);

        if canonical(&expected) != canonical(&actual) {
            failures.push(format!(
                "用例「{name}」(op = {op})\n  差异：{}",
                diff(&expected, &actual)
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{}：{} 个用例的 Rust 实现与 JS 行为不一致\n\n{}",
            path.display(),
            failures.len(),
            failures.join("\n\n")
        );
    }
    if !recorded.is_empty() {
        // 录制模式：把本次 JS 的（已归一化）输出写回 fixture，成为后续的黄金快照。
        let total = recorded.len();
        if let Some(list) = fixture
            .get_mut("cases")
            .and_then(Value::as_array_mut)
        {
            for (index, value) in recorded {
                list[index]["expected"] = value;
            }
        }
        let text =
            serde_json::to_string_pretty(&fixture).expect("fixture 可序列化");
        std::fs::write(&path, format!("{text}\n"))
            .unwrap_or_else(|error| panic!("写回 fixture {} 失败：{error}", path.display()));
        println!("{module}: 已录制 {total} 个用例的 JS 快照");
    }
    println!("{module}: {case_count} 个用例与 JS 实现完全一致");
}

/// 把输出里的沙箱绝对路径还原成 `$BASE` 占位符。
///
/// 冻结快照里若留着录制机的绝对路径，换机器、换目录布局（例如 crate 从 `core-rs/` 移到
/// `src-tauri/core/`）都会让对拍整片误报；比较前两侧统一还原成占位符即可避免。
fn mask_sandbox_paths(value: &mut Value, base: &str) {
    match value {
        Value::Object(map) => {
            for item in map.values_mut() {
                mask_sandbox_paths(item, base);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                mask_sandbox_paths(item, base);
            }
        }
        Value::String(text) if text.contains(base) => {
            *text = text.replace(base, "$BASE");
        }
        _ => {}
    }
}

/// 比较用的归一化：随机/时间派生的字段在比较前规范化。
fn normalize(op: &str, output: &mut Value) {
    match op {
        // 通行做法（文档之外的既有约定）：id / created 由服务端生成，语义无关，仅比较其余字段。
        "protocol.completion" => {
            if let Some(map) = output.as_object_mut() {
                map.insert("id".to_string(), Value::String(String::new()));
                map.insert("created".to_string(), json!(0));
            }
        }
        // `repair.repair` 的诊断里带耗时（毫秒），不同实现必然不同。
        "repair.repair" => {
            if let Some(records) = output
                .pointer_mut("/meta/repaired")
                .and_then(Value::as_object_mut)
            {
                for record in records.values_mut() {
                    if let Some(map) = record.as_object_mut() {
                        if map.contains_key("ms") {
                            map.insert("ms".to_string(), json!(0));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    // `sync.*` 的失败文案只有「错误类别」可跨语言对齐：ENOENT 来自 Node 的 fs，
    // 非法 JSON 的细节来自 V8，两者必然不同；`kind = other` 的几处是同一批字面量，继续逐字比对。
    if op.starts_with("sync.") {
        blank_sync_messages(output);
    }
    // 随机标识（工具调用 id、completion id）由 `randomUUID` 生成，两侧必然不同；
    // 无论嵌套多深、挂在哪个键下，都在比较前统一抹平，避免拿随机值直接比对。
    mask_generated_ids(output);
}

/// 比较用规范化：递归排序对象键后序列化（两侧都是松散 JSON）。
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, String)> = map
                .iter()
                .map(|(key, item)| (key.clone(), canonical(item)))
                .collect();
            entries.sort();
            let body = entries
                .into_iter()
                .map(|(key, text)| {
                    format!("{}:{}", json_lib::js_stringify(&Value::String(key)), text)
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Array(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        other => json_lib::js_stringify(other),
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

/// 递归比对两份输出，只列出真正不同的字段路径（截断展示，避免整段 system 提示词刷屏）。
fn diff(expected: &Value, actual: &Value) -> String {
    let mut lines: Vec<String> = Vec::new();
    collect_diff("$", expected, actual, &mut lines);
    if lines.is_empty() {
        "(结构一致，差异在序列化层面)".to_string()
    } else {
        lines.join("\n        ")
    }
}

fn collect_diff(path: &str, expected: &Value, actual: &Value, lines: &mut Vec<String>) {
    if lines.len() >= 12 {
        return;
    }
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, left_item) in left {
                match right.get(key) {
                    Some(right_item) => collect_diff(&format!("{path}.{key}"), left_item, right_item, lines),
                    None => lines.push(format!("{path}.{key}：JS 有、Rust 缺 | JS={}", clip(left_item))),
                }
                if lines.len() >= 12 {
                    return;
                }
            }
            for (key, right_item) in right {
                if !left.contains_key(key) {
                    lines.push(format!("{path}.{key}：Rust 多出该键 | Rust={}", clip(right_item)));
                    if lines.len() >= 12 {
                        return;
                    }
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                lines.push(format!(
                    "{path}：数组长度不同 | JS={} 项，Rust={} 项",
                    left.len(),
                    right.len()
                ));
            }
            for (index, (left_item, right_item)) in left.iter().zip(right.iter()).enumerate() {
                collect_diff(&format!("{path}[{index}]"), left_item, right_item, lines);
                if lines.len() >= 12 {
                    return;
                }
            }
        }
        _ => {
            if expected != actual {
                lines.push(format!(
                    "{path}：取值不同 | JS={} | Rust={}",
                    clip(expected),
                    clip(actual)
                ));
            }
        }
    }
}

fn clip(value: &Value) -> String {
    const LIMIT: usize = 160;
    let text = pretty(value);
    if text.chars().count() > LIMIT {
        format!("{}…", text.chars().take(LIMIT).collect::<String>())
    } else {
        text
    }
}

/// 递归把所有「服务端随机生成的标识」抹成 `<generated>`。
///
/// - `call_<32hex>`：`protocol.decode` 里 `call_${randomUUID().replaceAll('-','')}`；
/// - `chatcmpl-<uuid>`：`protocol.completion` 里 `chatcmpl-${randomUUID()}`。
///
/// 只按「前缀 + 足够长的十六进制/UUID 字符集」判定，用例中写死的短 id（如 `call_1`）
/// 与固定值（如 `chatcmpl-fixed`）不满足条件，仍会逐字比对。
fn mask_generated_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for item in map.values_mut() {
                mask_generated_ids(item);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                mask_generated_ids(item);
            }
        }
        Value::String(text)
            if is_generated_id(text) => {
                *text = "<generated>".to_string();
            }
        _ => {}
    }
}

fn is_generated_id(value: &str) -> bool {
    ["call_", "chatcmpl-"].iter().any(|prefix| {
        value
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.len() >= 16 && rest.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-'))
    })
}

// ---------------------------------------------------------------------------
// 用例输入准备
// ---------------------------------------------------------------------------

/// 按用例的 `files` 声明在沙箱里落盘，保证 JS 与 Rust 读到的文件字节一致。
fn write_case_files(case: &Value, sandbox: &Path, base: &str) {
    let Some(files) = case.get("files").and_then(Value::as_object) else {
        return;
    };
    for (raw_name, content) in files {
        let name = raw_name.replace("$BASE", base);
        let path = if name.starts_with('/') {
            PathBuf::from(&name)
        } else {
            sandbox.join(&name)
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let bytes = match content {
            Value::String(text) => text.replace("$BASE", base).into_bytes(),
            Value::Object(map) => match map.get("json") {
                Some(Value::String(text)) => text.clone().into_bytes(),
                Some(other) => other.to_string().into_bytes(),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        std::fs::write(&path, bytes)
            .unwrap_or_else(|error| panic!("写用例文件 {} 失败：{error}", path.display()));
    }
}

/// 把输入里所有字符串（含对象键）的 `$BASE` 换成沙箱绝对路径，与描述器的 `replaceBase` 对齐。
fn substitute(value: Option<&Value>, base: &str) -> Option<Value> {
    match value? {
        Value::String(text) => Some(Value::String(text.replace("$BASE", base))),
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                .map(|item| substitute(Some(item), base).expect("数组元素可替换"))
                .collect(),
        )),
        Value::Object(map) => Some(Value::Object(
            map.iter()
                .map(|(key, item)| {
                    (
                        key.replace("$BASE", base),
                        substitute(Some(item), base).expect("对象值可替换"),
                    )
                })
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

/// 用例级 `toolsOverride` 优先，否则用 fixture 的全局 `tools`。
fn tools_for(case: &Value, file_tools: Option<&Value>, base: &str) -> Option<Value> {
    match case.get("toolsOverride") {
        Some(overridden) => substitute(Some(overridden), base),
        None => file_tools.and_then(|value| substitute(Some(value), base)),
    }
}

/// 用例级 `modelsOverride` 优先，否则用 fixture 的全局 `models`。
fn models_for(case: &Value, file_models: Option<&Value>, base: &str) -> Option<Value> {
    match case.get("modelsOverride") {
        Some(overridden) => substitute(Some(overridden), base),
        None => file_models.and_then(|value| substitute(Some(value), base)),
    }
}

/// 把 fixture 的全局 `tools` / `models` 注入到输入里，让两侧看到完全相同的入参。
fn inject_input(input: &mut Value, op: &str, tools: Option<Value>, models: Option<Value>) {
    let Some(map) = input.as_object_mut() else {
        return;
    };
    if op.starts_with("protocol.") {
        if let Some(models) = models {
            map.insert("models".to_string(), models);
        }
        if let Some(tools) = tools {
            if let Some(body) = map.get_mut("body").and_then(Value::as_object_mut) {
                body.insert("tools".to_string(), tools);
            }
        }
        return;
    }
    if op.starts_with("handoff.") || op.starts_with("repair.") || op.starts_with("atomic.") {
        if let Some(tools) = tools {
            map.insert("tools".to_string(), tools);
        }
    }
}

fn array_of(input: &Value, key: &str) -> Vec<Value> {
    input
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn env_of(input: &Value) -> HashMap<String, String> {
    input
        .get("env")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn required_str(input: &Value, key: &str) -> Result<String, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("用例缺少字符串字段 {key}"))
}

/// 把 JS 侧捕获的 `{ code, message }`（可能缺席或为 null）折算成 `(message, code)`。
fn failure(value: &Option<Value>) -> Option<(String, String)> {
    let value = value.as_ref()?;
    if value.is_null() {
        return None;
    }
    Some((
        value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        value
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    ))
}

// ---------------------------------------------------------------------------
// JS 侧调用
// ---------------------------------------------------------------------------

fn call_javascript(op: &str, input: &Value, sandbox: &Path, case_name: &str) -> Value {
    let harness = harness_path();
    let payload = json!({ "op": op, "input": input }).to_string();
    let mut child = Command::new(node_binary())
        .arg(&harness)
        .arg(sandbox)
        .current_dir(repo_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "用例「{case_name}」无法启动 node：{error}；请确认 node 在 PATH 中、且 {} 存在",
                harness.display()
            )
        });
    child
        .stdin
        .as_mut()
        .expect("node 的 stdin 可用")
        .write_all(payload.as_bytes())
        .unwrap_or_else(|error| panic!("用例「{case_name}」写入用例 JSON 失败：{error}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("用例「{case_name}」等待 node 结束失败：{error}"));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "用例「{case_name}」的 JS 输出不是合法 JSON：{error}\n状态码：{:?}\nstdout：{stdout}\nstderr：{stderr}",
            output.status.code()
        )
    });
    if let Some(message) = envelope.pointer("/error/message").and_then(Value::as_str) {
        panic!("用例「{case_name}」的 JS 描述器抛错：{message}\nstderr：{stderr}");
    }
    envelope.get("output").cloned().unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// Rust 侧求值
// ---------------------------------------------------------------------------

fn rust_output(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "json.parseJson" => {
            let text = required_str(input, "text")?;
            Ok(match json_lib::parse_json(&text) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(_) => json!({ "ok": false }),
            })
        }
        "platform.dataDirectory" => {
            let name = required_str(input, "platform")?;
            let home = required_str(input, "home")?;
            Ok(Value::String(platform::data_directory_with(
                &name,
                &env_of(input),
                &home,
            )))
        }
        "platform.runtimePackage" => {
            let name = required_str(input, "platform")?;
            let arch = required_str(input, "arch")?;
            Ok(match platform::runtime_package(&name, &arch) {
                Ok(package) => json!({ "ok": true, "value": { "name": package.name, "binary": package.binary } }),
                Err(message) => json!({ "ok": false, "message": message }),
            })
        }
        "atomic.replaceWithRetry" => {
            let plan: Vec<Option<String>> = input
                .get("plan")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let delays: Vec<u64> = input
                .get("delays")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_else(|| atomic::DELAYS.to_vec());
            let mut attempt = 0usize;
            let mut sleeps: Vec<u64> = Vec::new();
            let result = {
                let mut rename = |_temp: &Path, _target: &Path| {
                    let step = plan.get(attempt).cloned().flatten();
                    attempt += 1;
                    match step {
                        Some(code) => Err(atomic::ReplaceError::new(Some(&code), code.clone())),
                        None => Ok(()),
                    }
                };
                let mut sleep = |millis: u64| sleeps.push(millis);
                match atomic::replace_with_retry_with(
                    Path::new("temp-path"),
                    Path::new("target-path"),
                    &mut rename,
                    &mut sleep,
                    &delays,
                ) {
                    Ok(()) => json!({ "ok": true }),
                    Err(error) => json!({ "code": error.code }),
                }
            };
            Ok(json!({ "sleeps": sleeps, "result": result }))
        }
        "workbuddy_config.validateModelsFile" => {
            // JS 侧对「非字符串 / 相对路径 / 非 models.json」给出同一条报错；Rust 是静态类型，
            // 非字符串入参不存在，等价地落到同一条非法路径分支，保证两侧输出一致。
            let file = input.get("file").and_then(Value::as_str).unwrap_or("");
            Ok(match workbuddy_config::validate_models_file(file) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(workbuddy_config::ConfigError::Io(_)) => {
                    json!({ "ok": false, "kind": "io", "message": "" })
                }
                Err(workbuddy_config::ConfigError::Json(_)) => {
                    json!({ "ok": false, "kind": "json", "message": "" })
                }
                Err(workbuddy_config::ConfigError::Invalid(message)) => {
                    json!({ "ok": false, "kind": "invalid", "message": message })
                }
            })
        }
        "workbuddy_config.resolveModelsFile" => {
            let home = required_str(input, "home")?;
            let saved = input.get("saved").and_then(Value::as_str);
            Ok(workbuddy_config::resolve_models_file(saved, &env_of(input), &home)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "reasoning.reasoningEfforts" => {
            let model = input.get("model").cloned().unwrap_or(Value::Null);
            Ok(Value::Object(reasoning::reasoning_efforts(&model)))
        }
        "reasoning.workBuddyReasoning" => {
            let model = input.get("model").cloned().unwrap_or(Value::Null);
            Ok(reasoning::work_buddy_reasoning(&model))
        }
        "model_status.modelResult" => {
            let ok = input.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let message = input.get("message").and_then(Value::as_str).unwrap_or("");
            Ok(model_status::model_result_at(
                FIXED_ISO,
                ok,
                message,
                input.get("status"),
                input.get("code").and_then(Value::as_str),
            ))
        }
        "model_status.withRequestMeta" => {
            let mut result = input.get("result").cloned().unwrap_or(Value::Null);
            if !result.is_object() {
                result = json!({});
            }
            let meta = input.get("meta").cloned().unwrap_or(Value::Null);
            model_status::with_request_meta(&mut result, &meta);
            Ok(result)
        }
        "model_status.clientModelID" => {
            let model = input.get("model").cloned().unwrap_or(Value::Null);
            Ok(Value::String(model_status::client_model_id(&model)))
        }
        "handoff.buildHandoff" => {
            let native = input.get("native");
            let args = input
                .get("args")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| json!({}));
            Ok(handoff::build_handoff(native, &args, &array_of(input, "tools"))
                .unwrap_or(Value::Null))
        }
        "handoff.handoffInput" => {
            let action = input.get("action").cloned().unwrap_or(Value::Null);
            let permission = input.get("permission").cloned().unwrap_or(Value::Null);
            Ok(handoff::handoff_input(&action, &permission))
        }
        "handoff.rejectFeedback" => {
            let reason = input.get("reason").and_then(Value::as_str).unwrap_or("");
            Ok(Value::String(handoff::reject_feedback(
                input.get("native"),
                reason,
            )))
        }
        "handoff.validateAction" => {
            let candidate = input.get("candidate").cloned().unwrap_or(Value::Null);
            Ok(match handoff::validate_action(&candidate, &array_of(input, "tools")) {
                Ok(value) => json!({ "ok": value }),
                Err(error) => json!({ "threw": { "code": error.code, "message": error.message } }),
            })
        }
        "protocol.prepare" => {
            let body = input.get("body").cloned().unwrap_or(Value::Null);
            Ok(
                match protocol::prepare(&body, &array_of(input, "models")) {
                    Ok(prepared) => json!({ "ok": true, "value": prepared.to_json() }),
                    Err(error) => json!({ "ok": false, "code": error.code, "status": error.status, "message": error.message }),
                },
            )
        }
        "protocol.decode" => {
            let body = input.get("body").cloned().unwrap_or(Value::Null);
            let prepared = match protocol::prepare(&body, &array_of(input, "models")) {
                Ok(prepared) => prepared,
                Err(error) => return Ok(json!({ "setup": "prepare_failed", "code": error.code })),
            };
            let text = required_str(input, "text")?;
            Ok(match protocol::decode(&text, &prepared) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(error) => json!({ "ok": false, "code": error.code, "status": error.status, "message": error.message }),
            })
        }
        "protocol.completion" => {
            let model = required_str(input, "model")?;
            let message = input.get("message").cloned().unwrap_or(Value::Null);
            Ok(protocol::completion_with(
                "",
                FIXED_MILLIS,
                &model,
                &message,
                input.get("tokens"),
            ))
        }
        "protocol.sendSSE" => {
            let result = input.get("result").cloned().unwrap_or(Value::Null);
            let include_usage = json_lib::truthy(input.get("includeUsage"));
            let role_sent = json_lib::truthy(input.get("roleSent"));
            Ok(Value::String(protocol::send_sse(
                &result,
                include_usage,
                role_sent,
            )))
        }
        "repair.repairSystem" => Ok(Value::String(repair::REPAIR_SYSTEM.to_string())),
        "repair.clientConventions" => Ok(Value::String(repair::client_conventions(&array_of(
            input, "tools",
        )))),
        "repair.rawMaterial" => {
            let response = input
                .get("response")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({}));
            let request = input
                .get("request")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({}));
            Ok(repair::raw_material(
                &response,
                &request,
                input.get("adapterError"),
            ))
        }
        "repair.toolCatalog" => Ok(Value::Array(repair::tool_catalog(&array_of(input, "tools")))),
        "repair.repairBody" => {
            let shape = input.get("shape").and_then(Value::as_str).unwrap_or("");
            let material = input.get("material").cloned().unwrap_or(Value::Null);
            Ok(Value::String(repair::repair_body(
                shape,
                &array_of(input, "tools"),
                &material,
                input.get("blocked"),
            )))
        }
        "repair.extractJson" => Ok(repair::extract_json(input.get("text")).unwrap_or(Value::Null)),
        "repair.translatorRequest" => {
            let model = required_str(input, "model")?;
            let body = required_str(input, "body")?;
            Ok(repair::translator_request(&model, &body))
        }
        "repair.resendPrompt" => Ok(Value::String(repair::resend_prompt(
            input.get("error"),
            input.get("repair"),
            input.get("blocked"),
        ))),
        "repair.repair" => {
            let reply = input.get("reply").cloned().unwrap_or(Value::Null);
            let reply_error = input.get("replyError").cloned();
            let validate_rejects = input.get("validateRejects").cloned();
            let translator = input
                .get("translator")
                .and_then(Value::as_str)
                .map(str::to_string);
            let request = input.get("request").cloned().unwrap_or_else(|| json!({}));
            let shape = input
                .get("shape")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let material = input.get("material").cloned().unwrap_or(Value::Null);
            let blocked = input.get("blocked").cloned();
            let mut meta = input
                .get("meta")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({}));
            let mut complete = move |_request: &Value| -> wbbridge_core::server::BoxFuture<Result<Value, protocol::BridgeError>> {
                let reply = reply.clone();
                let reply_error = reply_error.clone();
                Box::pin(async move {
                    match failure(&reply_error) {
                        Some((message, code)) => Err(protocol::BridgeError::with(message, 0, &code)),
                        None => Ok(reply),
                    }
                })
            };
            let mut validate = move |candidate: &Value| -> Result<Value, protocol::BridgeError> {
                match failure(&validate_rejects) {
                    Some((message, code)) => Err(protocol::BridgeError::with(message, 0, &code)),
                    None => Ok(candidate.clone()),
                }
            };
            let mut log = |_line: &str| {};
            let result = futures::executor::block_on(repair::repair(repair::RepairDeps {
                complete: &mut complete,
                translator,
                request: &request,
                shape: &shape,
                material: &material,
                blocked: blocked.as_ref(),
                validate: &mut validate,
                meta: &mut meta,
                log: &mut log,
            }));
            Ok(json!({ "result": result.unwrap_or(Value::Null), "meta": meta }))
        }
        "sync.mergeModels" => {
            let document = input.get("document").cloned().unwrap_or(Value::Null);
            let options = sync_options(input);
            let endpoint = required_str(input, "endpoint")?;
            let key = required_str(input, "key")?;
            Ok(match sync::merge_models(
                &document,
                &array_of(input, "models"),
                &endpoint,
                &key,
                options.allow_empty,
                options.shape,
            ) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(error) => sync_failure(&error),
            })
        }
        "sync.syncModels" => {
            let file = required_str(input, "file")?;
            let models = array_of(input, "models");
            let endpoint = required_str(input, "endpoint")?;
            let key = required_str(input, "key")?;
            let options = sync_options(input);
            let reply = sync_call(&file, &models, &endpoint, &key, options);
            let mut map = reply.as_object().cloned().unwrap_or_default();
            map.insert("files".to_string(), Value::Object(snapshot_parent(&file)?));
            Ok(Value::Object(map))
        }
        "sync.syncModelsTwice" => {
            let file = required_str(input, "file")?;
            let models = array_of(input, "models");
            let endpoint = required_str(input, "endpoint")?;
            let key = required_str(input, "key")?;
            let options = sync_options(input);
            let mut object = Map::new();
            object.insert(
                "first".to_string(),
                sync_call(&file, &models, &endpoint, &key, options),
            );
            object.insert(
                "second".to_string(),
                sync_call(&file, &models, &endpoint, &key, options),
            );
            object.insert("files".to_string(), Value::Object(snapshot_parent(&file)?));
            Ok(Value::Object(object))
        }
        "system_proxy.parseSystemProxy" => {
            let text = input.get("text").and_then(Value::as_str).unwrap_or("");
            Ok(match system_proxy::parse_system_proxy(text) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(error) => json!({ "ok": false, "message": error.message }),
            })
        }
        "system_proxy.parseWindowsProxy" => {
            let settings = input
                .get("settings")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| json!({}));
            Ok(match system_proxy::parse_windows_proxy(&settings) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(error) => json!({ "ok": false, "message": error.message }),
            })
        }
        "system_proxy.systemProxyEnvironment" => {
            let enabled = input.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            // 与 JS 同源的纯函数入口：`enabled = false` 不触碰平台分支，两侧结果都应一致；
            // `enabled = true` 会真的去跑 scutil/powershell，平台相关，不放进对拍。
            Ok(match system_proxy::environment_from_output(enabled, "darwin", "") {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(error) => json!({ "ok": false, "message": error.message }),
            })
        }
        "system_proxy.jsNumber" => {
            let items: Vec<Option<Value>> = match input.get("values") {
                Some(Value::Array(values)) => values.iter().cloned().map(Some).collect(),
                // JS 的 `Array.isArray(values) ? values : [undefined]`。
                _ => vec![None],
            };
            Ok(json!({
                "values": items
                    .into_iter()
                    .map(|item| number_tag(system_proxy::js_number(item)))
                    .collect::<Vec<Value>>(),
            }))
        }
        other => Err(format!("对拍驱动未覆盖的 op：{other}")),
    }
}

// ---------------------------------------------------------------------------
// sync / system-proxy 对拍辅助
// ---------------------------------------------------------------------------

/// 把沙箱还原成用例声明的初始状态（`sync.*` 会真写盘，跑完一侧必须回到原点）。
fn reset_case_files(case: &Value, sandbox: &Path, base: &str) {
    if sandbox.exists() {
        let _ = std::fs::remove_dir_all(sandbox);
    }
    std::fs::create_dir_all(sandbox)
        .unwrap_or_else(|error| panic!("重建沙箱 {} 失败：{error}", sandbox.display()));
    write_case_files(case, sandbox, base);
}

/// 对拍只跑迁移前 JS 的那一档形态：`Preserve`（数组进→数组出，对象进→对象出）。
/// 夹具因此不需要任何新字段，快照仍逐字节等价；按目标区分形态是 Rust 侧的新行为，
/// 由 `sync.rs` 的单元测试钉住。
fn sync_options(input: &Value) -> sync::SyncOptions {
    sync::SyncOptions {
        // 🔴 对拍恒为 false：JS 侧从未有过 `WB · auto` 这条合成路由（v1.1.15 才在 Rust 侧落地），
        // 夹具里的 `expected` 因此仍是「只有逐模型条目」的形态。生产发布链走
        // `orchestration.rs::sync_published`，那里显式打开该开关。
        auto_route: false,
        allow_empty: input
            .get("allowEmpty")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        require_existing: input
            .get("requireExisting")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        shape: sync::DocumentShape::default(),
    }
}

/// 与 JS 侧冻结时钟一致的 `SyncIo`：锁过期判定用 `FIXED_MILLIS`（`.bak` 命名已随分叉移除）。
fn parity_sync_io() -> sync::SyncIo {
    sync::SyncIo {
        now: || FIXED_MILLIS as u64,
        ..sync::SyncIo::default()
    }
}

/// 跑一次 `syncModels`，返回 `{ ok, value }` 或 `{ ok: false, kind, message }`（不含快照）。
fn sync_call(
    file: &str,
    models: &[Value],
    endpoint: &str,
    key: &str,
    options: sync::SyncOptions,
) -> Value {
    match sync::sync_models_with(
        Path::new(file),
        models,
        endpoint,
        key,
        &options,
        &parity_sync_io(),
    ) {
        Ok(outcome) => json!({ "ok": true, "value": outcome.to_json() }),
        Err(error) => sync_failure(&error),
    }
}

fn sync_failure(error: &sync::SyncError) -> Value {
    json!({
        "ok": false,
        "kind": sync_kind(&error.kind),
        "message": error.message,
    })
}

fn sync_kind(kind: &sync::SyncErrorKind) -> &'static str {
    match kind {
        sync::SyncErrorKind::InvalidJson => "json",
        sync::SyncErrorKind::NotFound => "not_found",
        sync::SyncErrorKind::Other => "other",
    }
}

/// 对应描述器的 `snapshot(path.dirname(file))`：递归读取目录下所有文本文件（键为相对路径）。
fn snapshot_parent(file: &str) -> Result<Map<String, Value>, String> {
    let path = Path::new(file);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut files = Map::new();
    if dir.exists() {
        walk_files(dir, "", &mut files)?;
    }
    Ok(files)
}

fn walk_files(dir: &Path, prefix: &str, files: &mut Map<String, Value>) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => return Err(format!("读取目录 {} 失败：{error}", dir.display())),
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect();
    found.sort();
    for (name, full) in found {
        let key = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if full.is_dir() {
            walk_files(&full, &key, files)?;
            continue;
        }
        let text = std::fs::read(&full)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|error| format!("读取文件 {} 失败：{error}", full.display()))?;
        files.insert(key, Value::String(text));
    }
    Ok(())
}

/// JS 侧 `numberTag`：NaN / ±Infinity 用字符串表达，其余按数值原样（`-0` 归一成 `0`）。
fn number_tag(value: f64) -> Value {
    if value.is_nan() {
        return Value::String("NaN".to_string());
    }
    if value == f64::INFINITY {
        return Value::String("Infinity".to_string());
    }
    if value == f64::NEG_INFINITY {
        return Value::String("-Infinity".to_string());
    }
    if value == 0.0 {
        return json!(0);
    }
    json!(value)
}

/// 把 `sync.*` 里跨语言必然不同的失败文案抹平（只保留 `kind = other` 的字面量）。
fn blank_sync_messages(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let failed = map.get("ok").and_then(Value::as_bool) == Some(false);
            let keep_message = map.get("kind").and_then(Value::as_str) == Some("other");
            if failed && !keep_message && map.contains_key("message") {
                map.insert("message".to_string(), Value::String(String::new()));
            }
            for item in map.values_mut() {
                blank_sync_messages(item);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                blank_sync_messages(item);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 仓库根：核心 crate 现在是 `src-tauri/core/`，往上两级才是根。
fn repo_root() -> PathBuf {
    crate_dir()
        .parent()
        .and_then(Path::parent)
        .expect("核心 crate 必须位于 src-tauri/core 下")
        .to_path_buf()
}

fn harness_path() -> PathBuf {
    crate_dir().join("tests").join("js").join("describe.mjs")
}

fn fixture_path(module: &str) -> PathBuf {
    crate_dir()
        .join("tests")
        .join("fixtures")
        .join(format!("{module}.json"))
}

/// 对拍沙箱：写在 cargo 给集成测试准备的 `target/tmp` 下，避免污染系统临时目录。
fn sandbox_root() -> PathBuf {
    match option_env!("CARGO_TARGET_TMPDIR") {
        Some(dir) => PathBuf::from(dir).join("js-parity"),
        None => crate_dir().join("target").join("tmp").join("js-parity"),
    }
}

/// 定位 Node 可执行文件：`NODE_BIN` > PATH > nvm/常见安装目录。
///
/// 构建机上的 Node 常来自 nvm，未必在 GUI/IDE 启动的进程 PATH 里，所以这里显式兜底。
fn node_binary() -> &'static Path {
    static NODE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    NODE.get_or_init(resolve_node)
}

fn resolve_node() -> PathBuf {
    if let Ok(explicit) = std::env::var("NODE_BIN") {
        let candidate = PathBuf::from(explicit);
        if is_runnable(&candidate) {
            return candidate;
        }
    }
    let bare = PathBuf::from("node");
    if is_runnable(&bare) {
        return bare;
    }
    for candidate in node_candidates() {
        if is_runnable(&candidate) {
            return candidate;
        }
    }
    panic!(
        "找不到可用的 node：请把它加入 PATH，或用 NODE_BIN 指定绝对路径（当前 PATH = {}）",
        std::env::var("PATH").unwrap_or_default()
    );
}

fn is_runnable(program: &Path) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn node_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        // nvm：优先取版本号最大的一版（目录名形如 v24.21.0）。
        let nvm = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(&nvm) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path().join("bin").join("node"))
                .filter(|path| path.is_file())
                .collect();
            versions.sort();
            versions.reverse();
            candidates.extend(versions);
        }
        candidates.push(home.join(".local").join("bin").join("node"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/node"));
    candidates.push(PathBuf::from("/usr/local/bin/node"));
    candidates.push(PathBuf::from("/usr/bin/node"));
    candidates
}
