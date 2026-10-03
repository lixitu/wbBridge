# WB Bridge 升级路线裁定与 1.0.3 多平台接入计划

> 日期：2026-10-03（同日**修订两次**）｜ 状态：**Stage 0 已基本收口，待开工** ｜ 决策人：维护者（用户）
>
> 修订记录：① 初稿把 1.0.3 定为「交付闭环版本」，维护者当日明确否决——**要功能性升级**；
> ② 同日 Stage 0 上游实测**推翻了原设计的两个前提**（四家的 npm 形态、`CostZero/Declared` 二分），
> 见 §2.1 与 §2.3。现以本文件为准。纯规划/文档改动，按现行 AGENTS.md 版本纪律**不推进版本号**；不提交、不 push、不 tag。

---

## 1. 本轮采到的决策

| # | 决策 | 采选项 | 影响 |
|---|---|---|---|
| D1 | 1.0.3 给谁 | **多平台免费模型接入**（初稿的「交付闭环」被维护者否决） | 1.0.3 = 功能版本；闭环类改动降级为同版本内的独立小提交，不再单独占版本 |
| D2 | 首批平台 | **四家全选**：ModelScope、SiliconFlow、腾讯混元 `tencent-tokenhub`、智谱 `zhipuai` | 见 §3 的分阶段顺序；原设计的「智谱需要 `Declared` 白名单」已被 §2.3 的实测结论合并掉（四家统一用注册表模型清单） |
| D3 | 本地运行时（Ollama / LM Studio / …） | **「没装也不打算装」** | 🔴 `docs/plans/2026-10-03-multi-upstream-gateway-design.md` **整条线下架**，不再排期。连带取消：本地来源发现、回环来源红线、冷启动探测预算、`/admin/probe` 异步化 |
| D4 | 版本纪律 | **只采纳 `check-version.mjs` 门禁**（有改动未升版即非零退出），保留六条禁止推进情形 | 纯文档/打磨继续不占版本号；不采纳「任何改动一律升 patch」 |
| D5 | 真实用户范围 | **只有维护者自己用** | 公证 / Developer ID / SmartScreen / 卸载体验 / 首次引导 = Won't；BYOK 的凭据面按「单人自持 Key」而非「多租户」设计 |
| D6 | 真实升级闭环 | 愿意本机手动跑，checklist 已备 | D5+D3 之后它不再是版本验收条件，而是**装 1.0.3 时顺手跑一遍**：见 `docs/qa/smoke-checklist.md`，重点取 §3 基线（GUI 首次实机、关窗即退出、`localStorage` 偏好、`usage` 口径）与 §5 预案，§4 升级闭环在 1.0.3 之后任一版本自然覆盖 |

**D3 是本文件的最大简化来源**：`Source` 抽象不再需要同时容纳「本地端点」与「云端平台」两种形态，1.0.3 只做**注册表驱动的 OpenCode provider 遍历**这一种形状。

---

## 2. 为什么是这四家、以及各自的技术形态

### 2.1 2026-10-03 的实测更正（原设计的两个前提是错的）

- ✅ **确认**：models.dev 的 provider 索引有 **226 条**，四家都在册（`modelscope` 7 个模型、`siliconflow-cn` 44、`tencent-tokenhub` 3、`zhipuai` 17）。
- ✅ **确认**：`qianfan` / `spark` / 独立的 `hunyuan` **不在册**（`vispark` 是另一家）。
- 🔴 **推翻原前提 ①**：这四家**全部是 `"npm":"@ai-sdk/openai-compatible"` 形态**，并各自带真实 baseURL。也就是说**四家都要写 provider 声明段**，不是只有千帆/星火要写。原设计的 `form: Builtin | Custom` 分叉因此**取消**——只剩一条代码路径。
- 🔴 **推翻原前提 ②**：OpenCode 官方文档口径是「集成 models.dev 的 75+ providers」，但其 provider 目录页**没有列这四家**；文档给出的启用方式只有两条：`/connect` 存凭据（**本项目刻意无登录流程，全仓 `auth.json`/`login` 零命中**）或在配置里写 `provider.<id> = { npm, options.baseURL, options.apiKey }`。→ 走配置段，通道就是既有的 `OPENCODE_CONFIG_CONTENT`。
- ⚠ **附带发现（待你判断是否要）**：目录里有一个 id 就叫 **`freemodel`** 的 provider（`@ai-sdk/anthropic`，`https://cc.freemodel.dev/v1`，10 个模型）。对一个「发布免费模型」的工具是天然相关，但**在册不等于可信**——第三方免费源的稳定性与合规性未知，若要接必须单独评估。另有 `zhipuai-coding-plan`（4 个模型）、`tencent-token-plan`（2 个）这类同族不同入口。

### 2.2 实测到的 baseURL（可直接写进注册表，不再是「待核」）

| 平台 | provider id | npm | baseURL（2026-10-03 实测自 models.dev 索引） |
|---|---|---|---|
| ModelScope | `modelscope` | `@ai-sdk/openai-compatible` | `https://api-inference.modelscope.cn/v1` |
| SiliconFlow（国内） | `siliconflow-cn` | `@ai-sdk/openai-compatible` | `https://api.siliconflow.cn/v1` |
| 腾讯混元 TokenHub | `tencent-tokenhub` | `@ai-sdk/openai-compatible` | `https://tokenhub.tencentmaas.com/v1` |
| 智谱 | `zhipuai` | `@ai-sdk/openai-compatible` | `https://open.bigmodel.cn/api/paas/v4`（**注意不是 `/v1`**） |

### 2.3 由此简化的判定设计

`CostZero` 与 `Declared` 的二分**合并成一条**：既然四家都要我们自己写声明段（含显式 `models` 清单），那么**注册表里的模型清单就是放行范围**——不再依赖上游 catalog 的标价，也就不存在「智谱 catalog 全是标价所以整家漏掉」这个问题。

- 收益：少一个判定分支、少一份独立维护的「免费白名单」、`free_models` 的改动面更小。
- 代价（必须如实记下）：**上游新增免费模型不会自动出现**，要改注册表发版；且「某个列进来的模型其实开始收费了」我们拿不到信号 → 面板文案必须保留「额度与可用性由平台决定」，且探测/请求失败要能看出是额度或计费问题。
- `opencode` 那条既有路径**保持 `CostZero` 判定不动**（回归保护，也是你日常真正在用的 8 个模型）。

---

## 3. 1.0.3 实施阶段（依赖有序，每段可单独验证）

### Stage 0｜上游事实核验（不写产品代码）

已在 2026-10-03 用 models.dev 索引实测解决的部分：**provider id、npm 形态、baseURL、各家模型数**（见 §2.2），以及「四家都要写声明段」这条路径判断。

🔴 **只剩一个未知，且只有带 Key 的实机能证**：往隔离配置注入 `provider.<id> = { npm, options.baseURL, options.apiKey }` 之后，当期 OpenCode 的 `providers.all[]` 会不会真的返回该平台与其 `models`（以及 `cost` 字段是否仍被 catalog 合并）。本仓库没有任何证据支持它会。

核验产出（**只要 provider id、模型 key、能力位与 cost，绝不回 Key**）：

1. `providers.all[]` 里目标平台是否出现、id 是否与注册表一致。
2. 该平台的 `models` 对象形态与 key 拼写（决定注册表怎么列放行范围）。
3. `capabilities.output.text`、`status`、`tool_call` 的实际取值分布（探测与 chatOnly 判定要用）。
4. 任选一个模型跑通一次真实对话（证明不只是列表里多一个名字）。

任一家的 `providers.all[]` 不出现 → 该平台从 1.0.3 摘掉，不硬做。

### Stage 1｜注册表 + 凭据通道 —— ✅ 已落地（2026-10-03，版本仍 1.0.2）

- 新增 `src-tauri/core/src/providers.rs`：`id / label / base_url / npm` 的单一事实来源表（§2.1 之后**不再有 `form` 与 `free_mode` 两个分叉字段**，四家同形）。~~`models[]`~~ **推迟到 Stage 4**，Stage 1 不带没人读的数据。
- 新增 `<data_dir>/providers.json`（`0600`，读写只在核心），沿用 `api-key` 的权限与失败处理写法。
- 新增三条管理动作（**不是新 IPC 命令**）：`/admin/provider-status`、`/admin/set-provider-key`、`/admin/clear-provider-key`，同步核心 `ACTION_ROUTES`（5 → 8，含数组长度与 `assert_eq!`）与壳侧 `ADMIN_ROUTES`（5 → 8）。
  ⚠ **与本节初稿的三处偏差（已按实读定稿）**：① `bridge.js` 的 `action()` 是泛化的，无需改动；② `generate_handler` 与 `capabilities/default.json` **都不需要动**（这三条走既有 `core_action` → `admin_call` 链路，不是壳自有命令，也不是插件命令）；③ `provider-status` **完全不回 Key**，连掩码尾字符都不返回（比初稿的「只回掩码」更严），返回形态只有 `{ providers: [{ id, label, configured }] }`。面板入口属 Stage 5。
- 红线：凭据不进日志、不进 `status.json`、不进 `localStorage`（`prefs.js` 白名单不动）、不进 `ENV_ALLOW`；`tests/red_lines.rs` 补第 10 项 `provider_registry_is_reviewed_and_status_echoes_no_key_material`。
- AGENTS.md 的「允许位」已补（强制规范「密钥零泄漏」+ 安全红线 #7），先于代码落盘。
- **实测数字**：核心 `cargo test` **218 通过 / 0 失败**（lib 197 + `js_parity` 11 + `red_lines` 10）；壳 `cargo test --lib` **9 通过**；两侧 `cargo clippy` **0 warning**；`npm run version:check` **5 处一致（1.0.2）**。细节见 `docs/validation.md` 顶部同名条目。
- **端到端仍未验证**：写进去的 Key 当前**没有消费者**（注入属 Stage 3），面板没有入口（Stage 5）。

### Stage 2｜命名空间与上游反查（**行为零变化的前置重构**）—— ✅ 已落地（2026-10-03，版本仍 1.0.2）

先把「`OC · ` 与 `opencode/` 前缀」从硬编码常量变成由 provider 决定的参数，**输出与今天逐字节一致**，再接第二家。这样 Stage 3 若出回归，归因唯一。

🔴 **夹具卡点已解除，结论与初稿相反**：Stage 2 **完全不碰 `tests/fixtures/*.json`**。`js_parity` 钉的是「模块函数对给定输入的输出」，而本阶段对 OpenCode 路径的输出必须逐字节不变 → **夹具本身就是零回归证明**，「重录依赖仓库外 JS 归档」这个前提不适用（该归档路径在工作区之外，本机 Agent 访问已被权限拦下）。配套纪律：第二家平台的**新行为一律用 `#[cfg(test)]` Rust 单测覆盖**，**不得**往 fixtures 手工加新 `expected`（那等于伪造 JS 真相）。验收硬线：`git diff --stat src-tauri/core/tests/fixtures` **必须为空**。

四处落点（行号为 2026-10-03 实读）：

| 落点 | 现状 | 改法 | 为何输出不变 |
|---|---|---|---|
| `model_status.rs`（新增前缀原语） | 无前缀原语 | `OPENCODE_NAMESPACE = "opencode"`、`join_namespace(ns, key)` = `"{ns}/{key}"`、`split_namespace(id) -> (&str, &str)`（**按第一个 `/` 切**；无 `/` 时返回 `("", id)`） | 纯新增，旧调用点未改前行为不变 |
| `backend.rs:88` `free_models(providers)` | 内部硬编码 `id == "opencode"`（`:94`）与 `format!("opencode/{key}")`（`:127`） | 新增 `free_models_in(providers, namespace)` 承载现逻辑（`:94` 的 `find` 与 `:97` 的错误文案 `"OpenCode provider missing"` **一字不动**，只是比较值换成 `namespace`）；`free_models(providers)` 保留为 `free_models_in(providers, OPENCODE_NAMESPACE)` 的**包装** | `Backend::models()`（`:959`）与两处单测（`:1792`/`:1804`）继续调包装，签名与输出都不变；`js_parity` 的 backend 组因此**不用改一行** |
| `backend.rs:1258-1269` 请求 payload | `providerID: "opencode"` + `modelID: id.get("opencode/".len()..)`（**定长截前 9 字节**） | `let (ns, model_id) = split_namespace(request.model()["id"])` → `providerID: ns`、`modelID: model_id`；id 缺失/为空时回落 `(OPENCODE_NAMESPACE, "")` | `free_models` 是这些 id 的**唯一生产者**（`"{ns}/{key}"`），`split_once` 与「截 9 字节」在 `ns == "opencode"` 时逐个等价（含 key 自带 `/`：`opencode/a/b` 两种算法都给 `a/b`）；唯一分歧是**不带命名空间**的 id，而那条路径不可达 —— 用单测把 `TRANSLATOR_ORDER` 四项（`orchestration.rs:74-77`）与夹具里出现过的每个 id 都断言「新旧算法同值」，把不可达变成有证据 |
| `model_status.rs:119` `client_model_id(model)` | `format!("OC · {}", display(model["name"]))` | **保持单参签名**（`js_parity.rs:732` 直接调它，改元参会惊动对拍台）；内部按 `model["id"]` 的命名空间取前缀：**只有 `providers::PROVIDERS` 里的平台换 `label`，其余（`opencode`、无 id、无 `/`、以及 `vendor/…` 这类合成命名空间）一律 `OC`** | 现有夹具、单测与合成入参的前缀恒为 `OC`，输出逐字节不变 |
| ~~未知命名空间怎么处理~~ | 本表初稿写「未知命名空间**原样透出**作前缀，不得伪装成 OC」 | 🔴 **落地时作废**：该规则会把 `sync.rs` 4 个既有测试（及对拍夹具）里的 `vendor/gpt` 从 `OC · gpt` 改成 `vendor · gpt`，即**真实行为变化**。首轮实现照抄本表立刻全红，现收紧为「只认注册表」。依据：`free_models_in` 之外不存在未知命名空间（Stage 3 的调用面只有 `OPENCODE_NAMESPACE` 与注册表 id），「原样透出」既不可达又有害 | 教训记入 `AGENTS.md` 状态表与 `docs/validation.md` |
| `sync.rs:406 model_entry` | `id`/`name` = 传入的 client id；`vendor` 硬编码 `"Custom"` | **本阶段不动**：它已经参数化（`client_id` 由 `sync.rs:327` 传入），Stage 3 改 `client_model_id` 输出后自动跟随 | 键顺序**不得重排**（整篇 `models.json` 逐字节对拍）；`vendor: "Custom"` 是 WorkBuddy 侧形态值、与 provider 无关，改它属非兼容变更 |

**明确不做**：不把 `opencode` 加进 `providers::PROVIDERS`（那张表的语义是「需要用户 Key 的平台」，OpenCode 免费路径不需要；加进去会让 `provider-status` 冒出一个恒为「未配置」的假平台，并打破 `red_lines` 的四元 id 集合）；不改 `orchestration.rs:74-77` 的 `TRANSLATOR_ORDER` 字面量（它们本就是 `opencode/…`）。

🔴 **一颗必须记着的雷（属 Stage 5，不在本阶段）**：`src/components/ModelRow.vue:52` 显示的是**模板里硬写的** `OC · {{ model.name }}`，而不是状态里真正的 client id（面板其余地方一律用 `model.id`，而 `model.id` 本身就已经是 `OC · 名称`）。今天两者恰好一致所以看不出问题；**Stage 3 一落地第二家，这一行就会给非 OpenCode 模型贴上错误的 `OC · `**。Stage 5 改法：直接渲染 `model.id`，删掉模板里的硬编码前缀。

验证协议（Stage 2 出口，全部实跑并如实报数）：`cargo test`（核心，**218 应保持不变**，其中 `js_parity` 11 全绿即证明输出未漂移）→ `cargo clippy --all-targets` 0 warning → `git diff --stat src-tauri/core/tests/fixtures` 为空 → `npm run version:check`（**不推进版本号**：等价移植属六种禁止情形之一）。

✅ **出口实测（2026-10-03）**：核心 `cargo test` **222 通过 / 0 失败**（lib **201** + js_parity **11** + red_lines **10**）；218→222 的 **+4 全部是本阶段新增单测**（`model_status.rs` 2 个、`backend.rs` 2 个），**没有改动任何夹具**——`git diff --stat src-tauri/core/tests/fixtures` 输出为空，js_parity 11 项全绿，逐字节对拍因此成立。壳 `cargo test --lib` **9 通过**；核心 `cargo clippy --all-targets` 与壳 `cargo clippy --no-deps --all-targets` 均 **0 warning**。唯一与本表不符的是上划掉的「未知命名空间原样透出」那格，实际落地按「只认注册表」收紧（见该行的 🔴 说明与 `docs/validation.md`）。

### Stage 3｜provider 声明注入 + `free_models` 注册表驱动

- `isolated_config()` 增加 `provider` 段：对**已配置凭据**的平台注入 `{ npm, options: { baseURL, apiKey } }`；未配置的平台**整段不出现**（不是注入空 Key）。
- `backend.rs:88-97` 现在硬编码 `all.iter().find(|item| item.get("id") == Some(&json!("opencode")))`，缺失即 `Err("OpenCode provider missing")`。改为按注册表遍历所有已配置平台；`opencode` 那条分支的既有逻辑、判定与错误文案**一字不动**（回归保护，也是你日常真正在用的 8 个模型）。

### Stage 4｜注册表模型清单（取代原 `Declared` 白名单方案）

按 §2.3 的简化：四家都要自己写声明段，所以**注册表里的 `models` 清单就是放行范围**，`CostZero` / `Declared` 的二分取消。

- 清单落在 `providers.rs` 的 Rust 常量表（单一事实来源，随版本发布走，**不做远程拉取**）。
- 🔴 **风险方向锁定为「漏不放行」**：上游新增免费模型不会自动出现（要改注册表发版）；被列进来的模型若悄悄开始收费，我们拿不到信号。仅自用场景下「漏」可以随时补，「误放行」会直接烧维护者账号的钱。
- 因此必须配套：面板文案保留「额度与可用性由平台决定，本应用不缓存额度」；上游返回的额度/计费类错误要能在面板与 `status.json` 里看出来（不得吞成通用失败）。
- 每家各一轮探测会消耗其免费额度，探测范围严格限于**已配置且已列入注册表**的模型。

### Stage 5｜发布闸门 + 探测收敛 + 面板 + 文档

- 只探测**已配置平台**的候选模型；单平台失败不得丢其他平台结果。
- 🔴 **空发布集闸门**（从初稿保留，且多平台后由「可选」变「必须」）：`orchestration.rs:1044`（探测收尾 `auto_import`）与 `:1915`（导入动作）走 `sync_published(None)` → `usable_models()`，而唯一线上同步点 `:609` 硬编码 `SyncOptions { allow_empty: true }`，`sync.rs:302` 的保护只在 `sync.rs:901` 测试里生效 → 一轮全败就会清空 `models.json` 中本工具全部条目。改法：给 `sync_published` 加显式「是否允许空集」入参，**故障致空拒绝发布并写 `sync.error`**（`:1915` 那条路径已有 `error` → 返回 `Err` → 面板 `FeedbackBar` 的现成链路），`:1356`（关停）/`:1682`（启动清旧）/`:1893`（换文件）三处**用户意图**清空维持 `true`。单测同时钉 `models` 与 `availableModels`。
- 面板新增「平台」视图（`VIEW_IDS` / 侧栏 `groups` / `App.vue` 的 `v-if` 三处同步），状态为「已配置 / 未配置」；**明文与掩码都不回显**（Stage 1 的 `provider-status` 已经只回 `configured`，面板沿用该形态）。
- 文档：`AGENTS.md` 凭据允许位与红线增补、`README.md`、`docs/wiki/*`、`docs/contract.md`（唯一无自动校验的一致性落点，人工核对）。
- 版本：`npm run version:set -- 1.0.3`（5 处落点）+ `docs/version/RELEASE-NOTES-v1.0.3.md` + `RELEASE-v1.0.md` 新分节 + `docs/version/README.md` 索引。

---

## 4. 验收标准

1. **零凭据路径不退化**：不录入任何 Key 时，行为与今天**逐字节一致**（既有 `OC · ` 模型照常发布、无新报错、无空列表、界面上「平台」视图只显示「未配置」）。
2. 四家逐一：录入 Key → 面板出现 `<label> · ` 模型 → 探测通过 → 导入 → **在 WorkBuddy 里真跑完一次对话**。
3. Key 填错 / 平台挂了：该平台不出现模型、不报错、不影响 `OC · ` 与其他平台。
4. 多平台同名模型共存，无静默丢失（`merge_models` 的同名丢弃只在真同名时发生）。
5. `providers.json` 不出现在日志、`status.json`、`git status`、任何 IPC 明文返回中。
6. 核心 `cargo test` 与 `red_lines` 全绿（**Stage 1 后基线已上移为 218 = lib 197 + `js_parity` 11 + `red_lines` 10**，后续 Stage 每加一项同步更新本文件与 AGENTS.md 的基线行）；`cargo clippy --all-targets` 0 warning；`npm run test:prefs` / `test:manifest` / `test:updater-key` 合计 30 通过；`npx eslint .` 0 problem；`npm run vite:build` 通过；`npm run version:check` 通过。
7. 装 1.0.3 时按 `docs/qa/smoke-checklist.md` 跑一遍（本项目**首次** GUI 实机），结果如实回填并据此更新 AGENTS.md 状态表——不得用「编译通过」替代。

---

## 5. 风险与未验证项（不得伪装成已验证）

| 风险 | 处置 |
|---|---|
| 🔴 `providers.all[]` 是否随注入的 `provider.<id>` 声明段出现该平台与其 `models` —— 本仓库无证据，且 OpenCode 文档的 provider 目录页未列这四家 | Stage 0 先证，证不过就摘平台，不硬做 |
| ~~🔴 `js_parity` 夹具重录依赖仓库外 JS 归档，该路径本机不可解析~~ → **已消解**（2026-10-03 定稿 Stage 2 改法后） | Stage 2 的输出对 OpenCode 路径必须逐字节不变，夹具因此是**证明**而非**障碍**：本阶段一行 `expected` 都不改，出口以 `git diff --stat src-tauri/core/tests/fixtures` 为空为硬线；新平台行为只写 Rust 单测，**严禁**手工往 fixtures 加 `expected`（那等于伪造 JS 真相） |
| `ModelRow.vue:52` 模板里硬写 `OC · {{ model.name }}`（与 `model.id` 今天恰好同值所以看不出来） | Stage 3 落地第二家后这行会贴错前缀 → Stage 5 改为直接渲染 `model.id`；在此之前它是**已记录、未修**的已知偏差，不得当作已解决 |
| 凭据进入子进程环境变量（`OPENCODE_CONFIG_CONTENT` 通道） | 已由设计稿定型、D5「仅自用」下接受；保留「写 `0600` 文件、凭据不进环境」为切换预案 |
| 注册表模型清单腐化（上游悄悄开始收费，我们拿不到信号） | §Stage 4 的风险方向锁定为「漏不放行」；面板保留「额度由平台决定」文案，计费类错误不得吞成通用失败 |
| catalog 的 `cost` 字段是否仍被合并进注入后的 provider（影响能否退回 `CostZero` 判定） | Stage 0 第 3 问顺带取证；结论不改变 §2.3 的设计，只决定 `opencode` 之外是否还有第二条可用的判定路径 |
| 四家各一轮探测消耗额度 | 面板明示；只探测已配置平台 |
| GUI 从未实机启动（长期遗留） | 新增「平台」视图会进一步扩大未验证面 → Stage 0 之后、Stage 1 之前**建议先做一次最小 GUI 冒烟**，把遗留 ❌ 清掉再叠新 UI |

## 6. 明确不做

- 本地运行时 / Ollama 网关（D3，整条下架）。
- 百度千帆、讯飞星火**不进 1.0.3，但理由变了**：原以为是「唯一需要自定义声明的两家、风险要隔离」；实测后四家**都**要写声明段，所以它们的边际成本已降到「在注册表里加一行 id + baseURL + 模型清单」。**真正的阻塞变成别的事**：它们不在 models.dev 目录里，baseURL 与模型清单要靠各自官方文档人工核对，且没有 catalog 元数据可借。（注：若 Stage 0 证明确实必须显式给 `models`，那四家与这两家在这一点上完全同形，届时可 1.0.4 直接扩。）
- 内置维护者自有 Key、中转/代理服务端（撞红线 + 引入云依赖）。
- `freemodel` 这类在册但来源不明的第三方免费源（§2.1 附带发现）：接入门槛不是技术，是**可信度评估**，未经单独判断不得进注册表。
- `vendor` 字段按平台填充、`owned_by` 改动（WorkBuddy 侧依赖未知，保守不动）。
- OAuth 登录、非 OpenAI 兼容协议、额度查询统计、PAC/SOCKS 代理形态。
- Developer ID / 公证 / SmartScreen（D5）。

## 7. 下一步

现状（2026-10-03 本轮）：Stage 0 里我能自证的部分**已经做完**——四家的 provider id、`@ai-sdk/openai-compatible` 形态、真实 baseURL、各家模型数与「不在册的两家」都已实测确认，并据此取消了 `form` 分叉与 `CostZero/Declared` 二分（§2.1、§2.3）。剩下的是唯一一件只有你能做的事。

1. **维护者（阻塞项）**：在你自己的 OpenCode 上给四家中任一家注入 `provider.<id> = { npm, options.baseURL, options.apiKey }`（临时配置即可，不动本仓库），回给我四个答案：
   - `providers.all[]` 里是否出现该平台、id 拼写；
   - 该平台 `models` 的 key 形态（决定注册表怎么写放行清单）；
   - 一个模型的 `capabilities` / `status` / `cost` 实际取值（判断 catalog 是否仍合并标价）；
   - 该模型能不能真跑完一次对话。
   **只回这四类元数据，绝不回 Key 本身。**
2. ~~**我（不阻塞，可并行）**：Stage 1 的逐文件实施清单…~~ **已完成**：Stage 1 已于同日落地并自证（见上节 Stage 1 的实测数字与 `docs/validation.md`）。这一层与上面那个未知**无关**，因此可以先落；写进去的 Key 目前还没有消费者。
3. ~~Stage 2 的前置仍未解：`js_parity` 夹具重录依赖仓库外 JS 归档，本机不可解析 → **开工前必须先确认可行的改法**（见 §5 风险表首行）。~~ **前置已解除，Stage 2 同日落地**：实际改法是自上而下保证**输出逐字节不变**（新平台行为只由 `#[cfg(test)]` 单测覆盖），因此根本不需要重录夹具——`git diff --stat src-tauri/core/tests/fixtures` 为空 + js_parity 11 全绿就是证据。唯一偏离是「未知命名空间原样透出」那条（会真实改动 `sync.rs` 的 4 个既有测试），已收紧为「只认注册表」，详见 §5 风险表划掉的那一行。
4. Stage 3 仍**阻塞**在上面第 1 项：`isolated_config()` 要不要注入 `provider` 段、`free_models` 的注册表遍历按什么 id 匹配，都要等你回的那四类元数据。

