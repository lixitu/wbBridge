---
AIGC:
    Label: "1"
    ContentProducer: 001191440300708461136T1XGW3
    ProduceID: 75cf1e850e5668c557334a2fa39f707b_5079e01dbf0111f1a05452540064ee0f
    ReservedCode1: Qy2y6ksgMcQk4enaR6sCHfDrB0vHQo8sVqLZFQjWKhuMQTbkLNbf+VqAo9Ai6NmS8LjliWE1KNFTBDqpzV/MXEGWlVblIDwBY5DSCp0nqJuWw7A8dPSxifHyFLmJrQw/IwVsMhaVb51p6dmajARTpLFxIKdEx//7RiIM3x2jEfAe98UfwTMEpgerlb0=
    ContentPropagator: 001191440300708461136T1XGW3
    PropagateID: 75cf1e850e5668c557334a2fa39f707b_5079e01dbf0111f1a05452540064ee0f
    ReservedCode2: Qy2y6ksgMcQk4enaR6sCHfDrB0vHQo8sVqLZFQjWKhuMQTbkLNbf+VqAo9Ai6NmS8LjliWE1KNFTBDqpzV/MXEGWlVblIDwBY5DSCp0nqJuWw7A8dPSxifHyFLmJrQw/IwVsMhaVb51p6dmajARTpLFxIKdEx//7RiIM3x2jEfAe98UfwTMEpgerlb0=
---



# 多平台免费模型接入设计（v1.0.3）

> 状态：方案已定型（B：用户自填凭据，默认零操作）｜日期：2026-10-03｜目标版本：1.0.3｜未提交
> 需求口径见 §2，已排除方案见 §2.1；定型后即可进 writing-plans。

## 0. 一句话

在不破坏"隔离 + 零凭据"既有安全边界的前提下，让 WB Bridge 支持从**多个上游平台**（含 models.dev 未收录的平台）发现并发布免费模型，凭据由用户在应用面板内录入、以 `0600` 存于平台数据目录。

## 1. 背景与动机

当前产品只认 OpenCode 自带的 `opencode` provider，免费模型全部来自 OpenCode Zen。用户诉求是"能力广度"——把可用的免费模型来源从 1 家扩到 6 家，且最终能继续扩。

## 2. 需求口径（已锁定）

| 维度 | 结论 |
|---|---|
| 方向 | 能力广度：支持更多平台免费模型 |
| 平台范围 | 一期 6 家：智谱、ModelScope、SiliconFlow、腾讯混元、百度千帆、讯飞星火 |
| "免费"口径 | 有免费额度即算，**按平台维护白名单**，不以 catalog 标价为准 |
| 架构路径 | 优先复用 OpenCode 的多 provider 能力；models.dev 未收录的平台走自定义 provider 声明 |
| 用户门槛 | **默认零操作**：开箱即用既有 OpenCode Zen 模型，不接触任何 Key；**扩平台为可选增强**：用户自行注册并在面板内粘贴 API Key |
| 凭据存放 | 平台数据目录 `0600`，需在 AGENTS.md 补一条允许位 |
| 版本纪律 | 固定 patch +1 → 1.0.3，必出 release notes；不提交、不 push、不 tag |

### 2.1 已排除的方案（决策记录）

| 方案 | 结论 | 理由 |
|---|---|---|
| 内置维护者自有 Key | **排除** | 分发即等于公开：Tauri 客户端运行时必须解密并传给 OpenCode，子进程环境变量可被读取；同时撞死"核心不持上游凭据"的红线；免费额度按账号计，全体用户共用会迅速耗尽并触发风控，主体风险由维护者承担 |
| 维护者自建中转服务（客户端不持上游 Key） | **排除（本期）** | 需常驻服务 + 域名 + 额度治理 + 限流，引入云端依赖，与"隔离 + 离线单机"定位冲突；若日后要做，应作为独立服务端项目立项，不并入客户端 |

结论：一期采用**用户自填凭据**；默认路径保持零操作，扩平台仅作可选增强。

## 3. 现状事实（只读勘察结论）

### 3.1 零凭据是怎么实现的（必须保住的边界）

- 核心**不持任何上游凭据**：全仓 `auth.json` / `login` 零命中，无登录流程。
- 机制：下载官方 OpenCode 二进制，`env_clear()` + 17 项白名单（`ENV_ALLOW`，`runtime.rs` L743-761）+ XDG 全隔离到 `<data_dir>/opencode/{config,data,cache,state}` 的空数据目录启动；OpenCode 自带的 `opencode` provider 恰好提供 cost 全 0 的模型，核心只"读价格、判免费"。
- 隔离配置 `isolated_config()`（`runtime.rs` L719-737）只写 `permission` / `autoupdate:false` / `share:disabled` / 两个 agent 定义，**无任何 provider、auth、端点字段**。
- 唯一落盘凭据是核心自签的 `<data_dir>/api-key`（`orchestration.rs` L181-201，`0600`），用途是本机回环 Bearer，非上游凭据。
- 红线：`ENV_ALLOW` 中不得出现任何凭据类变量名（`runtime.rs` L742 注释、`red_lines.rs` L245-266 断言宿主 `OPENAI_API_KEY`/`ANTHROPIC_API_KEY` 绝不进子进程）。

### 3.2 与本次改动直接相关的代码落点

| 事项 | 位置 | 现状 |
|---|---|---|
| provider 选择 | `backend.rs` L84-97 | 只认 `providers.all[]` 中 `id=="opencode"`，缺失即报错 |
| 免费判定 | `backend.rs` L100-118 | `cost.input/output/cache.read/cache.write` 四项全 0 + 支持文本输出 + 非 deprecated |
| 客户端模型 ID | `model_status.rs` L119-121 | `format!("OC · {}", name)`，全仓唯一命名空间 |
| 模型条目写入 | `sync.rs` L406-437 | `vendor` 硬编码 `"Custom"`；`id`/`name` = 上者；`apiKey` 写本地回环 Bearer |
| 合并与冲突 | `sync.rs` L295-381 | 保留非本服务条目；本服务条目全量替换；与既有条目**同名即静默丢弃**（不报错） |
| `/v1/models` | `server.rs` L738-747 | `owned_by` 硬编码 `"opencode"`；契约测试 `server.rs` L1451 固化 |
| 上游反查 | `protocol.rs` L173-197 | 靠 `opencode/` 前缀截串 + 内存目录反查，**无独立映射表** |
| 面板偏好 | `src/core/prefs.js` L51-61 | 白名单只有 `view`/`update`；`apiKey` 在值里也进不了存储（`prefs.test.js` L47-72 钉住） |
| IPC 契约 | `src/App.vue` L38-58 → `src/core/bridge.js` L82-111 → `src-tauri/src/lib.rs` L478-505 / L692-698 | 现有 5 个命令；新增须同步 `lib.rs` 与 `capabilities/default.json` |
| 版本落点 | `scripts/check-version.mjs` L22-28 | 5 处：`package.json`、`tauri.conf.json`、`src-tauri/Cargo.toml`、AGENTS.md 两行表格 |

### 3.3 平台在 models.dev 的收录情况（226 家全量核对，2026-10-03）

| 平台 | provider id | 收录 | catalog 标价 0 的模型 | 结论 |
|---|---|---|---|---|
| 智谱 | `zhipuai` | ✓（17） | **0 个**，全部标价 | 可接入，但 cost 判定会整家漏掉 |
| ModelScope | `modelscope` | ✓（7） | 多数（GLM-4.6、Qwen3 系列标 0） | 可接入 |
| SiliconFlow | `siliconflow-cn`(44) / `siliconflow`(57) | ✓ | 至少 1 个 | 可接入 |
| 腾讯混元 | 无 `hunyuan`；`tencent-tokenhub`(3) / `tencent-token-plan`(2) / `tencent-coding-plan`(8) | 部分 | tokenhub 的 `hy3`、coding-plan 8 个全 0 | 可接入，需选对入口 |
| 百度千帆 | — | **未收录** | — | 必须走自定义 provider |
| 讯飞星火 | — | **未收录**（`vispark` 是另一家视觉 Lab） | — | 必须走自定义 provider |

## 4. 目标与非目标

### 目标

1. 6 家平台可分别由用户在面板内录入凭据并生效；未录入的平台不出现、不报错、不影响其他平台。
2. models.dev 未收录的平台（百度千帆、讯飞星火）通过自定义 provider 声明接入并可用。
3. 免费判定支持"声明式白名单"：白名单平台的模型不依赖 catalog 价格。
4. 多平台同名模型不互相覆盖、不静默丢失。
5. 既有"隔离 + 不污染用户 OpenCode 配置与登录态"的边界不退化。
6. **默认路径零操作不退化**：未录入任何凭据时，既有 OpenCode Zen 模型照常可用，不出现空列表、不报错、不用引导阻塞主流程。

### 非目标

- 不内置任何维护者持有的上游凭据；不新增中转/代理服务端（理由见 §2.1）。
- 不做额度查询/统计（沿用"本应用不控制也不缓存额度"）。
- 不做 OAuth 登录、不做非 OpenAI 兼容协议、不做协议转换网关。
- 不改 `autoupdate` / `share` / 原生工具门禁。
- 不改 `prefs.js` 的 localStorage 白名单（凭据结构上仍进不了前端存储）。
- 不新增代理形态（PAC/SOCKS 仍不在本期）。

## 5. 平台注册表

新增一处**单一事实来源**（Rust 侧常量表，建议落 `src-tauri/core/src/providers.rs`），字段与取值：

| 字段 | 说明 |
|---|---|
| `id` | provider 标识（models.dev 在册者用其在册 id） |
| `label` | 面板显示名与客户端前缀（如 `智谱`） |
| `form` | `Builtin`（models.dev 在册，OpenCode 内置）/ `Custom`（需自写 provider 声明） |
| `free_mode` | `CostZero`（沿用价格判定）/ `Declared`（白名单放行，不看价格） |
| `base_url` | `Custom` 时必填 |
| `env_key` | 该平台 API Key 的规范环境变量名（仅用于文档与用户指引，**不进入 `ENV_ALLOW`**） |

一期取值（`base_url` 与 `env_key` **落地前必须逐家实测核对**，下表为待核值）：

| 平台 | id | form | free_mode | env_key（待核） |
|---|---|---|---|---|
| 智谱 | `zhipuai` | Builtin | **Declared**（catalog 无 0 价模型） | `ZHIPU_API_KEY` |
| ModelScope | `modelscope` | Builtin | `CostZero` | `MODELSCOPE_API_KEY` |
| SiliconFlow | `siliconflow-cn` | Builtin | `CostZero` | `SILICONFLOW_CN_API_KEY` |
| 腾讯混元 | `tencent-tokenhub` | Builtin | `CostZero`（`hy3`/`hy3-preview` 标 0） | `TENCENT_TOKENHUB_API_KEY` |
| 百度千帆 | `qianfan` | **Custom** | **Declared** | 待核 |
| 讯飞星火 | `spark` | **Custom** | **Declared** | 待核 |

上表已定型：智谱采用 `Declared` 宽口径放行，其余在册平台沿用 `CostZero`，百度千帆与讯飞星火走 `Custom` 声明。`env_key` 仅用于面板指引文案与环境变量命名参考，**不进入 `ENV_ALLOW`**。

## 6. 设计

### 6.1 凭据存储与录入

**存储**：新增 `<data_dir>/providers.json`，权限 `0600`，结构：

```json
{
  "zhipuai": { "apiKey": "<用户录入>" },
  "qianfan": { "apiKey": "<用户录入>" }
}
```

- 只存**用户已启用平台**的条目；未录入的平台即键不存在。
- 写入与读取只走核心；`0600` 与失败即终止的处理方式沿用 `api-key` 的既有实现（`orchestration.rs` L145-201）。
- **严禁**进入日志、`status.json`、仓库、面板 `localStorage`。

**录入链路**（面板 → 核心）：

1. 面板新增「平台」视图（或并入「模型」视图），列出注册表 6 家的状态：`已配置 / 未配置`，已配置只回显掩码（如 `sk-****abcd`）。
2. 新增 IPC 命令（建议命名 `provider_status` 只读、`set_provider_key`、`clear_provider_key`），同步改动三处：`src/core/bridge.js` 的 `action()`、`src-tauri/src/lib.rs` 的 `ADMIN_ROUTES` 与 `generate_handler!`、`capabilities/default.json`。
3. 核心侧新增对应 `/admin/*` 路由，落到 `providers.json` 的读写。
4. 前端**不新增** `SANITIZERS` 键，凭据不经过 `prefs.js`。

### 6.2 配置注入（保持隔离）

隔离配置仍由核心生成，`isolated_config()` 增加 `provider` 段：

- `Builtin` 平台：只注入 `options.apiKey`。
- `Custom` 平台：注入 `npm: "@ai-sdk/openai-compatible"`、`name`、`options.baseURL` + `options.apiKey`、`models`（显式模型清单，因为无 catalog 可查）。

**注入通道（已定型）**：沿用现有 `OPENCODE_CONFIG_CONTENT`（`runtime.rs` L790-793）承载。理由：它是"核心自填值"，与自签 `OPENCODE_SERVER_PASSWORD` 同级，且不触碰 `ENV_ALLOW` 白名单，隔离配置机制零新增。

- 已知代价：凭据会出现在子进程环境变量中（仅本机同用户可读），与现状同级，暴露面不因本功能扩大。
- 遗留备选（未采用，记录备查）：把 provider 段写入隔离目录下的 `opencode.json` 文件（`0600`，核心生成），凭据不进环境；若日后实测发现环境变量风险不可接受，再切换。
- **不得**把凭据透传进 `ENV_ALLOW`，不得写日志/`status.json`。

自定义 provider 声明格式以官方 Docs 为准（前轮已核实 `provider.<id>.options.baseURL` + `@ai-sdk/openai-compatible` 可接任意 OpenAI 兼容网关）；落地前按当期 OpenCode 版本再核一次字段名。

### 6.3 provider 遍历与免费判定

`backend.rs::free_models` 从"只认 `opencode`"改为**注册表驱动**：

1. 遍历 `providers.all[]`，命中注册表 `id` 且**该平台已配置凭据**者，纳入候选。
2. `free_mode = CostZero` → 沿用现有四项全 0 判定。
3. `free_mode = Declared` → **不看价格**，只保留"非 deprecated + 支持文本输出"两条筛选。
4. 未配置凭据的平台整体跳过，不计入错误。
5. `opencode` provider 的既有逻辑保持不变（回归保护）。

### 6.4 命名空间与上游反查

**问题**：`client_model_id` 全局唯一命名空间是 `OC · <name>`；`merge_models` 的冲突判据基于同名，两个平台出现同名模型时**静默丢弃**。

**设计**：`client_model_id` 的显示名改为 `<平台 label> · <模型名>`：

| 来源 | 前缀 |
|---|---|
| OpenCode Zen（既有） | `OC · `（**保持不变**，避免影响存量用户） |
| 智谱 | `智谱 · ` |
| ModelScope | `MS · ` |
| SiliconFlow | `SF · ` |
| 腾讯混元 | `HY · ` |
| 百度千帆 | `千帆 · ` |
| 讯飞星火 | `星火 · ` |

**上游反查**：`protocol.rs` 现靠 `opencode/` 前缀截串，多平台后不成立。改为**显式映射表**：模型发现时构建 `client_id → upstream_id`（如 `智谱 · GLM-5.3-Flash` → `zhipuai/glm-5.3-flash`）的内存映射，请求时按表命中；表随每次发现重建。

**连锁改动（4 处 + 夹具，必须同步）**：`model_status.rs` L119-121、`server.rs` L742、`sync.rs` L327、`protocol.rs` L185，以及 `tests/fixtures/model_status.json` 与 `server.rs` L1451 契约测试。

### 6.5 发布与展示

- `sync.rs::model_entry`：`id`/`name` 随 6.4 变化；`apiKey` 仍写本地回环 Bearer（不变）。
- `vendor` 字段：现状硬编码 `"Custom"`。**本期不动**（无法确认 WorkBuddy 侧对 `vendor` 的依赖），列为后续可选优化。
- `server.rs` 的 `/v1/models`：`owned_by` 是否随平台变化，**需实测** WorkBuddy 是否依赖该字段；默认保守不动，仅在确认无副作用后按平台填充。

### 6.6 探测

- 只探测**已配置平台**的候选模型，未配置平台不入探测队列。
- 探测会消耗平台免费额度，沿用现有"探测说明"文案，并在多平台文案中明确"每个平台各消耗少量额度"。
- 单平台探测失败不得导致其他平台结果丢失，也不得删除既有发布条目（沿用空列表保护，`sync.rs` L303-307）。

## 7. 改动清单

| 层 | 文件 | 改动 |
|---|---|---|
| 核心 | `src-tauri/core/src/providers.rs`（新增） | 平台注册表单一事实来源 |
| 核心 | `backend.rs` | `free_models` 注册表驱动；`Declared` 模式 |
| 核心 | `runtime.rs` | `isolated_config()` 注入 provider 段；凭据读取 |
| 核心 | `sync.rs` / `model_status.rs` / `server.rs` / `protocol.rs` | 命名空间、映射表、`/v1/models` |
| 核心 | （新增凭据读写） | `providers.json` `0600` 读写 |
| 壳 | `src-tauri/src/lib.rs` | 新增 IPC 命令 + `ADMIN_ROUTES` |
| 壳 | `capabilities/default.json` | 新命令能力声明 |
| 面板 | `src/core/bridge.js` | `action()` 新增分支 |
| 面板 | `src/views/` + `src/App.vue` + `src/views/SideBar.vue` + `src/core/prefs.js`（`VIEW_IDS`） | 新增「平台」视图（三处同步：`VIEW_IDS` / `groups` / `App.vue` 的 `v-if`） |
| 文档 | `AGENTS.md` | 补凭据允许位（见 §8）；红线条款增补 |
| 文档 | `README.md` / `docs/wiki/*` | 平台支持说明、凭据录入指引、首次使用流程 |
| 版本 | `scripts/` + 5 处落点 + `docs/version/RELEASE-NOTES-v1.0.3.md` | 1.0.2 → 1.0.3 |

## 8. 红线与文档补充

现有红线的**字面**并未禁止"数据目录存凭据"（禁的是：进 `localStorage`、进日志/`status.json`、进仓库、凭据类变量进 `ENV_ALLOW`、"读取/打印/回显/提交"凭据）。因此需要的是**补一条允许位**：

1. 允许在平台数据目录新增 `providers.json`，权限 `0600`，仅存用户录入的上游凭据。
2. 该文件不得进入日志、`status.json`、仓库、`localStorage`、任何 IPC 的明文返回（只回掩码）。
3. `ENV_ALLOW` 不得为其放开任何凭据类变量名。
4. 面板只提供录入/清除/掩码展示，不提供明文回显。
5. `providers.json` 的读写只发生在核心，壳与面板不得直接读写该文件。

## 9. 验收标准

1. 6 家平台逐一：面板录入 Key → 面板出现该平台模型 → 探测通过 → 导入 WorkBuddy → WorkBuddy 内可正常对话。
2. 未录入凭据的平台：不出现模型、不报错、不影响已配置平台。
3. 两家 `Custom` 平台（百度千帆、讯飞星火）经自定义 provider 声明后可用。
4. 智谱在 `Declared` 模式下能列出并调用其免费模型（不受 catalog 标价影响）。
5. 多平台同名模型共存，无静默丢失。
6. 存量 8 个 `OC · ` 模型行为不变（回归）。
7. `providers.json` 不出现在日志、`status.json`、git 状态中。
8. `cargo test` 全绿（含新增单测：注册表、`Declared` 判定、命名空间冲突、掩码回显）；`npm run version:check` 通过。
9. 全部 UI/安装包相关结论按仓库纪律标注"未验证"（GUI 实机、CI、签名公证、Win/Linux 等既有 11 项未验证项不变）。

## 10. 风险与未验证项

| 风险 | 说明 | 处置 |
|---|---|---|
| 自定义 provider 声明能否被当期 OpenCode 正确加载 | 本仓无先例，需实测 | 首批实测千帆/星火，失败则退化为"4 家 + 自建上游二期" |
| `base_url` / `env_key` 待核值 | 上表为待核 | 落地前逐家对照官方文档实测 |
| 智谱"永久免费"与账号实名/额度政策 | 平台侧政策，catalog 不体现；额度耗尽时行为未知 | 面板文案明确"额度与可用性由各平台决定"，沿用不缓存额度口径 |
| 凭据进入子进程环境变量 | 与现状同级，仅本机同用户可读 | 已定型采用 §6.2 环境变量通道；保留文件通道作为切换预案 |
| `/v1/models` 的 `owned_by` 改动副作用 | WorkBuddy 侧依赖未知 | 默认不动，确认后再改 |
| 多平台探测消耗额度 | 6 家各消耗一次 | 面板明示；仅探测已配置平台 |

## 11. 版本与交付节奏

1. 本设计稿已定型（方案 B：用户自填凭据，默认零操作）→ 进 writing-plans 出实施计划。
2. 实施：注册表 → 凭据通道 → provider 遍历与判定 → 命名空间与映射 → 面板 → 文档。
3. 版本推进至 **1.0.3**（5 处落点 + `docs/version/RELEASE-NOTES-v1.0.3.md`）。
4. 不提交、不 push、不 tag，留待用户决定。
*（内容由AI生成，仅供参考）*
*（内容由AI生成，仅供参考）*
