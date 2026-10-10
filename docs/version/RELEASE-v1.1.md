# 版本发布日志 · v1.1

> 本文件按主版本组织：v1.1.x 的全部迭代日志集中于此（最新在前）。
> 命名规则：`RELEASE-v{主版本}.md`；次版本迭代追加到文件顶部新分节。
> 版本纪律以根目录 `AGENTS.md`「当前基准版本」章节为准，本文件不另立规则。
> 整理规则：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本保留编号与合并指向，不再重复正文。
> 当前最新版本：**v1.1.12**（📝 待发布，2026-10-10 本轮全量代码审查的产出；**当前最新已发布**版本是 **v1.1.10**，见下节实测状态）。

---

## v1.1.12

> **状态**: 📝 **待发布**——尚未构建安装包、尚未打标签。七处落点实测一致为 `1.1.12`（`npm run version:check` 输出「全部 7 处版本号一致（1.1.12）」，两侧 `Cargo.lock` 由 cargo 同步）。
> **日期**: 2026-10-10
> **上一版本**: **v1.1.10**（已发布，2026-10-10 实测：Release `draft = false`、23 个资产、线上清单六条 url 逐条可取回）。
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.1.12.md`](RELEASE-NOTES-v1.1.12.md)（本轮出稿，含固定小节「macOS 首次打开（ad-hoc 签名放行）」，该小节自 v1.1.10 逐字沿用）。⚠ 上一版底本 `RELEASE-NOTES-v1.1.10.md` 本轮**未删**（它是已 Publish 线上正文唯一可在仓库里逐字对账的那份），但**随后被同一工作树的并行轮次删除**（`git status` 实读 ` D`，尚未提交；`git show HEAD:docs/version/RELEASE-NOTES-v1.1.10.md` 可取回）。恢复或删除都属维护者决定，本轮不代做。
> **本版主题**: **一轮全量代码审查（Rust 核心 + 壳 + Vue 面板）的缺陷修复**，不含新功能——断连泄漏在途会话、管理接口缺请求体预算、会话 id 空串退化，加上面板一批对比度 / 键盘 / 忙态 / ARIA 问题。
> **版本推进理由**: 缺陷修复 = patch 位。`1.1.10 → 1.1.11 → 1.1.12` **属同一轮**（`1.1.11` 是该轮中途的首次推进，从未构建、从未打标签，其内容与 `1.1.12` 是同一批改动，不是两个批次）。⚠ 如实记录：本轮期间一个并行轮次曾把落点推进到 **`1.2.0`**（其 WB.auto 智能路由**只有设计稿、无任何代码落地**——`src-tauri/core/src/auto.rs` 不存在，全仓 grep `WB.auto` / `auto_select` 无命中），已按维护者裁定回退；`1.2.x~1.3.x` 从未构建、从未打标签，**不得再占用**。

### 一、版本号落点（`npm run version:check` 本轮实测：7 处一致 = 1.1.12）

| 位置 | 值 |
|---|---|
| 根 `package.json` → `version`（单一来源） | **1.1.12** |
| `src-tauri/tauri.conf.json` → `version` | **1.1.12** |
| `src-tauri/Cargo.toml` → `[package] version` | **1.1.12** |
| `src-tauri/Cargo.lock` → `wbbridge` / `wbbridge-core` | **1.1.12** |
| `src-tauri/core/Cargo.toml` → `[package] version` | **1.1.12** |
| `src-tauri/core/Cargo.lock` → `wbbridge-core` | **1.1.12** |
| `AGENTS.md`「当前基准版本」三行 | **1.1.12** |

> `status.json` 的 `0.2.0` 仍是历史沿革值，未动。

### 二、本版内容

**A. 核心三处缺陷修复**

1. **客户端断开后泄漏在途会话与权限轮询任务**（`backend.rs`）。一次生成期间核心除了转发内容还会**每 250ms 轮询**该会话的原生工具待审批状态；清理原先写在生成函数的正常/错误返回路径上，而 **hyper 在客户端断开时直接丢弃整个 handler future**，那条清理一行都不执行。结果是每次取消（WorkBuddy 点停止、切会话、超时——日常操作）留下一个永不结束的轮询任务 + 一份会话状态（`active` / `usage_by_session` / 待审批条目），活动流也停不掉。修复形态：新增 `CompleteGuard`（`backend.rs:371`，`impl Drop` 在 `:381`）作为**销毁即清理**的守卫，构造点必须在会话已入表、守卫与 watch 都已建立之后（`:1378` 附近）；清理幂等（内联 finally 用 `finished` 标记占位，Drop 见位即返回）；异步收尾经 `Handle::try_current()` 在当前运行时派发，拿不到运行时的理论路径只做同步清理。
2. **`/admin/*` 补齐 20s 请求体读取预算**（`server.rs`）。`REQUEST_BODY_TIMEOUT` 原先只挂在 chat 路径外面那一层，五个管理调用点没有它；而管理路由**不受 ≤4 并发限制**，一个只发头不发体的连接能无限期占住任务与缓冲。收拢为 `read_body(body)` → `read_body_bounded(body, REQUEST_BODY_TIMEOUT)`（`:1203`/`:1210`），一处生效、五个调用点未动；超时映射 408 + `code = "timeout"`。
3. **OpenCode 会话响应无 id 时当场失败**（`backend.rs:1295` 附近）。原先 `.unwrap_or("")` 让空串成为一把**共享键**：并发请求在 `active` / `usage_by_session` 里落进同一条目互相干扰，DELETE 退化成 `/session/`。现用 `.filter(|id| !id.is_empty())` + 明确错误。

**B. 面板无障碍与反馈七项**（编号 4~10，数值来源为 `src/styles/variables.css` 的 token 实算，非目测）

4. `--orange` 浅色 `#bc752c` → **`#a4581a`**（兼作 13px 错误正文，原先在 `--orange-bg` 上 3.5:1 < 4.5:1；暗色 `#e9ad70` 已达标未动）。
5. `--switch-off` `#aab2ae` → **`#7f8a86`**（可交互控件按 WCAG 1.4.11 需 ≥3:1，同时保住白滑块与轨道的分界）。
6. **十处 11–13px 说明文字** `--muted` → `--muted-strong`：`App.vue` 副标题与页脚、`ModelRow.vue` 耗时、`ModelList.vue` 空态、`MetricsBar.vue` 说明、`ModelDetails.vue` 引导语、`ProvidersView.vue` 的 id / 申请页网址 / 提示、`AboutView.vue` 版本说明。**刻意未改**：`MetricsBar` 的「空闲」（22px 大字号，按 3:1 已过）、`SideBar` 的装饰性色底。
7. `App.vue::onKeydown` 的 `Esc` 对 `INPUT` / `TEXTAREA` / `isContentEditable` 放行——平台视图 Key 输入框里的 Esc 属于该控件自己的语义，不该被「收起详情」抢走。
8. `ProvidersView.vue`：`applyAll` 的成功分支原先**只算文案、不接状态**（成功横幅永不出现也永不消隐），现走 `noteSuccess('refresh', …)` 与其它动作对齐；该函数原先还遮蔽了 `bridge.js` 的 `action`，已改名。另补 `onUnmounted` 清掉待触发的消隐定时器（否则 5s 后往已卸载组件的 ref 上写状态）。
9. `ServiceStatus.vue` 新增 `retrying` prop、`IntegrationView.vue` 新增 `importing` prop：原先任何动作在飞都让这两个按钮显忙态（点「刷新」会把「导入」也转起来），现各自判 `busyAction === 'restart'` / `'import'`，并补 `aria-busy`。
10. `ModelList.vue` 的 `role="listbox"` 改为**列表非空时才挂**——listbox 的直接子元素必须是 option，而空态那句「进行中」说明是 `role="status"`。

### 三、本轮新增单测（3 项）

| 测试 | 位置 | 钉住的行为 |
|---|---|---|
| `abandoned_request_stops_permission_polling_and_clears_session_state` | `backend.rs` | handler future 被丢弃时权限轮询停止、会话状态与用量条目被清除、活动流收尾 |
| `complete_refuses_a_session_response_without_an_id` | `backend.rs` | 会话响应无 id 即返回错误，且**不**在 `active` / `usage_by_session` 落下空串键 |
| `read_body_applies_the_timeout_to_a_body_that_never_finishes` | `server.rs` | 永不结束的请求体被读取预算掐成 408 + `timeout`；另断言 `REQUEST_BODY_TIMEOUT == 20s` |

> ⚠ 三项都以「替换实现」跑在注入的假 OpenCode 服务上（`start_mock_session` 新增 `session` 入参正是为第二项服务），**不联网**。
> 🔴 **更正一条上一版写错的说明**：此前这里写「第一项的必要性由负向对照确立（撤掉守卫则断言失败）」——**该负向对照从未执行过**。本轮两次尝试临时撤掉守卫（`std::mem::forget` / 改动构造行）都**被工具的安全分类器拦下**（不得在无用户确认时临时回退一处修复），因此**没有任何「撤掉修复即失败」的实测证据**。第一项的依据只有两条，如实界定：① 可读源码的事实——手写 `finally` 位于 `complete()` 末尾，future 被丢弃后那一行都跑不到；② 测试本身直接构造了被弃事件（把 `complete()` 的 future 驱动到「已建会话、监视 task 已在轮询」之后 `drop(future)`，与 hyper 断开时的行为等价），随后断言 `GET /permission` 计数不再增长、abort 与 DELETE 各恰好一次、`usage_by_session` / `active` 无残留。第二项同理按解析式论证核实。**若需要真正的变异验证，请用户明确授权临时回退，本轮不擅自做。**
> ⚠ `tokio` 的 `start_paused` 需要 `test-util` feature（本 workspace 未启），因此第二/三项用**注入预算参数**（50ms）而非自动推进时钟——**不为此改依赖 feature**。

### 四、验证（本轮真实执行）

- 核心 `cargo test` **283 通过 / 0 失败**（lib **261** + `js_parity` 11 + `red_lines` 11）
- ⚠ **基线更正**：文档多处记作「278 = lib 256」，而按测试属性逐个数，HEAD 实有 **258** 个 lib 测试——本轮**之前**就少记 2 个（先前补测试时没回头改基线行）。本轮真实增量是 3。
- 核心 `cargo clippy --all-targets` **0 warning**（touch 后重检）、壳 `cargo clippy --no-deps --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**
- JS 四组 **8 / 11 / 9 / 13**、`npx eslint .` **0 problem**、`npm run vite:build` 通过
- `git diff --stat src-tauri/core/tests/fixtures` **为空**
- `npm run version:check` **7 处一致（1.1.12）**

### 五、未验证（不得伪装）

- 🔴 **GUI 一次都没启动**：4~10 项面板改动全部只有 eslint / vite:build / 单测层证据。
- 🔴 **断连清理的真实效果**：未与真实 WorkBuddy 取消对拍，修复前后的残留任务数没有实测对比。
- 🔴 **管理接口 20s 预算**：未实测半开发连接，也没有证据排除「某个合法管理请求会撞上 20 秒」（请求体都很小，判断是不会，但那是判断不是测量）。
- 🔴 **审查中提出而本轮未落地的 9 项**（A~I：`NODE_TLS_REJECT_UNAUTHORIZED=0`、async 路径上的阻塞 IO、`Backend::complete` 无绝对 deadline、chatOnly 兜底未共用 60s deadline、退出钩子代次竞态、退出链路阻塞事件循环、`capabilities` 里未用的窗口权限、CI 只跑核心测试、`gen-latest-json` 不校验 `.sig` 内容）**逐条列在 `docs/validation.md` 本轮条目里**，属待维护者裁定项，未悄悄丢弃也未顺手改。

---

## v1.1.10

> **状态**: ✅ **已发布**（2026-10-10 由 GitHub API 与线上清单实测，非取自既有记录）：远端存在 `refs/tags/v1.1.10`（= `befe96e7`，Merge PR #9；本地 `dev` 的 `441e4c3` 是其父提交，版本落点确实进入发布物），Release `v1.1.10` 已 Publish（`draft = false`，`published_at = 2026-10-10T04:10:03Z`）、**23 个资产**（六平台安装包 + 各自 `.sig` + mac 两个 `WB.Bridge_<arch>.app.tar.gz(+.sig)` + `latest.json`）；线上清单 `version` = `1.1.10`，六条 `url` 逐条只取首字节探测**全部 206**。七处落点实测一致为 `1.1.10`（`npm run version:check` 输出「全部 7 处版本号一致（1.1.10）」），代码与文档落点已提交（`441e4c3`）。🔴 **不得据此说成「已交付」**：六个包没有一个在实机装过，一次真实的应用内升级闭环从未走过；线上 Release 正文是否已替换成本地这份底本仍属人工核对项。
> **日期**: 2026-10-10
> **上一版本**: **v1.1.0**（上一个实际 Publish **且带 23 个资产**的 Release，2026-10-09）。🔴 与本版以下既有分节的表述**不符，以实测为准**：`api.github.com/repos/trexwb/wbBridge/releases` 查到 **v1.0.3（2026-10-03）、v1.0.5（2026-10-09）、v1.1.0（2026-10-09）三个 Release 均已 Publish、各带 23 个资产**，而 `v1.1.0` / `v1.1.2` 两节都写着「从未构建、从未打标签」。**v1.1.2 ~ v1.1.9 确实从未打标签**，本版把它们的内容一次性带上。
> **GitHub Release 正文**: 底本 `RELEASE-NOTES-v1.1.10.md`（2026-10-10 出稿并随 `441e4c3` 入库；同提交**删除**了未发布的草稿 `RELEASE-NOTES-v1.1.2.md`，正文以本版为准）。⚠ 该文件**现已不在工作区**（被后续并行轮次删除，尚未提交），历史内容取 `git show HEAD:docs/version/RELEASE-NOTES-v1.1.10.md`
> **本版主题**: 平台免费模型「真正可用」——ModelScope 的模型清单从 models.dev 的目录声明改为对方网关实际在册的清单；探测失败的上游原文再补两类可读中文说明（含「这把 Key 没开通」与「条目不在册」的区分）；外加 v1.1.5 的启动超时重试与构建占用、v1.1.2 并入的八项
> **版本推进理由**: v1.1.2 之后各轮按「缺陷修复（同一模块的追加修复）= patch」推进至 `1.1.5`（启动链路）、`1.1.6 ~ 1.1.9`（ModelScope 权威清单，属新增能力面但沿用未发布版本内的补丁位）、`1.1.10`（`unknown_service_id_message` 按措辞分两支 + 真实 Key 实测后下架 4 条打不通的条目）。⚠ 如实记录：**逐轮与版本号的对应关系仓库内无落点记录**（`AGENTS.md`「当前状态与验证边界」与 `docs/validation.md` 都没有 1.1.3~1.1.9 的条目，`docs/version/README.md` 索引仍停在 v1.1.2），本节按「相对 v1.1.0 已发布内容的净变化」记述。

### 一、版本号落点（`npm run version:check` 本轮实测：7 处一致 = 1.1.10）

| 位置 | 值 |
|---|---|
| 根 `package.json` → `version`（单一来源） | **1.1.10** |
| `src-tauri/tauri.conf.json` → `version` | **1.1.10** |
| `src-tauri/Cargo.toml` → `[package] version` | **1.1.10** |
| `src-tauri/Cargo.lock` → `wbbridge` | **1.1.10** |
| `src-tauri/core/Cargo.toml` → `[package] version`（核心 crate，2026-10-10 起随产品版本同步） | **1.1.10** |
| `src-tauri/core/Cargo.lock` → `wbbridge-core` | **1.1.10** |
| `AGENTS.md`「当前基准版本」三行 | **1.1.10** |

> 面板 `__APP_VERSION__` 由 `vite.config.js` 构建期注入 `v1.1.10`，非独立落点；`status.json` 的 `0.2.0` 仍是历史沿革值，未动。

### 二、本版内容（v1.1.0 已发布之后的净变化）

**A. 并入 v1.1.2 分节的八项**（正文见下节 `## v1.1.2`，此处不重复）：modelscope 锚点修复、重新检测「检测中」+ 面板反馈/节流补齐、启动沿用 + 定向重检 + **完全不写插件配置备份**并清扫存量、地区拒绝中文说明、「不支持函数调用」改走 chatOnly 降级（含兜底同理由被拒时确认仅对话、放行被 `StructuredOutputError` 吞掉的原文）、撤架模型中文说明、空插件目录补建 `models.json`。该轮的 `src/core/ops.js` 操作守卫（成功文案 + 在飞互斥 + 完成冷却 + 拒绝节流）随本版一起发布。

**B. v1.1.5：启动链路两项**
1. **`opencode --version` 回读超时改为重试一次**（`runtime.rs`）：新增 `probe_version(program, run)` + `VERSION_PROBE_TIMEOUT = 15s`（预算未放宽，按用户裁定）+ `VERSION_PROBE_ATTEMPTS = 2` + `should_retry_version_probe`（**只有超时才重试**——退出码非 0 / 不可执行重试只会把启动耗时翻倍，候选照旧被 `find_runtime` 判不可用走下一个来源），两处一次性 `--version` 调用（`default_probe` 与 `start_backend` 回读）都改走它；`version_probe_timeout_message` 两次都超时后给中文说明。契约：迁移前 JS 的 `{program} timed out` **逐字保留在句首**，其后追加说明；`code`/`status`/`status.json` 形状不变，`STATUS_SCHEMA_VERSION` 保持 1。
2. **开发构建磁盘占用**（`src-tauri/Cargo.toml`、`src-tauri/core/Cargo.toml`）：两侧各加 `[profile.dev.package."*"] debug = false`，壳侧另加 `[profile.dev.package.wbbridge-core] debug = true`（核心是 path 依赖、非 workspace 成员，会被 `*` 命中，不显式保就丢掉全部调试帧）；`[profile.release]` 一字未动。本机一次性清理后 `target` 14.1G → 2.8G（`*.rcgu.o` 46,579 个/7.65G → 754 个/0.15G），已签名交付物逐个核对仍在。

**C. v1.1.6 ~ v1.1.9：ModelScope 按对方网关实际在册的清单发布**
1. `providers.rs`：`Provider` 新增 `models: &'static [DeclaredModel]`，新类型 `DeclaredModel{id,name,context,output,tool_call}`，新函数 `served_models(namespace)`（无注册表条目 → 空切片）。**只有 `modelscope` 带清单**，其余三家为空切片＝沿用 catalog。
2. `runtime.rs::providers_section_for`：无清单时放**锚点条目** `models: { ANCHOR_MODEL_KEY: {} }`（2026-10-09 六组沙箱对照实验的结论——对 catalog 在册 provider，声明段不带非空 `models` 键时该平台全部模型都在 ai-sdk 层报 `has no provider supported`；空对象又会把 catalog 合并清空、发现 0 模型）；有清单时**逐条声明** `{ name, limit: { context, output }, cost: { input: 0, output: 0 }, tool_call }`，其中 `limit` 两个键**必须同时给**（少一个整份配置被判 `ConfigInvalidError`，沙箱实测）。
3. `backend.rs::free_models_in`：`served` 非空时**只放行清单内的 id**（OpenCode 是把注入段与 catalog **合并**而非替换，不过滤就仍会把对方实际不承接的过期 id 送进探测队列），锚点条目按保留名过滤；空清单路径行为与之前**逐字节一致**。
4. 依据与口径：2026-10-10 实测该网关公开可读的 `/v1/models` 在册约 35 个 id，与 catalog 声明的 7 个「免费」id **交集为空**。筛选规则＝只留通用「文本生成」，排除多模态/图像生成/垂直专用（SQL、评审、医疗）与未声明任务的条目，同能力新快照不重复收。清单规模 7（目录）→ 13（网关在册）→ 9（见 D 条）。🔴 `cost: 0` 是**本工具的主动声明**（免费判定依赖它），依据是对方公开口径「每日提供 2000 次免费 API 调用额度」，**不是接口回读值**；上下文上限按「实测可用档再低一档」保守声明、输出取 1/8。

**D. v1.1.10：`unknown_service_id_message` 按措辞分两支 + 真实 Key 实测后下架 4 条**
1. `probe.rs::unknown_service_id_message` 原先只说一种事实（「提供方没把这个服务 id 挂在在线推理清单里」）。同一句式实测到第二种形态——ModelScope 对 `PaddlePaddle/ERNIE-4.5-*` 给 HTTP 401 `The model does not exist or you do not have access to it.`：条目**在**清单里，只是这把 Key 没被开通。锅与下一步都不同（该去申请开通，不是换模型），故按原文是否含「没有访问权」再分一支，输出「你的这把 Key 没有该模型的访问权限…请到对方的模型页确认开通状态」。`code`/`status` 仍逐字保留、仍不触发重试。收紧点：`The endpoint is unavailable, please retry later.`（临时不可用）不得被说成权限问题。
2. `providers.rs` ModelScope 清单 13 → **9**：真实 Key 实测削掉 4 条「在册但打不通」的——3 个 `PaddlePaddle/ERNIE-4.5-{0.3B,21B-A3B,300B-A47B}-PT`（401 无访问权，需先在对方控制台开通）与 `meituan-longcat/LongCat-Flash-Lite`（400 `Unsupported model (model=LongCat-Flash-Chat)`，网关把它映射到自己也不承接的变体）。留着只在面板上常驻红色行并重复烧额度，故直接从清单去掉；对方日后开放免费额度再加回（注册表随版本发布、不做远程拉取）。
3. 壳 `src-tauri/src/lib.rs` 的 `watch_status` 把 `ticks % 8 == 0` 改为 `ticks.is_multiple_of(8)`（clippy 等价改写，**无行为变化**）。

### 三、验证（本轮真实执行，数字来自当前 HEAD 工作树）

- 核心 `cargo test` **278 通过 / 0 失败**（lib **256** + `js_parity` 11 + `red_lines` 11）
- 核心 `cargo clippy --all-targets` **0 warning**（touch `src-tauri/core/src/lib.rs` 后重跑确认，避免缓存复用）
- `git diff --stat src-tauri/core/tests/fixtures` 相对 HEAD **为空**（对拍仍逐字节等价；`sync.json` 的有意分叉随 v1.1.2 那轮落库，非本轮改动）
- JS 四组套件 **8 / 11 / 9 / 13 通过、0 失败**（合计 41）；`npx eslint .` **0 problem**；`npm run vite:build` 通过
- `npm run version:check` **7 处一致（1.1.10）**
- 沙箱实测（2026-10-09，真实 OpenCode 1.18.35、与核心同款隔离方式）：`/provider` 无条件返回全部 226 家 catalog；各家 cost 字段随合并保留，CostZero 判定可复用；`OPENCODE_CONFIG_CONTENT` 通道下两个自定义 agent 可见；注入段缺 `limit` 任一键 → `ConfigInvalidError` 且 `all` 回空
- 用户以**真实 Key** 实测得出、本机型无法独立复现（本机出网被拦截/重定向）：catalog 的 7 个 ModelScope 免费 id 全部未承接；网关在册清单与 catalog 交集为空；`ERNIE-4.5-*-PT` 三条 401 无访问权、`LongCat-Flash-Lite` 400 映射不承接
- ✅ **壳侧门禁已补跑**（文档同步轮实测，覆盖上面那条「本轮未跑」的缺口）：`cargo clippy --no-deps --all-targets` **0 warning**（`touch src-tauri/src/lib.rs` 强制重检，不是复用缓存——正是这一轮把 HEAD 上真实存在的 `manual implementation of .is_multiple_of()` 定位出来并改成 `ticks.is_multiple_of(8)` 的），`cargo test --lib`（`src-tauri/`）**9 通过 / 0 失败**

### 四、未验证（不得伪装）

- ❌ **完整端到端闭环**：ModelScope 当前 9 条是否逐一探测通过、发布后在 WorkBuddy / CodeBuddy 里的真实对话与工具调用表现
- ❌ **GUI 实机**：平台视图保存 Key 后的定向热生效、重新检测行内忙态、重启后沿用上次列表、空插件目录补建后的首次真实写盘、`open_external` 打开系统浏览器
- ❌ 补建出来的 `models.json` 能否被插件自身正确读入（按既有语义写成空数组 `[]`，插件若只认 `{"models":[]}` 对象形态可能读不出）
- ❌ **交付未验证**：本版安装包**本机从未构建**；CI 已构建完成（run `38023106905`、head `befe96e7`、completed / success），Release `v1.1.10` 实测 **23 个资产**（六平台包 + 各自 `.sig` + mac 两个 `app.tar.gz(+.sig)` + `latest.json`），线上清单六条 url 逐条实测**全部 200**、六条签名的签名者 key ID 全 = `2B11F78BEA8A43F`（与配置 pubkey 一致）、trusted comment 的 `file:` / `version:` 与实际资产名和 1.1.10 逐条对得上。**但**「包在、清单通、签名者 key ID 对得上」**不等于交付已验证**：六个包没有一个在实机装过，一次真实的应用内升级闭环（客户端拉到包 → 装完 `relaunch()` 带新核心起来）从未走过
- ❌ **线上 Release 正文里没有本底本**（2026-10-10 实测，更正此前的说法）：`api.github.com` 取到的 `v1.1.10` body 只有 **204 字符**——就是 CI 硬编码的那句「WB Bridge — 让 WorkBuddy 使用 OpenCode 免费模型。」加 GitHub 自动生成的 changelog（PR #9 链接 + `v1.1.0...v1.1.10` 对比链接），**没有任何人工粘贴的发布说明**。此前本节写的「线上正文是出稿时的旧版（含待发布状态与指向已删除 `RELEASE-NOTES-v1.1.2.md` 的死链）」不成立，该死链只存在于**本地文件历史**里。若要把 `RELEASE-NOTES-v1.1.10.md` 呈现到 Release 页，需人工粘贴——**属共享状态变更，由用户操作**，Agent 不代改
- ❌ v1.0.2 那轮实测出的「资产名空格 vs 点」缺陷**已闭环**：脚本与 CI 对账闸门修好后，2026-10-10 的 `v1.1.10` 这轮把对账步骤真实跑在 CI 上（success），线上清单六条 url 实测全部 200；`releases/latest` 端点现在返回的是 v1.1.10 的清单，那份 v1.0.2 坏清单已不再被下发，**重传一事无必要**（v1.0.0 / v1.0.1 早于 updater 接线，无已交付客户端受害）
- ❌ 探测额度消耗（清单变化后候选数变了，每模型 60s 预算）需使用后观察；`[profile.dev]` 关依赖调试信息后的首次冷编译耗时与核心断点可用性，也要下一次 `npm run tauri:dev` 才是首次执行

---

## v1.1.2

> **状态**: 📝 待发布。**尚未构建任何安装包、尚未打标签**；五处落点已由 `npm run version:set -- 1.1.2` 统一为 `1.1.2`（`npm run version:check` 实测「全部 5 处版本号一致（1.1.2）」）。按铁律先提交、再在那一个提交上打 `v1.1.2`。
> **日期**: 2026-10-10
> **上一版本**: v1.1.0（代码提交 `d8984a0`。⚠ 本行原先写「**从未构建、从未打标签**」，2026-10-10 `api.github.com` 实测**不成立**：标签 `v1.1.0` 存在、Release 已 Publish、`published_at = 2026-10-09T10:00:33Z`、**23 个资产**；未打标签的是 `v1.1.2`，其内容并入 `v1.1.10`）
> **GitHub Release 正文**: 底本 `RELEASE-NOTES-v1.1.2.md` 已随 `441e4c3`（2026-10-10，v1.1.10 那一轮）删除，正文以 v1.1.10 那份为准（该文件同样已不在工作区，见上上条说明）；出稿时它以 `RELEASE-NOTES-v1.1.0.md` 为底本（标题与资产名改写为 1.1.2 + 追加七项改动清单），那份也已随 `215fa1b`（2026-10-10）删除
> **本版主题**: 多平台免费模型接入（v1.1.0 的内容）+ 之后七项改动的合并发布版（六轮补丁打磨 + 空插件目录补建 `models.json`）
> **版本回退理由**: 维护者裁定「**版本号推进不正确**」——v1.1.0 之后的六轮改动被逐轮推进成 `1.2.0` 与 `1.3.0~1.3.4`，而 v1.1.0 **从未构建、从未打标签**，那些改动都属同一未发布版本内的补丁与打磨，不应当占用 minor 位与连续 patch 位。`1.3.4 → 1.1.1（2026-10-10 随第 7 条优化推进为 1.1.2）` 回退后，`1.2.x~1.3.x` **不得再占用**。

### 一、版本号落点（`npm run version:set -- 1.1.2` + `npm run version:check` 实测）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.1.2** | 版本单一来源 |
| `src-tauri/tauri.conf.json` → `version` | **1.1.2** | 打包产物版本（安装包名随之为 `WB Bridge_1.1.2_*`）与更新清单 `version` 来源 |
| `src-tauri/Cargo.toml` → `[package] version` | **1.1.2** | 壳工程同步落点 |
| `src-tauri/Cargo.lock` → `wbbridge` | **1.1.2** | 由 `cargo metadata` 自动同步，须一并提交 |
| `AGENTS.md`「当前基准版本」两行 | **1.1.2** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.1.2` | `vite.config.js` 构建期注入（自带 `v` 前缀），非独立落点 |

### 二、本版内容（v1.1.0 之后并入的八项改动）

1. **modelscope 锚点修复**：注入声明段补 `models: { "wbbridge-provider-anchor": {} }`，消除 `has no provider supported`（原记 1.1.2）。
2. **重新检测全程「检测中」+ 面板反馈/节流补齐**：核心 `singleProbes` 顶层状态与在飞集合；`ModelRow` 行内忙态、`pendingText` 区分、FeedbackBar 全视图可见（原记 1.2.0）。
3. **启动沿用 + 定向重检 + 不再写插件配置备份**：`/admin/probe` 载荷扩 `{ model?, providers? }`、按平台范围重取重检、启动沿用上次结果；`sync.rs` 的备份区块先改为「只留最新一份」（原记 1.3.0），最终按用户裁定改为**完全不写 `.bak`**、每次同步（含无变化那次）清扫旧版攒下的同族存量备份（`sweep_old_backups`），`SyncOutcome.backup` 与 `sync.targets.*.backup` 一并移除。
4. **地区不可用的上游拒绝改中文说明**（原记 1.3.1）：`probe.rs::region_unavailable_message`。
5. **「不支持函数调用」改走 chatOnly 降级**（原记 1.3.2）：`probe.rs::tool_call_unsupported`。
6. **提供方撤架模型的上游失败改中文说明**（原记 1.3.3）+ **旧版攒下的插件配置备份在「无变化同步」里也收敛**（原记 1.3.4）。
7. **插件目录已在、但 `models.json` 缺失时主动补建**（2026-10-10）：`~/.workbuddy` 或 `~/.codebuddy` 目录存在而没有 `models.json` 时，按空数组配置补建一个（此前这种情况一律判「未检测到」，装了插件却永远等不到发布）。补建只走**默认发现位置**、只新建不改写、目录不存在时不替插件建目录、显式指定的位置失效仍照旧报未检测到。新增 4 项单测。
8. **OpenCode 的「不支持函数调用」模型确认仅对话而非判失败**（2026-10-10）：用户实测 OpenCode 部分模型检测也报 `Bad Request: Function call is not supported for this model.`。除既有 `tool_call_unsupported` → `chat_only_attempt` 降级外，补两处差集：① 兜底那次纯对话请求若也被网关以**同一理由**拒绝，说明 OpenCode 的 chat-only 通道对该模型仍走工具路由，主探测与兜底指向同一结论，仍按 `chat_only` 发布（不再把能对话的模型整个丢弃）；② `backend.rs` 的 `info.error` 处理曾对 `name == "StructuredOutputError"` 整段跳过（交给下游 decode/repair），OpenCode 把该错包成 `StructuredOutputError` 时会被吞成泛化「未返回信封」、`tool_call_unsupported` 永远拿不到原文，现已在「实质是不支持函数调用」时放行给降级逻辑。降级成功提示改为干净的「模型不支持函数调用，已按仅对话发布」，不再回显吓人的上游原文。新增 1 项单测（`chat_only_fallback_rejection_confirms_chat_only`）。

### 三、验证

沿用各轮已实测结论（详见 `AGENTS.md`「当前状态与验证边界」各轮条目与 `docs/validation.md`）：核心 `cargo test`、两侧 `clippy` 0 warning、壳 `cargo test --lib` 9 通过、JS 四组套件、`npx eslint .`、`vite:build`、`version:check` **5 处一致（1.1.2）**。本轮回退本身**只改版本号与文档**，未触碰代码；随后并入的第 7 条（空插件目录补建 `models.json`）是代码改动，已实测：核心 `cargo test` **263 通过 / 0 失败**（lib 241 + `js_parity` 11 + `red_lines` 11，新增 4 项）、核心 `cargo clippy --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**、JS 四组套件 **8 / 11 / 9 / 13**、`npx eslint .` **0 problem**、`npm run vite:build` 通过、`version:check` **5 处一致（1.1.2）**。第 8 条（OpenCode 仅对话确认）在 7 条基础上再新增 1 项单测，实测：核心 `cargo test` **264 通过 / 0 失败**（lib 242 + `js_parity` 11 + `red_lines` 11）、核心 `cargo clippy --all-targets` **0 warning**、`version:check` 5 处一致（1.1.2）。

### 四、未验证（不得伪装）

- ❌ 补建出来的 `models.json` 能否被插件自身正确读入（按既有语义写成空数组 `[]`，插件若只认 `{"models":[]}` 对象形态则可能读不出；文件原本不存在，无从得知插件偏好）。
- ❌ v1.1.2 安装包（本机与 CI）尚未构建；真实升级闭环仍未验证。
- ❌ GUI 实机（平台视图保存 Key、重新检测忙态、重启后沿用上次列表）。
- ❌ 真实平台 Key 的端到端（探测通过 / 真实对话 / 发布）。

---

## v1.1.0

> **状态**:。五处落点已由 `npm run version:set -- 1.1.0` 统一为 `1.1.0`（`npm run version:check` 实测「全部 5 处版本号一致（1.1.0）」），但**尚未构建任何安装包、尚未打标签**，且版本落点提交仍未执行——按铁律先提交、再在那一个提交上打 `v1.1.0`。
> ⚠ **覆盖提示（2026-10-10 按 `api.github.com` 实测更正）**：本行原先写「本分节的 `1.1.0` 落点已被 2026-10-09 的回退覆盖…本版内容并入 v1.1.2 一起发布，**不再单独出 `v1.1.0` 版本号**」——**不成立**。`v1.1.0` 确实打了标签并发过 Release（`draft = false`、`published_at = 2026-10-09T10:00:33Z`、23 个资产）。当天回退覆盖的是**其后的落点**（v1.1.0 之后六轮被误推进成 `1.2.0` / `1.3.0~1.3.4`，裁定作废并回退为 `1.1.1`→`1.1.2`），`1.1.2` 从未打标签，其内容最终随 `v1.1.10` 发布。
> **日期**: 2026-10-09
> **上一版本**: v1.0.5（五处落点已一致。⚠ 本行原先写「尚未打标签」，2026-10-10 `api.github.com` 实测**不成立**：标签 `v1.0.5` 存在、Release 已 Publish、`published_at = 2026-10-09T06:19:17Z`、**23 个资产**）
> **GitHub Release 正文**: 底本 `RELEASE-NOTES-v1.1.0.md` 已随 `215fa1b`（2026-10-10 整理，滚动单份底本）删除，正文以线上 Release 为准 → [Release v1.1.0](https://github.com/trexwb/wbBridge/releases/tag/v1.1.0)（`draft = false`、23 个资产）
> **本版主题**: 多平台免费模型接入——面板「平台」视图（自带 Key + 三步申请引导 + 系统浏览器直达官方申请页）、核心 Key 注入与多平台聚合发现、空发布集闸门、单平台失败隔离
> **版本推进理由**: 与 v1.0.5 **不同类、不同根因**——新增一个完整的功能维度（平台 Key 管理 + 多平台免费模型发现与发布 + 申请引导），构成用户可感知的新能力面；**维护者裁定按语义化版本推进 minor 位**（`npm run version:set -- 1.1.0`，1.0.6 → 1.1.0），不再占用 patch 位。

### 一、版本号落点（`npm run version:set -- 1.1.0` + `npm run version:check` 实测）

| 位置 | 值 | 说明 |
|---|---|---|
| 根 `package.json` → `version` | **1.1.0** | 版本单一来源 |
| `src-tauri/tauri.conf.json` → `version` | **1.1.0** | 打包产物版本（安装包名随之为 `WB Bridge_1.1.0_*`）与更新清单 `version` 来源 |
| `src-tauri/Cargo.toml` → `[package] version` | **1.1.0** | 壳工程同步落点（`src-tauri/Cargo.lock` 由 cargo 自动同步，须一并提交） |
| `AGENTS.md`「当前基准版本」两行 | **1.1.0** | 文档侧同步落点 |
| 面板 `__APP_VERSION__` | `v1.1.0` | `vite.config.js` 构建期注入（自带 `v` 前缀），非独立落点 |

### 二、本版内容（代码范围 `v1.0.5..HEAD`，未提交的工作区改动）

#### 1. 核心多平台接入（Stage 3）

- **`runtime.rs`**：`isolated_config(providers_section)` 增加声明段注入位（空段输出与接入前逐字节一致）；新增 `providers_section_for(data_dir)`——从 `providers.json` 读已配置平台构造 `{ npm, options: { baseURL, apiKey } }`；Key 只经 `OPENCODE_CONFIG_CONTENT` 进子进程，不进 `ENV_ALLOW`。
- **`backend.rs`**：`Inner.configured_providers`（只存 id，不含 Key）；`Backend::models()` 改聚合发现——`opencode` 判定与错误文案一字不动 + 逐个已配置平台 `free_models_in`（CostZero 判定复用），单平台失败只记日志不丢其他平台；`set_configured_providers` 注入。
- **`orchestration.rs`**：启动与刷新两条 `start_backend` 链路注入已配置平台集合；`sync_published` 加显式 `allow_empty` 入参——探测收尾 / 单模型通过 / chatOnly 降级 / 导入动作传 `false`（空集提前拒绝并写 `sync.error`，形状与 `aggregate_sync` 顶层 error 同形），关停 / 启动清旧 / 换文件三处用户意图清空传 `true`。

#### 2. 面板「平台」视图（Stage 5）+ 申请引导

- 新增 `src/views/ProvidersView.vue`：四家平台卡片**恒常渲染**（静态清单驱动，不依赖核心在线；`provider-status` 只刷新「已配置」徽章——修复了初版卡片随请求失败整页消失的缺陷）；Key 表单 password 型、保存后立即清空、不回显不掩码；**保存 / 清除后自动热生效**（触发 `/admin/refresh`：只重启隔离的 OpenCode 子进程——`start_backend` 每次启动重新读 `providers.json` 注入声明段，`run_refresh` 重新注入已配置平台集合——应用与 HTTP 服务不动），底部另有「读取免费模型（重新应用）」兜底按钮。
- 申请引导：每张卡片常开「申请 Key（三步）」指引 +「打开官方申请页 ↗」按钮，四家官方入口已核实（modelscope.cn/my/access/token、cloud.siliconflow.cn/account/ak、console.cloud.tencent.com/tokenhub/apikey、open.bigmodel.cn/usercenter/apikeys）。
- **`lib.rs` 新增 `open_external` 壳命令**（`generate_handler` 5 → 6）：系统浏览器打开 https 外链（macOS `open` / Windows `explorer` 直接传参 / Linux `xdg-open`），仅 https + RFC 3986 字符白名单——Tauri WebView 里 `target=_blank` 默认静默失败，外链必须经壳转发；应用自有命令不经 capability 授权。
- `bridge.js` 新增 `openExternal(url)` 只读封装；`prefs.js` 的 `VIEW_IDS` 加 `providers`；`SideBar.vue`「模型」组加入口；`App.vue` 接分支。
- `ModelRow.vue` 改渲染 `model.id`：多平台后前缀按平台 label（`ModelScope · …` / `智谱 · …`），顺带清掉 Stage 5 记录在案的模板硬编码 `OC · ` 已知偏差。

#### 3. 前置实测（2026-10-09，四轮 /tmp 沙箱对照实验）

用真实 OpenCode 1.18.35 按核心同款隔离方式实测：`/provider` 无条件返回全部 226 家 catalog（注入不是平台出现的前提，声明段只负责鉴权）；四家 cost 字段随 catalog 合并保留，CostZero 判定直接复用（免费模型 ModelScope 7 / SiliconFlow 3 / 腾讯 2 / 智谱 3）；最小声明段足够，**Stage 4 注册表模型清单不再需要**；`/provider` 不回显 apiKey；`OPENCODE_CONFIG_CONTENT` 通道下两个自定义 agent 可见。

### 三、验证（2026-10-09 本轮真实执行）

核心 `cargo test` **243 通过 / 0 失败**（lib 221 + `js_parity` 11 + `red_lines` 11，本版新增 5 项：runtime 注入语义 2 项、orchestration 闸门 / 聚合顺序 / 单平台失败隔离 3 项）、`git diff --stat src-tauri/core/tests/fixtures` 为空（对拍逐字节等价）、核心 `cargo clippy --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**、壳 clippy **0 warning**、JS 三组 **8 / 9 / 13**、`npx eslint .` 0 problem、`npm run vite:build` ✓、`npm run version:check` **5 处一致（1.1.0）**。

### 四、未验证（不得伪装）

- ❌ GUI 实机：平台视图渲染、Key 保存、`open_external` 打开系统浏览器、保存后自动热生效链路（`set-provider-key` → 面板自动 `refresh` → 隔离子进程重启 → 重新发现与探测）全部未实机点过。
- ❌ 真实 Key 端到端：录入真实 Key 后探测通过、在 WorkBuddy 里真实对话成功——假 Key 无法证明 `@ai-sdk/openai-compatible` 对四家 baseURL 的实际调用行为（尤其 zhipuai 的 `/api/paas/v4`）。
- ❌ 探测对已配置平台的额度消耗（新增约 15 个候选模型，每模型 60s 预算）需使用后观察。
- ❌ v1.1.0 安装包（本机与 CI）尚未构建；一次真实升级闭环仍未验证。

---

