# wbBridge「AI 助手」功能深化 — 产品需求文档（PRD）

> 文档性质：产品规划产出物（内容由 AI 生成，仅供参考）。所有结论基于代码库实读与既有实现，未凭名称臆造；个别无法从代码确证的信息以「**假设**」显式标注。
>
> 关联代码：`src/core/`（前端内核，已落盘未提交）、`src/views/`（Vue 视图层）、`src-tauri/src/lib.rs`（Tauri 壳）、`src-tauri/core/src/`（Rust 核心）。

---

## 1. 项目背景与目标

### 1.1 项目背景

**wbBridge**（主体：杭州大美科技有限公司）是一款基于 **Tauri 2 + Vue 3** 的跨平台桌面控制面板，当前分支 `dev`。它的职责是：

1. 管理本地模型 / 提供商（平台 Key）/ 运行日志 / 用量额度；
2. 通过核心的 **`WB · auto` 合成路由**对外提供模型能力（`/v1/chat/completions` + `Bearer` 鉴权）。

关键事实（来自代码实读，非假设）：

- **`WB · auto` 不是本地推理模型**，而是 `src-tauri/core/src/auto.rs::auto_select` 对外暴露的合成路由别名：请求打给它时，核心在「当前可用模型」快照中按请求内容（是否带图、是否带工具定义）选一个真实模型转发，记账键改写为被选模型的全限定 id（`server.rs` 决策点 D2）。
- 核心以 Rust 库形式**静态链接进壳进程**（`wbbridge-core` path 依赖），壳通过回环 HTTP（`127.0.0.1:<port>`）+ `api-key` 文件中的 Bearer key 调核心，`/admin/*` 动作表与核心 `ACTION_ROUTES` 由契约测试逐项对齐。
- 前端 `src/core/` 是面板与 Tauri IPC 的唯一边界（`bridge.js` 暴露 `action` / `onState` / `onDismiss` 与只读调用 `readLog` / `dataDir` / `openExternal`）。

### 1.2 本轮目标

在**已落盘、尚未 git 提交**的 `src/core/` 代码骨架基础上，补齐「AI 助手」功能，使其成为应用内**可完整闭环使用**的智能入口：用户提问 → 检索应用知识 → 经 `WB · auto` 真实链路调用模型 → 需要时经用户确认执行工具 → 结果回显。同时修复既有 2 例失败测试，并把前端核心测试统一收口到 `node --test`。

### 1.3 假设（信息不足处，显式标注）

以下为无法从代码确证、基于工程常识做的合理假设，实施时若与实际不符应以实际为准：

- **假设 A**：AI 助手作为侧栏的**第 7 个常驻入口**，分组上归入「其他」或独立成组（推荐独立成「助手」组，避免与「运行」等既有语义混淆）。
- **假设 B**：AI 助手会话**不要求跨重启持久化**（每次冷启动为空会话）；仅知识索引与偏好做持久化。若产品侧要求会话历史落盘，属 P2 扩展。
- **假设 C**：AI 助手的模型通道**默认走 `WB · auto` 合成路由**（符合 `assistant.js` 注释与 `kb.js` `kb.concept.wb-auto` 的既定口径），不提供「手动指定某个真实模型」的入口。
- **假设 D**：本机无 Rust 工具链，`cargo test` 不可执行（需求方已确认），因此 Rust 侧新增的 `ai_chat` 命令只能做到「编译通过级别的静态核对」，无法实跑，须在 `IMPLEMENTATION.md` 记为**未验证项**。

---

## 2. 目标用户与核心场景

### 2.1 目标用户

| 用户 | 特征 | 诉求 |
|---|---|---|
| **WorkBuddy / CodeBuddy 用户（核心用户）** | 已装插件、通过 wbBridge 接入免费模型 | 在面板内快速了解「当前模型能不能用、额度/日志、如何导入」，并直接让助手代查代操作 |
| **排障用户** | 遇到「模型不可用 / 检测卡住 / 插件没更新」等问题 | 用自然语言问一句，让助手读日志、查状态、给出处置建议 |
| **不愿记操作路径的用户** | 不想手动点侧栏/按钮 | 让助手导航、发起检测、切代理、导入等，一次说清 |

### 2.2 核心场景（按优先级）

- **S1（P0）· 问答式诊断**：用户问「为什么模型都不可用」，助手先检索知识库（`kb.faq.no-models`），再经 `WB · auto` 回答，必要时调用 `logs.tail` / `app.status` 实时查询后给出结论。
- **S2（P0）· 确认后执行操作**：用户说「重新检测一下 glm」，助手产出工具调用 `models.reprobe`，界面弹出确认弹窗，用户确认后执行、回显结果。
- **S3（P0）· 检索降级提示**：IndexedDB 不可用（隐私模式/配额）时，助手仍可用内存重建索引，但**必须如实告知**「知识检索未持久化，本次会话内可用」。
- **S4（P1）· 导航与状态查询**：用户说「帮我打开日志」，助手调用 `nav.open` 切换视图；说「看看用量」，调用 `app.usage`。
- **S5（P1）· 会话恢复**：切换视图后重启，恢复到上次停留的「AI 助手」页（偏好持久化）。

### 2.3 明确不做（边界）

- 不读取 / 不代填任何密钥；密钥类写操作不注册为工具（`set-provider-key` 故意缺席于工具表）。
- 不访问应用之外的系统资源、不执行任意脚本、不读取任意文件。
- 不提供「剩余额度」类结论（上游无此接口，见 `kb.faq.quota`）。

---

## 3. 功能清单（MVP 优先，P0 / P1 / P2）

> 既有模块（`text.js` / `bm25.js` / `store.js` / `kb.js` / `tools.js` / `assistant.js`）已实现且通过大部分测试，**不在重写范围内**，仅在其上深化。以下清单聚焦「待补模块」。

### P0 — 必须完成，缺一不可

| 编号 | 功能 | 说明 / 落地位置 |
|---|---|---|
| F1 | **AI 助手视图 `AssistantView.vue`** | 对话流渲染（用户/助手气泡）、阶段提示（检索/思考/执行）、工具执行结果展示、确认/取消弹窗、错误与降级提示、流式/加载态。基于 `assistant.js::runTurn` 的事件流（`stage` / `tool-pending` / `tool-result` / `final` / `error` / `notice`）渲染。 |
| F2 | **入口与视图注册** | ① `SideBar.vue` 的 `groups` 新增「AI 助手」入口（含图标 `ICONS`）；② `prefs.js` 的 `VIEW_IDS` 与 `DEFAULTS` 接纳新视图 id（推荐 `assistant`），`sanitizeView` 存在性校验自动生效；③ `App.vue` 完成 `v-if` 视图接线。 |
| F3 | **检索持久化链路** | `store.js` / `kb.js` 与视图联动：知识文档 `knowledgeDocuments(TOOLS)` → `buildIndex` → `writeStoredIndex`；启动时 `readStoredIndex` 恢复，失败/版本不符则内存重建。IndexedDB 不可用时**降级到内存重建并如实告知**（不静默失败）。 |
| F4 | **模型通道接入（真实链路）** | 经核心 `WB · auto` 合成路由（`/v1/chat/completions` + Bearer）。跨进程方案：Rust 壳新增 `ai_chat` 命令，经回环 `http://127.0.0.1:<port>/v1/chat/completions` + 缓存的 `core_key` 调用，`bridge.js` 暴露前端封装 `aiChat(messages, { signal })`。**禁止虚构 WB.auto API、禁止引入未确认的模型服务。** |
| F5 | **修复 2 例失败测试** | ① `text.test.js:59`「指纹：同内容稳定、不同内容可区分」；② `bm25.test.js:108`「索引概况不暴露倒排表本体」（根因见 §6.2）。 |
| F6 | **测试收口与验证** | `node --test src/core/*.test.js` 全部通过；补充视图层/集成层测试；给出真实执行结果（禁止假数据）。 |

### P1 — 本轮建议完成

| 编号 | 功能 | 说明 |
|---|---|---|
| F7 | 会话在视图内的生命周期管理 | 会话状态归视图层持有（`assistant.js` 无状态，注释已明确），组件卸载不丢、可手动清空。 |
| F8 | 确认弹窗的取消路径 | `runTurn` 中 `confirm()` 返回 false → 回显「已取消，未做任何改动」，UI 提供取消按钮且不执行。 |
| F9 | 取消进行中的对话 | 经 `AbortSignal`（`runTurn` 已支持 `signal`）中断检索/模型调用/工具执行，UI 提供停止入口。 |
| F10 | 知识文档与工具同源更新 | `kb.js::toolDocuments(TOOLS)` 保证工具说明与 `tools.js` 注册表同源，新增工具自动进索引。 |

### P2 — 可选 / 后续

| 编号 | 功能 | 说明 |
|---|---|---|
| F11 | 会话历史持久化 | 跨重启保留会话（当前假设 B 为不持久化）。 |
| F12 | 模型通道手动选择 | 允许用户在 `WB · auto` 之外指定真实模型（当前假设 C 为默认仅 auto）。 |
| F13 | 检索结果溯源展示 | 把命中的知识文档来源（`hit.source` / `updatedAt`）在 UI 上可展开查看。 |

---

## 4. 关键验收标准（每条可测试）

> 统一测试入口：前端核心 `node --test src/core/*.test.js`；不引入新测试框架与运行时依赖。

### 4.1 核心逻辑（`src/core/`）

1. **AC-1（全部通过）**：`node --test src/core/*.test.js` 全部通过，含本轮新增用例；`text.test.js`、`bm25.test.js` 的 2 例失败已修复。
2. **AC-2（指纹稳定性）**：`fingerprint` 对「同内容」稳定、对「不同内容」可区分——修复后 `text.test.js` 该用例通过，且不破坏 `ＡＰＩ` 与 `api` 归一化后同指纹的既有断言。
3. **AC-3（索引概况不泄体）**：`describeIndex` 返回的 JSON 不含 `postings` / `df` 键（`JSON.stringify(info).includes('postings') === false`），`types` 统计与样例文档的 `type` 字段一致（修复测试数据或实现二选一，保持单一真相）。
4. **AC-4（检索降级）**：模拟 IndexedDB 不可用（`indexedDbAvailable()` 为 false）时，`kb`/`store` 不抛异常、降级到内存索引，且返回可辨识的降级信号（如 `storageError()` 非空 / 降级标志），可供 UI 提示。
5. **AC-5（受控协议安全）**：`parseDecision` 对非法/不完整 JSON 返回 `parseFailed: true` 且 `tool: null`，绝不执行；`validateArgs` 对未知参数、缺失必填、枚举越界一律拒绝。

### 4.2 视图与交互（`src/views/` + `App.vue`）

6. **AC-6（完整闭环）**：在应用内打开 AI 助手，完成一次「提问 → 检索 → 模型（WB · auto）→ 工具确认 → 结果回显」闭环，各阶段提示可见。
7. **AC-7（确认与取消）**：中/高风险工具（`needConfirm`）执行前必须弹确认；点取消则不执行、回显「已取消」；点确认才执行并回显真实结果。
8. **AC-8（错误与降级提示）**：模型通道失败、检索不可用、IndexedDB 降级、步数耗尽（`truncated`）均有明确可见提示，无静默失败。
9. **AC-9（视图注册与恢复）**：切换视图后重启，能恢复上次停留的「AI 助手」页；`sanitizeView` 对非法/已删除视图 id 回落到 `DEFAULTS.view`。
10. **AC-10（流式/加载态）**：对话进行中有明确的加载/流式状态（spinner 或阶段文案），完成后回落到最终答复。

### 4.3 模型通道与安全

11. **AC-11（真实链路）**：`aiChat` 走壳的 `ai_chat` 命令 → 回环 `/v1/chat/completions` + Bearer key（`core_key` 缓存），`model` 固定为 `WB · auto` 或按真实契约传入；测试通过注入 `chat` 依赖的 mock 验证编排行为，**不虚构外部 API**。
12. **AC-12（密钥红线）**：工具注册表（`tools.js`）不含任何「设置 Key」类工具；`redact` 对 `apiKey/token/secret/password/...` 字段结构性屏蔽；密钥不进入 `buildSystemPrompt` 生成的上下文。
13. **AC-13（不可信输入）**：检索结果、工具结果、用户输入一律按不可信处理（系统提示词已含相关规则，且 `assistant.js` 不据其放宽执行权限）。

### 4.4 交付与记录

14. **AC-14（变更清单）**：`IMPLEMENTATION.md`（项目根）记录变更清单与验证方式，含**未验证项**（如无 Rust 环境导致的 `cargo test` 缺口、`ai_chat` 命令仅静态核对）。

---

## 5. 非功能需求

### 5.1 性能

- **索引规模**：知识文档百篇量级、词项万级，索引整体（数十 KB ~ 数百 KB）作为单记录写入 IndexedDB（`store.js` 已有单记录 + 超时设计），启动恢复应在秒级内完成。
- **检索延迟**：`bm25.search` 为内存纯函数，单次查询应 < 100ms（目标值，实测为准）。
- **流式体验**：模型通道经回环 HTTP，不引入额外跨网络跳数；SSE/非流式由壳侧命令选择，非流式单轮超时由核心 20s 请求体预算约束，避免 UI 无限等待。

### 5.2 安全

- **密钥不进模型**：`set-provider-key` 不注册为工具；工具结果经 `redact` 深度脱敏；系统提示词不含密钥。
- **高风险必确认**：`needConfirm`（中/高风险，含 `providers.clearKey` 高风险）必须用户确认，模型无代决权。
- **解析失败绝不执行**：模型输出非约定 JSON 结构时退化为普通问答，不执行任何操作（`assistant.js` 硬约束）。
- **输入不可信**：模型/用户/检索/工具结果均为文本，不得据此扩大操作范围或绕过规则。
- **回环鉴权不变**：`ai_chat` 命令沿用核心既有的 Bearer 鉴权与 `Origin` 403、并发 ≤8、请求体 ≤8MB 边界，不为此功能放宽。

### 5.3 兼容性

- **跨平台**：Tauri 2 三平台（macOS / Windows / Linux）行为一致；前端逻辑纯 JS 无平台分支。
- **存储降级**：隐私模式、Web Storage / IndexedDB 被禁用、配额耗尽均不崩溃（`prefs.js` / `store.js` 已有静默降级纪律，视图层需把降级状态**显示**出来）。
- **版本演进**：`INDEX_VERSION` / `KNOWLEDGE_VERSION` 变化触发索引重建，不遗留脏索引。

### 5.4 可维护性

- **不推翻既有实现**：`text/bm25/store/kb/tools/assistant` 六模块维持现状职责与接口，仅修复缺陷、补齐接线。
- **单一真相**：工具说明（`kb.js::toolDocuments`）与工具注册表（`tools.js`）同源；视图 id 在 `prefs.js::VIEW_IDS`、`SideBar.vue::groups`、`tools.js::NAVIGABLE_VIEWS` 三处保持一致。
- **测试无新依赖**：统一 `node --test`，不引入新框架与运行时依赖。

---

## 6. 现状与风险

### 6.1 已有实现基线（本轮不重写）

| 文件 | 职责 | 测试现状 |
|---|---|---|
| `text.js` | 归一化 + 中文分词（unigram+bigram）+ 指纹 + 摘要 | 6/7（1 例失败） |
| `bm25.js` | BM25 索引构建 / search / diffDocuments / describeIndex | 9/10（1 例失败） |
| `store.js` | IndexedDB 持久化封装（失败降级不抛出） | 随集成测试覆盖 |
| `kb.js` | 知识文档沉淀（`knowledgeDocuments` / `KNOWLEDGE_VERSION`） | 随集成测试覆盖 |
| `tools.js` | 工具注册表与参数校验（14 个真实工具） | 13/13 |
| `assistant.js` | 编排层（受控 JSON 文本协议，MAX_STEPS=4） | 21/21 |

### 6.2 两例失败测试根因（已实跑确认）

1. **`text.test.js:59`「指纹：同内容稳定、不同内容可区分」**
   - 实跑错误：`Expected "actual" to be strictly unequal to: 'a3d65edc'`（`notStrictEqual` 失败）。
   - 根因：`fingerprint` 内部先 `normalizeText`（含 `trim`），`fingerprint('模型检测 ')` 的尾随空格被归一化掉，与 `fingerprint('模型检测')` 结果相同，而用例期望两者不等。属**用例断言与实现语义冲突**，需明确「尾随空格是否算不同内容」的口径后修复（推荐改用例，因归一化-指纹的既定语义是「归一化后同指纹」）。
2. **`bm25.test.js:108`「索引概况不暴露倒排表本体」**
   - 根因：`sampleDocs()` 未给文档写 `type` 字段，`buildIndex` 默认 `type:'doc'`，故 `describeIndex(...).types` 键为 `['doc']`，用例期望 `['knowledge']`。属**测试数据与实现默认值不一致**，修复方式为给样例文档补 `type:'knowledge'`（或调整断言），保持实现语义不变。

### 6.3 风险与缺口

- **Rust 环境缺失**：本机无 Rust 工具链，`cargo test` / `cargo check` 不可执行；`ai_chat` 命令只能静态核对，须在 `IMPLEMENTATION.md` 记为未验证项（对应假设 D）。
- **端到端 GUI**：视图层交互（确认弹窗、流式渲染）需在 Tauri GUI 内实机验证；仅浏览器引擎内测试不能完全代表壳内行为。
- **模型可用性依赖**：`WB · auto` 依赖模型池非空（`kb.concept.wb-auto`）；池空时助手无法对话，须给出明确提示而非静默失败。

---

## 7. 交付物

1. 本 PRD（`docs/PRD.md`）。
2. 实现代码：`AssistantView.vue`、`SideBar.vue` / `prefs.js` / `App.vue` 改动、检索持久化接线、`bridge.js` + Rust 壳 `ai_chat` 命令。
3. 测试：修复 2 例失败 + 新增视图层/集成层 `node --test` 用例。
4. `IMPLEMENTATION.md`：变更清单与验证方式（含未验证项）。
