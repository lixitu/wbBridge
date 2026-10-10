# WB.auto — 智能路由模型设计计划

> 日期：2026-10-10 ｜ 状态：**设计定稿，待开工** ｜ 依赖：v1.1.10 完整可用模型池（`usable_models()`）

---

## 1. 动机

WB Bridge 已经聚合了多平台免费模型——OpenCode 自带 8 个 + ModelScope / SiliconFlow / 腾讯 / 智谱。每个模型的能力不同（图片输入、工具调用、仅对话），agent 调用时得手动选模型。

**WB.auto**：对外暴露一个合成模型名 `"WB.auto"`。后端收到请求后——根据请求内容自动选适配的模型转发，调用方完全无感知。

```
agent 配置 model = "WB.auto"
  → 发图片请求 → 后端选一个支持图片的模型
  → 发工具调用   → 后端选一个支持 toolcall 的模型
  → 纯文本对话   → 随机选一个可用模型（负载分散）
```

---

## 2. 核心设计

### 2.1 路由决策树

```
model == "WB.auto" ?
│
├─ 消息体包含 image_url？
│  └─ YES → 筛 usable_models()：
│            ✅ images == true
│            ✅ chatOnly == false（看图请求通常也要工具）
│            → 随机选一个（均匀分布）
│
├─ 消息体包含 tools（非空）？
│  └─ YES → 筛：
│            ✅ toolcall == true
│            ✅ chatOnly == false
│            → 随机选一个
│
└─ 纯文本、无工具
   └─ 从全部 usable_models() 随机选一个（含 chatOnly，它们更快/更轻）
```

**降级策略**：按能力筛完之后候选集为空时，退回到全部 `usable_models()` 中随机选一个，不报错——让上游模型自己的报错（如"不支持图片"）原样透传，和用户手动选错模型的行为一致。

**唯一选**：候选集只有 1 个时，直接选它，不随机。

### 2.2 对外形态

`GET /v1/models` 返回列表末尾追加一条：

```json
{ "id": "WB.auto", "object": "model", "owned_by": "wbBridge", "name": "WB.auto" }
```

调用方按正常 OpenAI 兼容方式请求：

```bash
curl http://127.0.0.1:41980/v1/chat/completions \
  -H "Authorization: Bearer <api-key>" \
  -d '{"model":"WB.auto","messages":[{"role":"user","content":"hello"}]}'
```

响应体中的 `"model"` 字段填**实际被选中的模型名**（不是 `"WB.auto"`），方便 agent 出错时定位。

### 2.3 关键设计取舍

| 决策 | 选项 | 理由 |
|---|---|---|
| 响应 `model` 字段 | 填实际模型名 | agent 出错时能定位；`WB.auto` 无助于排查 |
| chatOnly 进纯文本池 | 可以 | chatOnly 模型通常更快/更轻；有工具请求排除（会报错） |
| 随机算法 | 均匀随机（`fastrand`） | 不需要加权，免费模型轮着用、负载分散 |
| 图片路没 hit 时 | 退回全量池 | 宁可模型自己报「不支持图片」，不要服务端替用户做错误判断 |
| 单个可用模型 | 直接选中 | 不需要随机 |
| 并发安全 | 无状态纯函数 | `auto_select(body, models)` 不读不写任何共享状态 |

---

## 3. 改动面（三个落点）

### 3.1 新增 `src-tauri/core/src/auto.rs`（~100 行）

纯函数模块，无状态、无 IO。

```rust
/// WB.auto 合成模型名。
pub const AUTO_MODEL_ID: &str = "WB.auto";

/// 按请求内容从可用模型池中选一个最适配的模型。
/// 
/// 决策树见 §2.1。返回被选中的模型 id（全限定形态，如 "opencode/gpt-4o-mini"）。
/// 候选集为空时退回全量池。
pub fn auto_select(body: &Value, models: &[Value]) -> Option<String>;

/// 扫描消息体中是否包含 image_url 类型的 content part。
fn has_images(messages: &[Value]) -> bool;

/// 请求是否携带非空的 tools 数组。
fn has_tools(body: &Value) -> bool;
```

单测覆盖（至少 8 项）：
- 纯文本请求 → 从全量池随机选中（可重现种子）
- 含图片请求 → 只命中 `images==true && chatOnly==false`
- 含工具请求 → 只命中 `toolcall==true && chatOnly==false`
- 图片路候选集为空 → 退回全量池
- 工具路候选集为空 → 退回全量池
- 全量池只有 1 个模型 → 直接返回
- 空模型列表 → 返回 `None`
- 图片 + 工具同时存在 → 图片路径优先（图片模型通常也支持工具）
- `has_images` / `has_tools` 各 1 项边界（空消息、null tools）

### 3.2 修改 `src-tauri/core/src/server.rs`（~15 行）

**落点 A** — `/v1/models` 响应追加合成条目（约 line 795 之前）：

```rust
let mut data = (handlers.get_models)()
    .iter()
    .map(|model| { ... })
    .collect::<Vec<_>>();
data.push(json!({
    "id": "WB.auto",
    "object": "model",
    "owned_by": "wbBridge",
    "name": "WB.auto"
}));
```

**落点 B** — `chat()` 函数中转（约 line 913，`prepare()` 调用之前）：

当 `body.get("model") == Some("WB.auto")` 时：

1. 调用 `auto_select(&body, &(handlers.get_models)())`
2. 选中后改写 `body["model"]` 为实际模型 id
3. 后续 `prepare()` + `complete()` 链路**一字不动**

```rust
let mut body = body;
if body.get("model").and_then(Value::as_str) == Some("WB.auto") {
    let selected = auto_select(&body, &(handlers.get_models)());
    if let Some(id) = selected {
        body["model"] = json!(id);
    }
}
// 继续原有 prepare + complete 流程
```

### 3.3 修改 `src-tauri/core/src/lib.rs`

注册新模块（一行 `pub mod auto;`）。

---

## 4. 不改的东西

| 不改 | 原因 |
|---|---|
| `protocol.rs::prepare()` | WB.auto 在进入 `prepare()` 之前已被改写为实际模型名，匹配逻辑无缝 |
| `backend.rs::complete()` | 同上，下游只看到解析后的实际模型 |
| `orchestration.rs` 探测/发布链路 | 合成模型不需要探测——它只是路由别名 |
| `sync.rs` 写入 `models.json` | WB.auto **不写进** WorkBuddy/CodeBuddy 的 `models.json`（避免 agent 对合成模型做二次路由） |
| 面板 UI (`src/`) | 本轮纯后端，面板不加新视图 |
| `providers.rs` 注册表 | WB.auto 不是平台，不需要 Key |
| `status.json` / `STATUS_SCHEMA_VERSION` | 不加新字段，加法式变更不升 schema |

---

## 5. 验证协议

### 5.1 单元测试

- 核心 `cargo test` 至少新增 8 项（`auto.rs` 模块测试）
- 现有 278 项全部保持通过（`git diff --stat src-tauri/core/tests/fixtures` 为空）

### 5.2 端到端（需实机，本轮不阻塞）

1. 启动 WB Bridge → 面板模型列表正常
2. `curl GET /v1/models` → 列表末尾出现 `WB.auto`
3. `curl POST /v1/chat/completions {model:"WB.auto", messages:[{role:"user",content:"hello"}]}` → 返回正常对话，`model` 字段为实际模型名
4. 带图片的请求 → 选中的模型支持图片
5. 带 tools 的请求 → 选中的模型支持 toolcall
6. WorkBuddy/CodeBuddy 的 `models.json` 里**不出现** `WB.auto`

### 5.3 门禁

- 核心 `cargo clippy --all-targets` 0 warning
- 壳 `cargo clippy --no-deps --all-targets` 0 warning
- 壳 `cargo test --lib` 9 通过（无改动，不应漂移）
- `npm run version:check` N 处一致（**不推进版本号**：纯后端加法式变更无外部行为变化，属禁止推进情形）

---

## 6. 不做的（明确边界）

| 方向 | 理由 |
|---|---|
| 面板「自动路由」开关/配置视图 | 本轮纯后端，不叠 UI |
| 写入 WorkBuddy/CodeBuddy `models.json` | 合成模型不写进插件配置——避免 agent 把它当普通模型做二次路由 |
| 加权/优先级路由 | 免费模型都是免费的，均匀随机足够；加权调度是下一轮的事 |
| 把 WB.auto 作为「探测候选」 | 它没有真实后端，不需要探测 |
| 流式/非流式分支差异化处理 | `server.rs` 里在 `prepare()` 之前改写，流式与非流式无差别 |
| 负载均衡/最少使用优先 | 当前模型池规模（~20-30 个）不需要 |
| 把路由选择结果写进日志/status.json | 暂不——先跑通再决定要不要可观测性 |

---

## 7. 与后续迭代的衔接

- **「对外服务」面板视图**（上一轮讨论）：WB.auto 是其配套——agent 配置 `model: "WB.auto"` 后不需要关心底层有哪些模型
- **更多平台接入**（freemodel / qianfan / spark）：新平台的模型探测通过后自动进入 `usable_models()` → 自动进入 WB.auto 路由池，**零改动**
- **面板路由偏好**（未来的 UI 层）：可在面板加「优先/排除某模型」的勾选，`auto_select` 多加一个 `exclude: HashSet` 参数即可——函数签名预留扩展点
