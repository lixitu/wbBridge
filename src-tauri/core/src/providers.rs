//! 平台注册表与凭据通道（多平台免费模型接入的底座）。
//!
//! 这里只有两张表，且职责互不相通：
//!
//! 1. [`PROVIDERS`]：接入哪些平台、面板显示什么标签、注入 OpenCode 配置段时用哪个 npm 包与
//!    baseURL，以及（仅在有权威清单的平台上）对方实际承接推理的模型清单。
//!    **随版本发布的常量表，不做远程拉取** —— 上游新增免费模型不会自动出现，
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

/// 平台**实际承接推理**的一个模型条目（权威清单的来源见 [`Provider::models`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredModel {
    /// 上游的模型 id，逐字用作注入声明段 `models` 的键，也因此成为请求体里的 `modelID`
    /// （`split_namespace` 按**第一个** `/` 切，`org/model` 形态整体落在 `modelID` 里）。
    pub id: &'static str,
    /// 面板与客户端展示名（前缀是 `Provider::label`）。
    pub name: &'static str,
    /// 声明的上下文上限（token）。**取保守值**：ModelScope 的 `config.json` 里
    /// `max_position_embeddings` 与网关实际承接的上限不一致（实测 `MiniMax-M1-80k` 报
    /// 10240000、多数模型根本不给这个键），所以这里按「观察值往下降一档」给，宁可让客户端
    /// 提前收着点，也不要报一个打过去就超限的大数。
    pub context: u64,
    /// 声明的输出上限（token），按 `context` 的 1/8 给。
    ///
    /// 🔴 **不得省略**：OpenCode 的配置校验要求 `limit` 里 `context` 与 `output` **同时存在**，
    /// 只给 `context` 会让整份 `OPENCODE_CONFIG_CONTENT` 被判成 `ConfigInvalidError`
    /// （2026-10-10 沙箱实测：`/provider` 直接返回错误体、`all` 为空，等于所有平台一起没有模型，
    /// 而不只是这一个条目被丢）。
    pub output: u64,
    /// 是否宣称支持函数调用。全部给 `true`，让**探测**去真实验证：目录声明为真、网关实际
    /// 不让调用时报「不支持函数调用」，由 `probe.rs::tool_call_unsupported` 走 chat-only 降级
    /// 按仅对话发布。这里写 `false` 反而会让探测入口直接本地判失败、失去降级机会。
    pub tool_call: bool,
}

/// 接入的一个平台。
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
    /// 该平台的**权威模型清单**：非空时它替代 models.dev 的 catalog 声明，同时约束两处——
    /// 注入声明段只写这份清单里的模型（`runtime::providers_section_for`），发现阶段也只放行
    /// 这份清单里的 key（`backend::free_models_in`）。
    ///
    /// 🔴 为什么需要它（2026-10-10 实测）：ModelScope 免费推理网关与 models.dev 的声明对不上——
    /// 该网关 `GET /v1/models`（公开可读）在册 35 个 id，而 catalog 里那 7 个「免费」模型
    /// **一个都不在其中**，逐条探测全部按 `Model id : <id> , has no provider supported` 失败。
    /// OpenCode 又是把注入段与 catalog **合并**而非替换，所以只声明不够、发现阶段还得按这份
    /// 清单把 catalog 的过期 id 过滤掉，否则探测队列里依旧是对方不承接的那批。
    ///
    /// 空切片 = 「没有权威清单」，沿用 catalog 声明 + 锚点占位的既有形态（其余三家目前如此：
    /// 它们的 catalog 免费模型能被真实承接，改用清单只会平白缩小可选面）。
    pub models: &'static [DeclaredModel],
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
        // 逐条来自 2026-10-10 对 `GET https://api-inference.modelscope.cn/v1/models`（公开可读、
        // 无需 Key）的实测抓取，并按模型页的分类筛过一遍：只留通用「nlp / 文本生成」，
        // 视觉多模态理解（Qwen3.5/3.8、InternVL、Intern-S、MiniMax-M3、ERNIE-VL、DeepSeek-V4.1-Flash、
        // Step-3.7-Flash）、图片生成（Qwen-Image-Edit）、专用垂类（XiYanSQL 文生 SQL、
        // CompassJudger 评审、AntAngelMed 医疗）、无分类的 early-access/EA-29B-A4B 全部排除。
        // 同族的 DeepSeek-V4-Pro-0813 未收录：与 V4-Pro 是同一能力的两个快照，多收一个就多烧一份
        // 探测额度（每模型 60s 预算），面板也不需要两个入口。
        //
        // 🔴 同日**真实 Key 实测**后再削掉 4 条「在册但打不通」的（面板上它们只是常驻的红色行，
        // 除了烧额度没有任何用处，所以直接从清单里去掉，而不是留着让用户看）：
        // `PaddlePaddle/ERNIE-4.5-{0.3B,21B-A3B,300B-A47B}-PT` → HTTP 401
        // `The model does not exist or you do not have access to it.`（条目在 `/v1/models` 里，
        // 但这把 Key 没被开通授权，需要先在对方控制台申请）；
        // `meituan-longcat/LongCat-Flash-Lite` → HTTP 400 `Unsupported model (model=LongCat-Flash-Chat)`
        // （网关把它映射到一个自己也不承接的 Chat 变体）。
        // 若日后对方给这些条目开了免费额度，把它们加回这张表即可（随版本发布，不做远程拉取）。
        models: &[
            DeclaredModel {
                id: "MiniMax/MiniMax-M1-80k",
                name: "MiniMax M1 80k",
                context: 65_536,
                output: 8_192,
                tool_call: true,
            },
            DeclaredModel {
                id: "ZhipuAI/GLM-4.7-Flash",
                name: "GLM 4.7 Flash",
                context: 131_072,
                output: 16_384,
                tool_call: true,
            },
            DeclaredModel {
                id: "ZhipuAI/GLM-5.2",
                name: "GLM 5.2",
                context: 131_072,
                output: 16_384,
                tool_call: true,
            },
            DeclaredModel {
                id: "deepseek-ai/DeepSeek-V4-Flash-0731",
                name: "DeepSeek V4 Flash 0731",
                context: 131_072,
                output: 16_384,
                tool_call: true,
            },
            DeclaredModel {
                id: "deepseek-ai/DeepSeek-V4-Pro",
                name: "DeepSeek V4 Pro",
                context: 131_072,
                output: 16_384,
                tool_call: true,
            },
            DeclaredModel {
                id: "mistralai/Mistral-Large-Instruct-2407",
                name: "Mistral Large Instruct 2407",
                context: 65_536,
                output: 8_192,
                tool_call: true,
            },
            DeclaredModel {
                id: "nex-agi/Nex-N2.5-Pro",
                name: "Nex N2.5 Pro",
                context: 32_768,
                output: 4_096,
                tool_call: true,
            },
            DeclaredModel {
                id: "nex-agi/Nex-N2.5-mini",
                name: "Nex N2.5 mini",
                context: 32_768,
                output: 4_096,
                tool_call: true,
            },
            DeclaredModel {
                id: "stepfun-ai/Step-3.5-Flash",
                name: "Step 3.5 Flash",
                context: 131_072,
                output: 16_384,
                tool_call: true,
            },
        ],
    },
    Provider {
        id: "siliconflow-cn",
        label: "SiliconFlow",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://api.siliconflow.cn/v1",
        models: &[],
    },
    Provider {
        id: "tencent-tokenhub",
        label: "腾讯混元 TokenHub",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://tokenhub.tencentmaas.com/v1",
        models: &[],
    },
    Provider {
        // 注意 baseURL 是 `/api/paas/v4`，不是常见的 `/v1`。
        id: "zhipuai",
        label: "智谱",
        npm: OPENAI_COMPATIBLE,
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        models: &[],
    },
];

/// 该平台的权威模型清单；空切片表示没有（沿用 models.dev 的 catalog 声明）。
///
/// 命名空间不在注册表内（`opencode` 与合成命名空间）同样返回空切片——发现阶段据此放行，
/// 所以「查不到平台」绝不能变成「一个模型都不出」。
pub fn served_models(namespace: &str) -> &'static [DeclaredModel] {
    match find(namespace) {
        Some(provider) => provider.models,
        None => &[],
    }
}

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

    /// 权威清单的形态自检：这份表是**手工抓取**的（不联网、没有远端真相可对账），
    /// 所以每一条都得靠断言钉住可用性与「清单与非清单平台的分界」。
    /// 只有 ModelScope 有清单是 2026-10-10 的实测结论（那家 catalog 与网关交集为空），
    /// 其余三家按 catalog 声明即可命中，加清单只会平白缩小可选面。
    #[test]
    fn the_authoritative_model_list_is_well_formed_and_scoped_to_modelscope() {
        for provider in PROVIDERS {
            let has_list = !provider.models.is_empty();
            assert_eq!(
                has_list,
                provider.id == "modelscope",
                "{} 的清单存在性与预期平台不符",
                provider.id
            );
            let mut ids: Vec<&str> = Vec::new();
            let mut names: Vec<&str> = Vec::new();
            for model in provider.models {
                // id 必须是上游的 `org/model` 全限定形态：`split_namespace` 按第一个 `/` 切，
                // 平台命名空间在前、这一段整体作为请求体里的 modelID。
                assert!(
                    model.id.contains('/') && !model.id.starts_with('/') && !model.id.ends_with('/'),
                    "{} 的模型 id 形态不对：{}",
                    provider.id,
                    model.id
                );
                assert!(!model.name.trim().is_empty(), "{} 必须有展示名", model.id);
                assert!(model.context > 0, "{} 的上下文上限必须为正", model.id);
                // 🔴 `output` 是 OpenCode 配置校验的必填项：缺失会让**整份**注入配置被判
                // ConfigInvalidError（2026-10-10 沙箱实测），所以这里按必填项钉住，且不得超过上下文。
                assert!(
                    model.output > 0 && model.output <= model.context,
                    "{} 的输出上限必须是落在上下文以内的正数（context={}，output={}）",
                    model.id,
                    model.context,
                    model.output
                );
                assert!(
                    ids.iter().all(|seen| *seen != model.id),
                    "同一平台不得重复声明 {}",
                    model.id
                );
                assert!(
                    names.iter().all(|seen| *seen != model.name),
                    "展示名重复会让面板出现两行同名：{}",
                    model.name
                );
                ids.push(model.id);
                names.push(model.name);
            }
        }
        // 清单必须按 id 升序：注入段的键序来自这张表，排序让 diff 与抓取口径稳定。
        let modelscope = find("modelscope").expect("注册表含 modelscope");
        let mut sorted = modelscope.models.to_vec();
        sorted.sort_by(|a, b| a.id.cmp(b.id));
        assert_eq!(sorted, modelscope.models.to_vec(), "清单必须按 id 升序");
    }

    /// 查不到平台必须回落**空清单**（＝不过滤），而不是「有清单但一条都不在」：
    /// 后者会把该命名空间发现出的模型全数丢掉。`opencode` 与合成命名空间都走这条路。
    #[test]
    fn served_models_is_empty_for_any_namespace_without_a_list() {
        for namespace in ["opencode", "vendor", "siliconflow-cn", "", "not-registered"] {
            assert!(
                served_models(namespace).is_empty(),
                "{namespace} 不该有清单，否则发现阶段会全量过滤"
            );
        }
        assert!(
            !served_models("modelscope").is_empty(),
            "modelscope 必须带清单，否则 catalog 的过期 id 会继续进探测队列"
        );
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
