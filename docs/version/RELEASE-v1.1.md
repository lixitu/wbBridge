# 版本发布日志 · v1.1

> 本文件按主版本组织：v1.1.x 的全部迭代日志集中于此（最新在前）。
> 命名规则：`RELEASE-v{主版本}.md`；次版本迭代追加到文件顶部新分节。
> 版本纪律以根目录 `AGENTS.md`「当前基准版本」章节为准，本文件不另立规则。
> 整理规则：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本保留编号与合并指向，不再重复正文。
> 当前最新版本：**v1.1.2**（2026-10-09 由误推进的 `1.3.4` 回退统一而来；v1.1.0 不单独出版本号，内容并入本版，见下）。

---

## v1.1.2

> **状态**: 📝 待发布。**尚未构建任何安装包、尚未打标签**；五处落点已由 `npm run version:set -- 1.1.2` 统一为 `1.1.2`（`npm run version:check` 实测「全部 5 处版本号一致（1.1.2）」）。按铁律先提交、再在那一个提交上打 `v1.1.2`。
> **日期**: 2026-10-10
> **上一版本**: v1.1.0（代码已提交 `d8984a0`，**从未构建、从未打标签**）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.1.2.md`](RELEASE-NOTES-v1.1.2.md)（2026-10-10 已出稿；底本为 [`RELEASE-NOTES-v1.1.0.md`](RELEASE-NOTES-v1.1.0.md)，标题与资产名已改写为 1.1.2，并追加七项改动的完整清单）
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

### 二、本版内容（v1.1.0 之后并入的七项改动）

1. **modelscope 锚点修复**：注入声明段补 `models: { "wbbridge-provider-anchor": {} }`，消除 `has no provider supported`（原记 1.1.2）。
2. **重新检测全程「检测中」+ 面板反馈/节流补齐**：核心 `singleProbes` 顶层状态与在飞集合；`ModelRow` 行内忙态、`pendingText` 区分、FeedbackBar 全视图可见（原记 1.2.0）。
3. **启动沿用 + 定向重检 + 不再写插件配置备份**：`/admin/probe` 载荷扩 `{ model?, providers? }`、按平台范围重取重检、启动沿用上次结果；`sync.rs` 的备份区块先改为「只留最新一份」（原记 1.3.0），最终按用户裁定改为**完全不写 `.bak`**、每次同步（含无变化那次）清扫旧版攒下的同族存量备份（`sweep_old_backups`），`SyncOutcome.backup` 与 `sync.targets.*.backup` 一并移除。
4. **地区不可用的上游拒绝改中文说明**（原记 1.3.1）：`probe.rs::region_unavailable_message`。
5. **「不支持函数调用」改走 chatOnly 降级**（原记 1.3.2）：`probe.rs::tool_call_unsupported`。
6. **提供方撤架模型的上游失败改中文说明**（原记 1.3.3）+ **旧版攒下的插件配置备份在「无变化同步」里也收敛**（原记 1.3.4）。
7. **插件目录已在、但 `models.json` 缺失时主动补建**（2026-10-10）：`~/.workbuddy` 或 `~/.codebuddy` 目录存在而没有 `models.json` 时，按空数组配置补建一个（此前这种情况一律判「未检测到」，装了插件却永远等不到发布）。补建只走**默认发现位置**、只新建不改写、目录不存在时不替插件建目录、显式指定的位置失效仍照旧报未检测到。新增 4 项单测。

### 三、验证

沿用各轮已实测结论（详见 `AGENTS.md`「当前状态与验证边界」各轮条目与 `docs/validation.md`）：核心 `cargo test`、两侧 `clippy` 0 warning、壳 `cargo test --lib` 9 通过、JS 四组套件、`npx eslint .`、`vite:build`、`version:check` **5 处一致（1.1.2）**。本轮回退本身**只改版本号与文档**，未触碰代码；随后并入的第 7 条（空插件目录补建 `models.json`）是代码改动，已实测：核心 `cargo test` **263 通过 / 0 失败**（lib 241 + `js_parity` 11 + `red_lines` 11，新增 4 项）、核心 `cargo clippy --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**、JS 四组套件 **8 / 11 / 9 / 13**、`npx eslint .` **0 problem**、`npm run vite:build` 通过、`version:check` **5 处一致（1.1.2）**。

### 四、未验证（不得伪装）

- ❌ 补建出来的 `models.json` 能否被插件自身正确读入（按既有语义写成空数组 `[]`，插件若只认 `{"models":[]}` 对象形态则可能读不出；文件原本不存在，无从得知插件偏好）。
- ❌ v1.1.2 安装包（本机与 CI）尚未构建；真实升级闭环仍未验证。
- ❌ GUI 实机（平台视图保存 Key、重新检测忙态、重启后沿用上次列表）。
- ❌ 真实平台 Key 的端到端（探测通过 / 真实对话 / 发布）。

---

## v1.1.0

> **状态**:。五处落点已由 `npm run version:set -- 1.1.0` 统一为 `1.1.0`（`npm run version:check` 实测「全部 5 处版本号一致（1.1.0）」），但**尚未构建任何安装包、尚未打标签**，且版本落点提交仍未执行——按铁律先提交、再在那一个提交上打 `v1.1.0`。
> ⚠ **覆盖提示**：本分节的 `1.1.0` 落点已被 2026-10-09 的回退覆盖，仓库当前落点为 **1.1.2**；本版内容并入 v1.1.2 一起发布，**不再单独出 `v1.1.0` 版本号**。
> **日期**: 2026-10-09
> **上一版本**: v1.0.5（五处落点已一致、尚未打标签）
> **GitHub Release 正文**: [`RELEASE-NOTES-v1.1.0.md`](RELEASE-NOTES-v1.1.0.md)
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

