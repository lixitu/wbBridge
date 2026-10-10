//! WB · auto —— 一个对外可见的合成模型名，按请求内容从**已可用**的模型里选一个转发。
//!
//! 本模块**无 IO、无状态、无 JS 前身**：只做「给一份 `usable_models()` 快照 + 一份请求体，
//! 选出该用哪个模型的全限定 id」。选中之后的链路（`prepare` → `backend` → `record`）一字不改，
//! 因此被选中模型的失败仍走既有判决（`orchestration.rs::record` 的 `validated.remove`）。
//!
//! 三条设计约束（详见 `docs/plans/2026-10-10-wb-auto-smart-router.md` §2）：
//! 1. 返回值必须是**全限定 id**（如 `opencode/glm-4.7-flash-free`），不能是面板用的 client id
//!    （`OC · 名称`）——记账键与实际模型必须是同一个值，否则失败判决打在假键上。
//! 2. 能力谓词一律按「缺键即 false」比较（`== Some(&json!(true))`），与
//!    `backend.rs::free_models_in` 归一出来的 `images` / `toolcall` 布尔值同形态。
//! 3. 图片与工具混合请求单列一档：`prepare` 的两个闸门（图片 → 400 `unsupported_content`，
//!    chatOnly 带 tools → 400 `tools_not_supported`）都是**服务端本地拒绝**，选错档位会让
//!    「带截图 + 带工具定义」这种常见 agent 请求注定 400。

use crate::protocol::random_uuid;
use serde_json::{json, Value};

/// 对外暴露的唯一字面量：客户端把 `model` 配成它即可获得自动路由。
pub const AUTO_MODEL_ID: &str = "WB · auto";

/// 请求是否携带图片：任一 message 的 `content` 是数组且含 `type == "image_url"` 的 part。
/// 字符串 content 恒为 false —— 与 `prepare` 的多模态解析同形态（`protocol.rs` 的 content 分支）。
pub fn has_images(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts.iter().any(|part| {
                    part.get("type").and_then(Value::as_str) == Some("image_url")
                })
            })
    })
}

/// 请求是否携带**非空** tools 数组。null / 缺失 / 空数组 / 非数组都是 false，
/// 与 `prepare` 把 `tools` 归一成 `[]` 后再判定的口径一致。
pub fn has_tools(body: &Value) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

/// 能力位读取：缺失、null、非布尔真值都按 false（不做 truthy 宽松判定）。
fn flag(model: &Value, key: &str) -> bool {
    model.get(key) == Some(&json!(true))
}

/// 按请求内容判定单个候选模型是否合格。
fn fits(model: &Value, images: bool, tools: bool) -> bool {
    if flag(model, "chatOnly") {
        // chatOnly 模型没有工具能力（`prepare` 会当场 400），只能服务不含 tools 的请求。
        return !tools;
    }
    if images && tools {
        return flag(model, "images") && flag(model, "toolcall");
    }
    if images {
        return flag(model, "images");
    }
    if tools {
        return flag(model, "toolcall");
    }
    true
}

/// 池里是否存在**能接工具调用请求**的候选（即 `fits(model, false, true)` 为真）。
///
/// 唯一的消费者是插件配置里那条 `WB · auto` 的 `supportsToolCall`（`sync.rs::auto_entry`）：
/// 全池都是 chatOnly 时把该字段写成 true，插件就会按工具形态发请求，而工具档没有候选 → 按 D3
/// 退回全池 → 落到 chatOnly 上被 `prepare` 当场 400，这条别名于是恒定失败。声明口径必须与
/// 选择器的档位谓词同源，否则「插件以为能给」和「核心选得出」会分叉。
pub fn any_tool_capable(models: &[Value]) -> bool {
    models.iter().any(|model| fits(model, false, true))
}

/// 核心选择器（纯函数，选择器注入以便单测钉住「候选域是谁」）。
///
/// `pool` 必须是调用方**只取读一次**的 `usable_models()` 快照；`pick(len)` 返回候选集中的下标
/// （只被调用一次，故取 `FnOnce`，测试因此能在闭包里记下候选域大小），`None` 表示放弃本次路由
/// （交回 `prepare` 走既有 400）。候选层为空时退回全池，全池也为空则返回 `None` ——
/// 不做静默兜底、不新增错误码。
pub fn select_with(
    body: &Value,
    pool: &[Value],
    pick: impl FnOnce(usize) -> Option<usize>,
) -> Option<String> {
    let messages: &[Value] = body
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let images = has_images(messages);
    let tools = has_tools(body);

    let matching: Vec<&Value> = pool
        .iter()
        .filter(|model| fits(model, images, tools))
        .collect();
    // 退全池而非「报一个专用错误」：与用户手动选到能力不匹配的模型时的行为保持一致。
    let candidates = if matching.is_empty() {
        pool.iter().collect::<Vec<&Value>>()
    } else {
        matching
    };
    let index = pick(candidates.len())?;
    candidates
        .get(index)?
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 生产入口：均匀随机（负载分散），复用既有 `random_uuid()`（uuid v4，OS CSPRNG 驱动），
/// 取其前 4 个 hex 位（16 bit）对候选数取模。池规模 ~30 时模偏差 <0.05%，
/// 足够分散；不为此引入 `fastrand` / `rand`。
pub fn auto_select(body: &Value, pool: &[Value]) -> Option<String> {
    select_with(body, pool, |len| {
        if len == 0 {
            return None;
        }
        let hex = random_uuid().replace('-', "");
        let head = hex.get(..4)?;
        let value = u32::from_str_radix(head, 16).ok()?;
        Some((value % len as u32) as usize)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// 池条目的最小形态：`usable_models()` 的字段远多于这些，路由只读 id 与三个能力位。
    fn entry(id: &str, images: bool, toolcall: bool, chat_only: bool) -> Value {
        json!({ "id": id, "images": images, "toolcall": toolcall, "chatOnly": chat_only })
    }

    fn pool() -> Vec<Value> {
        vec![
            entry("opencode/text-only", false, false, false),
            entry("opencode/vision", true, false, false),
            entry("opencode/agent", false, true, false),
            entry("opencode/all", true, true, false),
            entry("opencode/chat", false, false, true),
        ]
    }

    fn body(model: &str, content: Value, tools: Value) -> Value {
        json!({ "model": model, "messages": [{ "role": "user", "content": content }], "tools": tools })
    }

    /// 固定选择器：始终取第一个候选（顺序由池本身的顺序决定），便于断言候选域。
    fn first(len: usize) -> Option<usize> {
        (len > 0).then_some(0)
    }

    #[test]
    fn plain_text_considers_the_whole_pool_including_chat_only_models() {
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &pool(), first);
        assert_eq!(
            got.as_deref(),
            Some("opencode/text-only"),
            "纯文本的候选域必须是全池（含 chatOnly）"
        );
        // 候选长度即全池大小，说明没有任何能力过滤。
        let mut seen: Option<usize> = None;
        let len = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &pool(), |n| {
            seen = Some(n);
            Some(n - 1)
        });
        assert_eq!(seen, Some(5), "纯文本层不得过滤掉任何模型");
        assert_eq!(len.as_deref(), Some("opencode/chat"), "末位候选是 chatOnly 模型");
    }

    #[test]
    fn image_requests_only_select_models_declaring_image_input() {
        let images = json!([
            { "type": "text", "text": "看看这张图" },
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } },
        ]);
        let got = select_with(&body(AUTO_MODEL_ID, images, Value::Null), &pool(), first);
        assert_eq!(got.as_deref(), Some("opencode/vision"), "首个合格候选");
        // content 是数组但全是 text part：没有图片，候选域仍是全池（末位即 chatOnly）。
        let plain = json!([{ "type": "text", "text": "只有文字" }]);
        let got = select_with(&body(AUTO_MODEL_ID, plain, Value::Null), &pool(), |n| Some(n - 1));
        assert_eq!(got.as_deref(), Some("opencode/chat"));
    }

    #[test]
    fn tool_requests_exclude_chat_only_and_image_only_models() {
        let tools = json!([{ "type": "function", "function": { "name": "read_file" } }]);
        let got = select_with(&body(AUTO_MODEL_ID, json!("用工具"), tools.clone()), &pool(), first);
        assert_eq!(got.as_deref(), Some("opencode/agent"));
        let mut domain: Vec<usize> = Vec::new();
        let _ = select_with(&body(AUTO_MODEL_ID, json!("用工具"), tools), &pool(), |n| {
            domain.push(n);
            Some(0)
        });
        assert_eq!(domain, vec![2], "工具层候选只有 toolcall 且非 chatOnly 的两个");
    }

    #[test]
    fn image_plus_tools_requires_both_capabilities() {
        let content = json!([
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
        ]);
        let tools = json!([{ "type": "function", "function": { "name": "run" } }]);
        let mut domain = Vec::new();
        let got = select_with(&body(AUTO_MODEL_ID, content, tools), &pool(), |n| {
            domain.push(n);
            Some(0)
        });
        assert_eq!(got.as_deref(), Some("opencode/all"), "混合请求只能落在双能力模型上");
        assert_eq!(domain, vec![1], "候选集恰为唯一的双能力模型");
    }

    #[test]
    fn empty_image_tier_falls_back_to_the_whole_pool() {
        // 池里没有 images 模型时，图片请求退全池（该请求随后要么被 prepare 本地 400、要么成功）。
        let only_text = vec![entry("opencode/text-only", false, false, false)];
        let content = json!([{ "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }]);
        let got = select_with(&body(AUTO_MODEL_ID, content, Value::Null), &only_text, first);
        assert_eq!(got.as_deref(), Some("opencode/text-only"), "候选空时退回全池而不是报新错");
    }

    #[test]
    fn empty_tool_tier_falls_back_to_the_whole_pool() {
        let only_text = vec![entry("opencode/text-only", false, false, false)];
        let tools = json!([{ "type": "function", "function": { "name": "run" } }]);
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), tools), &only_text, first);
        assert_eq!(got.as_deref(), Some("opencode/text-only"));
    }

    #[test]
    fn an_empty_pool_selects_nothing() {
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &[], |_| Some(0));
        assert_eq!(got, None, "池空必须交回 prepare 报既有 400，绝不凭空造键");
    }

    #[test]
    fn a_single_candidate_still_goes_through_the_picker_with_len_one() {
        let dual = vec![entry("opencode/all", true, true, false)];
        let content = json!([{ "type": "image_url", "image_url": { "url": "data:image/gif;base64,AAAA" } }]);
        let tools = json!([{ "type": "function", "function": { "name": "run" } }]);
        let mut seen = None;
        let got = select_with(&body(AUTO_MODEL_ID, content, tools), &dual, |n| {
            seen = Some(n);
            Some(0)
        });
        assert_eq!(seen, Some(1), "唯一候选也要走选择器（len=1），与多候选同一条路径");
        assert_eq!(got.as_deref(), Some("opencode/all"));
    }

    #[test]
    fn has_images_only_reads_array_content_with_image_parts() {
        assert!(has_images(&[json!({ "content": [{ "type": "image_url" }] })]));
        assert!(
            !has_images(&[json!({ "content": "图呢？" })]),
            "字符串 content 恒为无图片（与 prepare 的解析同形态）"
        );
        assert!(!has_images(&[json!({ "content": [] })]), "空 part 数组不算图片请求");
        assert!(
            !has_images(&[json!({ "content": [{ "type": "text", "text": "x" }] })]),
            "非 image_url 的 part 不得被当成图片"
        );
        assert!(
            !has_images(&[json!({ "content": null })]),
            "content 为 null 时 prepare 会归一成空串，同样不该被当成图片"
        );
        assert!(
            has_images(&[
                json!({ "content": [{ "type": "text", "text": "x" }] }),
                json!({ "content": [{ "type": "image_url", "image_url": { "url": "data:image/jpeg;base64,/9j" } }] }),
            ]),
            "任一条 message 带图片即算图片请求"
        );
    }

    #[test]
    fn has_tools_reads_only_nonempty_arrays() {
        assert!(has_tools(&json!({ "tools": [{ "type": "function" }] })));
        for (label, body) in [
            ("null", json!({ "tools": Value::Null })),
            ("缺失", json!({ "model": "x" })),
            ("空数组", json!({ "tools": [] })),
            ("非数组", json!({ "tools": { "name": "run" } })),
            ("字符串", json!({ "tools": "run" })),
        ] {
            assert!(!has_tools(&body), "{label} 必须按无工具处理");
        }
    }

    #[test]
    fn the_selected_id_is_the_fully_qualified_model_id() {
        // 记账键（validated / modelResults / usage）与响应回显都取这个值，client id 会让失败判决打在假键上。
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &pool(), |_| Some(3));
        assert_eq!(got.as_deref(), Some("opencode/all"));
        assert_ne!(got.as_deref(), Some("OC · All"));
    }

    #[test]
    fn picker_returning_none_abandons_the_route() {
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &pool(), |_| None);
        assert_eq!(got, None, "选择器放弃时不得改写 body，交给 prepare 的既有 400");
    }

    #[test]
    fn auto_select_always_lands_inside_the_pool_and_never_invents_wb_auto() {
        let body = body(AUTO_MODEL_ID, json!("hi"), Value::Null);
        let ids: Vec<String> = pool().iter().map(|m| m["id"].as_str().unwrap().to_string()).collect();
        let mut distinct = std::collections::HashSet::new();
        for _ in 0..200 {
            let got = auto_select(&body, &pool()).expect("池非空必有选择");
            assert!(ids.contains(&got), "选中的必须是池内 id：{got}");
            assert_ne!(got, AUTO_MODEL_ID, "绝不可能把自己路由给自己");
            distinct.insert(got);
        }
        assert!(distinct.len() > 1, "200 次选择必须分散到多个模型，否则是退化成固定选第一个");
    }

    #[test]
    fn auto_select_on_an_empty_pool_selects_nothing() {
        assert_eq!(auto_select(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &[]), None);
    }

    #[test]
    fn chat_only_models_stay_reachable_for_plain_text_but_never_for_tools() {
        let chat = vec![entry("opencode/chat", false, false, true)];
        let text = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &chat, first);
        assert_eq!(text.as_deref(), Some("opencode/chat"), "纯文本时 chatOnly 合法");
        let tools = json!([{ "type": "function", "function": { "name": "run" } }]);
        let forced = select_with(&body(AUTO_MODEL_ID, json!("hi"), tools), &chat, first);
        assert_eq!(
            forced.as_deref(),
            Some("opencode/chat"),
            "池里只有 chatOnly 时按「候选空→退全池」处理，随后的 400 由 prepare 给出（决策点 D3）"
        );
    }

    #[test]
    fn missing_capability_keys_are_read_as_false() {
        // 真实池条目一定有 images/toolcall（free_models_in 归一成布尔），这里钉住「缺键即 false」。
        let bare = vec![json!({ "id": "opencode/bare" }), entry("opencode/vision", true, false, false)];
        let content = json!([{ "type": "image_url", "image_url": { "url": "data:image/webp;base64,AAAA" } }]);
        let got = select_with(&body(AUTO_MODEL_ID, content, Value::Null), &bare, first);
        assert_eq!(got.as_deref(), Some("opencode/vision"));
    }

    #[test]
    fn selected_ids_survive_a_model_without_an_id_entry() {
        // 池里混进无 id 的条目时不得 panic，也绝不返回空串当模型 id。
        let mixed = vec![json!({ "name": "no id" }), entry("opencode/ok", false, false, false)];
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &mixed, |_| Some(0));
        assert_eq!(got, None, "选中条目没有 id 时必须放弃路由，而不是交出一个空键");
        let got = select_with(&body(AUTO_MODEL_ID, json!("hi"), Value::Null), &mixed, |_| Some(1));
        assert_eq!(got.as_deref(), Some("opencode/ok"));
    }

    #[test]
    fn auto_model_id_is_the_only_literal_the_surface_uses() {
        assert_eq!(AUTO_MODEL_ID, "WB · auto");
        assert!(!pool().iter().any(|m| m["id"] == json!(AUTO_MODEL_ID)));
    }
}
