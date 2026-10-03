# WB Bridge v1.0.3

> **GitHub Release 正文**（推送 `v1.0.3` 标签或手动运行 `release.yml` 时，可直接复制本文件内容作为 Release body）。
> 发布日期：待定 ｜ 上一版本：v1.0.2 ｜ 内部 crate `wbbridge-core` 版本仍为 `0.1.0`（与产品版本解耦，不随本次递增）

**本版主题：多平台接入第一阶段（BYOK 注册表 + 三条管理动作）+ 四项安全/数据红线修复 + 更新清单下载 URL 缺陷修复与 CI 对账闸门。**

---

> ## 🍎 macOS 用户请先看这条：首次打开若提示「已损坏」，执行下面这行命令
>
> 本应用为 **ad-hoc 签名、未做 Apple 公证**，macOS 首次打开（以及更新后再次替换镜像时）可能被 Gatekeeper 拦截。提示「**已损坏**」且你已确认安装包来源可信时，在「终端」执行：
>
> ```sh
> xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
> ```
>
> 若只是被拦截、未报「已损坏」：先尝试打开 → 「系统设置 → 隐私与安全性」→ 点「仍要打开」。原理与更多情形见下文「macOS 首次打开（ad-hoc 签名放行）」小节。

---

## ⚠ 发布状态（请如实阅读）

- ❌ **桌面 GUI 从未实机启动**（v1.0.2、v1.0.3 都一样）。本版新增的管理动作、命名空间参数化与四项修复，证据只有编译 / 单测 / 构建，**没有任何一项在真实窗口里点过**。
- ⚠ **多平台接入 Stage 1 只有核心 + 壳侧链路，端到端不产生模型**：写进 `providers.json` 的 Key **目前没有消费者**（注入隔离配置属 Stage 3），面板也**没有入口**（Stage 5）。所以现在按这三条动作提交 Key，**不会多发布一个模型**。
- ✅ **Stage 2（命名空间参数化）行为零变化**：`tests/js_parity.rs` 11 项全绿且 `git diff --stat src-tauri/core/tests/fixtures` 为空（夹具未改），即对外输出逐字节不变。
- ✅ **已实测（本轮真实跑数）**：核心 `cargo test` **227 通过 / 0 失败**（lib 205 + js_parity 11 + red_lines 11）、核心与壳 `cargo clippy` **0 warning**、壳 `cargo test --lib` **9 通过**、`npm run test:prefs` **8** / `test:manifest` **9** / `test:updater-key` **13** 全通过、`npx eslint .` **0 problem**、`npm run vite:build` ✓、`npm run version:check` **5 处一致（1.0.3）**。
- ❌ **v1.0.3 的安装包尚未构建**：本机没有 v1.0.3 的任何产物，CI 也未跑过 `v1.0.3` 标签。
- ❌ **一次真实的「检查 → 下载 → 安装 → 重启 → 首启」从未走过**（v1.0.2 起接线至今仍然没有）。
- ⚠ **本版新增的那道 CI 对账步骤本身还没在 CI 上实跑过**：「校验清单 url 指向的资产在 Release 上真的存在」只在本机拿线上那份坏清单与修正后的清单各跑一遍同段逻辑（前者 6/6 报错退出 1、后者 6/6 OK 退出 0）。
- ❌ **凭据侧未验证项**：真实 Key 从未在 GUI 输入过；`providers.json` 的 `0600` 只在 unix 断言过；上游 OpenCode 是否把注入的 `provider.<id>` 回显进 `providers.all[]` **仍未实测**（这是 Stage 3 能否复用免费模型发现链路的前提）。
- 🔴 **发布前置（当前工作区仍未提交）**：版本推进的 5 个落点（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`、`AGENTS.md`）与本轮文档更正**尚未提交**。按仓库铁律：**先提交版本落点，再在那一个提交上打 `v1.0.3` 标签**（v1.0.1 那次标签指错就是栽在顺序上）。推标签前先配好仓库级 Variables（`TAURI_SIGNING_PRIVATE_KEY` = 私钥**全文**、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 非空口令），并本地跑一次 `npm run tauri:build` 确认注入器打印「已注入内联签名私钥 → 公钥配对 OK（`2B11F78BEA8A43F`）」——否则整套 Rust 编译跑完才在打包那一步失败。

---

## 🧩 多平台接入 Stage 1：平台注册表 + 用户自持 Key 通道

这是「接入 WorkBuddy 之外的多家免费/低价模型上游」路线图的第一段。本段只铺**注册表**与**凭据通道**，不含注入与界面。

- **注册表随版本发布，不做远程拉取**：`src-tauri/core/src/providers.rs::PROVIDERS` 是唯一真相，收录 `modelscope`、`siliconflow-cn`、`tencent-tokenhub`、`zhipuai` 四家（每项 `id` / `label` / `npm` / `base_url`，全部 https）。注册表外的平台 id 一律拒绝，**不会**被当成「未知但可用」放行。
- **三条管理动作**（HTTP 侧 `POST /admin/*`，与既有 5 条同构）：
  | 动作 | 路由 | 入参 | 返回 |
  |---|---|---|---|
  | `provider-status` | `/admin/provider-status` | 无（不读请求体，同 `refresh`） | 各平台 `{ id, label, configured }`，**不含任何 Key 材料** |
  | `set-provider-key` | `/admin/set-provider-key` | `{ "provider": string, "apiKey": string }` | 写入后新的 status 形态 |
  | `clear-provider-key` | `/admin/clear-provider-key` | `{ "provider": string }` | 删除后新的 status 形态（未配置过也成功，幂等） |
- **凭据口径（本契约里唯一允许把用户凭据带进核心的入口）**：`apiKey` 只经请求体进入，只落 `<数据目录>/providers.json`（权限 `0600`，走既有原子写）；**不出现在**响应体、`status.json`、日志、面板偏好（`src/core/prefs.js` 的键白名单里没有它）与子进程环境（`runtime::ENV_ALLOW`）里。Key 长度上限 4096 字符；校验失败只回文案与错误码，**不回显入参**。
- **新增错误码**：`invalid_provider`（注册表外 / 形态非法）、`invalid_provider_key`（空 / 全空白 / 超长 / 非字符串）。
- **契约是加法式的**：核心 `ACTION_ROUTES` 由 **5 → 8** 条，壳侧 `ADMIN_ROUTES` 同步为 8 条并有单测逐项对账（`shell_action_routes_match_the_core_contract`）。**自有 IPC 命令名与事件名一个都没新增或改名**（仍是 `core_action` / `restart_core` / `core_running` / `data_dir_path` / `read_log` + `core-status` / `core-activity` / `core-failed`）。
- **红线由测试钉死**：`providers.rs` 7 项单测（注册表唯一性、空目录状态、set/read/clear 往返与只删指定平台、状态与落盘文件都不含 Key、坏输入不落盘、损坏或外来形态按「未配置」读）+ `server.rs::provider_actions_take_whole_body_and_never_echo_the_key` + `tests/red_lines.rs::provider_registry_is_reviewed_and_status_echoes_no_key_material`（钉死四平台 id 集合、https-only、`npm` 形态，以及写入真实 Key 后 `status()` 序列化里既无 Key 也无 `apiKey`）。
- 🔴 **现在按它们提交 Key 不会多出一个模型**：注入隔离配置（`isolated_config()` 的 `provider.<id>`）与注册表驱动的模型发现属 Stage 3，面板入口属 Stage 5。

## 🗂 Stage 2：模型命名空间参数化（行为零变化）

为多平台做准备的一次**等价移植**，把原先写死 `opencode/` 前缀的地方参数化：

- `model_status.rs` 新增 `OPENCODE_NAMESPACE`、`join_namespace(ns, key)`、`split_namespace(id)`（按**第一个** `/` 切，无 `/` 时命名空间为空串），成为全限定模型 id 的唯一拆拼点。
- `backend.rs` 的免费模型发现改为 `free_models_in(providers, namespace)`，`free_models()` 只是 `namespace = opencode` 的包装；`model_target(model)` 拆出 `{providerID, modelID}`。
- **展示 ID 的前缀规则收紧**：只有**注册表内**的平台用其 `label` 当前缀，其余（含 `opencode` 与任何未知命名空间）一律 `OC`。这条是测试逼出来的——原方案想让未知命名空间原样当前缀，但 `sync.rs` 的既有测试与对拍夹具用的是 `vendor/gpt` 这类合成命名空间，前缀一变输出就变。
- **验证方式**：`js_parity` 11 项全绿 + `git diff --stat src-tauri/core/tests/fixtures` 为空，即「输出逐字节不变」；`model_target` 对无命名空间 id 的回落与旧的「截掉前 9 字节」写法不同（该形态今日不可达），已用单测把两种写法在全部真实 id 上钉成同值。

## 🔒 四项安全 / 数据红线修复（同日全量代码复审后）

对约 1.7 万行做一次全量复审后落地的四项**最小改动**，都不动对外契约，共新增 5 项测试。

1. **🔴 一次性子进程不再携带宿主完整环境**（安全红线 7）。`runtime.rs` 新增公开函数 `allowed_environment(host_env)`，把宿主环境按 `ENV_ALLOW` 白名单过滤；`default_probe`（定位候选时的 `--version` 探测）与 `start_backend` 里「安装后回读 `--version`」两处调用改为先过它。此前这两处走 `run_command(env: None)`，而该分支**不做 `env_clear`**——等于把宿主进程的一切环境变量（可能含 OpenAI / Anthropic / 云厂商凭据）透传给**尚未信任的外部二进制**（托管下载的 OpenCode 或本机候选）。`isolated_environment` 改为复用同一函数，行为不变。新红线守卫：`version_probe_child_environment_only_passes_the_allow_list`。
2. **探测路径不再可能启用辅助模型转写**（数据红线）。`orchestration.rs::probe_meta()` 让两个探测入口都带 `probe: true`；纯文本（`buddy-chat`）那一侧原先传空对象，而 `backend.rs` 的两处转写闸门都以这个标记为开关，因此探测失败**可以**走到辅助模型转写。
3. **损坏的 `status.json` 不再能整进程退出**。`restored_model_results` 把上一份快照里**非对象**的 `modelResults` 回落成空表。此前只挡「字段缺失」，而 `"x"` 这类形状会让后续一次键索引 panic——发布配置是 `panic = "abort"`（`src-tauri/Cargo.toml`），panic 会**连带整个 GUI 进程**没掉。
4. **并发请求不再互相吞掉逐模型结果**。`record()` 原先先读整份 `modelResults` 快照、改一键后**整体**塞回补丁，两个并发请求会各自读到旧快照、后写者覆盖先写者。现改为把合并挪进锁内、**只写调用方自己那一键**（`apply_patch` 的第三个入参），`update()` 与 `update_with_usage()` 的既有语义不变。新增 3 项 lib 单测覆盖「两写共存 / 非对象 map 自愈 / 探测元信息带标记」。

> 另两条复审指控经源码核对**不成立**，未做改动，也不应「顺手修」：① serde_json 1.0.151 默认 `remaining_depth: 128` 且未开 `unbounded_depth`，超深请求体是 `TooDeep` 错误而非栈溢出；② 非流式与流式链路在记录前都查了 `signal.is_aborted()`（`server.rs:972`、`server.rs:1074`），socket 关闭时 hyper 直接丢弃 handler，`on_result` 不会执行——不存在「客户端取消被记成成功」。

## 📦 发布链路：清单下载 URL 的空格缺陷修复 + CI 资产对账闸门

- 🔴 **根因**：GitHub 上传时会把 Release 资产名里的**空格规范化成 `.`**（磁盘上 `WB Bridge_…` → API 里 `WB.Bridge_…`），而 `/releases/download/<tag>/<带空格的名字>`（含 `%20`）**一律 404**。v1.0.2 那一轮六平台产物齐全、六条 `.sig` 的签名者 key ID 逐字节解出全为 `2B11F78BEA8A43F`、清单生成成功、CI 8/8 全绿——`latest.json` 的六条 `url` 却全部不可下载，**只有客户端走到下载那一步才暴露**。
- **修复**：`scripts/gen-latest-json.mjs` 拼 url 前把空格换成点再编码；`npm run test:manifest` 的成功路径断言同步改成点形式（**9 用例**）。
- **新增闸门**：`release.yml` 的 `update-manifest` 作业加了一步「**校验清单 url 指向的资产在 Release 上真的存在**」——带 `GITHUB_TOKEN` 列出 Release 资产，把六条 `url` 末段与实际资产名逐条对账，任一不符 `exit 1`。这类缺陷清单本身看不出来（六平台齐全、`.sig` 也配得上公钥）。
- ⚠ **对 v1.0.2 已发布那份坏清单的影响**：更新端点指向 `https://github.com/trexwb/wbBridge/releases/latest/download/latest.json`，而 `latest` 解析到**最新的已发布 Release**。因此一旦 v1.0.3 以修正后的生成器发布，`releases/latest/download/latest.json` 就会被**新版那份正确的清单覆盖**，v1.0.2 客户端无需单独重传也能取到可下载的 url——**这是按配置推断的结论，从未实测**；若希望 v1.0.2 那一份本身也可用，重传属共享状态，由维护者操作。

## 🖥 面板

- 「关于与更新」里「已是最新版本」一行修掉**双 `v`**：`vite.config.js` 注入的 `__APP_VERSION__` 本身就带前缀（`'v' + pkg.version`），模板又写了一次 `v{{ version }}`，此前实际显示为「已是最新版本（vv1.0.2）」。现在该行直接用 `{{ version }}`，与该视图其他版本展示口径一致。**只有这一行文案变化**，面板没有新增入口，`src/core/prefs.js` 的偏好白名单也没动。

## 🧾 文档

- `docs/contract.md`：补三条 `provider-*` 动作的契约行与「凭据口径」小节（含 Stage 1 的边界标注：面板无入口、Key 无消费者）。
- `docs/validation.md`：补 Stage 1 / Stage 2 与四项修复的实测条目与未验证项。
- `docs/plans/2026-10-03-upgrade-roadmap-and-v1.0.3-multi-provider-plan.md`：多平台接入路线图（Stage 1–5 的边界与前提）。
- `docs/plans/2026-10-03-multi-upstream-gateway-design.md`、`docs/superpowers/specs/2026-10-03-multi-platform-free-models-design.md`、`docs/superpowers/specs/2026-10-03-agent-context-and-version-discipline-design.md`：同日的设计与纪律文档；`docs/qa/smoke-checklist.md`：发布前冒烟清单。
- `docs/wiki/*` 与 `docs/version/README.md`、`docs/version/RELEASE-v1.0.md`：当前版本口径、发布链路与未验证项同步更正。
- `AGENTS.md`：验证边界表新增本轮修复一行、命令清单与测试规范基线更新为 **227**、模块清单补 `providers`、`allowed_environment`、`probe_meta`、`apply_patch`、`restored_model_results` 等新增项。

## 📊 测试与质量基线

| 项 | v1.0.2 | v1.0.3 |
|---|---|---|
| 核心 `cargo test` | 207（lib 187 + js_parity 11 + red_lines 9） | **227**（lib 205 + js_parity 11 + red_lines 11） |
| └ 其中 `red_lines` | 9 | **11**（+1 平台注册表守卫、+1 一次性子进程环境白名单） |
| 壳 `cargo test --lib` | 9 | **9**（`shell_action_routes_match_the_core_contract` 的对账项由 5 条增至 8 条） |
| `cargo clippy --all-targets`（核心）/ `--no-deps --all-targets`（壳） | 0 warning | **0 warning** |
| `npm run test:prefs` / `test:manifest` / `test:updater-key` | 8 / 9 / 13 | **8 / 9 / 13**（manifest 的成功路径断言改为点形式 url） |
| `npx eslint .` / `npm run vite:build` | 0 problems / ✓ built | **0 problems / ✓ built** |
| `npm run version:check` | 5 处一致（1.0.2） | **5 处一致（1.0.3）** |
| JS↔Rust 对拍夹具 | 冻结 | **未改动**（等价移植的判据） |

核心测试由 207 增至 227：Stage 1 的 `providers.rs` 7 项与三条动作的服务层测试、Stage 2 的 4 项命名空间断言、复审轮的 5 项（lib 4 + red_lines 1）。

## 🍎 macOS 首次打开（ad-hoc 签名放行）

应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截：先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

## 🧭 升级与兼容

- **对外契约只增不改**：`/admin/*` 动作由 5 条增至 8 条（三条 `provider-*`）；新增错误码 `invalid_provider`、`invalid_provider_key`；HTTP 路由、既有错误码、自有 IPC 命令与事件名全部与 v1.0.2 一致。
- **`status.json` 无新增顶层字段**，`STATUS_SCHEMA_VERSION` 仍为 `1`（核心的 `status.json` 内置 `version: "0.2.0"` 是历史沿革值，未动）。
- **数据目录新增一个可选文件**：`providers.json`（`0600`），只有在你提交过平台 Key 后才会存在；旧版本不读它，**回滚装回 v1.0.2 不需要清理数据目录**，该文件会原样留着等下一次装回来。
- **WorkBuddy `models.json` 写入规则不变**：仍只增删 `OWNER = buddy-bridge-v1` 名下条目，其他条目与元数据原样保留。
- **v1.0.2 → v1.0.3 是第一次「理论上可以走应用内更新」的升级**：v1.0.2 是第一个接线更新器并带 `.sig` 的版本，且本次发布会用修正后的生成器覆盖写 `releases/latest/download/latest.json`。⚠️ 但如「发布链路」一节所述，这条推断**从未实测**，真实闭环（拉包 → 安装 → `relaunch()` 起来）仍是本仓库最大的未验证项；任何一步失败时，手动下载 v1.0.3 安装包覆盖安装始终是可用退路。
- **v1.0.1 及更早用户**：那一版没有接线更新器（产物无 `.sig`、面板无更新区），**必须手动下载 v1.0.3 安装包覆盖安装**；此后应用内更新才可能自我生效。
- ⚠️ **未公证 + 更新器替换镜像**的组合可能让 Gatekeeper 在更新后重新拦截；真出现时按「macOS 首次打开」小节处理或重新下载 `.dmg` 覆盖安装。

## 🙋 反馈

Issues：<https://github.com/trexwb/wbBridge/issues>

---

*本说明由仓库实读结果与当日真实测试输出整理（v1.0.2..HEAD 的文件级 diff 逐项核对）；未验证项已显式标注，不代表功能已在真实桌面环境验证通过。*
