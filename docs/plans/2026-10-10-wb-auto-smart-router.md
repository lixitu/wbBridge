# WB · auto — 智能路由模型设计计划（深化版）

> 日期：2026-10-10 ｜ 状态：**核心代码已落地（v1.1.14，见 §10 落地实录）**，端到端与 GUI 仍未实测｜ 依赖：v1.1.13 完整可用模型池（`orchestration.rs:606 usable_models()`）
>
> 本版对应 AGENTS.md 条目：「WB · auto 智能路由模型——设计定稿并已落地核心代码（`auto.rs` + `server.rs` 两处拦截），实机边界未验证」。初版曾误推进版本到 `1.2.0`，已由维护者裁定回退；本次深化属**纯文档轮**，按版本号规则**不递增**（落点维持并行轮已推进的 1.1.13，7 处一致）。
>
> **2026-10-10 追加要求（用户）**：「WB · auto 只使用可用的模型；使用过程中出现受限、超时、不可用时，同时从可用模型中移除，并更新模型列表中该模型的状态。」→ 新增 **§2.4**（逐行实读把这条拆成「既有 `record()` 已做到的」与「本功能必须自己保证的两条」），并据此把 **D2 由推荐项升为强制项**、新增 **D6（同请求内换模型重试）**与 **D7（对话请求整体超时预算）**、§7.2 契约测试由 3 项扩到 6+ 项、§7.3 补三条实机项。⚠ 本节引用的行号（`server.rs:914`/`:974-977`/`:1076-1078`、`orchestration.rs:65-70`/`:831-867`/`:606-634`、`App.vue:343`/`:354`）按**含并发 4→8 那轮的 HEAD** 逐条 grep 核对过，§0/§3 里引用的旧行号可能仍偏 1-3 行。

---

## 0. 对初版的实读更正（必须先知道的三个事实）

初版的 §3.2 落点 B「改写 `body["model"]`，后续链路一字不动」**不成立**。实读 `server.rs::chat()`（:913-958、:971-1022）后确认：

### 更正 1：响应 `model` 字段根本不会自动回填实际模型

`server.rs:929` 在改写点之后**重新读取** `let body_model = body.get("model")`，并在两条出口把它**强行塞回响应**：

- 非流式：`server.rs:984-986` — `target.insert("model", body_model)`
- 流式：`server.rs:1082-1084` — 同样插入后再 `send_sse`

也就是说：即便按初版在 `prepare()` 之前把 `body["model"]` 改成实际模型 id，只要 `body_model` 的取读位置不动，响应里仍是 `"WB · auto"`；而如果把 `body["model"]` 改成实际 id 让 `body_model` 自然取到，那 §4「protocol/backend 一字不动」仍然成立，但见更正 2 的副作用。**「响应 model 回填实际模型」不是零改动就能得到的，必须显式设计**（决策点 D2）。

### 更正 2：`body["model"]` 同时是用量与发布判决的记账键

同一行 :913 的 `model`（鉴权后首次取读）沿 `record()` → `on_result`（`server.rs:1139-1149` → `orchestration.rs:2378+` → `:784 record()`）进入：

- `status.json` 顶层 `usage.models["<clientModelId>"]`（用量视图的键）
- `modelResults.<id>`（逐模型状态）
- **`validated` 集合**：`orchestration.rs:843-850` — 非格式类失败会 `validated.remove(model)`

后果：若记账键停在 `"WB · auto"`，则①用量归到一个不存在的模型上；②一次格式类以外的失败（如限流、鉴权失败透传）会把 `"WB · auto"` 从 `validated` 里 remove——那是**全限定 id 命名空间下的假键**，看似无害，实则让这次失败**逃过了对真实被选模型的判决**（真实模型该摘除的没摘除）。反之，失败本该记在真实模型上（它确实失败了，摘除它正是既有语义）。所以**记账键必须是实际选中模型的 id**（决策点 D2 推荐形态即为此）。

### 更正 3：`fastrand` 不在依赖树，且新依赖需过供应链纪律

`src-tauri/core/Cargo.toml` 实读：无 `fastrand`、无 `rand`。随机设施现成可用的是 `uuid v4`（`protocol.rs:783 random_uuid()`，OS CSPRNG 驱动）。为 ~30 个候选的均匀选择引入一个新 crate 不值得（AGENTS.md：「能用标准库或既有依赖完成的绝不引包」）。方案见 §3.3。

### 附带更正（小）

- 初版 §5.1 写「现有 278 项」——HEAD 实测基线为 **283 通过 / 0 失败（lib 261 + js_parity 11 + red_lines 11）**。
- 初版 §5.3 写「不推进版本号（纯后端加法式变更无外部行为变化）」——两条都错：**有外部行为变化**（`/v1/models` 多一条目），按 2026-10-09 强制递增规则应属 **minor**；「不占 minor 位」的裁定针对的是**本轮设计文档本身**（纯文档不递增），不是未来的代码落地轮。
- `GET /v1/models` 现有条目形态（`server.rs:788-795`）是 `{ "id": <client id>, "object": "model", "owned_by": "opencode", "name": <同一个 client id> }`，没有 `created`。`WB · auto` 条目保持同形即可（§3.2）。

---

## 1. 动机

WB Bridge 已聚合多平台免费模型——OpenCode 自带 + ModelScope / SiliconFlow / 腾讯 TokenHub / 智谱。每个模型能力不同（图片输入 `images`、工具调用 `toolcall`、仅对话 `chatOnly`），agent 调用时得手动选模型、手动避开能力不匹配的模型。

**WB · auto**：对外暴露一个合成模型名 `"WB · auto"`。后端按请求内容自动选适配模型转发，调用方无需维护模型清单。

```
agent 配置 model = "WB · auto"
  → 发图片请求   → 选一个 images==true 的模型
  → 发工具调用   → 选一个 toolcall==true 且非 chatOnly 的模型
  → 纯文本对话   → 从全池均匀随机选（负载分散）
```

**定位边界（2026-10-10 用户扩展）**：WB · auto 是 `/v1/models` 上的**路由别名**，服务**一切 OpenAI 兼容客户端**——不再只面向 WorkBuddy/CodeBuddy。~~它**不写进** WorkBuddy/CodeBuddy 的 `models.json`（同步链零改动，见 §4）；插件用户看到的仍是逐模型列表，而其它 agent 经本地 HTTP 直连使用。~~
> 🔴 **该句已被同日 v1.1.15 撤销**（用户实测后裁定「肯定要把 WB · auto 写到插件配置，不然谁都不知道如何使用」）。现状：**发布集非空时，`WB · auto` 会作为一条独立条目写进两个插件的 `models.json`**（排在逐模型条目之后，`sync::auto_entry`），插件的模型选择器里因此选得到它；同步链**不再是零改动**，但 `SyncOptions.auto_route` 默认 `false`，对拍路径与逐模型条目的字节形态一字未变。§4 表内「不写进任何插件 `models.json`」一行、§6 验收第 7 条、§9 决策表同条同样作废，实读以 `docs/contract.md` 的「`WB · auto` 合成模型名」一节为准。本段以下正文保留原裁定文字作为决策沿革。

### 1.1 外部 agent 接入面与能力边界（实读确认，落到 `docs/contract.md` 时逐条复述）

| 维度 | 事实（出处） | 对接入方意味着 |
|---|---|---|
| 可用端点 | `GET /health`、`GET /v1/models`、`POST /v1/chat/completions`（`server.rs:4-6` 路由集合）；`/admin/*` 8 条是**壳专用** | agent 只需后两个；管理面不经 agent 暴露 |
| 鉴权 | `Authorization: Bearer <数据目录 api-key 文件内容>`（:743-747，无 key → 401） | 每个 agent 手工配一次本地 key；`/health` 同样要鉴权 |
| Origin | **任何非空 `Origin` 头一律 403**（`has_origin`，:749-754、:1195-1200） | CLI/桌面/后端运行时 agent（不带 Origin）畅通；**网页内嵌 fetch 直连不可行**（含浏览器插件页、web 版 agent），这是红线 2，不为 WB · auto 放宽 |
| 监听 | 仅 `127.0.0.1`（红线 3） | 只服务本机 agent；跨机/远程不是本工具的设计目标 |
| 并发 | ≤8，超出 429 `busy`（`MAX_CONCURRENT_REQUESTS`，2026-10-10 由用户裁定从 4 调到 8） | 多 agent 并行、或带并行子任务的 coding agent 仍可能触顶——WB · auto 的路由是负载**分散**不是负载**扩容**；429 与文案「At most eight requests may run at once」是接入方必须知道的既有行为 |
| 工具调用 | 请求带 `tools` → 路由到 `toolcall==true` 模型；原生工具审批一律 `ask`、模型侧绝不本地执行（红线 5），调用以 handoff/`tool_calls` 形态交回客户端 | agent 能执行工具 → 标准 function calling 闭环；不能执行 → 不带 tools 即可（纯文本池含 chatOnly） |
| 图片 | 仅接受 **base64 data URL**、mime 白名单 png/jpeg/webp/gif（`protocol.rs:318-331`），远程 http 图片 URL 直接 400 | 发远程链接的 agent 需自行下载内联，与 WB · auto 无关 |
| 参数子集 | `n=1` only；`reasoning_effort` 按模型 variants 校验（`prepare` :199-231） | 批量采样、任意 effort 的 agent 配置需预期 400 |
| 流式 | SSE + 10s 心跳注释帧（`send_sse`），结尾 `data: [DONE]` | 标准 OpenAI 流式客户端可直接消费 |

---

## 2. 核心设计

### 2.1 路由决策树（含与下游校验的衔接）

```
body.model == "WB · auto" ?
│
├─ has_images(messages) && has_tools(body)        ← 初版未定义的组合形态，见下
│  └─ 候选 = { m ∈ pool | m.images==true && m.toolcall==true && m.chatOnly!=true }
│
├─ has_images(messages)
│  └─ 候选 = { m ∈ pool | m.images==true && m.chatOnly!=true }
│
├─ has_tools(body)
│  └─ 候选 = { m ∈ pool | m.toolcall==true && m.chatOnly!=true }
│
└─ 纯文本
   └─ 候选 = pool（含 chatOnly）

候选为空 → 退回全池；全池也为空 → 不改写，落 prepare() 的既有 400 model_not_found
```

**为什么图片+工具要单列一档**：`prepare()` 的两个闸门决定了混合请求的失败形态——

- 图片闸门（`protocol.rs:312-316`）：所选模型 `images` 非真值 → **当场 400 `unsupported_content`**，请求根本不出网；
- chatOnly+工具闸门（`protocol.rs:365-370`）：`chatOnly` 模型带非空 tools → **400 `tools_not_supported`**。

初版「图片路径优先、忽略工具要求」会让混合请求（agent 带工具定义 + 截图，很常见）**注定 400**，且是服务端本地拒绝而非上游透传——违背「让上游自己的报错原样透传」的降级原则。单列一档后，只有「池里没有任何同时支持图片与工具的模型」时才退档，退档顺序见决策点 D3。

**能力字段来源**（全部实读确认存在）：`images`/`toolcall`/`reasoning` 由 `backend.rs::free_models_in`（:147-164）从 catalog 的 `capabilities` 归一成布尔；`chatOnly` 由 `usable_models()`（:621-632）注入，缺省 `false`。比较一律用 `== Some(&json!(true))`，缺失键按 false 处理（与既有代码风格一致，不引入 truthy 误判）。

**唯一选与随机**：候选集大小 1 直接选中；>1 均匀随机（§3.3）。

### 2.2 对外形态

`GET /v1/models` 在既有列表**末尾**追加（`server.rs:794` 之后，条件见 D1）：

```json
{ "id": "WB · auto", "object": "model", "owned_by": "wbBridge", "name": "WB · auto" }
```

请求侧与初版一致（OpenAI 兼容方式，`model: "WB · auto"`）。

响应侧的 `model` 字段与记账键语义见决策点 D2——**这是初版设计里唯一必须用户裁定的契约问题**。

### 2.3 关键设计取舍

| 决策 | 选项 | 理由 |
|---|---|---|
| 记账键（usage / modelResults / validated） | **实际选中模型的全限定 id** | 更正 2：记到 `"WB · auto"` 会让真实模型的失败逃过判决、用量归错账 |
| 响应 `model` 回填 | 推荐=实际选中模型的**全限定 id**（改写 body 后由既有插回逻辑自然带出，零特判） | 见 D2 的两个候选与代价 |
| chatOnly 进纯文本池 | 可以 | 纯文本无工具时 chatOnly 完全合法且通常更快/更轻 |
| 随机算法 | 既有 `random_uuid()` 取字节映射，不新增依赖 | 更正 3；均匀性对 ~30 池足够（偏差 <0.002%） |
| 可测性 | `auto_select` 收注入的 `usize→usize` 选择器；随机实现与纯逻辑分离 | 与项目既有注入风格一致（`FetchFn/LatestFn/ProbeFn/SyncIo`），单测可钉住「选中了谁」 |
| 快照一致性 | `get_models()` **只调一次**，同一 `&[Value]` 喂 `auto_select` 与 `prepare` | 见 §3.4：两次取读之间发生 refresh 会让刚选中的模型消失 → 400 |
| 图片+工具混合 | 单列一档（要求双能力） | §2.1：否则注定 400 |
| 候选为空 | 退回全池 → 全池空则不改写、报 400 | 与「手动选错模型」行为一致；不做静默兜底、不加新错误码 |
| 运行期失败即摘除 | **复用既有 `record()` 语义，不新写任何摘除与状态更新代码** | §2.4（2026-10-10 用户要求）：只要记账键是实际模型 id，摘除＋状态更新是既有链路的既有产物 |

### 2.4 运行中受限 / 超时 / 不可用即从可用池摘除（2026-10-10 用户要求）

> 用户口径原文：**「WB · auto 只使用可用的模型，如果使用过程中出现受限、超时、不可用时，同时从可用模型中移除，同时需要更新模型列表中模型的状态。」**
> 本节按 HEAD（v1.1.13，已含并发 4→8 那轮改动）逐行实读，把这条要求拆成「既有链路已经做到的」与「本功能必须自己保证的」两部分——**不得把前者说成新增能力，也不得把后者藏进 §4「不改的东西」**。

#### 2.4.1 要求的三条子句分别落在哪一段代码

| 子句 | 落点（实读出处） | 结论 |
|---|---|---|
| 只使用**可用**的模型 | `orchestration.rs:606-634 usable_models()` = 目录 ∩ `validated` ∩ `modelResults[id].ok===true`，经 `orchestration.rs:2361 .get_models(usable_models)` 喂给 `chat()` | **零新增**：`auto_select` 的池就是它，天然不含未通过/已被摘除的模型 |
| 受限 / 超时 / 不可用 → **从可用模型中移除** | `orchestration.rs:843-850`：`record()` 在非早退分支上 `ok==false` 即 `validated.remove(model)` | **零新增**，但**前提是记账键 `model` = 实际模型全限定 id**（= §3.2 不变量 1，见 2.4.3） |
| 同时**更新模型列表状态** | 同一次 `record()` 里：`:855-863` 重算 `availableModels`、`:866-867` 逐键写 `modelResults[model]`（`model_status.rs::model_result` 归类 category）→ `status.json` 原子写 → 壳 `watch_status` 推 `core-status` / `core-activity` → 面板 `App.vue:343 :results="state.modelResults"`、`:354 :result="state.modelResults[selected.id]"` | **零新增、零面板改动**：行内状态与「/v1/models 立刻少一条」由同一次写入同时得到。**准确形态**：列表行**不消失**（数据源是目录 `App.vue:342 :models="state.models"`，`available` 只参与排序与角标——`ModelList.vue:32`、`:70`），而是徽标转成「不可用」（`ModelRow.vue:30 unavailable`）并**长出「重新检测」入口**（`ModelRow.vue:35 canReprobe`）——这正是 2.4.5 的恢复路径 |

#### 2.4.2 用户列的三种形态，逐一对照现有错误码（关键：只有格式类四项不摘除）

`orchestration.rs:65-70 REQUEST_SHAPED_FAILURES = { invalid_model_output, invalid_tool_call, native_tool_activity, output_truncated }` —— 数据红线 4 规定这四项**不得撤销已发布模型**，`record()` 在 `:831-841` 早退（只记 `lastRequest` 与用量，不动 `validated`）。**用户要求的三类一项都不在这四项里**，因此都会走到摘除分支：

| 形态 | 实际到达 `record()` 的 `code` | 是否摘除 | 说明 |
|---|---|---|---|
| 受限（地区拒绝 / 撤架 / 未承接 / 服务 id 不在册 / Key 无访问权 / 401·403·404 透传） | `upstream_error`、`model_error`（`probe.rs::probe_failure` 只改中文文案，**`code`/`status` 逐字保留**） | ✅ 摘除 | 与手动选到同一模型失败时**完全同一条分支**，WB · auto 不产生额外语义 |
| 超时（上游给出 timeout / timed out） | 上游错误经 `backend.rs:1891-1897 to_bridge_error` 落到 `upstream_error`（缺 code 时的默认值），面板 category = `timeout`（`model_status.rs:18` 常量、`:38-43` 的 `timeout\|timed out` 正则、`:66` 赋值） | ✅ 摘除 | ⚠ **边界必须如实写**：本工具对**对话请求**没有整体超时预算（`REQUEST_BODY_TIMEOUT=20s` 只掐请求体读取，`PROBE_TIMEOUT_MS=60s` 只在探测路径）。因此「上游挂死且客户端一直不取消」今天**既不会失败也不会摘除**——该情形属决策点 **D7**，不在本节默认范围内 |
| 不可用（模型不再服务） | `upstream_error` / `model_error`；若模型已被摘出池，则下一次 `auto_select` 再也选不到它 | ✅ 摘除 | 与「候选集为空」互补：池随失败收缩，`/v1/models` 同步收缩 |

#### 2.4.3 本功能为了让这条要求成立**必须**遵守的两条（这才是新增内容）

1. **记账键只能是实际模型 id**（把 D2 从「可裁定」升为**强制**）。`server.rs:914` 的 `model` 取的是**客户端字面值**，`record()` 拿它当 `validated` / `modelResults` / `usage` 的键。若 WB · auto 不改写 `body["model"]` 而让 `"WB · auto"` 成为键，则用户要求的摘除会**打在一个不存在的键上**：真实模型留在 `validated` 里，下一次 `auto_select` 又选中它，同一个受限模型被反复命中、反复失败。故 §3.2 的「改写点位于首次取读之前」不是实现细节，而是这条要求的**唯一支点**。
2. **`WB · auto` 永不进入 `validated`**（否则 :845-846 的 `ok==true` 分支会把合成名插进通过集，污染 `usable_models()` 的域）。当前形态天然满足：改写后 `record()` 只会看到实际模型 id。落地轮必须用一条断言把「池里永远没有 `WB · auto`」钉死（§7.2）。

#### 2.4.4 不得被误当成「失败即摘除」的五条早退（逐条实读确认，摘除只能由真实模型失败触发）

| 情形 | 早退位置 | 结果 |
|---|---|---|
| 客户端断开 / 取消（含流式心跳发不出去） | `server.rs:974-977`、`:1076-1078`（`signal.is_aborted()` 直接返回，**不调 `on_result`**）；流式 `:1066-1070` 心跳失败即 `controller.abort()` 并返回 | **不记录、不摘除**——取消不是模型的判决（红线：客户端取消不得记为失败） |
| 并发触顶 429 `busy` | `server.rs:900-906`，位于读体与 `prepare` 之前 | 不记录、不摘除 |
| 请求体读取超时 408 `timeout` | `read_body_bounded`（`server.rs:1211`），同样早于 :914 | 不记录、不摘除；**不得**因为 code 叫 `timeout` 就以为它会摘除某个模型 |
| 本地 400（`model_not_found` / `unsupported_content` / `tools_not_supported` / 非法 role 等） | `server.rs:915-918`：`prepare` 报错即 `return`，**没有任何 `on_result`** | 不记录、不摘除（池里没有的模型不该被判决；这也正是 §3.2 不变量 3 选不出时不改写、交回 400 的理由） |
| 鉴权 401 / Origin 403 / 体积 413 / 非法 JSON | 全部在 :914 之前 | 不记录、不摘除 |

#### 2.4.5 摘除之后的两件事（如实边界，不得写成「已全链路撤下」）

- **恢复只能靠探测**：`validated.remove()` 是内存态，把模型放回去的只有 `/admin/probe`（单模型「重新检测」或定向 `probe_platforms`）与 refresh；应用重启则按 `restored_*`/`validated_from_results`（`orchestration.rs:522`、`:2104`）从上一份 `status.json` 恢复——**被摘除的模型其 `modelResults[id].ok` 已是 false，因此重启后仍保持摘除**，不会复活。反过来，探测通过后它重新进 `validated`，WB · auto 的下一次选择自然又能选到——恢复那一步同样零改动，但**它不是自动发生的**，必须有一轮探测成功。
- **插件 `models.json` 不会因一次请求失败而立即撤下该模型**：`record()` 不调 `sync_published`（其调用点全在探测 / refresh / 关停 / 启动沿用链路，`orchestration.rs:1246`、`:1285`、`:1324`、`:1345`、`:1693`、`:1849`、`:2204`、`:2317`、`:2517`、`:2540`）。所以「移除 + 更新状态」的准确范围是：`/v1/models`、`status.json` 的 `availableModels` 与 `modelResults`、面板列表状态**当次即生效**；WorkBuddy/CodeBuddy 名下的发布条目要等下一轮同步。这是既有行为，本功能不改动它，也不得在文档里声称已撤下。

---

## 3. 改动面（三个落点 + 一处清理）

### 3.1 新增 `src-tauri/core/src/auto.rs`（~120 行，纯函数、无 IO）

```rust
/// WB · auto 合成模型名（对外唯一字面量）。
pub const AUTO_MODEL_ID: &str = "WB · auto";

/// 请求是否携带图片（任一 message.content 为数组且含 type=="image_url" 的 part；
/// 字符串 content 恒为 false——与 prepare 的多模态解析同形态，protocol.rs:298-348）。
pub fn has_images(messages: &[Value]) -> bool;

/// 请求是否携带非空 tools 数组（body.tools 为非空 Value::Array 才为真；
/// null/缺失/空数组 = false，与 prepare:233-239 的归一一致）。
pub fn has_tools(body: &Value) -> bool;

/// 核心选择器：纯函数，pick 注入（pick(len) -> Option<usize>；None = 放弃选择）。
/// pool 为 usable_models() 的**同一份快照**。
/// 返回选中的模型全限定 id；候选与全池皆空时返回 None。
pub fn select_with(
    body: &Value,
    pool: &[Value],
    pick: &dyn Fn(usize) -> Option<usize>,
) -> Option<String>;

/// 生产入口：pick = random_uuid 前 4 个 hex 位取模（§3.3）。
pub fn auto_select(body: &Value, pool: &[Value]) -> Option<String>;
```

`select_with` 内部按 §2.1 决策树依次构造候选 filter，**第一个非空候选层**即定案（含全池兜底层）。不做多级退档（退档顺序属 D3，落地时若裁定多档再改）。

### 3.2 修改 `src-tauri/core/src/server.rs`（~25 行）

**落点 A — `/v1/models`**（`server.rs:788-795`）：

```rust
let mut data = (handlers.get_models)()
    .iter()
    .map(|model| { /* 既有映射，一字不动 */ })
    .collect::<Vec<_>>();
if !data.is_empty() {
    // 池为空时 WB · auto 无路可路由，与其返回一条必 400 的条目，不如不出现（D1）。
    data.push(json!({ "id": AUTO_MODEL_ID, "object": "model", "owned_by": "wbBridge", "name": AUTO_MODEL_ID }));
}
json_response(StatusCode::OK, json!({ "object": "list", "data": data }))
```

**落点 B — `chat()` 改写**（`server.rs:913-914` 之间，`prepare()` 之前）：

```rust
// WB · auto：先取一次池快照，选择与校验共用同一份（§3.4 快照一致性）。
let models = (handlers.get_models)();
let mut body = body;
let selected = body
    .get("model")
    .and_then(Value::as_str)
    .filter(|name| *name == AUTO_MODEL_ID)
    .and_then(|| auto_select(&body, &models));
if let Some(id) = &selected {
    body["model"] = json!(id);
}
let mut model = body.get("model").cloned().filter(|value| truthy(Some(value)));
let prepared = match prepare(&body, &models) { … };   // 原 :914，改喂同一份 models
```

关键不变量：

1. `body["model"]` 改写发生在 :913 首次取读**之前** → `model`（记账键）与 `body_model`（响应回显，:929）都自动取到实际 id，两处下游（`record` 链、两条出口插回）**零特判**；
2. `prepare(&body, &models)` 用同一份快照 → 选中的模型必然还在池里，且全限定 id 精确匹配（`strict_eq`，`protocol.rs:183`）不存在 client id 重名歧义；
3. 选不出（池空）→ 不改写，`body.model` 仍是 `"WB · auto"`，`prepare` 按既有逻辑报 400 `model_not_found`（文案「Select an available free model from /v1/models」原样，**不加新错误码、不改文案**）；
4. `auto_select` 绝不被探测/发布链路调用；`attach_translator` 等候选遍历本就以 `usable_models()` 全限定 id 为域，`WB · auto` 不在其中，无自路由风险。

**落点 C — 观测（一行）**：改写成功时 `log_line`（既有日志通道）记「WB · auto 路由至 <id>」。**不写 `status.json`、不加新字段、不动 `STATUS_SCHEMA_VERSION`**（维持初版 §6 边界；日志是数据目录内 `opencode.log` 同级的运行日志通道，无凭据风险）。

### 3.3 随机实现（不新增依赖）

`auto_select` 的 pick：`random_uuid()` 去掉连字符后取前 4 个 hex 字符（16 bit，0–65535）对候选长度取模。对 ≤65 个候选，模偏差 ≈ len/65536，池规模 ~20-30 时偏差 <0.05%，远低于本功能的均匀性要求；CSPRNG 性质顺带保证不可预测（防「同文案恒打同一模型」被外部探测利用）。测试经 `select_with` 注入固定选择器，不依赖随机。

### 3.4 修改 `src-tauri/core/src/lib.rs`（一行）

`pub mod auto;`。

### 3.5 快照一致性说明（为什么必须一次取读）

`get_models` 是 `Arc<dyn Fn() -> Vec<Value>>`（`server.rs:299`），背后是 `usable_models()`——读锁内 `validated` + 快照 `modelResults`。改写与 `prepare` 之间若发生探测完成/refresh，两次取读结果可能不同：选了 T1 里的模型、prepare 喂 T2 的池，模型已被摘除 → 用户看到与请求内容无关的 400。改动后 `chat()` 全程共用一份 `models`，既有路径（非 WB · auto）行为不变（本来也是同一行取读一次）。

---

## 4. 不改的东西

| 不改 | 原因 |
|---|---|
| `protocol.rs::prepare()` / `decode()` / `completion()` | WB · auto 在进入 `prepare()` 前已改写为实际全限定 id；`server.rs` 侧只动了取读位置 |
| `backend.rs::complete()` | 下游只看到解析后的实际模型 |
| `orchestration.rs` 探测/发布/转写链路 | 合成模型不需要探测；`TRANSLATOR_ORDER`/`attach_translator` 候选域是池内 id，天然不含 WB · auto。**`record()` 的摘除与状态写入（:831-867）一字不动**——§2.4 的用户要求完全靠它既有语义达成，本功能只负责把正确的记账键交进去 |
| `sync.rs` / `targets.rs` | ~~WB · auto **不写进**任何插件 `models.json`（避免 agent 对合成模型二次路由）~~ 🔴 **v1.1.15 撤销**：非空发布集时写入两个插件（`sync::auto_entry`，`auto_route` 默认 false 保住对拍）；请求失败仍**不触发同步**（§2.4.5 不变，`record()` 不调 `sync_published`） |
| 面板 `src/` | 本轮纯后端；用量视图键是实际模型 id，无需任何改动即可正确归账；模型列表的「不可用」渲染同样复用既有（`App.vue:343`/`:354` 读 `modelResults`，§2.4.1） |
| `providers.rs` | WB · auto 不是平台，不需要 Key |
| `status.json` / `STATUS_SCHEMA_VERSION` / 两侧壳 IPC | 不加字段、不改事件与命令 |
| 红线（鉴权/Origin/限流/体积/环境白名单） | 改写点位于鉴权之后、`prepare` 之前，红线链路一字不动 |

---

## 5. 契约影响清单（落地轮必须在回复中向用户复述）

1. `GET /v1/models` 列表**末尾多一条** `WB · auto`（加法式；池空时不出现 = D1 推荐形态）。直接轮询该端点做模型清单缓存的调用方会看到新条目——WorkBuddy/CodeBuddy 插件读的是 `models.json`（不受影响），受影响的是**手写 agent 配置**。
2. `POST /v1/chat/completions` 对 `model:"WB · auto"` 从「必 400」变为「正常响应」——外部行为新增，属**加法式**；对既有具体模型 id 的行为逐字不变。
3. `status.json`：`usage.models` 与 `modelResults` 会新出现按实际模型 id 的 WB · auto 流量条目（记账键设计使然，非形状变更）；schema 保持 1。
4. 运行日志新增「WB · auto 路由至 …」一行（落点 C）。
5. **一次 WB · auto 请求的失败会摘除被选中的真实模型**（§2.4）：`/v1/models` 少一条、`status.json` 的 `availableModels` 与 `modelResults.<id>` 同步更新、面板该行走既有「不可用」渲染。形状零变更，但**用户可见的模型数量会因流量而减少**——原先只有探测/刷新会这样，现在真实请求也会（这正是 2026-10-10 用户要求的语义）。
6. `docs/contract.md` 需同步补写 WB · auto 条目与 §1.1 的接入面口径——对外契约文档落后于实现属本仓库明令禁止的状态（「禁止编造已验证状态」同款纪律适用于文档准确性）。
7. 版本：**落地轮按强制递增规则推进 minor（→ 1.2.0 之外的合规档位需当时裁定）**；⚠ 注意 AGENTS.md 既有裁定「`1.2.x~1.3.x` 不得再占用」——落地轮的档位由用户在当时裁定，不得默认占用历史作废区间。

---

## 6. 待用户裁定的决策点（开工前需要答案）

| # | 问题 | 推荐 | 备选与代价 |
|---|---|---|---|
| D1 | 池空时 `/v1/models` 还列 `WB · auto` 吗 | **不列**（无路可路由的别名是假入口） | 恒列：实现更简，但用户配置了 WB · auto 却每请求 400，报错文案还指向一个"看得见"的模型名 |
| D2 | 响应 `model` 回显什么 | ~~推荐~~ → **已由 §2.4 定为强制项：实际选中模型的全限定 id**。用户「失败即摘除」的要求只有在记账键 = 实际模型 id 时才成立（§2.4.3 支点 1），回显 `"WB · auto"` 属**否决级错误** | ① 回显 client id（`ModelScope · X`）：面板口径统一，但需把 `body_model` 的取读与改写解耦、多两处插桩——**注意**：这只改回显，记账键仍必须是全限定 id，两处不得合并成一个值；② 回显 `"WB · auto"`：agent 出错无法归因，且摘除会打在假键上，双重否决；③ 同时回显两者：非标 OpenAI 形状，否决 |
| D3 | 图片路候选为空时退全池——落 `images==false` 模型会被 `prepare` **本地 400**，并非"上游透传" | 维持单层退全池 + 如实写明「候选空时该请求要么撞上本地 400、要么成功」，文案可诊断性交给 400 消息本身（`OpenCode does not declare image input for this model` 已足够可读） | 多级退档（双能力→图片→工具→全池）：更细腻，但 `select_with` 分支×3、测试×3，池小 今日收益低 |
| D4 | 全池空（服务还没探测完）时 WB · auto 报 400 的**文案**是否值得专门化 | **不专门化**，沿用既有 `model_not_found` 文案 | 专门化要新增分支或新码，触碰「禁止编造错误码」纪律，收益仅是措辞 |
| D5 | 路由选择日志（落点 C）是否默认开 | **默认开、一行、只进运行日志** | 关掉则排查「为什么这次答得差」无从对账 |
| D6 | 同一次请求内被选模型失败后，是否**换下一个模型重试** | **一期不做**。三条实读依据：① 流式分支在首帧之前失败虽然还能换模型，但 `VALIDATING_FRAME` 已入通道（`server.rs:944` 的 `try_send`）、`StreamJob.body_model` 在 :930 就已定值，换模型要重开一整套帧并携带新的回显值；② 摘除已按要求生效，**下一次请求自然选到别的模型**，池是自愈的；③ 重试会把一次用户请求变成两份上游消耗，与探测额度的既有节俭口径冲突 | 做：需在 `chat()` 外层加循环 + 失败码白名单（只能重试「摘除类」失败，格式类四项不得重试，否则违反数据红线 4）+ 每次重试重取池快照，改动面从 ~25 行涨到上百行、测试翻倍 |
| D7 | 用户列的「超时」里，**上游挂死且客户端不取消**这一格今天无人管（§2.4.2 的 ⚠）：是否为对话请求新增整体超时预算 | **本轮不加**。`REQUEST_BODY_TIMEOUT=20s` 只管读体、`PROBE_TIMEOUT_MS=60s` 只在探测路径，给生成期加总超时是**影响一切客户端的既有契约变更**（长回答、慢代理网络下会掐断合法请求），且属于「遇到不确定宁可询问」的安全边界 | 加：需定新常量 + 明确它归 `record()` 的哪一类（若落进 `REQUEST_SHAPED_FAILURES` 就不摘除、与用户要求相悖；若落不进去则慢请求会批量摘除健康模型），并同步 `AGENTS.md` 常量表与红线文档 |

---

## 7. 验证协议

### 7.1 单元测试（`auto.rs`，≥11 项）

| 用例（测试名保护的行为） | 钉住点 |
|---|---|
| 纯文本 → 全池且含 chatOnly | pick 注入固定索引，断言候选域 = 全池 |
| 图片 → 只命中 `images==true && chatOnly!=true` | filter 谓词 |
| 工具 → 只命中 `toolcall==true && chatOnly!=true` | filter 谓词 |
| 图片+工具 → 要求双能力 | §2.1 新增档 |
| 图片路候选空 → 退全池 | 兜底层 |
| 工具路候选空 → 退全池 | 兜底层 |
| 池空 → `select_with` 返回 None | 不改写、交给 prepare |
| 唯一候选 → pick 收到 len=1 | 无随机退化 |
| `has_images`：字符串 content / 空数组 / 非 image_url part | 边界 |
| `has_tools`：null / 缺失 / 空数组 / 非数组 | 边界 |
| 返回值是**全限定 id**（非 client id） | 记账键正确性（更正 2） |

### 7.2 `server.rs` 契约测试（沿既有 `recording_server` 风格，≥6 项）

- `/v1/models` 末尾出现 WB · auto 条目、池空时不出现（D1）；
- `model:"WB · auto"` 走通：断言 `record` 收到的记账键 = 实际模型全限定 id、响应 `model` 字段 = 同一 id（钉住更正 1/2 的实现）；
- 池空时 `model:"WB · auto"` → 400 `model_not_found` 且**不落任何 record**（prepare 早退）；
- **摘除链（§2.4 的三条必须各有归属）**：WB · auto 选中的模型返回 `upstream_error`（含地区拒绝、撤架、未承接、无访问权、上游 timeout 文案）→ 断言 `on_result` 收到的 `model` 是**实际 id**、`ok=false`，并让编排层的 `record` 走到 `validated.remove` + `availableModels` 重算 + `modelResults[id]` 写入（这三处已由 `orchestration.rs` 既有单测覆盖，本项只钉「WB · auto 把实际 id 交到这条链上」）；
- **不得摘除的早退（§2.4.4）**：① 客户端断开（`signal.is_aborted()`）→ 断言 `on_result` **一次都没被调用**，因而 `validated` 不变；② 429 `busy` 与 408 读体超时同样断言零 `on_result`；
- **格式类四项不摘除**（数据红线 4 不变）：`model:"WB · auto"` 且被选模型回 `output_truncated` / `invalid_tool_call` → 断言记账键仍是实际 id 但走 `record()` 的早退分支（保持发布）；
- 附带一条池纯净断言：任何 WB · auto 请求成功后 `usable_models()` 与 `validated` 里**都不得出现 `"WB · auto"`**（§2.4.3 支点 2）。

### 7.3 端到端（实机，本轮不阻塞、不得伪装已验证）

1. 启动 → 面板模型列表正常（回归）；
2. `GET /v1/models` 末条 = `WB · auto`；
3. 纯文本请求 → 200，`model` = 实际模型；连发 10 次观察分散；
4. 带 base64 图片请求 → 选中模型 `images==true`（对照 `status.json` 的 `modelResults` 归账）；
5. 带 tools 请求 → 选中模型 `toolcall==true`；
6. 探测全失败（池空）→ 列表无 WB · auto、直发请求 400；
7. ~~WorkBuddy/CodeBuddy `models.json` **不出现** WB · auto~~ 🔴 **v1.1.15 反转该验收项**：现为「发布集非空时两个插件的 `models.json` 都**出现**一条 `WB · auto`，且插件里选得到；发布集为空时不出现」。**两条都从未实机验证过**（沙箱单测只证明落盘形态，插件侧能否按它发请求未知）；
8. 用量视图按实际模型归账正确；
9. **摘除实测**：连发 WB · auto 请求直到命中一个受限/不可用模型 → 面板该模型行**当场变不可用并带中文说明**、`GET /v1/models` 少掉它、后续 WB · auto 请求不再选它（对照 `status.json` 的 `availableModels`）；
10. **取消实测**：流式请求生成中途断开 → 该模型**仍留在列表**、用量不计（这条是 §2.4.4 唯一的实机证据来源，单测只跑到注入的 `is_aborted`）；
11. **恢复实测**：对第 9 步被摘除的模型点「重新检测」→ 通过后回到池里、WB · auto 重新可能选中它。

### 7.4 门禁（落地轮逐项实跑并如实报数）

- 核心 `cargo test`：基线 283/0 之上 + 本轮新增，**全部实跑**；
- `git diff --stat src-tauri/core/tests/fixtures` 为空（对拍不允分叉）；
- `cargo clippy --all-targets`（核心）与 `cargo clippy --no-deps --all-targets`（壳）**0 warning**；
- 壳 `cargo test --lib` 9 通过（壳本轮零改动）；
- `tests/red_lines.rs`：评估是否补一条「WB · auto 改写点位于鉴权之后」的守卫断言——若路由测试已覆盖则不叠床架屋；
- `npm run version:check` 7 处一致 + `version:set`（**落地轮递增，档位按 §5.5 由用户裁定**）。

---

## 8. 不做的（明确边界）

| 方向 | 理由 |
|---|---|
| 面板「自动路由」开关/配置视图 | 本轮纯后端 |
| ~~写入插件 `models.json`~~ | ~~合成模型不写插件配置——避免二次路由~~ 🔴 **v1.1.15 撤销**：用户裁定「肯定要把 WB · auto 写到插件配置，不然谁都不知道如何使用」，非空发布集时写入两个插件（保守声明 + 只在池非空时写，见 §11）。「二次路由」的顾虑由「插件把 `WB · auto` 原样填进 `model`、服务端一次改写即落到实际模型」承担，不存在嵌套改写 |
| 加权/最少使用/优先级路由 | 免费池均匀随机足够；调度是下一轮 |
| WB · auto 作为探测候选 / 转写候选 | 无真实后端；候选域天然不含它（§3.2 不变量 4） |
| 流式/非流式差异化 | 改写点在两分支汇合之前，无差别 |
| 路由结果写 `status.json` | 只进运行日志一行；status 形状零变更 |
| 新错误码 / 改既有错误文案 | 池空路径完全复用 `model_not_found` |
| 把 WB · auto 塞进 `client_model_id`/命名空间体系 | 它没有命名空间、不是发布模型，混入会污染 `split/join` 域 |
| **自写一套「失败摘除 + 状态更新」** | §2.4：既有 `record()` 已经做完，另写一份会得到两套互相覆盖的 `validated` 判决，且必然与数据红线 4 的格式类豁免打架 |
| 请求失败后立刻撤下插件 `models.json` 里的该模型 | 既有语义是「下一轮同步才撤」（§2.4.5），`record()` 不调 `sync_published`。改它会牵动 WorkBuddy/CodeBuddy 的写盘链与 OWNER 增量合并，超出本功能范围 |

---

## 9. 与后续迭代的衔接

- **「对外服务」面板视图**：WB · auto 是其配套入口（任何 agent 配 `model:"WB · auto"` + `base_url=http://127.0.0.1:<port>/v1` 即可接入，不需要关心池构成）。该视图落地时的展示素材直接取 §1.1 表——鉴权方式、Origin/回环/并发三条边界、图片内联要求，都应在面板上写给用户，而不是藏在文档里。
- **更多平台接入**：新平台模型探测通过 → 自动进 `usable_models()` → 自动进路由池，**零改动**（前提：`images/toolcall` 归一仍走 `free_models_in`，已核对）。
- **面板路由偏好**：`select_with` 的选择器注入点即预留的扩展位；未来加 exclude 集合时只扩 filter 谓词，签名不动。
- **可观测性**（若 D5 落地后仍不够用）：把「本次路由」回写进 `lastRequest` 的 meta 是下一个合规落点（加法式、不动 schema）。

---

## 10. 落地实录（2026-10-10，v1.1.14）

> 状态口径：**核心代码已落地并通过全部本地门禁；§7.3 的 11 项端到端与 GUI 实机一项都没跑过**。下列行号以落地后的工作树为准。

### 10.1 三个落点的实际形态

| 落点 | 实际内容 | 与设计稿的差 |
|---|---|---|
| **A. 新增 `src-tauri/core/src/auto.rs`** | `AUTO_MODEL_ID` + `has_images` + `has_tools` + `fits`（档位谓词）+ `select_with` + `auto_select`，18 项 `#[cfg(test)]` | `select_with` 的选择器参数取 **`impl FnOnce(usize) -> Option<usize>`**，不是稿中的 `&dyn Fn`：候选集只调用一次选择器，而测试需要在闭包里记下候选域大小（`Fn` 不能改捕获，实测报 E0594/E0596）。`FnOnce` 同时让调用方免去装箱 |
| **B. `server.rs` 两处拦截** | `/v1/models`（:788-808）：池快照只取读一次，非空时末位追加 WB · auto 条目（D1）；`chat()`（:908-930）：`read_body` 之后**先**取池快照 → 命中合成名则改写 `body["model"]` → 再取记账键 → `prepare(&body, &models)` 复用同一份快照（§3.5 的一致性要求）；9 项契约测试 | 与稿一致；新增 `use crate::auto::…` 全限定调用，未在文件头加 import（只两处用到） |
| **C. 路由日志行** | **未做** | 🔴 如实记录：稿中「在编排层记一条路由活动文案」不可实施——`log_line`（`orchestration.rs:205`）是编排层私有函数，`server.rs::Handlers` **没有任何日志通道**，为它新开一条属跨层改动、超出「最小改动优先」。替代观测路径：记账键与 `lastRequest.meta.model` **都是实际模型 id**（本轮由 `wb_auto_records_and_echoes_the_real_model_id` 钉住），路由结果因此可从 `status.json` 反查，只是看不到「为什么选它」 |
| **D. `lib.rs`** | `pub mod auto;`（:45）+ 模块表新增一行（归入「迁移之后新增的模块没有 JS 前身」，与 `providers` 并列） | 与稿一致 |

### 10.2 决策点的实际取值

| 决策点 | 本轮采用 | 依据 |
|---|---|---|
| D1 池空是否列 WB · auto | **不列** | 用户批准「按推荐默认」；`wb_auto_is_listed_only_while_the_pool_has_models` |
| D2 记账键 = 实际模型全限定 id | **强制，已落地** | §2.4.3 支点 1；`wb_auto_records_and_echoes_the_real_model_id` + `a_failed_wb_auto_request_records_the_real_model_id` + `a_format_shaped_wb_auto_failure_still_keys_the_real_model` |
| D3 候选层为空 | **单层退全池**，不为「能力不匹配」造新错误码 | `empty_image_tier_falls_back_to_the_whole_pool` / `empty_tool_tier_falls_back_to_the_whole_pool` / `chat_only_models_stay_reachable_for_plain_text_but_never_for_tools` |
| D4 路由失败的专用文案 | **不做**，沿用 `probe.rs` 既有五个上游文案漏斗 | 用户批准默认；本轮 `server.rs` 无任何新文案 |
| D5 路由日志 | 见 10.1 落点 C（**未做，且有实施障碍**） | — |
| D6 同请求内换模型重试 | **不做**（推荐项） | 稿中推荐即默认 |
| D7 对话请求整体超时预算 | **不做**（推荐项）。🔴 边界如实保留：上游挂死且客户端不取消时，该请求**既不会失败也不会摘除**（`REQUEST_BODY_TIMEOUT=20s` 只掐请求体读取，`PROBE_TIMEOUT_MS=60s` 只在探测路径） | 稿中推荐即默认；用户未另加指令 |

### 10.3 §7.2 契约测试的落点与**唯一一条未做项**

已落地 9 项（`server.rs::tests`，注入 `auto_server(models, AutoOutcome)` 复用既有 `recording_server` 风格）：列表形态（D1 双向）、记账键与响应回显、失败仍键实际 id、跨 8 次请求池纯净（键 ∈ 池且 ≠ WB · auto）、池空 → 400 `model_not_found` 且零记录、取消 → 零记录、429 `busy` → 零记录、格式类 `output_truncated` 仍键实际 id、带 tools 请求避开 chatOnly。

🔴 **未做的一条**：稿中「408 读体超时 ⇒ 零 `on_result`」**没有**在 router 级补测——`REQUEST_BODY_TIMEOUT` 在 `chat()` 里是硬编码常量（20s），router 级实测要真等 20s；该早退由 `read_body` 返回错误即 `return error_response(&error)`（:909-912），位置**在记账键首次取读之前**，行为由既有的 `read_body_applies_the_timeout_to_a_body_that_never_finishes`（注入 50ms 真实预算）钉住。**不为它新增注入通道**（同 v1.1.12 轮那条 E0599 裁定：不为此引 `tokio/test-util`）。

`red_lines.rs` 按 §7.4 的评估**不补**「WB · auto 改写点位于鉴权之后」断言：改写发生在 `chat()` 内部，而 `chat()` 只在鉴权（`authorized`）、Origin、体积、并发四道闸门之后才可达，这三条已由既有 `health_requires_bearer_and_returns_status`、`browser_origin_is_forbidden` 与 `red_lines` 的鉴权/Origin 守卫钉死；再补一条是同义反复。

### 10.4 门禁实测（逐项实跑，不复用缓存）

- 核心 `cargo test`：**310 通过 / 0 失败** = lib **288** + `js_parity` **11** + `red_lines` **11**（lib 基线 261 → 288：`auto.rs` +18、`server.rs` +9）
- `git diff --stat src-tauri/core/tests/fixtures`：**为空**（对拍未分叉；`/v1/models` 的条目形态不在夹具里，夹具只在 `protocol.json` 出现过该路径字符串）
- `cargo clippy --all-targets`（核心，`touch` 三个改动文件后强制重检）：**0 warning**
- 壳与面板本轮**零改动**，故未跑 `cargo clippy --no-deps --all-targets` / `cargo test --lib` / 四组 JS 套件（跑壳会重编 ~3G debug 树，需用户点头）
- `npm run version:set -- 1.1.14` + `npm run version:check`：**全部 7 处版本号一致（1.1.14）**
- 🔴 **未做**：GUI 实机、§7.3 的 11 项端到端（含「摘除」「取消不摘除」「恢复」三条实机证据）、构建安装包、打标签、提交

---

## 11. v1.1.15 追加落地：名字统一为 `WB · auto` ＋ 写入插件配置（2026-10-10）

> 状态口径：**核心代码已落地并通过全部本地门禁；插件侧能否按这条合成路由发请求，一项都没实机验证过**。
> 本节是对 §1.1「不写进插件 `models.json`」、§4 表 `sync.rs` 行、§6 验收第 7 条、§9 决策表「写入插件 `models.json`」行的**撤销记录**，不是新增设计。前面各节保留原裁定文字作为沿革，不再逐字追平。

### 11.1 触发与两条裁定

用户实测后的两句追问构成整轮依据：「workbuddy 和 codebuddy 没有显示 `WB.auto` 的模型接口」「那怎么使用？」，随后裁定「**肯定要把 WB.auto 写到插件配置，不然谁都不知道如何使用**」。落地前用 AskUserQuestion 钉了两个默认值，用户选择：**保守声明**（能力字段宁少勿多）与**只在池非空时写**。

名称上用户还否掉了我提出的 `OC · WB.auto`（前缀套前缀），裁定「**只有 `WB · auto`**」⇒ 全局改名，`WB.auto` 不再被任何入口接受。前提如实记下：`1.1.11~1.1.15` 从未构建、从未打标签，所以没有任何已交付二进制带过旧名字；日后若要读回旧名，那是新增兼容分支，不是改常量。

### 11.2 落地形态

| 落点 | 实际内容 |
|---|---|
| `auto.rs` | `AUTO_MODEL_ID` 改成 `"WB · auto"`（分隔符逐字节 `20 c2 b7 20`，与 `client_model_id` 的 `前缀 · 名称` 同形）；新增 `any_tool_capable(models)` = `models.iter().any(|m| fits(m, false, true))` |
| `sync.rs` | `SyncOptions.auto_route`（**默认 false**）；`merge_models_auto` 与私有的 `merge_with_auto`（原 `merge_models` 主体，`auto_route` 只是其中一个开关，因此逐模型条目的字节形态一字未变）；`auto_entry`、`smallest_input`；`owned_count` **排除别名** |
| `orchestration.rs` | `sync_published` 的**唯一生产调用点**传 `auto_route: true`；WorkBuddy 与 CodeBuddy 共用这条写路径，两边一起生效 |
| `server.rs` / `lib.rs` | 只有注释与测试字面量随改名更新；**两处拦截的逻辑一字未动**（判据本来就是读 `AUTO_MODEL_ID`） |
| `tests/js_parity.rs` | `sync_options` 显式钉 `auto_route: false`，并注释说明夹具 `expected` 只有逐模型条目 |

条目形态：`id`/`name` = `WB · auto`（**刻意绕过** `client_model_id`，走进去会变成 `OC · WB · auto`、与 `chat()` 的字面比较永不匹配）；`vendor` = `Custom`；`url`/`apiKey`/`buddyBridgeOwner` 与逐模型条目同源；`supportsToolCall` = `any_tool_capable(models)`；`supportsImages` = `false`；`maxInputTokens` = 池内最小 `input`（缺失回落 `context`，非数值/0/负数跳过，一个都没有就**不写该键**）；**不写** `reasoning`/`maxOutputTokens`；**排在逐模型条目之后**（与 `/v1/models` 的末位追加同口径）。

### 11.3 与原设计不同的两处，如实标注

1. ⚠ **`supportsToolCall` 比所选的「保守声明」更收紧一步**：没做成固定 `true`，而是随池能力变化。理由是发布集里只有 `chatOnly` 模型时，声明 `true` 会让插件侧的工具请求恒定落到 D3 兜底、再被 `prepare` 拒成 400；共用 auto.rs 的同一个档位谓词后，**插件侧声明与核心选档不可能分叉**。
2. ⚠ **别名写进插件、但不写进 `status.json` 的模型列表**：所以 `sync.count` / `sync.targets.<t>.count` 仍只数真实模型（`owned_count` 显式排除），面板也不会多出一行「WB · auto」。这两处口径不一致是**有意的**，否则「同步数量」与用户能理解的「发布了几个模型」会对不上。

### 11.4 门禁实测（逐项实跑）

- 核心 `cargo test`：**316 通过 / 0 失败** = lib **294** + `js_parity` **11** + `red_lines` **11**（lib 288 → 294：`sync.rs` 新增 6 项——末位追加与逐字段保守声明、空池不写、`supportsToolCall` 随池变化、真实临时文件里「写进去 → 被该文档可用列表列出 → 下一轮空同步删干净」、手动同名条目不被覆盖、非 `auto_route` 路径不写别名）
- `git diff --stat src-tauri/core/tests/fixtures`：**为空**（`auto_route` 默认 false，因此**不需要**像 v1.1.12 那轮那样如实分叉夹具）
- `cargo clippy --all-targets`（核心，`touch src/sync.rs src/auto.rs src/server.rs` 后强制重检）：**0 warning**
- `npm run version:set -- 1.1.15` + `npm run version:check`：**全部 7 处版本号一致（1.1.15）**
- 🔴 **未做**：GUI 实机、真实模型端到端、真实插件目录写入、构建安装包、打标签、提交；壳与面板**零改动**（IPC 命令/事件/`ACTION_ROUTES` 一字未动），故未跑壳侧 clippy 与 `cargo test --lib`、也未跑四组 JS 套件
- ⚠ **档位裁定**：按「新功能 = minor」本轮应占 `1.2.0`，但 `1.2.x~1.3.x` 已被维护者裁定作废且不得再占用，minor 位**无处可进**，故按「同一未发布能力的追加」落 patch（`1.1.14 → 1.1.15`）
