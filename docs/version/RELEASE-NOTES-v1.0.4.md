# WB Bridge v1.0.4

> 本文件可直接复制为 GitHub Release body。发布状态标注见文末「发布状态与未验证项」。

**本版主题**：模型发布支持**多插件写入**——自动检测本机同时安装的 WorkBuddy 与 CodeBuddy，向所有检测到的目标分发写入；单目标失败不影响其他目标，未安装的目标自动跳过并说明原因。

**版本推进理由**：与 v1.0.3 **不同类、不同根因**——新增第二个写入目标（检测、定位、分发、聚合均为此前不存在的功能），且维护者明确要求推进版本号（`npm run version:set -- 1.0.4`），末位 +1。

---

## 新功能：多插件写入

在此之前，探测通过的免费模型只会写入 WorkBuddy 的 `models.json`。本版起，写入目标由 `targets.rs` 统一定义（`Target::ALL`，顺序即分发顺序）：

| 目标 | 配置文件（同名同形） | 默认探测位置 | 覆盖环境变量 |
|---|---|---|---|
| WorkBuddy | `models.json` | `~/.workbuddy/models.json` | `BUDDY_MODELS_FILE` > `WORKBUDDY_CONFIG_DIR` > `WORKBUDDY_DATA_FOLDER_NAME` |
| CodeBuddy | `models.json` | `~/.codebuddy/models.json` | `BUDDY_CODEBUDDY_MODELS_FILE` > `CODEBUDDY_CONFIG_DIR` > `CODEBUDDY_DATA_FOLDER_NAME` |

### 安装状态检测逻辑

- **唯一判据：定位函数返回 `Some`**——候选配置文件必须真实存在，且通过与 WorkBuddy 完全相同的路径与形状校验（绝对路径 + 文件名 `models.json` + 顶层是数组或 `models` 是数组）。不猜进程、不猜注册表、不猜包管理器数据库。
- 定位优先级两目标对称：显式环境变量 > 已保存设置 > 配置目录变量 > 数据目录名变量 > 平台默认位置；空字符串按假值处理。
- 探测不到（含「目录在但文件损坏」）一律报「未检测到」并给出原因，**绝不静默回退**到其他位置。
- 条件执行：两个插件都装 → 同时写入两者；只装一个 → 只写已安装的那个；都没装 → 两个目标都报未检测到（不算致命错误，与旧版「找不到配置就报错」的语义对齐点在「全部定位目标都失败」时才出现顶层错误）。

### 写入语义（两目标共用同一套实现）

CodeBuddy **不是第二套写入实现**，与 WorkBuddy 共用 `sync::sync_models`：

- **幂等**：内容无变化时零写入（`changed: false`），多次运行不产生重复条目；
- **只动自己的**：仅清理/更新 `buddyBridgeOwner = buddy-bridge-v1` 名下条目，手动添加的模型原样保留，同 id / 同展示名冲突的新条目整体丢弃；
- **安全写入**：文件锁 + 二次读取（防并发踩踏）+ `.bak` 备份 + 原子替换（Windows 共享冲突重试）。

## 契约变更（`status.json`，加法式）

- `sync` 新增 `targets.{workBuddy, codeBuddy}`：每目标 `status ∈ ok | missing | error`；`ok` 携带 `count / changed / backup?`，`missing` 携带 `reason`，`error` 携带 `error`；
- 顶层 `sync.count` = 各成功目标条数**之和**；顶层 `sync.error` 仅在「至少一个目标被定位且全部定位目标都失败」时出现；
- 新增顶层 `codeBuddyModelsFile` 字段（CodeBuddy 配置路径，未检测到为 `null`）；
- 按既有口径（参照 `usage` 先例），上述均为加法式变更，两侧 `STATUS_SCHEMA_VERSION` 保持 `1` 不递增。

## 面板

- 「WorkBuddy 集成」视图逐目标展示发布状态（已写入条数 / 未检测到原因 / 失败原因），并新增「CodeBuddy 配置」只读行；
- 底部摘要行措辞不再绑定单一目标（count 为合计）；
- 旧核心（无 `targets` 字段）下面板自动回退单行展示，前后兼容。

## 文档

- `docs/contract.md` 新增「写入目标与检测规则」一节（目标表、判据、`sync` 形状）。

---

## 验证（2026-10-08 本轮真实执行）

| 项 | 结果 |
|---|---|
| 核心 `cargo test`（`src-tauri/core/`） | **237 通过 / 0 失败**（lib 215 + `js_parity` 11 + `red_lines` 11；本版新增 10 项：`targets.rs` 6 + `codebuddy_config.rs` 4） |
| JS↔Rust 对拍 | 11 全绿，`git diff --stat src-tauri/core/tests/fixtures` 为空（输出逐字节不变） |
| 核心 `cargo clippy --all-targets` | **0 warning** |
| 壳 `cargo test --lib`（`src-tauri/`） | **9 通过 / 0 失败** |
| JS 三组单测 | `test:prefs` **8** / `test:manifest` **9** / `test:updater-key` **13** |
| `npx eslint .` / `npm run vite:build` | 0 problem / 构建成功 |
| `npm run version:check` | **5 处一致（1.0.4）** |

## 版本号落点（`npm run version:set -- 1.0.4`）

| 位置 | 值 |
|---|---|
| 根 `package.json` → `version` | **1.0.4**（版本单一来源） |
| `src-tauri/tauri.conf.json` → `version` | **1.0.4**（安装包名随之 `WB Bridge_1.0.4_*`） |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.4**（`src-tauri/Cargo.lock` 已由 cargo 同步） |
| `AGENTS.md`「当前基准版本」两行 | **1.0.4** |

---

## 发布状态与未验证项（如实标注，不得伪装成已验证）

- ✅ 已验证：上表全部校验均在本机真实执行。
- ❌ **GUI 未实机验证**：托盘 / 面板的逐目标展示、以及真实双插件环境下的写入效果，属「必须实机点一遍」类别。
- ❌ **CodeBuddy 默认目录是假设**：`~/.codebuddy/models.json` 系比照 WorkBuddy 约定推断（仓库此前无任何 CodeBuddy 线索）。若实际路径不同，改 `codebuddy_config.rs` 的 `DEFAULT_DATA_FOLDER` 常量即可；也可不经改码用 `BUDDY_CODEBUDDY_MODELS_FILE` 指定。
- ❌ v1.0.4 的安装包（本机与 CI）**尚未构建**；按版本纪律先提交版本落点、再在该提交上打 `v1.0.4` 标签。
- ⚠ 工作区既有状态（非本版改动）：`docs/version/RELEASE-NOTES-v1.0.3.md` 在工作区处于已删除未提交状态，由维护者决定恢复或提交删除。

## 升级与兼容

- 从 v1.0.2/v1.0.3 升级：无破坏性变更；未安装 CodeBuddy 的用户行为与旧版一致（只写 WorkBuddy）。
- 自动更新通道自 v1.0.2 起可用；升级后首次发布时，若检测到 CodeBuddy 会自动多写一份配置（同样只动 `buddy-bridge-v1` 名下条目）。
- 回退到旧版本安全：旧版只认 `modelsFile` 与旧 `sync` 字段，未知字段忽略。

---

## macOS 首次打开（ad-hoc 签名放行）

本应用为 **ad-hoc 签名**（`bundle.macOS.signingIdentity: "-"`）、**未做 Apple 公证**。首次打开（或更新后首次启动）若提示「已损坏，无法打开」或无法验证开发者：

1. 确认安装包来源可信（本仓库 Release 附件）；
2. 执行放行命令（把 `WB Bridge.app` 换成实际安装路径）：

```sh
sudo xattr -rd com.apple.quarantine "/Applications/WB Bridge.app"
```

3. 重新打开应用。`spctl` 判定 rejected 属 ad-hoc 签名的预期行为，不代表包损坏。
