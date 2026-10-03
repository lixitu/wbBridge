# WB Bridge 多上游 AI 网关 —— 首个切片设计（本地 OpenAI 兼容运行时）

> 日期：2026-10-03 ｜ 状态：**待评审（尚未开工）** ｜ 分支 `dev`
>
> 本文是 Brainstorming 收敛后的**设计稿**，不是实现记录。
>
> **符号核对状态（2026-10-03 已做）**：本文引用的主要符号已逐项回源文件核对，更正了初稿的三处错误，结果记在 §11「评审记录」。仍**未实读**的只有 §4.3 的候选端口表（外部事实）与面板侧 `src/**` 的具体组件结构。

---

## 1. 定位变化（一句话）

WB Bridge 从「OpenCode 免费模型搬运工」升级为「**本机 AI 网关**」：对外仍是单一 OpenAI 兼容端点 + 单一 models.json 合并写入，对内从「只有一个推理后端」变成「多个来源并行聚合」。

```
                         WorkBuddy（视野不变）
                               │
                127.0.0.1:<port>/v1/*   +   models.json 合并写入
                               │
┌──────────────────────────────▼──────────────────────────────┐
│                     WB Bridge 核心（网关）                    │
│   鉴权 / Origin 拒绝 / 并发 ≤4 / 8MB 体量限制  ← 红线，不变   │
│   protocol::prepare  →  Backend 分发  →  protocol::decode    │
└───────┬──────────────────────────────┬───────────────────────┘
        │                              │
   ┌────▼──────────┐          ┌────────▼───────────────┐
   │ OpenCode      │          │ Local OpenAI 兼容端点   │ ← 本次新增
   │ （现有后端）  │          │ Ollama / LM Studio / … │
   │ OC · <name>   │          │ Local · <name>          │
   └───────────────┘          └─────────────────────────┘
```

**第一约束：对外契约不变。** WorkBuddy 不需要知道有几个来源，所有新增复杂度都必须封在本机内。

---

## 2. 假设与成功指标（动手前写死）

| 假设 | 内容 | 证伪即收手 |
|---|---|---|
| **H1 机制可行** | 多来源聚合 → 独立命名 → 统一探测 → 合并发布 → 按来源路由请求，能在一个 Backend 抽象下跑通，且不破坏现有红线 | 有一条红线守不住，整条线下架 |
| **H2 有价值** | 用户愿意为多一组模型跑本地运行时，且它在 WorkBuddy 里**真的能用**（不只是列表里多个名字） | 上线 30 天 `Local ·` 调用量趋近 0 → 转向云 BYOK |
| **H3 无回归** | 加入第二来源后，OpenCode 那套行为零变化 | 老链路受影响立即暂停 |

**成功指标（四条，可客观判定）**

1. **冷路径**：本机没装本地运行时 → 来源视图显示「0 个本地来源」，不报错、不影响 `OC ·` 模型发布。
2. **热路径**：装了 Ollama 且已 pull 过模型 → 点「刷新」后 `Local · <name>` 与 `OC · <name>` **并列**出现在模型列表，且能在 WorkBuddy 里跑完一次对话。
3. **归因**：`status.json` 的 `usage.models` 按 clientModelId 分别累计 → 前缀天然就是来源分组，**零额外埋点**即可看出本地模型被调了多少次、平均耗时多少。
4. **不退化**：核心 `cargo test` **207** 通过、`tests/red_lines.rs` **9** 通过；新增判定一律配对应单测。

---

## 3. 范围边界

### In（本切片做）

- 本机回环 OpenAI 兼容运行时的**自动发现**
- 第二条推理后端（纯对话：非流式 JSON + SSE）
- 来源列表 UI（启用 / 禁用 / 刷新）
- 模型并列发布 + **来源下线的模型回收**
- usage 按来源可读（依赖前缀，几乎零成本）
- 相应单测 + `docs/contract.md` 三处同步

### Out（本切片明确不做）

- 云端 BYOK 与任何**用户凭据存储**（理由见 §7）
- 模型下载与管理（不替用户去 Ollama 拉模型）
- 自定义系统提示、参数面板、`STATUS_SCHEMA_VERSION` 升级

### Grey（本稿不决，列入后续）

| # | 待定项 | 为什么不现在定 |
|---|---|---|
| G1 | **本地模型工具调用透传** | 反向决定 Backend 抽象形态。本切片先按 chat-only 实现，遇工具调用返回与 `buddy-chat` 同类的明确错误，留作子需求 |
| G2 | 允许手填 localhost 端口 | 取决于自动发现的覆盖率，用数据决定 |
| G3 | 模型列表收纳（分组 / 按来源筛选） | 本地可能十几个模型，先看真实规模 |
| G4 | per-source 并发上限 | 现状是全局 ≤4；本地模型可能只能串行，待实测 |
| G5 | 长上下文与参数传递 | 各运行时参数差异大，先按上游默认 |

---

## 4. 架构设计

### 4.1 Source（来源描述符）

新增概念（示意结构，非最终代码）：

```
Source {
  id:            "opencode" | "local:<endpoint-hash>"   // 稳定主键
  kind:          ManagedUpstream | LocalEndpoint        // 后续可扩展 RemoteByok
  display_name:  "Ollama (127.0.0.1:11434)"
  base_url:      "http://127.0.0.1:11434/v1"            // 仅 LocalEndpoint 有
  enabled:       bool
}
```

**落入点**：`settings.json`（现持久化 `useSystemProxy` / `workBuddyModelsFile`，由核心写）。

> 区分两类数据：**用户意图**（启用哪个来源）进 `settings.json`；**运行时状态**（发现结果、健康度）进 `status.json`。两者不要混在一起，否则一次扫描失败会污染用户配置。

### 4.2 Backend 抽象

现有 `backend::complete(...)` 是唯一实现（OpenCode 会话 + 事件流 + `native_permissions`）。多来源后在这里分叉：

```
protocol::prepare(body, models)
      │
      ├── 模型 → 来源路由（clientModelId 前缀即可解析：'OC · ' / 'Local · '）
      │
      ├── Backend::OpenCode      （现状，零改动）
      └── Backend::OpenAICompat  （新增：直发 /v1/chat/completions，SSE 透传）
      │
protocol::decode(text, prepared)
```

> ⚠ **关键设计决定**：现有 `protocol::decode` 校验的是 OpenCode 的 `{content, calls}` 信封，并有「格式修正一次 → 仍失败则 translator 辅助模型重转」的补救链。而 OpenAI 兼容端点返回的是**标准 OpenAI 格式**，不是那套信封。
>
> 因此本地走**独立的响应解析路径**，不复用 `decode` 信封校验、也不触发 translator 重转——否则会把本地的正常回答误判为格式错误，再白烧一次无效重试。这条必须有单测钉住。

### 4.3 发现（Discovery）

```
候选端口（默认表，仅回环）：
  11434 Ollama · 1234 LM Studio · 8080 llama.cpp / LocalAI · 8000 vLLM · 1337 Jan

判定：GET <base>/v1/models，超时 1s，返回 2xx 且 data 数组合法 → 可用
     失败一律静默：不重试、不弹错误、不阻断启动
```

- **触发时机**：应用启动、`/admin/refresh`、来源视图手动刷新。
- **必须可关闭**：提供「启用本地运行时发现」开关——不用本地模型的人不该被多发请求。

### 4.4 探测（Probe）—— 本切片最大的隐性风险

| 维度 | OpenCode（现状） | 本地运行时（新增） |
|---|---|---|
| 单模型预算 | `PROBE_TIMEOUT_MS = 60000`，重试共用同一 deadline | **需独立且更长** |
| 典型失败 | 网络 / 鉴权 / 信封不符 | **模型冷启动**（Ollama 首次要把模型加载进内存，可能几十秒，大模型更久） |
| 重试语义 | 失败可重试 1 次 | 同理，但冷启动超时 ≠ 模型不可用 |

🔴 **已识别的冲突**：壳调用 `/admin/*` 的 `admin_call` 默认超时 **60s**（只有 `/admin/shutdown` 收紧到 5s）。本地首个模型冷启动**可能超过 60s**，届时 IPC 先超时，用户看到一次莫名其妙的失败，而实际上再等一分钟模型就好了。

三种处理，**建议选 b**：

- a) 把本地探测预算压进 60s → 大模型必然被误判为不可用（✗）
- **b) `/admin/probe` 改为「发起即返回」，结果经 `core-activity` / `status.json` 异步回传**（现有已有 500ms 轮询与活动事件，复用成本可控）（✓）
- c) 给这条路由单独调大超时 → **技术上可行且不影响其他动作**（壳已有 `admin_call_with(port, key, route, body, timeout)`，`admin_call` 只是它的 60s 默认值包装）；但 IPC 会阻塞一到两分钟，期间面板无反馈（✗ 仅因体验不佳，非因技术不可行）

> **初稿更正（2026-10-03 评审）**：初稿称方案 c「会拖累其他管理动作」是错的——`admin_call_with` 已是逐路由可指定超时，`/admin/shutdown` 就是用它以 5s 覆盖默认值的现成例子。方案 c 的真实代价只有「IPC 长时间阻塞、面板无反馈」这一条。建议仍取 **b**，但理由应为异步可作进度反馈，而非「改不了超时」。

### 4.5 命名与发布 ID

- 现状 `OC · <name>`，规范为 **`<前缀> · <name>`**，本地取 **`Local · <name>`**
- `models.json` 的 `OWNER = buddy-bridge-v1` **不变**——所有来源仍归本工具名下，合并逻辑（只增删自己条目、保留他人条目）**因此无需改动**

### 4.6 ⚠ 来源下线后的模型回收 —— 初稿判断有误，已更正

初稿称「这条逻辑现在完全不存在，必须新增」。**核对源码后这是错的**：`merge_models`（`src-tauri/core/src/sync.rs:295`）本身就是全量替换语义——

```rust
let kept: Vec<&Value> = list.iter().filter(|model| !is_owned(model)).collect();
```

即**所有属主为 `buddy-bridge-v1` 的旧条目一律丢弃**、只保留非本服务条目；`availableModels` 也同步按 `dropped` 清理（同文件 344–374 行）。**只要某来源的模型不出现在新的发布集里，它就会被自动剔除，无需新增回收逻辑。**

真正的风险在别处，而且比初稿描述的更危险：

| 风险 | 依据 |
|---|---|
| **清空保护在生产路径上是关闭的** | 唯一的线上调用点 `orchestration.rs:609` 用的是 `SyncOptions { allow_empty: true, require_existing: true }`。`allow_empty: false` 那道「拒绝空发布集、保留旧配置」的防线，在生产路径上从未生效 |
| **多来源会让「部分为空」成为常态** | 单来源时代，发布集为空 = 灾难性异常；多来源时代，某一来源返回 0 个模型是日常。一旦某条路径把整体发布集算成空，就会**顺手抹掉 WorkBuddy 里全部 WB Bridge 条目** |

**因此本切片真正要新增的不是「回收」，而是一条分工明确的闸门**：发布集可以因**用户显式禁用某来源**而变小甚至变空（这是意图），但绝不能因**某来源发现或探测失败**而变小——后者必须保留上次已知的模型并给出可见告警。这条建议进单测，且要同时钉住 `models` 与 `availableModels` 两处。

### 4.7 请求链路差异

```
本地请求路径（G1 未做之前）：
  WorkBuddy → 鉴权 / Origin 拒绝 / 并发 → 模型到来源路由
     → OpenAICompat::complete（非流式 JSON 或 SSE 透传）
     → 上游连接失败 / 超时 → 映射为明确错误（不得套用 native_tool_activity 语义）
     → on_result：计入 usage.models['Local · xxx']
```

OpenCode 那套 `native_permissions`（`'*': 'ask'`，工具调用阻塞并交回客户端 handoff）**不适用于本地运行时**——本地模型要么不支持工具调用，要么直接返回 `tool_calls`。方案见 G1。

### 4.8 状态与计数

`status.json` 已有 `usage: { since, total, models: { <clientModelId>: { requests, ok, failed, lastMs, avgMs } } }`，按 **clientModelId** 聚合 → `Local ·` 与 `OC ·` 天然分开，**不需要改 Schema**。

按仓库纪律：`STATUS_SCHEMA_VERSION = 1`，向后兼容的**加法式顶层字段不递增**。新增 `sources`（运行时来源快照）属加法，不升到 2；只有动了既有字段语义才需同步 +1（本切片不动）。

---

## 5. 受影响位置（2026-10-03 已核到文件:行）

| 位置 | 现有内容 | 本切片改动 |
|---|---|---|
| `src-tauri/core/src/server.rs:64` | `pub const ACTION_ROUTES: [(&str, &str, &str); 5]` | **数组长度写死 5**，新增动作须同步改常量、`server.rs:1300` 的 `assert_eq!(ACTION_ROUTES.len(), 5)` 也要改 |
| `src-tauri/src/lib.rs:470` | `const ADMIN_ROUTES: [(&str, &str); 5]` | 同步（有 `shell_action_routes_match_the_core_contract` 自动断言，`lib.rs:787`） |
| `src-tauri/src/lib.rs:94` | `admin_call` → `admin_call_with(…, Duration::from_secs(60))` | 见 §4.4；`admin_call_with` 已存在，改超时成本很低 |
| `src-tauri/core/src/model_status.rs:120` | `format!("OC · {}", display(model.get("name")))` | **命名前缀的唯一出处**，改这里；同文件 248–249 行两处单测断言了 `"OC · GPT-5"` / `"OC · undefined"` |
| `src-tauri/core/src/sync.rs:295` | `merge_models`（全量替换 + `availableModels` 清理） | **本身不用改**；新增的是 §4.6 那条「意图 vs 故障」闸门 |
| `src-tauri/core/src/sync.rs:41` | `pub const OWNER: &str = "buddy-bridge-v1"` | 不变 |
| `src-tauri/core/src/probe.rs:17` | `PROBE_TIMEOUT_MS: u64 = 60_000` | 本地来源需独立预算（现状是全局常量，`orchestration.rs:951` 直接用它做 sleep） |
| `src-tauri/core/src/orchestration.rs:609` | 唯一线上同步点，`SyncOptions { allow_empty: true, require_existing: true }` | §4.6 闸门的落点 |
| `src-tauri/core/src/protocol.rs` | `prepare` / `decode` / `repair`（translator） | 本地走独立解析路径，不得进 translator |
| `src-tauri/core/src/backend.rs` | `complete`（OpenCode 会话 + 事件流） | 新增 OpenAICompat 实现 |
| `src-tauri/core/tests/red_lines.rs` | 9 项红线，已逐个核过 | 保持通过；建议新增第 10 项（见 §6） |
| `docs/contract.md` | 三处一致性（唯一无自动校验的一份） | 人工同步 |

> ✅ 一处意外便利：`red_lines.rs:32` 的 `routes()` 是**从 `server::ACTION_ROUTES` 派生**的，不是手写清单。因此新增的 admin 路由会**自动**被「每条路由都必须过 Bearer 鉴权」这条红线覆盖，不需要额外登记。
| `src/App.vue` / `src/views/SideBar.vue` 等 | 视图路由（**未实读**） | 新增来源视图或并入现有视图 |

---

## 6. 红线与安全边界（本切片建议新增一条）

**保持不动**：`Bearer` 定时安全比较、非空 `Origin` → 403、并发 >4 → 429、体量 >8MB → 413、`api-key` 文件权限 `0600`、数据目录 `0700`。

🔴 **新增红线：本地来源仅限回环。**

来源的 base URL 必须落在 `127.0.0.1` / `::1` / 解析后为回环地址的 `localhost`；**拒绝**局域网段（`192.168.*`、`10.*` 等）与任何公网地址。

> 理由：一旦允许任意 URL，WB Bridge 就从「本地工具」变成可被配置成 SSRF 跳板的东西——面板里填一个内网地址，应用就会带着本机身份去访问它。建议直接进 `tests/red_lines.rs`，成为第 10 项红线。
>
> **这个写法有现成先例**：`red_lines.rs:8-9` 已经有一条「监听地址只能是回环」的红线，但它**注明「无法由测试覆盖，靠代码审查守住」**；而「来源 URL 仅限回环」是可以写成断言的。同一主题从人工审查升级为自动守卫，属于净收益。另一处可对照的先例是现有第 9 项 `runtime_downloads_never_leave_the_registry_allow_list`（下载源白名单）——本建议是它在**出站方向**的对偶条款。

**凭据边界不动**：本切片不引入用户密钥，`src/core/prefs.js` 的「存储里绝不写凭据」白名单保持原样。这正是选「本地优先」作为首个切片的主要收益。

---

## 7. 为什么 BYOK 不在这个切片里

云端 BYOK 必须同时解决三件事：凭据怎么存（`prefs.js` 明令凭据零落盘，需要新建一条核心侧、`0600` 权限的凭据读写路径）、怎么保证不被回显或不经 WebView 读到、以及随之而来的第二处持久化。这三件里任意一件做错都是安全事故。

先把**不需要凭据**的那段链路跑通，届时 BYOK 就只剩「补一段存储」这一件事。

---

## 8. 验证方案（怎么算做完）

| 层 | 判据 |
|---|---|
| 单元 | 回环强制（含 `192.168.*` / 公网 / `localhost` 解析三类录入用例）、来源禁用后模型回收、本地探测预算与超时、**本地响应不走 `decode` 信封校验** |
| 回归 | 核心 `cargo test` **207** 通过、`red_lines` **9**（+ 建议新增第 10 项）通过、壳 `cargo test --lib` 保持 **9** |
| 静态 | 核心 `cargo clippy --all-targets` 与壳 `cargo clippy --no-deps` 均 **0 warning**；`npx eslint .` **0 problem**；`npm run vite:build` 成功 |
| 契约 | `docs/contract.md` 与两张动作表人工核对一致 |
| 行为 | §2 的四条成功指标全部可客观判定 |

> ⚠ **口径不可越**：本项目 GUI 从未实机启动过（既有遗留）。本切片若新增 UI，**同样只能拿到「编译 + 单测 + 构建」证据**，不得据此宣称面板已验证。这条要原样追加到 `docs/wiki/已知限制与未验证项.md`。

---

## 9. 风险清单

| # | 风险 | 应对 |
|---|---|---|
| 1 | 本地模型冷启动 > IPC 60s 超时 | §4.4 方案 b（异步回传） |
| 2 | 设备门槛（内存 / 显存），不是每台机器跑得动 | 来源视图区分「已发现但未启用」并给出原因；H2 指标就是用来证伪这条的 |
| 3 | 模型列表爆炸（本地十几个） | G3 先观察真实规模；至少先支持按来源筛选 |
| 4 | **发布集因某来源归零而被整体算空 → 抹掉 WorkBuddy 全部 WB Bridge 条目**（N1） | §4.6 闸门：区分「用户显式禁用」与「发现/探测失败」，后者保上次已知模型并告警；单测同时钉 `models` 与 `availableModels` |
| 5 | 面板 GUI 未实机验证的遗留风险叠加到新 UI | 明确标注，不伪装 |
| 6 | 命名前缀迁移影响老用户 | `buddy-bridge-v1` 旧条目由合并逻辑清理；跨版本幂等性需单测覆盖 |

---

## 10. 下一步

1. 评审本稿 → 冻结 §3 的 In / Grey 边界
2. 若同意，再产出把「发现 → 第二后端 → 发布聚合」切成**可单独验证**阶段的实施计划
3. **评审通过前不要开始写代码**

---

## 11. 评审记录（2026-10-03）

初稿写完后回源码核对了一遍（`src-tauri/core/src/{server,sync,probe,model_status,orchestration}.rs`、`src-tauri/src/lib.rs`、`tests/red_lines.rs`）。核对前后对比如下：

| # | 初稿论断 | 核对结果 | 处置 |
|---|---|---|---|
| 1 | §4.6「来源回收逻辑现在完全不存在，必须新增」 | ❌ **错**。`merge_models` 本就是全量替换语义（`sync.rs:316`），旧的本服务条目一律丢弃，`availableModels` 也同步清理 | 已改写成「真正的闸门是意图 vs 故障的区分」，见 §4.6 |
| 2 | §4.4 方案 c「调大 admin_call 超时会拖累其他动作」 | ❌ **错**。`admin_call_with` 已支持逐路由超时 | 已更正；结论仍取 b，但理由改为「IPC 长阻塞 + 无反馈」 |
| 3 | §5 位置清单只到模块级 | ✅ 已补到文件:行 | 已更新，并标出 `ACTION_ROUTES` 数组长度写死 5、`server.rs:1300` 有配套断言 |
| 4 | 命名前缀改动范围不明 | ✅ 唯一出处是 `model_status.rs:120` | 连带 6 处断言（`model_status.rs` 2 / `sync.rs` 6 / `server.rs` 1 / `protocol.rs` 1 / `orchestration.rs` 6+） |
| 5 | 「`red_lines` 共 9 项」沿用文档说法 | ✅ 实测数过 `#[]` 属性：9 个用例，与 AGENTS.md 一致 | 无需改；新增第 10 项后基线应为 **10** |

### 核对过程中发现的**新问题**（初稿未覆盖）

**N1 · 清空保护在生产路径上是关闭的。** `orchestration.rs:609` 是唯一线上同步点，用的是 `allow_empty: true`。`merge_models` 里那句 `Empty model discovery; existing configuration preserved` 的防线从未在运行时生效。单来源时代这是「极端异常」，多来源时代某一来源归零是日常——**必须有 §4.6 那条闸门，否则一次发现失败就会被记账成一次全量清空。**

**N2 · `PROBE_TIMEOUT_MS` 是全局常量。** 本地来源需要更长的独立预算，而当前 `orchestration.rs:951` 直接拿它做 sleep。改成 per-source 预算要连带动这里。

**N3 · 冗余红利：新 admin 路由会自动被鉴权红线覆盖。** `red_lines.rs:32` 的 `routes()` 从 `ACTION_ROUTES` 派生，不是手写表（注释里明写「手写表会在有人新增动作时悄悄漏掉那一条」）——这个坑已经被人踩过并堵上了。

### 仍未核对

- §4.3 候选端口表属外部事实（Ollama / LM Studio 等默认端口），需实机或官方文档确认
- `protocol.rs` 的 `decode` / `repair` 内部实现细节（初稿关于「本地必须走独立解析路径」的结论依据的是 `docs/wiki/架构设计.md` 的描述，**尚未读该源文件的函数体**，列为实施前的第一件事）
- 面板侧 `src/**` 组件结构

---

*本文为设计稿。§11 已记录 2026-10-03 的源码核对结果与更正；标 ❌ 的两条初稿论断请勿再引用。*
