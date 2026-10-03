//! 平台注册表与凭据通道（多平台免费模型接入的底座）。
//!
//! 这里只有两张表，且职责互不相通：
//!
//! 1. [`PROVIDERS`]：接入哪些平台、面板显示什么标签、注入 OpenCode 配置段时用哪个 npm 包与
//!    baseURL。**随版本发布的常量表，不做远程拉取** —— 上游新增免费模型不会自动出现，
//!    需要改表发版；反之也不会把没人复核过的模型放给用户。风险方向是「漏不放行」，
//!    因为「误放行」花的是维护者自己账号里的额度。
//! 2. `providers.json`（数据目录，权限 `0600`）：用户自持的平台 Key。
//!
//! Key 不在这张表里，也不在任何代码路径的返回值里 —— 见下条红线。
//!
//! 🔴 凭据纪律：Key 只经 `POST /admin/set-provider-key` 的请求体进入核心，只落
//! `providers.json`。不写日志、不进 `status.json`、不进面板偏好（`src/core/prefs.js` 的键白名单
//! 不含它）、不进 `ENV_ALLOW`；`provider-status` 只回「是否已配置」，连 Key 的尾几位都不回显。
//! Stage 3 把 Key 交给 OpenCode 走 `OPENCODE_CONFIG_CONTENT` 的配置内容，同样不经环境变量。

use crate::platform::join_host;
use crate::sync::atomic_write;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// 接入的一个平台。模型清单在探测与发布链路真正用上之前不放进来（避免一份没人读的表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provider {
    /// OpenCode 配置段的键名，也是 `providers.json` 的键名与面板用的稳定 id。
    pub id: &'static str,
    /// 面板显示名（也是 `<label> · <name>` 展示 ID 的前缀来源）。
    pub label: &'static str,
    /// 注入 `provider.<id>.npm` 的 SDK 包名。
    pub npm: &'static str,
    /// 注入 `provider.<id>.options.baseURL` 的地址（2026-10-03 实测自 models.dev 索引）。
    pub base_url: &'static str,
}

/// 四家都是 OpenAI 兼容端点，因此 Stage 3 只有一条注入代码路径。
const OPENAI_COMPATIBLE: &str = "@ai-sdk/openai-compatible";

/// 注册表：唯一真相。`id` 一旦发布出去（`providers.json` 的键、面板动作的 `provider` 参数）
/// 就不得改名，改了等于让用户已填的 Key 失效。
pub const PROVIDERS: [Provider; 4] = [
    Provider {
        id: "modelscope",
        label: "ModelScope",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://api-inference.modelscope.cn/v1",
    },
    Provider {
        id: "siliconflow-cn",
        label: "SiliconFlow",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://api.siliconflow.cn/v1",
    },
    Provider {
        id: "tencent-tokenhub",
        label: "腾讯混元 TokenHub",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://tokenhub.tencentmaas.com/v1",
    },
    Provider {
        // 注意 baseURL 是 `/api/paas/v4`，不是常见的 `/v1`。
        id: "zhipuai",
        label: "智谱",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://open.bigmodel.cn/api/paas/v4",
    },
];

/// 数据目录里的凭据文件名。
pub const PROVIDERS_FILE: &str = "providers.json";

/// Key 长度上限。输入来自面板，属于系统边界；这条只为挡住误粘贴（整份配置文件、二进制串），
/// 真实的平台 Key 远短于此。
pub const MAX_KEY_CHARS: usize = 4096;

/// 按 id 查注册表条目。
pub fn find(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

/// 「读-改-写」的串行锁：两个动作同时写同一份文件时，后一个不得覆盖前一个的 Key。
static WRITE: Mutex<()> = Mutex::new(());

fn write_lock() -> MutexGuard<'static, ()> {
    // 中毒只可能来自写入过程中的一次 panic，此时文件状态已不可信，继续串行没有意义。
    WRITE.lock().unwrap()
}

fn file(data_dir: &str) -> PathBuf {
    PathBuf::from(join_host(&[data_dir, PROVIDERS_FILE]))
}

/// 读出的 Key 表：只保留注册表内、且值是非空字符串的条目。
///
/// 文件缺失 / 坏 JSON / 顶层不是对象 / 值形态不对，一律当作「这个平台没配过」而不是报错 ——
/// 与 `settings.json` 的容错读法一致，面板因此照常显示「未配置」。写入永远是整份对象重写，
/// 所以注册表外的残留键（例如某平台改名后）会在下一次写入时自然清掉。
pub fn read_keys(data_dir: &str) -> Map<String, Value> {
    let mut keys = Map::new();
    let Ok(text) = std::fs::read_to_string(file(data_dir)) else {
        return keys;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return keys;
    };
    let Some(object) = parsed.as_object() else {
        return keys;
    };
    for provider in PROVIDERS {
        match object.get(provider.id).and_then(Value::as_str) {
            Some(key) if !key.trim().is_empty() => {
                keys.insert(provider.id.to_string(), json!(key.trim()));
            }
            _ => {}
        }
    }
    keys
}

/// 面板唯一能拿到的平台信息：`id` / `label` / 是否已配置。**不含任何 Key 材料。**
pub fn status(data_dir: &str) -> Value {
    let keys = read_keys(data_dir);
    json!({
        "providers": PROVIDERS.iter().map(|provider| json!({
            "id": provider.id,
            "label": provider.label,
            "configured": keys.contains_key(provider.id),
        })).collect::<Vec<Value>>()
    })
}

fn persist(data_dir: &str, keys: &Map<String, Value>) -> Result<(), String> {
    let text = serde_json::to_string(&Value::Object(keys.clone())).map_err(|error| error.to_string())?;
    // `atomic_write` 的临时文件走 `0600` 独占创建再 rename，凭据不会有一瞬间落在可读文件里。
    atomic_write(&file(data_dir), &text)
        .map(|_| ())
        .map_err(|error| error.message)
}

/// 校验用户提交的 Key 形态并返回裁掉首尾空白后的值。
///
/// 这是系统边界（输入来自面板），不是内部信任路径：空白、超长、含控制字符的一律拒掉。
/// 公开是因为 HTTP 层要把它映射成 400，而 [`set_key`] 内部同样调用它，两道守卫是同一份实现。
pub fn check_key(key: &str) -> Result<String, String> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("Key 不能为空".to_string());
    }
    if trimmed.chars().count() > MAX_KEY_CHARS {
        return Err(format!("Key 过长（上限 {MAX_KEY_CHARS} 字符）"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err("Key 含控制字符".to_string());
    }
    Ok(trimmed.to_string())
}

/// 写入或更新一个平台的 Key。入参错误返回文案（由调用方映射为 400），IO 错误原样上报。
pub fn set_key(data_dir: &str, id: &str, key: &str) -> Result<(), String> {
    let Some(provider) = find(id) else {
        return Err(format!("未知平台：{id}"));
    };
    let trimmed = check_key(key)?;
    let _guard = write_lock();
    let mut keys = read_keys(data_dir);
    keys.insert(provider.id.to_string(), json!(trimmed));
    persist(data_dir, &keys)
}

/// 删除一个平台的 Key。未配置过也算成功（幂等：面板上「清除」不该因为状态滞后而报错）。
pub fn clear_key(data_dir: &str, id: &str) -> Result<(), String> {
    let Some(provider) = find(id) else {
        return Err(format!("未知平台：{id}"));
    };
    let _guard = write_lock();
    let mut keys = read_keys(data_dir);
    if keys.remove(provider.id).is_none() {
        return Ok(());
    }
    persist(data_dir, &keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(1);

    /// 每个用例一个独立数据目录（写入只碰它，测试之间不互相干扰、也不碰真实数据目录）。
    fn temp_dir() -> String {
        let base = std::env::temp_dir().join(format!(
            "wbbridge-providers-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&base).expect("临时数据目录");
        base.to_string_lossy().to_string()
    }

    #[test]
    fn registry_ids_are_unique_and_complete() {
        let mut ids: Vec<&str> = PROVIDERS.iter().map(|provider| provider.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "注册表里的 id 必须唯一");
        for provider in PROVIDERS {
            assert!(provider.label.starts_with(|c: char| !c.is_whitespace()));
            assert!(provider.base_url.starts_with("https://"), "{} 必须走 HTTPS", provider.id);
            assert_eq!(find(provider.id).map(|found| found.id), Some(provider.id));
        }
        assert_eq!(find("not-registered"), None);
    }

    #[test]
    fn status_lists_every_provider_as_unconfigured_when_nothing_is_stored() {
        let value = status(&temp_dir());
        let listed = value["providers"].as_array().expect("providers 是数组");
        assert_eq!(listed.len(), PROVIDERS.len());
        assert!(listed
            .iter()
            .all(|item| item["configured"] == json!(false)));
        assert_eq!(listed[0]["id"], json!("modelscope"));
        assert_eq!(listed[0]["label"], json!("ModelScope"));
    }

    #[test]
    fn set_then_read_roundtrips_and_clear_removes_only_that_platform() {
        let dir = temp_dir();
        set_key(&dir, "modelscope", "  ms-secret-value  ").expect("写入成功");
        set_key(&dir, "zhipuai", "glm-secret-value").expect("写入成功");
        let keys = read_keys(&dir);
        assert_eq!(keys.get("modelscope"), Some(&json!("ms-secret-value")), "首尾空白必须被裁掉");
        assert_eq!(keys.get("zhipuai"), Some(&json!("glm-secret-value")));

        clear_key(&dir, "modelscope").expect("清除成功");
        let keys = read_keys(&dir);
        assert!(!keys.contains_key("modelscope"));
        assert!(keys.contains_key("zhipuai"), "清除一个平台不得波及其他平台");
        // 幂等：再清一次仍然成功。
        clear_key(&dir, "modelscope").expect("重复清除");
    }

    /// 🔴 凭据红线：`provider-status` 是面板唯一能读到的平台信息，它不得含任何 Key 材料。
    /// 这条断言在 `status()` 里加回「掩码尾位」或任何回显字段时会立刻失败。
    #[test]
    fn status_never_contains_key_material() {
        let dir = temp_dir();
        set_key(&dir, "siliconflow-cn", "sk-abcdef1234567890").expect("写入成功");
        let text = serde_json::to_string(&status(&dir)).expect("状态可序列化");
        assert!(!text.contains("sk-abcdef1234567890"));
        assert!(!text.contains("abcdef1234567890"));
        assert!(!text.contains("sk-"));
        assert!(!text.contains(PROVIDERS_FILE), "状态里不该出现凭据文件路径");
    }

    #[test]
    fn stored_file_holds_the_key_but_the_status_shape_does_not() {
        let dir = temp_dir();
        set_key(&dir, "tencent-tokenhub", "hunyuan-secret").expect("写入成功");
        let raw = std::fs::read_to_string(file(&dir)).expect("读回凭据文件");
        assert!(raw.contains("hunyuan-secret"));
        assert_eq!(raw.matches('"').count(), 4, "文件形态：{{\"<id>\":\"<key>\"}}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&dir)).expect("凭据文件可 stat").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "凭据文件必须 0600");
        }
    }

    #[test]
    fn bad_input_is_rejected_without_touching_the_file() {
        let dir = temp_dir();
        assert_eq!(check_key("  spaced key  ").expect("合法 Key"), "spaced key", "只裁首尾空白");
        assert!(set_key(&dir, "not-registered", "whatever").is_err(), "注册表外的 id 必须拒绝");
        assert!(set_key(&dir, "zhipuai", "   ").is_err(), "空白 Key 必须拒绝");
        assert!(set_key(&dir, "zhipuai", "line\nbreak").is_err(), "控制字符必须拒绝");
        assert!(
            set_key(&dir, "zhipuai", &"x".repeat(MAX_KEY_CHARS + 1)).is_err(),
            "超长 Key 必须拒绝"
        );
        assert_eq!(read_keys(&dir).len(), 0, "被拒绝的写入不得留下条目");
        assert!(clear_key(&dir, "not-registered").is_err(), "清除同样只认注册表里的 id");
    }

    #[test]
    fn corrupt_or_foreign_shaped_file_reads_as_unconfigured() {
        let dir = temp_dir();
        std::fs::write(file(&dir), "{ not json").expect("写入坏文件");
        assert_eq!(read_keys(&dir).len(), 0);
        assert_eq!(status(&dir)["providers"][0]["configured"], json!(false));

        std::fs::write(file(&dir), r#"{"modelscope":42,"nope":"k"}"#).expect("写入异形值");
        assert_eq!(read_keys(&dir).len(), 0, "非字符串值与注册表外的键都不算已配置");

        // 整份重写：注册表外的残留键随下一次写入自然清掉，不会永久占着文件。
        set_key(&dir, "zhipuai", "glm-2").expect("写入成功");
        let raw = std::fs::read_to_string(file(&dir)).expect("读回凭据文件");
        assert!(!raw.contains("nope"));
        assert!(!raw.contains("42"));
    }
}
