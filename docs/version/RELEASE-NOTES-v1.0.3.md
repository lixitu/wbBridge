# WB Bridge v1.0.3

> **GitHub Release 正文**（推送 `v1.0.3` 标签或手动运行 `release.yml` 时可直接复制本文件作为 Release body）。
> 发布日期：待定 ｜ 上一版本：v1.0.2 ｜ 内部 crate `wbbridge-core` 仍为 `0.1.0`（与产品版本解耦，不随本次递增）

**本版主题：多平台接入前两阶段（BYOK 平台注册表 + 模型命名空间参数化）+ 四项安全 / 数据红线修复 + 更新清单下载 URL 缺陷修复与 CI 对账闸门。**

---

> ## 🍎 macOS 用户请先看这条：首次打开若提示「已损坏」，执行下面这行命令
>
> 本应用为 **ad-hoc 签名、未做 Apple 公证**，macOS 首次打开（以及更新后再次替换镜像时）可能被 Gatekeeper 拦截。提示「**已损坏**」且你已确认安装包来源可信时，在「终端」执行：
>
> ```sh
> xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
> ```
>
> 若只是被拦截、未报「已损坏」：先尝试打开 → 「系统设置 → 隐私与安全性」→ 点「仍要打开」。详见下文「macOS 首次打开（ad-hoc 签名放行）」小节。

---

## ⚠ 发布状态（请如实阅读）

- ❌ **桌面 GUI 从未实机启动**（v1.0.2、v1.0.3 都一样）；**v1.0.3 尚未构建、未打标签**。
- ⚠ **Stage 1 端到端不产生模型**：写进 `providers.json` 的 Key **目前没有消费者**（注入隔离配置属 Stage 3），面板也**没有入口**（Stage 5）。现在提交 Key **不会多发布一个模型**。
- ✅ **Stage 2 行为零变化**：`js_parity` 11 项全绿且 `git diff --stat src-tauri/core/tests/fixtures` 为空（夹具未改），即对外输出逐字节不变。
- ✅ **本轮真实跑数**：核心 `cargo test` **227 通过 / 0 失败**、核心与壳 clippy **0 warning**、壳 `cargo test --lib` **9 通过**、`test:prefs` **8** / `test:manifest` **9** / `test:updater-key` **13**、`npx eslint .` **0 problem**、`vite:build` ✓、`version:check` **5 处一致（1.0.3）**。
- ❌ **未验证**：一次真实的「检查 → 下载 → 安装 → 重启 → 首启」；本版新增的 CI 资产对账步骤**本身**（只在本机用线上坏清单 / 修正后清单各跑过同段逻辑）；真实 Key 从未在 GUI 输入过，`providers.json` 的 `0600` 只在 unix 断言过。
- 🔴 **发布前置（当前仍未提交）**：版本落点 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`、`AGENTS.md` **尚未提交**。铁律：**先提交版本落点，再在那一个提交上打 `v1.0.3`**。推标签前配好仓库级 Variables（`TAURI_SIGNING_PRIVATE_KEY` = 私钥**全文**、`_PASSWORD` = 非空口令），并本地跑一次 `npm run tauri:build` 确认注入器打印「已注入内联签名私钥 → 公钥配对 OK（`2B11F78BEA8A43F`）」——否则整套 Rust 编译跑完才在打包那一步失败。

## ✨ 本版内容

1. **多平台接入 Stage 1：平台注册表 + 用户自持 Key 通道**
   - `src-tauri/core/src/providers.rs::PROVIDERS` 是唯一真相，收录 `modelscope`、`siliconflow-cn`、`tencent-tokenhub`、`zhipuai`（每项 `id`/`label`/`npm`/`base_url`，全部 https）；**随版本发布、不做远程拉取**，注册表外的 id 一律拒绝。
   - 三条新管理动作：`provider-status`（不读请求体，返回各平台 `{id,label,configured}`，**不含任何 Key 材料**）、`set-provider-key`（`{provider, apiKey}`）、`clear-provider-key`（`{provider}`，幂等）。新增错误码 `invalid_provider`、`invalid_provider_key`（校验失败**不回显入参**）。
   - 凭据口径：`apiKey` 只经请求体进入，只落 `<数据目录>/providers.json`（`0600`、原子写、上限 4096 字符）；**不出现在**响应体、`status.json`、日志、面板偏好与子进程环境。这是本契约里唯一允许把用户凭据带进核心的入口。
   - 契约是**加法式**：核心 `ACTION_ROUTES` **5 → 8**，壳 `ADMIN_ROUTES` 同步并有单测逐项对账；**自有 IPC 命令与事件名一个都没新增或改名**。红线由 `providers.rs` 7 项单测 + `server.rs::provider_actions_take_whole_body_and_never_echo_the_key` + `red_lines` 的注册表守卫钉死。
2. **Stage 2：模型命名空间参数化（行为零变化的前置移植）**
   - 新增 `OPENCODE_NAMESPACE`、`join_namespace(ns, key)`、`split_namespace(id)`（按**第一个** `/` 切）成为全限定 id 的唯一拆拼点；免费模型发现改为 `free_models_in(providers, namespace)`，`free_models()` 只是 `opencode` 的包装；新增 `model_target(model)` 拆 `{providerID, modelID}`。
   - 展示 ID 前缀规则收紧：**只有注册表内的平台用其 `label` 当前缀，其余一律 `OC`**（含 `opencode` 与任何未知命名空间）。
3. **四项安全 / 数据红线修复**（同日全量代码复审后，均不动对外契约，共新增 5 项测试）
   - **一次性子进程不再携带宿主完整环境**：`runtime.rs` 新增 `allowed_environment(host_env)` 按 `ENV_ALLOW` 白名单过滤，`--version` 探测（定位候选 + 安装后回读）两处改为先过它。此前它们走 `run_command(env: None)`，而该分支**不做 `env_clear`**——等于把宿主全部环境变量（可能含 OpenAI / Anthropic / 云厂商凭据）透传给**尚未信任的外部二进制**。新红线守卫 `version_probe_child_environment_only_passes_the_allow_list`。
   - **探测路径不再可能启用辅助模型转写**：`probe_meta()` 让两个探测入口都带 `probe: true`；纯文本（`buddy-chat`）那一侧原先传空对象，而两处转写闸门都以这个标记为开关。
   - **损坏的 `status.json` 不再能整进程退出**：`restored_model_results` 把上一份快照里**非对象**的 `modelResults` 回落空表。此前只挡字段缺失，而 `"x"` 这类形状会让后续键索引 panic —— 发布配置是 `panic = "abort"`，panic 会连带整个 GUI 进程没掉。
   - **并发请求不再互相吞掉逐模型结果**：`record()` 原先读整份 `modelResults` 快照、改一键后整体塞回补丁，两个并发请求会各自拿旧快照、后写者覆盖先写者；现改为锁内**只写自己那一键**（`apply_patch` 第三入参），`update()` / `update_with_usage()` 既有语义不变。
   - 另两条复审指控经源码核对**不成立**、未做改动：serde_json 1.0.151 默认 `remaining_depth: 128`（超深是 `TooDeep` 错误而非栈溢出）；非流式与流式链路在记录前都查 `signal.is_aborted()`（`server.rs:972`、`server.rs:1074`），不存在「客户端取消被记成成功」。
4. **发布链路：清单下载 URL 的空格缺陷 + CI 对账闸门**
   - 根因：GitHub 上传时把 Release **资产名里的空格规范化成 `.`**（磁盘 `WB Bridge_…` → API `WB.Bridge_…`），而 `/releases/download/<tag>/<带空格名>`（含 `%20`）**一律 404**。v1.0.2 那一轮六平台齐全、`.sig` 的 key ID 全部对上、CI 8/8 全绿，`latest.json` 的六条 `url` 却全部不可下载——**只有客户端走到下载那一步才暴露**。
   - 修复：`scripts/gen-latest-json.mjs` 拼 url 前把空格换成点再编码，`test:manifest` 成功路径断言同步（9 用例）；`release.yml` 的 `update-manifest` 作业新增一步「**校验清单 url 指向的资产在 Release 上真的存在**」，用 API 逐条对账、任一不符 `exit 1`。
   - 对已发布那份坏清单的影响：更新端点是 `releases/latest/download/latest.json`，`latest` 解析到最新的已发布 Release，所以 v1.0.3 发布后会用修正后的清单**覆盖写**该 URL，v1.0.2 客户端理论上无需单独重传即可拿到可下载 url。**这是按配置推断，从未实测**；若要单独修 v1.0.2 那份，重传资产属共享状态、由维护者操作。
5. **面板一处文案**：「关于与更新」的「已是最新版本」去掉重复的 `v`——`vite.config.js` 注入的 `__APP_VERSION__` 本身带前缀（`'v' + pkg.version`），模板又写了一次 `v{{ version }}`，此前显示为「已是最新版本（vv1.0.2）」。仅这一行文案，面板无新增入口。

## 📊 测试与质量基线

| 项 | v1.0.2 | v1.0.3 |
|---|---|---|
| 核心 `cargo test` | 207（lib 187 + js_parity 11 + red_lines 9） | **227**（lib 205 + js_parity 11 + red_lines 11） |
| 壳 `cargo test --lib` | 9 | **9**（动作对账项由 5 条增至 8 条） |
| clippy（核心 `--all-targets` / 壳 `--no-deps --all-targets`） | 0 warning | **0 warning** |
| `test:prefs` / `test:manifest` / `test:updater-key` | 8 / 9 / 13 | **8 / 9 / 13**（manifest 断言改为点形式 url） |
| `npx eslint .` / `npm run vite:build` | 0 problems / ✓ built | **0 problems / ✓ built** |
| `npm run version:check` | 5 处一致（1.0.2） | **5 处一致（1.0.3）** |

207 → 227 的来源：Stage 1 的 `providers.rs` 7 项 + 三条动作的服务层测试、Stage 2 的 4 项命名空间断言、复审轮的 5 项（lib 4 + red_lines 1）。JS↔Rust 对拍夹具**未改动**。

## 🧭 升级与兼容

- **契约只增不改**：`/admin/*` 由 5 条增至 8 条，新增两个错误码；HTTP 路由、既有错误码、自有 IPC 命令与事件名与 v1.0.2 一致。
- **`status.json` 无新增顶层字段**，`STATUS_SCHEMA_VERSION` 仍为 `1`（核心内置的 `version: "0.2.0"` 是历史沿革值，未动）。
- **数据目录新增一个可选文件** `providers.json`（`0600`），只有提交过平台 Key 才会存在；旧版不读它，**回滚装回 v1.0.2 不需要清理数据目录**。
- **WorkBuddy `models.json` 写入规则不变**：仍只增删 `OWNER = buddy-bridge-v1` 名下条目。
- **v1.0.2 → v1.0.3 是第一次「理论上可走应用内更新」的升级**（v1.0.2 是第一个接线更新器并带 `.sig` 的版本）。⚠️ 如第 4 条所述该推断**从未实测**，真实闭环仍是本仓库最大的未验证项；手动下载 v1.0.3 安装包覆盖安装始终是可用退路。**v1.0.1 及更早**用户必须手动安装一次，此后应用内更新才可能自我生效。

## 🍎 macOS 首次打开（ad-hoc 签名放行）

应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截：先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

## 🙋 反馈

Issues：<https://github.com/trexwb/wbBridge/issues>

---

*本说明由 `v1.0.2..HEAD` 的逐项 diff 与当日真实测试输出整理；未验证项已显式标注，不代表功能已在真实桌面环境验证通过。*
