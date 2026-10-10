//! `core/src/sync.js` 的 Rust 等价实现（方案B 阶段二）。
//!
//! 职责：把探测到的模型合并进 WorkBuddy 的 `models.json`，并用「文件锁 + 二次读取 +
//! 原子替换」保证并发刷新不会互相踩踏。这里刻意保持**同步阻塞**实现：Node 侧是 `fs/promises`，
//! 但整个写入过程极短，Rust 侧由阶段三的调用方放进 `spawn_blocking`（`atomic::replace_with_retry`
//! 内部还有最长 1.5s 的退避睡眠，更不能直接压在 async 执行器线程上）。
//!
//! 与 JS 逐条对齐的语义：
//!
//! | JS | Rust |
//! |---|---|
//! | `atomicWrite(file, text)` | [`atomic_write`] / [`atomic_write_with`] |
//! | `mergeModels(document, models, endpoint, key, {allowEmpty})` | [`merge_models`] |
//! | `syncModels(file, models, endpoint, key, options)` | [`sync_models`] / [`sync_models_with`] |
//! | `Date.now()`、`randomUUID()`、`replaceWithRetry` | [`SyncIo`] 注入（单测/对拍可控） |
//!
//! 落盘文本必须与 `JSON.stringify(merged, null, 2) + "\n"` 逐字节一致，因此走
//! [`json::js_stringify_pretty`]（先做 JS 数字规范化再缩进）。
//!
//! 已知边界（与 JS 的差异，均已记录、不在本项目数据范围内）：
//! - 读文件用 `String::from_utf8_lossy`（Node 的 `'utf8'` 读法同样把非法字节替换成 U+FFFD）；
//! - 除 ENOENT 之外的 IO 错误消息不逐字对齐 Node 的 errno 文案，仅保留 `错误码: 描述`。

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

use serde_json::{Map, Value};

use crate::atomic::{replace_with_retry, ReplaceError};
use crate::json;
use crate::model_status::client_model_id;
use crate::protocol::{random_uuid, BridgeError};
use crate::reasoning::work_buddy_reasoning;

/// 写进 `buddyBridgeOwner` 的归属标记，用于区分「本服务写入的条目」与用户自己的条目。
pub const OWNER: &str = "buddy-bridge-v1";

/// 锁文件超过该时长视为上一次同步已经崩溃，可以抢占。
pub const LOCK_STALE_MS: u64 = 5 * 60 * 1000;

/// 归属字段名。
const OWNER_KEY: &str = "buddyBridgeOwner";

/// 同步过程中出现的错误类别。
///
/// JS 侧这些失败都是普通 `Error`（服务层统一映射成 502 `upstream_error`），
/// 这里额外带上类别，是为了让 JS↔Rust 对拍能在**不逐字比较 Node 错误文案**的前提下
/// 断言两边落在同一个失败分支上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncErrorKind {
    /// `models.json` 不是合法 JSON（对应 Node 的 `SyntaxError`）。
    InvalidJson,
    /// 目标文件不存在且 `requireExisting` 为真（对应 Node 的 `ENOENT`）。
    NotFound,
    /// 其余业务/IO 失败。
    Other,
}

/// 与 JS 版 `Error` 对应的同步失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncError {
    /// 面向调用方的消息。
    pub message: String,
    /// 失败类别（供对拍断言分支）。
    pub kind: SyncErrorKind,
}

impl SyncError {
    /// 普通失败。
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: SyncErrorKind::Other,
        }
    }

    /// `models.json` 解析失败。
    pub fn invalid_json(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: SyncErrorKind::InvalidJson,
        }
    }

    /// 目标文件缺失且要求必须存在。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: SyncErrorKind::NotFound,
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for SyncError {}

/// JS 侧普通 `Error` 在服务层的归宿：502 `upstream_error`。
impl From<SyncError> for BridgeError {
    fn from(error: SyncError) -> Self {
        BridgeError::with(error.message, 502, "upstream_error")
    }
}

/// `models.json` 的外层形态（按写入目标区分，见 `targets.rs::Target::document_shape`）。
///
/// 两个插件的真实文件**不一样**（2026-10-10 用用户机器上的真实文件核对）：
/// - WorkBuddy：顶层是**裸数组** `[…]`；
/// - CodeBuddy：顶层是对象 `{ "models": […] }`。
///
/// 写成对方那一种是「发布成功、插件里一个模型都看不到」的成因（Windows 上实测到）：
/// 补建与合并都按目标自己的形态落盘，而不是「照文档原样」。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DocumentShape {
    /// 原样保留文档既有外层形态（数组进→数组出，对象进→对象出）。迁移前 JS 的唯一行为。
    #[default]
    Preserve,
    /// 一律写成 `{ "models": […] }`。已是裸数组的文档（旧版补建出来的 Windows 文件即如此）
    /// 会在下一次同步时被就地收敛，条目一条不丢。
    ModelsObject,
}

/// [`sync_models`] 的选项。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOptions {
    /// 允许空模型列表（`mergeModels` 的 `allowEmpty`）。
    pub allow_empty: bool,
    /// 目标文件必须已存在，缺失时报 `ENOENT` 而不是当成空配置。
    pub require_existing: bool,
    /// 外层形态；默认 [`DocumentShape::Preserve`]，与迁移前 JS 逐字节一致。
    pub shape: DocumentShape,
}

/// `syncModels` 的结果（JS 原形态是 `{ changed, count, backup? }`）。
///
/// 🔴 **有意的行为分叉（2026-10-09 用户裁定）**：JS 与本项目 1.3.3 及以前都会在写盘前留一份
/// `<配置文件>.buddy-bridge-<毫秒>.bak`，现在**不再产生任何备份**，`backup` 这个键因此从返回形态
/// 里消失（`tests/fixtures/sync.json` 已同步更正并记录该分叉）。取舍：备份从来没有读取方，
/// 面板也没有恢复入口，它唯一的作用是手工救急，却会在用户的插件配置目录里按发布次数无限堆积；
/// 保留下来的保护是「文件锁 + 二次读取 + 原子替换」，仍然不可能写出半个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// 是否真的改动了文件。
    pub changed: bool,
    /// 合并后属于本服务的条目数。
    pub count: u64,
}

impl SyncOutcome {
    /// 对应 JS `JSON.stringify(result)` 后的对象形态（`backup` 见上方分叉说明）。
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("changed".to_string(), Value::Bool(self.changed));
        object.insert("count".to_string(), Value::from(self.count));
        Value::Object(object)
    }
}

/// [`SyncIo::replace`] 的钩子签名（抽成别名仅为降低类型噪声，语义不变）。
pub type ReplaceFn = fn(&Path, &Path) -> Result<(), ReplaceError>;

/// 可注入的运行环境（时间、随机名、原子替换、二次读取前的钩子）。
///
/// 用 `fn` 指针而不是闭包，是为了让 [`SyncIo::default`] 能是 `'static` 且无需生命周期参数；
/// 单测里需要携带状态时，把状态放进 `thread_local!` 或写成 `fn` 参数（`before_reread` 会收到文件路径）。
#[derive(Debug, Clone, Copy)]
pub struct SyncIo {
    /// `Date.now()`：锁过期判定（`.bak` 命名已随「不再写备份」一并移除）。
    pub now: fn() -> u64,
    /// `randomUUID()`：临时文件名后缀。
    pub temp_suffix: fn() -> String,
    /// 二次读取之前调用，供单测模拟「同步期间配置被外部改写」。
    pub before_reread: Option<fn(&Path)>,
    /// 原子替换实现（默认 [`replace_with_retry`]），返回 `Err` 用于模拟 Windows 共享冲突。
    pub replace: Option<ReplaceFn>,
    /// 锁文件过期阈值。
    pub lock_stale_ms: u64,
}

impl Default for SyncIo {
    fn default() -> Self {
        Self {
            now: system_now_ms,
            temp_suffix: random_uuid,
            before_reread: None,
            replace: None,
            lock_stale_ms: LOCK_STALE_MS,
        }
    }
}

impl SyncIo {
    /// 真实环境（与不传 `io` 的默认行为一致）。
    pub fn real() -> Self {
        Self::default()
    }
}

fn system_now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_millis() as u64,
        Err(_) => 0,
    }
}

/// 锁文件守卫：无论正常返回还是中途 `?` 提前返回，都会关闭句柄并删除锁文件
/// （对应 JS `finally { await lock.close(); await fs.unlink(lockFile).catch(() => {}); }`）。
struct LockGuard {
    path: PathBuf,
    handle: Option<File>,
}

impl LockGuard {
    fn release(&mut self) {
        self.handle = None;
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// 用 `OsString` 拼接同级路径，避免 `display()` 在非 UTF-8 路径上做有损转换。
fn sibling(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn parent_of(file: &Path) -> &Path {
    match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Node 风格的 `错误码: 描述` 文案。
fn io_error(error: &std::io::Error, path: &Path) -> SyncError {
    let code = crate::atomic::node_code_for_io(error);
    let message = match code {
        Some(code) => format!("{code}: {error}"),
        None => error.to_string(),
    };
    let message = format!("{message} ({})", path.display());
    if code == Some("ENOENT") {
        SyncError::not_found(message)
    } else {
        SyncError::other(message)
    }
}

/// `mode: 0o700` 的递归 `mkdir`（已存在时不报错，也不改权限）。
fn create_dir_private(path: &Path) -> std::io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)
}

/// `flag: 'wx'`（存在即失败）+ `mode: 0o600`。
fn create_new_private(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

/// `atomicWrite(file, text)`：先写同目录临时文件，再用 rename 原子替换。
pub fn atomic_write(file: &Path, text: &str) -> Result<(), SyncError> {
    atomic_write_with(file, text, &SyncIo::default())
}

/// [`atomic_write`] 的可注入版本。
pub fn atomic_write_with(file: &Path, text: &str, io: &SyncIo) -> Result<(), SyncError> {
    create_dir_private(parent_of(file)).map_err(|error| io_error(&error, parent_of(file)))?;
    let temp = sibling(file, &format!(".{}.tmp", (io.temp_suffix)()));
    let result = (|| -> Result<(), SyncError> {
        let mut handle = create_new_private(&temp).map_err(|error| io_error(&error, &temp))?;
        handle
            .write_all(text.as_bytes())
            .and_then(|()| handle.flush())
            .map_err(|error| io_error(&error, &temp))?;
        drop(handle);
        match io.replace {
            Some(replace) => replace(&temp, file).map_err(|error| SyncError::other(error.message)),
            None => replace_with_retry(&temp, file).map_err(|error| SyncError::other(error.message)),
        }
    })();
    // 与 JS 的 `finally { await fs.unlink(temp).catch(() => {}) }` 一致：无论成败都清理临时文件，
    // 且清理失败不影响已经产生的错误。
    let _ = fs::remove_file(&temp);
    result
}

/// `mergeModels(document, models, endpoint, key, { allowEmpty })`。
///
/// 保留 `document` 里非本服务写入的条目，冲突（同 `id` 或同 `OC · <name>`）的新条目整体丢弃。
/// 外层形态由 `shape` 决定：[`DocumentShape::Preserve`] 下数组形态的文档直接返回合并后的数组
/// （迁移前 JS 的行为），[`DocumentShape::ModelsObject`] 下一律产出 `{ "models": […] }`。
pub fn merge_models(
    document: &Value,
    models: &[Value],
    endpoint: &str,
    key: &str,
    allow_empty: bool,
    shape: DocumentShape,
) -> Result<Value, SyncError> {
    if models.is_empty() && !allow_empty {
        return Err(SyncError::other(
            "Empty model discovery; existing configuration preserved",
        ));
    }
    let list: &[Value] = match document {
        Value::Array(items) => items,
        Value::Object(object) => match object.get("models") {
            Some(Value::Array(items)) => items,
            _ => return Err(unrecognized()),
        },
        _ => return Err(unrecognized()),
    };

    let kept: Vec<&Value> = list.iter().filter(|model| !is_owned(model)).collect();
    // JS 的 `new Set(kept.map(m => m.id))` + `has()`：SameValueZero 比较，缺失的 id 参与比较。
    let kept_ids: Vec<Option<&Value>> = kept.iter().map(|model| model.get("id")).collect();
    let conflicts = |value: Option<&Value>| {
        kept_ids
            .iter()
            .any(|kept_id| json::strict_eq(*kept_id, value))
    };

    let mut entries: Vec<Value> = Vec::new();
    for model in models {
        let client_id = client_model_id(model);
        if conflicts(model.get("id")) || conflicts(Some(&Value::String(client_id.clone()))) {
            continue;
        }
        entries.push(model_entry(model, &client_id, endpoint, key));
    }

    let mut combined: Vec<Value> = kept.into_iter().cloned().collect();
    combined.extend(entries.iter().cloned());

    let mut updated = match document {
        Value::Object(object) => object.clone(),
        // 裸数组文档 + 目标只认对象形态：条目原样搬进 `models`，不新增任何键。
        Value::Array(_) if shape == DocumentShape::ModelsObject => Map::new(),
        Value::Array(_) => return Ok(Value::Array(combined)),
        _ => unreachable!("非对象/数组的 document 已在上面报错"),
    };
    // 先取一份再改写：`updated` 接下来要被 insert，不能同时持有它的借用。
    let available = match updated.get("availableModels") {
        Some(Value::Array(items)) => Some(items.clone()),
        _ => None,
    };
    if let Some(available) = available {
        // 旧条目里属于本服务、且这一次不再保留的 id（等价于 JS 的
        // `new Set(list.filter(m => m.buddyBridgeOwner === OWNER && !kept.includes(m)).map(m => m.id))`：
        // `kept` 恰好是「非本服务」的元素，引用比较对 owner 条目恒为 false）。
        let dropped: Vec<Option<&Value>> = list
            .iter()
            .filter(|model| is_owned(model))
            .map(|model| model.get("id"))
            .collect();
        let mut next: Vec<Value> = Vec::new();
        for id in &available {
            if dropped
                .iter()
                .any(|old| json::strict_eq(*old, Some(id)))
            {
                continue;
            }
            if next.iter().any(|seen| json::strict_eq(Some(seen), Some(id))) {
                continue;
            }
            next.push(id.clone());
        }
        for entry in &entries {
            if let Some(id) = entry.get("id") {
                if !next.iter().any(|seen| json::strict_eq(Some(seen), Some(id))) {
                    next.push(id.clone());
                }
            }
        }
        updated.insert("availableModels".to_string(), Value::Array(next));
    }
    updated.insert("models".to_string(), Value::Array(combined));
    Ok(Value::Object(updated))
}

fn unrecognized() -> SyncError {
    SyncError::other("Unrecognized WorkBuddy models.json; left unchanged")
}

fn is_owned(model: &Value) -> bool {
    matches!(model.get(OWNER_KEY), Some(Value::String(owner)) if owner == OWNER)
}

/// 合并结果里属于本服务的条目数（等价于 JS 的 `merged.models.filter(...).length`）。
fn owned_count(merged: &Value) -> u64 {
    const NONE: &[Value] = &[];
    let list: &[Value] = match merged {
        Value::Array(items) => items,
        Value::Object(object) => match object.get("models") {
            Some(Value::Array(items)) => items,
            _ => NONE,
        },
        _ => NONE,
    };
    list.iter().filter(|model| is_owned(model)).count() as u64
}

/// `models.filter(...).map(m => ({ id, name, vendor, url, apiKey, supportsToolCall, supportsImages,
/// ...workBuddyReasoning(m), buddyBridgeOwner, maxInputTokens?, maxOutputTokens? }))`。
///
/// 键顺序必须与 JS 的对象字面量一致：`availableModels` 只比较集合，但整篇 `models.json`
/// 会被逐字节对拍，键顺序不同即视为不一致。
fn model_entry(model: &Value, client_id: &str, endpoint: &str, key: &str) -> Value {
    let mut entry = Map::new();
    entry.insert("id".to_string(), Value::String(client_id.to_string()));
    entry.insert("name".to_string(), Value::String(client_id.to_string()));
    entry.insert("vendor".to_string(), Value::String("Custom".to_string()));
    entry.insert("url".to_string(), Value::String(endpoint.to_string()));
    entry.insert("apiKey".to_string(), Value::String(key.to_string()));
    entry.insert(
        "supportsToolCall".to_string(),
        Value::Bool(!json::truthy(model.get("chatOnly"))),
    );
    entry.insert(
        "supportsImages".to_string(),
        Value::Bool(matches!(model.get("images"), Some(Value::Bool(true)))),
    );
    if let Value::Object(reasoning) = work_buddy_reasoning(model) {
        for (name, value) in reasoning {
            entry.insert(name, value);
        }
    }
    entry.insert("buddyBridgeOwner".to_string(), Value::String(OWNER.to_string()));
    let tokens = json::coalesce(model.get("input"), model.get("context"));
    if json::truthy(tokens) {
        if let Some(tokens) = tokens {
            entry.insert("maxInputTokens".to_string(), tokens.clone());
        }
    }
    if json::truthy(model.get("output")) {
        if let Some(output) = model.get("output") {
            entry.insert("maxOutputTokens".to_string(), output.clone());
        }
    }
    Value::Object(entry)
}

/// `syncModels(file, models, endpoint, key, options = {})`。
pub fn sync_models(
    file: &Path,
    models: &[Value],
    endpoint: &str,
    key: &str,
    options: &SyncOptions,
) -> Result<SyncOutcome, SyncError> {
    sync_models_with(file, models, endpoint, key, options, &SyncIo::default())
}

/// [`sync_models`] 的可注入版本。
pub fn sync_models_with(
    file: &Path,
    models: &[Value],
    endpoint: &str,
    key: &str,
    options: &SyncOptions,
    io: &SyncIo,
) -> Result<SyncOutcome, SyncError> {
    let parent = parent_of(file);
    create_dir_private(parent).map_err(|error| io_error(&error, parent))?;

    let lock_file = sibling(file, ".buddy-bridge.lock");
    if let Ok(metadata) = fs::metadata(&lock_file) {
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64);
        if let Some(mtime_ms) = mtime_ms {
            if (io.now)() as i64 - mtime_ms > io.lock_stale_ms as i64 {
                let _ = fs::remove_file(&lock_file);
            }
        }
    }

    // JS 用 `fs.open(lockFile, 'wx', 0o600).catch(...)`：任何失败都归因于「已有同步在跑」。
    let mut guard = LockGuard {
        path: lock_file.clone(),
        handle: Some(
            create_new_private(&lock_file)
                .map_err(|_| SyncError::other("Model sync already running; no changes made"))?,
        ),
    };

    let old = read_text(file, options.require_existing)?;
    let document = match &old {
        None => Value::Array(Vec::new()),
        Some(text) => json::parse_json(text).map_err(|error| {
            SyncError::invalid_json(format!("Invalid JSON in WorkBuddy models.json: {error}"))
        })?,
    };
    let merged = merge_models(&document, models, endpoint, key, options.allow_empty, options.shape)?;
    let count = owned_count(&merged);

    if json::js_stringify(&merged) == json::js_stringify(&document) {
        // 无变化也是「一次成功的同步」：旧版攒下的存量备份就是靠这里收尾的，而启动沿用让
        // 无变化成为常态，缺了这一句，升级后那些文件永远没有机会被清掉。
        sweep_old_backups(file);
        guard.release();
        return Ok(SyncOutcome {
            changed: false,
            count,
        });
    }

    if let Some(hook) = io.before_reread {
        hook(file);
    }
    let current = read_text(file, options.require_existing)?;
    if current != old {
        guard.release();
        return Err(SyncError::other(
            "WorkBuddy configuration changed during sync; retry refresh",
        ));
    }

    atomic_write_with(file, &format!("{}\n", json::js_stringify_pretty(&merged)), io)?;
    sweep_old_backups(file);
    guard.release();

    Ok(SyncOutcome {
        changed: true,
        count,
    })
}

/// 清掉**旧版本**留下的同族备份：`<配置文件>.buddy-bridge-<毫秒>.bak`。
///
/// 1.3.3 及以前每次写盘都留一份，用户目录里按发布次数堆积（几天下来几十个），而那些旧内容
/// 从来没有读取方、面板也没有恢复入口。现在不再写备份，但**存量必须能自己收干净**：无变化的
/// 同步也算一次同步，所以两条路径都调用它。
///
/// 判定是**逐段白名单**而不是通配删除：文件名必须以 `<配置文件名>.buddy-bridge-` 开头、
/// 以 `.bak` 结尾、中间全是 ASCII 数字，且必须是普通文件。用户放在同一目录里的任何其它文件
/// （包括自己命名的 `*.bak`）都不在删除范围内；列目录失败静默返回——清理属收尾，绝不能把一次
/// 已成功的同步报成失败。
fn sweep_old_backups(file: &Path) {
    let Some((prefix, dir)) = backup_prefix(file) else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if !backup_of(&prefix, &entry) {
            continue;
        }
        let _ = fs::remove_file(entry.path());
    }
}

/// `<配置文件名>.buddy-bridge-` 前缀与其所在目录。
fn backup_prefix(file: &Path) -> Option<(String, &Path)> {
    let dir = file.parent()?;
    let name = file.file_name()?.to_string_lossy().into_owned();
    Some((format!("{name}.buddy-bridge-"), dir))
}

/// 这个目录项是不是本工具旧版留下的备份（只删文件，判定见 [`sweep_old_backups`]）。
///
/// 非数字中段直接不认；目录也算不认。非 ASCII 文件名在 lossy 形态里会变成替换字符，
/// 所以数字判定顺带挡住了「前缀后缀巧合对上、中间是乱码」的误删。
fn backup_of(prefix: &str, entry: &fs::DirEntry) -> bool {
    if entry.file_type().map_or(true, |file_type| file_type.is_dir()) {
        return false;
    }
    let name = entry.file_name();
    let Some(stem) = name.to_str() else {
        return false;
    };
    let Some(rest) = stem.strip_prefix(prefix) else {
        return false;
    };
    match rest.strip_suffix(".bak") {
        Some(stamp) => !stamp.is_empty() && stamp.bytes().all(|byte| byte.is_ascii_digit()),
        None => false,
    }
}

/// `fs.readFile(file, 'utf8')` + `catch(e => e.code === 'ENOENT' && !requireExisting ? null : e)`。
fn read_text(file: &Path, require_existing: bool) -> Result<Option<String>, SyncError> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(error) => {
            let missing = crate::atomic::node_code_for_io(&error) == Some("ENOENT");
            if missing && !require_existing {
                return Ok(None);
            }
            Err(io_error(&error, file))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atomic::ReplaceError;
    use serde_json::json;
    use std::cell::RefCell;

    thread_local! {
        /// `before_reread` 钩子携带的状态：下一次二次读取前要把哪个文件改成什么内容。
        static REWRITE: RefCell<Option<(PathBuf, String)>> = const { RefCell::new(None) };
    }

    fn frozen_now() -> u64 {
        1_700_000_000_000
    }

    fn fixed_suffix() -> String {
        "test-uuid".to_string()
    }

    fn failing_replace(_temp: &Path, _target: &Path) -> Result<(), ReplaceError> {
        Err(ReplaceError::new(
            Some("EBUSY"),
            "EBUSY: resource busy or locked",
        ))
    }

    fn rewrite_hook(file: &Path) {
        let pending = REWRITE.with(|cell| cell.borrow_mut().take());
        if let Some((path, text)) = pending {
            fs::write(path, text).expect("hook 写入");
        }
        let _ = file;
    }

    fn test_io() -> SyncIo {
        SyncIo {
            now: frozen_now,
            temp_suffix: fixed_suffix,
            before_reread: None,
            replace: None,
            lock_stale_ms: LOCK_STALE_MS,
        }
    }

    fn sandbox(name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("tmp")
            .join("sync-unit");
        let dir = root.join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("创建沙箱");
        dir
    }

    fn model(name: &str) -> Value {
        json!({ "id": format!("vendor/{name}"), "name": name })
    }

    #[test]
    fn merge_into_array_document_keeps_foreign_entries() {
        let document = json!([{ "id": "mine" }, { "id": "other", "buddyBridgeOwner": OWNER }]);
        let merged = merge_models(
            &document,
            &[model("gpt")],
            "http://127.0.0.1:1",
            "k",
            false,
            DocumentShape::Preserve,
        )
        .expect("合并成功");
        let items = merged.as_array().expect("数组");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], json!("mine"));
        assert_eq!(items[1]["id"], json!("OC · gpt"));
        assert_eq!(items[1]["vendor"], json!("Custom"));
        assert_eq!(items[1]["buddyBridgeOwner"], json!(OWNER));
        assert_eq!(items[1]["supportsToolCall"], json!(true));
        assert_eq!(items[1]["supportsReasoning"], json!(false));
    }

    #[test]
    fn merge_into_object_document_keeps_available_models_in_sync() {
        let document = json!({
            "models": [
                { "id": "keep", "buddyBridgeOwner": "user" },
                { "id": "OC · old", "buddyBridgeOwner": OWNER }
            ],
            "availableModels": ["keep", "OC · old", "keep"],
            "other": 1
        });
        let merged = merge_models(
            &document,
            &[model("new")],
            "http://e",
            "k",
            false,
            DocumentShape::Preserve,
        )
        .expect("合并");
        assert_eq!(merged["other"], json!(1));
        assert_eq!(merged["availableModels"], json!(["keep", "OC · new"]));
        assert_eq!(merged["models"].as_array().expect("数组").len(), 2);
        // 键顺序与 JS 对象字面量一致：models 保留原位置，availableModels 紧随其后。
        let keys: Vec<&String> = merged.as_object().expect("对象").keys().collect();
        assert_eq!(keys, vec!["models", "availableModels", "other"]);
    }

    #[test]
    fn merge_rejects_empty_discovery_and_unknown_shape() {
        let empty = merge_models(&json!([]), &[], "http://e", "k", false, DocumentShape::Preserve)
            .expect_err("空列表报错");
        assert_eq!(
            empty.message,
            "Empty model discovery; existing configuration preserved"
        );
        let allowed = merge_models(&json!([]), &[], "http://e", "k", true, DocumentShape::Preserve)
            .expect("allowEmpty");
        assert_eq!(allowed, json!([]));

        let bad = merge_models(
            &json!({ "nope": 1 }),
            &[model("a")],
            "http://e",
            "k",
            false,
            DocumentShape::Preserve,
        )
        .expect_err("结构不识别");
        assert_eq!(bad.message, "Unrecognized WorkBuddy models.json; left unchanged");
        let scalar = merge_models(
            &json!("text"),
            &[model("a")],
            "http://e",
            "k",
            false,
            DocumentShape::Preserve,
        )
        .expect_err("标量不识别");
        assert_eq!(scalar.kind, SyncErrorKind::Other);
    }

    #[test]
    fn merge_skips_conflicting_ids_and_client_ids() {
        let document = json!([{ "id": "OC · taken" }, { "id": "vendor/dup" }]);
        let merged = merge_models(
            &document,
            &[model("taken"), model("dup"), model("fresh")],
            "http://e",
            "k",
            false,
            DocumentShape::Preserve,
        )
        .expect("合并");
        let ids: Vec<&Value> = merged
            .as_array()
            .expect("数组")
            .iter()
            .map(|item| item.get("id").expect("id"))
            .collect();
        assert_eq!(ids, vec![&json!("OC · taken"), &json!("vendor/dup"), &json!("OC · fresh")]);
    }

    #[test]
    fn merge_carries_token_limits_and_reasoning() {
        let document = json!([]);
        let rich = json!({
            "id": "vendor/r", "name": "r", "context": 32000, "output": 4096,
            "images": true, "chatOnly": true, "reasoning": true,
            "variants": { "low": { "reasoningEffort": "low" }, "off": { "disabled": true } }
        });
        let merged =
            merge_models(&document, &[rich], "http://e", "k", false, DocumentShape::Preserve)
                .expect("合并");
        let entry = &merged.as_array().expect("数组")[0];
        assert_eq!(entry["maxInputTokens"], json!(32000));
        assert_eq!(entry["maxOutputTokens"], json!(4096));
        assert_eq!(entry["supportsImages"], json!(true));
        assert_eq!(entry["supportsToolCall"], json!(false));
        assert_eq!(entry["supportsReasoning"], json!(true));
        assert_eq!(entry["onlyReasoning"], json!(true));
        assert_eq!(entry["reasoning"]["supportedEfforts"], json!(["low"]));
    }

    #[test]
    fn a_codebuddy_shaped_merge_moves_an_array_document_into_the_models_key() {
        let document = json!([{ "id": "mine" }, { "id": "OC · old", "buddyBridgeOwner": OWNER }]);
        let merged = merge_models(
            &document,
            &[model("gpt")],
            "http://e",
            "k",
            false,
            DocumentShape::ModelsObject,
        )
        .expect("合并");
        let object = merged.as_object().expect("对象");
        assert_eq!(object.keys().collect::<Vec<_>>(), vec!["models"], "不新增任何键");
        let items = object["models"].as_array().expect("数组");
        assert_eq!(
            items.iter().map(|item| &item["id"]).collect::<Vec<_>>(),
            vec![&json!("mine"), &json!("OC · gpt")],
            "外来条目原样搬进去，本工具的旧条目照常摘掉"
        );
    }

    /// Windows 上真实发生过的形态：旧版把 CodeBuddy 的 `models.json` 补建、并一路写成裸数组，
    /// 插件读不出任何模型。收敛到对象形态必须**就地自愈**，用户不必手动删文件。
    #[test]
    fn a_codebuddy_sync_rewrites_a_bare_array_file_in_place() {
        let dir = sandbox("sync-codebuddy-shape");
        let file = dir.join("models.json");
        fs::write(&file, r#"[{"id":"own-1"}]"#).expect("预置裸数组文件");
        let options = SyncOptions {
            allow_empty: true,
            require_existing: true,
            shape: DocumentShape::ModelsObject,
        };

        let outcome =
            sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io()).expect("写入");
        assert!(outcome.changed, "形态被收敛必须算一次真实写入");
        let text = fs::read_to_string(&file).expect("读取");
        assert!(text.starts_with("{\n  \"models\": [\n"), "{text}");
        assert!(text.contains("\"id\": \"own-1\""), "用户自己的条目不得丢");
        assert!(text.contains("\"id\": \"OC · a\""), "{text}");

        // 幂等：第二次不再改动盘。
        let second =
            sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io()).expect("二次");
        assert!(!second.changed, "收敛一次之后就稳定");
        assert_eq!(fs::read_to_string(&file).expect("读取"), text);
    }

    #[test]
    fn atomic_write_replaces_and_cleans_up() {
        let dir = sandbox("atomic");
        let file = dir.join("nested").join("models.json");
        atomic_write_with(&file, "hello", &test_io()).expect("写入");
        assert_eq!(fs::read_to_string(&file).expect("读取"), "hello");
        assert!(!dir.join("nested").join("models.json.test-uuid.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).expect("元数据").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn atomic_write_reports_replace_failure_and_still_cleans_temp() {
        let dir = sandbox("atomic-fail");
        let file = dir.join("models.json");
        fs::write(&file, "old").expect("预置");
        let io = SyncIo {
            replace: Some(failing_replace),
            ..test_io()
        };
        let error = atomic_write_with(&file, "new", &io).expect_err("替换失败");
        assert_eq!(error.message, "EBUSY: resource busy or locked");
        assert_eq!(fs::read_to_string(&file).expect("读取"), "old");
        assert!(!dir.join("models.json.test-uuid.tmp").exists());
    }

    #[test]
    fn sync_creates_missing_file_without_backup() {
        let dir = sandbox("sync-create");
        let file = dir.join("models.json");
        let outcome = sync_models_with(
            &file,
            &[model("a")],
            "http://127.0.0.1:1",
            "secret",
            &SyncOptions::default(),
            &test_io(),
        )
        .expect("同步");
        assert_eq!(
            outcome,
            SyncOutcome {
                changed: true,
                count: 1
            }
        );
        assert_eq!(outcome.to_json(), json!({ "changed": true, "count": 1 }));
        let text = fs::read_to_string(&file).expect("读取");
        assert!(text.ends_with("]\n"), "{text}");
        assert!(text.contains("\n  {\n    \"id\": \"OC · a\","), "{text}");
        assert!(!dir.join("models.json.buddy-bridge.lock").exists());
        assert!(!dir.join("models.json.test-uuid.tmp").exists());
        assert_eq!(file_names(&dir), ["models.json".to_string()], "首轮写入不该有备份");
    }

    /// 目录里的**全部**文件名（排序后返回）：备份策略的断言必须能同时看出「该删的删了」
    /// 和「不该删的一个没少」，按模式过滤就看不出后者。
    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("列目录")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn sync_second_run_reports_no_change() {
        let dir = sandbox("sync-noop");
        let file = dir.join("models.json");
        let options = SyncOptions::default();
        sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io()).expect("首次");
        let first = fs::read_to_string(&file).expect("读取");
        let outcome =
            sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io()).expect("二次");
        assert!(!outcome.changed);
        assert_eq!(outcome.count, 1);
        assert_eq!(fs::read_to_string(&file).expect("读取"), first);
        assert_eq!(file_names(&dir), ["models.json".to_string()]);
    }

    #[test]
    fn a_changed_sync_writes_no_backup_and_sweeps_the_whole_family() {
        let dir = sandbox("sync-backup-sweep");
        let file = dir.join("models.json");
        let options = SyncOptions::default();
        fs::write(&file, "[{\"id\":\"own-1\"}]").expect("预置");

        // 1.3.3 及以前每轮写盘都留一份，几天下来攒了几十个；升级后一个都不该再多写。
        // 中段非数字、后缀不是 .bak、以及用户自己命名的备份，都**不在**删除范围内。
        for name in [
            "models.json.buddy-bridge-1500000000000.bak",
            "models.json.buddy-bridge-1600000000000.bak",
            "models.json.buddy-bridge-999999999999.bak",
            "models.json.buddy-bridge-abc.bak",
            "models.json.my-own.bak",
            "models.json.buddy-bridge-1600000000000.tmp",
        ] {
            fs::write(dir.join(name), "遗留").expect("预置");
        }

        sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io()).expect("写入");

        assert_eq!(
            file_names(&dir),
            [
                "models.json".to_string(),
                "models.json.buddy-bridge-1600000000000.tmp".to_string(),
                "models.json.buddy-bridge-abc.bak".to_string(),
                "models.json.my-own.bak".to_string(),
            ],
            "同族三份遗留备份要一次清干净，不匹配白名单的文件一个都不能少"
        );
        assert!(fs::read_to_string(&file)
            .expect("读取")
            .contains("OC · a"));
    }

    #[test]
    fn an_unchanged_sync_still_sweeps_leftover_backups() {
        let dir = sandbox("sync-unchanged-sweep");
        let file = dir.join("models.json");
        let options = SyncOptions::default();
        fs::write(&file, "[{\"id\":\"own-1\"}]").expect("预置");
        sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io()).expect("建立基线");
        let published = fs::read_to_string(&file).expect("读取");

        // 启动沿用让「内容相同」成为常态：清理若只挂在写盘之后，存量备份永远删不掉。
        // 12 位与 13 位并存，字典序会把 999999999999 排在最后，数值判定才能把它一起删掉。
        for stamp in [1_500_000_000_000_u64, 999_999_999_999] {
            fs::write(
                dir.join(format!("models.json.buddy-bridge-{stamp}.bak")),
                "遗留备份",
            )
            .expect("预置");
        }
        fs::write(dir.join("models.json.my-own.bak"), "用户自己的").expect("预置");

        let outcome = sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io())
            .expect("内容相同的那次");
        assert!(!outcome.changed, "内容相同应报无变化");

        assert_eq!(
            file_names(&dir),
            ["models.json".to_string(), "models.json.my-own.bak".to_string()],
            "无变化也要收敛存量"
        );
        assert_eq!(
            fs::read_to_string(&file).expect("配置"),
            published,
            "清理不得动到正在使用的配置"
        );
    }

    #[test]
    fn sync_refuses_when_lock_is_held() {
        let dir = sandbox("sync-lock");
        let file = dir.join("models.json");
        fs::write(dir.join("models.json.buddy-bridge.lock"), "").expect("占锁");
        let error = sync_models_with(
            &file,
            &[model("a")],
            "http://e",
            "k",
            &SyncOptions::default(),
            &test_io(),
        )
        .expect_err("锁冲突");
        assert_eq!(error.message, "Model sync already running; no changes made");
        assert!(!file.exists());
    }

    #[test]
    fn sync_reclaims_stale_lock() {
        let dir = sandbox("sync-stale");
        let file = dir.join("models.json");
        let lock = dir.join("models.json.buddy-bridge.lock");
        fs::write(&lock, "").expect("占锁");
        let io = SyncIo {
            // 冻结时间远晚于锁文件的实际 mtime，视为过期。
            now: system_now_ms_plus_an_hour,
            ..test_io()
        };
        let outcome =
            sync_models_with(&file, &[model("a")], "http://e", "k", &SyncOptions::default(), &io)
                .expect("抢占过期锁");
        assert!(outcome.changed);
        assert!(!lock.exists());
    }

    fn system_now_ms_plus_an_hour() -> u64 {
        system_now_ms() + 3_600_000
    }

    #[test]
    fn sync_detects_concurrent_rewrite() {
        let dir = sandbox("sync-conflict");
        let file = dir.join("models.json");
        fs::write(&file, "{\n  \"models\": []\n}\n").expect("预置");
        REWRITE.with(|cell| {
            *cell.borrow_mut() = Some((file.clone(), "{\n  \"models\": [{ \"id\": \"x\" }]\n}\n".into()))
        });
        let io = SyncIo {
            before_reread: Some(rewrite_hook),
            ..test_io()
        };
        let error = sync_models_with(
            &file,
            &[model("a")],
            "http://e",
            "k",
            &SyncOptions::default(),
            &io,
        )
        .expect_err("并发改写");
        assert_eq!(
            error.message,
            "WorkBuddy configuration changed during sync; retry refresh"
        );
        assert_eq!(
            fs::read_to_string(&file).expect("读取"),
            "{\n  \"models\": [{ \"id\": \"x\" }]\n}\n"
        );
        assert!(!dir.join("models.json.buddy-bridge.lock").exists());
    }

    #[test]
    fn sync_reports_invalid_json_without_touching_file() {
        let dir = sandbox("sync-invalid");
        let file = dir.join("models.json");
        fs::write(&file, "{oops").expect("预置");
        let error = sync_models_with(
            &file,
            &[model("a")],
            "http://e",
            "k",
            &SyncOptions::default(),
            &test_io(),
        )
        .expect_err("非法 JSON");
        assert_eq!(error.kind, SyncErrorKind::InvalidJson);
        assert_eq!(fs::read_to_string(&file).expect("读取"), "{oops");
        assert!(!dir.join("models.json.buddy-bridge.lock").exists());
    }

    #[test]
    fn sync_require_existing_reports_not_found() {
        let dir = sandbox("sync-required");
        let file = dir.join("models.json");
        let options = SyncOptions {
            allow_empty: false,
            require_existing: true,
            shape: DocumentShape::Preserve,
        };
        let error = sync_models_with(&file, &[model("a")], "http://e", "k", &options, &test_io())
            .expect_err("缺失文件");
        assert_eq!(error.kind, SyncErrorKind::NotFound);
        assert!(!file.exists());
        assert!(!dir.join("models.json.buddy-bridge.lock").exists());
    }

    /// 关停清理（以及启动清旧、换配置文件）走的是同一条通道：**空发布集 + allow_empty**。
    /// 数据红线在这里钉死——只摘掉 `OWNER` 名下的条目与 `availableModels` 里对应的 id，
    /// 用户手动配置的条目、其他键与键顺序一概不动。
    #[test]
    fn an_empty_sync_at_exit_removes_only_this_tools_entries() {
        let dir = sandbox("sync-exit-cleanup");
        let file = dir.join("models.json");
        fs::write(
            &file,
            format!(
                "{{\"models\":[{{\"id\":\"mine\",\"name\":\"手动条目\"}},\
                 {{\"id\":\"OC · old\",\"name\":\"OC · old\",\"buddyBridgeOwner\":\"{OWNER}\",\
                 \"url\":\"http://127.0.0.1:1/chat/completions\"}}],\
                 \"availableModels\":[\"mine\",\"OC · old\"],\"other\":1}}"
            ),
        )
        .expect("预置");
        let options = SyncOptions {
            allow_empty: true,
            require_existing: true,
            shape: DocumentShape::Preserve,
        };
        let outcome = sync_models_with(&file, &[], "http://e", "k", &options, &test_io())
            .expect("退出清理");
        assert_eq!(
            outcome,
            SyncOutcome {
                changed: true,
                count: 0
            }
        );
        let text = fs::read_to_string(&file).expect("读取");
        let document = json::parse_json(&text).expect("清理后仍是合法 JSON");
        assert_eq!(
            document["models"],
            json!([{ "id": "mine", "name": "手动条目" }]),
            "非本工具名下的条目必须原样保留（含其全部字段）"
        );
        assert_eq!(document["availableModels"], json!(["mine"]));
        assert_eq!(document["other"], json!(1));
        let keys: Vec<&String> = document.as_object().expect("对象").keys().collect();
        assert_eq!(
            keys,
            vec!["models", "availableModels", "other"],
            "键顺序是合并语义的一部分，清理不得重排"
        );
        assert_eq!(file_names(&dir), ["models.json".to_string()], "清理不留备份");
    }

    /// 同一份「归属标记不是本工具」的条目即使在退出清理里也必须留下：`buddyBridgeOwner` 的值
    /// 不同就不是我们的条目（用户手动配置或另一版本写入的形态）。
    #[test]
    fn an_empty_sync_at_exit_keeps_foreign_owner_entries() {
        let dir = sandbox("sync-exit-foreign");
        let file = dir.join("models.json");
        fs::write(
            &file,
            "[{\"id\":\"OC · other\",\"buddyBridgeOwner\":\"someone-else\"}]",
        )
        .expect("预置");
        let options = SyncOptions {
            allow_empty: true,
            require_existing: true,
            shape: DocumentShape::Preserve,
        };
        let outcome = sync_models_with(&file, &[], "http://e", "k", &options, &test_io())
            .expect("清理");
        assert!(!outcome.changed, "没有本工具名下的条目时，文件内容逐字节不变");
        assert_eq!(
            fs::read_to_string(&file).expect("读取"),
            "[{\"id\":\"OC · other\",\"buddyBridgeOwner\":\"someone-else\"}]"
        );
    }

    #[test]
    fn sync_propagates_replace_failure_and_releases_lock() {
        let dir = sandbox("sync-replace-fail");
        let file = dir.join("models.json");
        let io = SyncIo {
            replace: Some(failing_replace),
            ..test_io()
        };
        let error = sync_models_with(
            &file,
            &[model("a")],
            "http://e",
            "k",
            &SyncOptions::default(),
            &io,
        )
        .expect_err("替换失败");
        assert_eq!(error.message, "EBUSY: resource busy or locked");
        assert!(!file.exists());
        assert!(!dir.join("models.json.test-uuid.tmp").exists());
        assert!(!dir.join("models.json.buddy-bridge.lock").exists());
    }

    #[test]
    fn sync_error_maps_to_upstream_error() {
        let error = BridgeError::from(SyncError::other("boom"));
        assert_eq!(error.status, 502);
        assert_eq!(error.code, "upstream_error");
        assert_eq!(error.message, "boom");
    }
}
