# wbBridge Agent 上下文与版本纪律设计

- 日期：2026-10-03
- 状态：待维护者 review（已落盘，**未提交**，按要求不 `commit` / `push` / `tag`）
- 目标版本：1.0.2 → **1.0.3**
- 已确认的三个岔路口：①A 单文件 + 薄跳转层 / ②A 版本门禁自动化 / ③A 一轮一版本

---

## 1. 背景

仓库盘点（2026-10-03，只读）结论：

1. **上下文资产高度集中但单点**：全仓库仅 `AGENTS.md` 一个规则文件（541 行、13 个层级，技术栈 / 命令 / 约定 / 红线 / 交付流程覆盖密度都很高），`CLAUDE.md`、`.cursor/rules/*`、`.github/copilot-instructions.md`、`.windsurfrules` 全部缺失，也没有"其它工具如何指向 `AGENTS.md`"的说明。
2. **`AGENTS.md` 存在两处已过期口径**：第 12 行"文件操作默认根目录"指向 `/Users/wbtrex/website/localServer/node/trexwb/git/wbBridge`，与实际工作副本不符；文中"跟踪文件 103 个"，实际为 112 个。
3. **覆盖度最弱一环**：有命令、有红线，但没有"可照抄的最小改动范例"。
4. **版本纪律需要改约定**：维护者决定自 2026-10-03 起，任何改动都必须升版本号（固定 patch +1）且必须生成对应的 release notes；`AGENTS.md` 原有的"6 条禁止推进情形"（含"纯文档 / 工具链改动不推进版本号"）作废。

## 2. 目标与非目标

### 2.1 目标

- 让任何 AI 工具进入仓库都能拿到同一份、不漂移的项目规则；
- 消除 `AGENTS.md` 中会持续腐化的写法（写死的绝对路径、写死的文件计数）；
- 让"每次改动升版本 + 出 notes"这条纪律有可执行的门禁，而非依赖记性。

### 2.2 非目标

- 不改动任何 Rust / Vue 业务逻辑（核心 `wbbridge-core`、壳 `wbbridge`、面板 `src/`）；
- 不新增产品功能；
- 不 `git commit` / `push` / `tag`，改动全部留在工作区；
- 不推进 crate 内部版本 `0.1.0` 与 `status.json` `0.2.0`；
- 不改写历史 RELEASE-NOTES 与历史版本分节。

## 3. D1 —— 口径纠偏（修改 `AGENTS.md`）

| 处 | 现状 | 改法 |
|---|---|---|
| 文件操作默认根目录（第 12 行附近） | 写死 `/Users/wbtrex/website/localServer/node/trexwb/git/wbBridge`，与实际工作副本不一致 | 改为**不写死绝对路径**：`文件操作默认根目录 = 本仓库根目录（即 AGENTS.md 所在目录）`。从根上消除同类漂移，而不是换一个路径继续漂 |
| 跟踪文件数 | "103 个"，实际 112 | 去掉易腐化的具体计数，改为 `以 git ls-files \| wc -l 的输出为准` |

## 4. D2 —— 薄跳转层（新增 3 个文件）

| 新增文件 | 内容要点 |
|---|---|
| `CLAUDE.md` | 3–5 行：声明规则唯一真相是仓库根的 `AGENTS.md`，并使用 Claude Code 的导入语法指向它 |
| `.cursor/rules/wbbridge.mdc` | front-matter 含 `description` 与 `alwaysApply: true`，正文指向 `AGENTS.md` |
| `.github/copilot-instructions.md` | 3–5 行：声明唯一真相 + 指向路径 |

硬约束：

- **三个文件都不含规则正文**，只做跳转，避免多份规则漂移；
- 新增维护约定：今后新增任何工具适配文件，一律沿用"薄跳转 + 指向 `AGENTS.md`"这一模式，禁止复制规则正文；
- **未核实项**：Claude Code 的 `@AGENTS.md` 导入语法、Cursor 的引用语法在当前版本是否可用，须在实现前查一次官方文档确认。若确认不了，退化为纯文字指引（"开始任何工作前请先阅读仓库根的 `AGENTS.md`"），不写未经验证的语法。

## 5. D3 —— 最小改动范例（`AGENTS.md` 新增一节）

新增一节，放两个可照抄的骨架（每个 15–30 行）。**范例必须以仓库真实源码为蓝本，实现时现读源码后编写，不得凭印象编造常量名、函数名或字段名。**

### 范例一：跨层契约改动（新增一个 `POST /admin/*` 动作）

覆盖四步、缺一不可：

1. 核心 `ACTION_ROUTES` 登记新动作（**唯一真相**）；
2. 壳侧 `ADMIN_ROUTES` 对齐同名契约；
3. 面板 `src/core/bridge.js` 侧调用；
4. 同步 `docs/contract.md` 表格（该表为手写副本，当前无自动校验）。

选这个场景是因为"三处动作表一致性"是本仓库盘点中明确指出的易漏点。

### 范例二：前端单层改动（新增一个面板视图）

覆盖：只用 token 定义颜色与时长（禁止写死）、图标一律内联 SVG（`stroke=currentColor`，CSP `default-src 'self'` 下禁止外部资源）。

## 6. D4 —— 版本纪律改写（修改 `AGENTS.md`）

删除「版本号规则」中现有的"6 条禁止推进情形"，替换为以下 5 条：

1. **任何改动（含纯文档、工具链改动）一律升 patch**：`node scripts/bump-version.mjs <x.y.z>`，一次写入 5 处落点（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`AGENTS.md` 两行）。
2. **一轮工作只升一个号**：不得一事一号，也不得一号多事。
3. **每个新版本必须同步产出**：
   - 新建 `docs/version/RELEASE-NOTES-v<x.y.z>.md`（作 GitHub Release 正文）；
   - `docs/version/RELEASE-v1.0.md` 顶部新增该版本分节；
   - `docs/version/README.md` 索引同步；
   - "macOS 首次打开放行"等版本无关的固定小节**原样复制**到新 notes。
4. `package-lock.json` 中两处 version 为人工同步（不在 `version:check` 范围内）。
5. 不 `commit` / 不 `tag`，留待维护者决定。

同步落点：`docs/version/README.md` 的「约定」处固化第 1–3 条；`docs/wiki/版本与发布.md` 同步说明。

**历史划断**：在 `docs/version/README.md` 注明"本纪律自 2026-10-03 起生效，历史 RELEASE-NOTES 中'不推进版本号'的表述不回溯改写"。

## 7. 版本门禁（修改 `scripts/check-version.mjs`）

在现有"5 处落点一致性检查"之外，新增第二类检查：

- **基线**：`git describe --tags --abbrev=0` 得到的最近 tag 版本；
- **判据**：
  - 若当前 `package.json` 版本 **大于** 基线版本 → 通过（已升版）；
  - 否则，扫描 `<tag>..HEAD` 的提交与当前工作区改动（取并集，按文件路径去重），**剔除忽略名单**后若仍有文件被改动 → **非零退出**，报"存在改动但未升版本"；
- **忽略名单**（这些文件自身的改动不要求升版）：`package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`AGENTS.md`、`docs/version/**`；
- **无 tag 的兜底**：`git describe` 失败（仓库无 tag）时，跳过该检查并在输出中注明"无 tag 基线，跳过未升版检查"，避免误报阻塞；
- 按既有 `scripts/*.test.mjs` 模式补 `node --test` 单测（正向：升版后通过；反向：造一处非忽略改动且不升版须报错）。

现有 5 处落点检查逻辑保持不变。

## 8. 本轮占号与交付物

本轮（D1–D4 视为**一轮工作**）：1.0.2 → **1.0.3**，产出一份 `RELEASE-NOTES-v1.0.3.md`。

| 动作 | 文件 |
|---|---|
| 修改 | `AGENTS.md`（D1 + D3 + D4） |
| 修改 | `scripts/check-version.mjs` + 新增其单测 |
| 新增 | `CLAUDE.md`、`.cursor/rules/wbbridge.mdc`、`.github/copilot-instructions.md` |
| 修改 | `docs/version/README.md`、`docs/wiki/版本与发布.md` |
| 修改 | `docs/version/RELEASE-v1.0.md`（顶部新分节） |
| 新增 | `docs/version/RELEASE-NOTES-v1.0.3.md` |
| 修改 | 版本落点 5 处 + `package-lock.json` 两处 |

## 9. 验证计划

| 验证项 | 命令 / 方式 | 通过标准 |
|---|---|---|
| 版本一致性 | `npm run version:check` | 5 处落点一致 |
| 新门禁正向 | 升版后运行 `npm run version:check` | 通过 |
| 新门禁反向 | 临时改一个非忽略文件、不升版后运行 | 报错并非零退出（验证后还原） |
| 脚本单测 | `node --test` 新增测试用例 | 全通过 |
| 前端 lint | `npm run lint` | 无报错 |
| 文档口径 | 全仓 grep 旧版根目录路径、"103 个"、"禁止推进"等旧表述 | 无残留 |

本轮**不跑** `cargo test` / `cargo clippy`（不涉及 Rust 改动）。

## 10. 风险与未核实项

| 项 | 说明 | 处置 |
|---|---|---|
| 门禁基线依赖 tag | v1.0.2 目前未打 tag | 已在第 7 节明确：比较"当前版本 > 基线版本"，并使用"无 tag 跳过"兜底 |
| 新纪律与历史表述冲突 | 既有 RELEASE-NOTES 写有"不推进版本号" | 用"自 2026-10-03 起生效、不回溯"划断 |
| 跳转语法未核实 | Claude Code / Cursor 的引用语法 | 实现前查官方文档；核不到退化为纯文字指引 |
| 范例真实性 | 骨架若凭印象编写会引入假常量 | 实现时现读源码，逐项核对 |

## 11. 后续

1. 维护者 review 本设计稿；
2. 通过后进入实施计划（writing-plans），再逐项落地；
3. 全部改动留在工作区，由维护者决定是否提交与发版。
*（内容由AI生成，仅供参考）*
