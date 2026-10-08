# 版本迭代日志

> 本目录专门存储 wbBridge 每次升级迭代的发布日志。
> 命名规则：`RELEASE-v{主版本}.md`（如 v1.0.x → RELEASE-v1.0.md，v1.1.x → RELEASE-v1.1.md）
>
> GitHub Release 正文按完整版本单独成文：`RELEASE-NOTES-v{完整版本}.md`（如 `RELEASE-NOTES-v1.0.1.md`），内容可直接复制到 Release body。
>
> **Release 正文必备固定小节**：每份 `RELEASE-NOTES-v{完整版本}.md` 必须包含「macOS 首次打开（ad-hoc 签名放行）」小节——写明应用为 ad-hoc 签名、**未做 Apple 公证**，首次打开（含更新后重新被拦截）提示「已损坏」时确认来源可信后执行放行命令；该小节是发布正文的固定组成部分，**新增版本必须沿用同一小节，不得省略或改写**。
>
> **版本纪律（与根目录 `AGENTS.md`「当前基准版本」章节严格对齐）**：
> - **版本单一来源**：根目录 `package.json` 的 `version`（当前 **1.0.4**）；`src-tauri/tauri.conf.json`
>   与 `src-tauri/Cargo.toml` 的 `[package] version` 是同步落点（当前同为 **1.0.4**）
> - `src-tauri/core/Cargo.toml` 的内部 crate 版本（当前 **0.1.0**，crate `wbbridge-core`）与产品版本**有意解耦**：
>   它是库自身的演进节奏，**不是**版本号落点，`version:check` 不校验它，也不随产品版本递增
> - 版本号末位仅在「不同类新功能 / 不同根因新修复 + 用户明确允许」时 +1
> - **绝对禁止推进版本号的场景**：同一问题多轮往返跟进、同日同模块追加修复、用户明确要求不改版本号、仅文档更新（`docs/`、`README.md`、`AGENTS.md` 等）、纯文案/注释/日志措辞/去抖体验打磨
> - `status.json` 中由 `src-tauri/core/src/orchestration.rs` 写入的 `version: "0.2.0"`（历史沿革值，另有
>   `schemaVersion: 1` 描述快照结构）为**历史沿革值**，**不得在无用户指令的情况下静默改动**
> - 用户要求版本回退时，根 `package.json`、`AGENTS.md`「当前基准版本」、本目录索引（以及
>   `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）必须同时回退到用户指定值
> - **即使不推进版本号，也必须追加发布日志**：对应主版本文件顶部新分节，注明日期与"不推进版本号"；历史日志只增不改

---

## 日志索引

| 文件 | 覆盖版本 | 状态 |
|------|---------|------|
| [v1.0.4 GitHub Release 正文](RELEASE-NOTES-v1.0.4.md) | v1.0.4（模型发布多插件写入：自动检测 WorkBuddy + CodeBuddy 并向所有检测到的目标分发；`sync.targets` 逐目标状态与顶层 `codeBuddyModelsFile`） | 📝 待发布：**尚未构建、未打标签**；版本落点已改至 1.0.4 但仍未提交——按铁律先提交、再在该提交上打 `v1.0.4` |
| [v1.0.3 GitHub Release 正文](RELEASE-NOTES-v1.0.3.md) | v1.0.3（多平台接入 Stage 1 + Stage 2、四项安全/数据红线修复、清单 url 空格→点修复与 CI 资产对账闸门） | 📝 待发布：**尚未构建、未打标签**；版本落点（`package.json` / `src-tauri/Cargo.toml` / `Cargo.lock` / `tauri.conf.json` / `AGENTS.md`）仍未提交——按铁律先提交、再在该提交上打 `v1.0.3` |
| [v1.0 日志](RELEASE-v1.0.md) | v1.0.4（当前基准版本）＋ v1.0.3 ＋ v1.0.2 ＋ v1.0.1 ＋ v1.0.0 基线快照，最新在前 | 📝 待发布 |
| [v1.0.2 GitHub Release 正文](RELEASE-NOTES-v1.0.2.md) | v1.0.2（含 2026-10-03 追加的「渲染与轮询降耗」与「签名注入链路重写 + 单条公钥 + hdiutil dmg」两节） | 📝 标记待维护者定夺（标签 `v1.0.2` 已打出、**Release 已 Publish、23 个资产**；GUI 与更新链路未实机验证，同轮 `latest.json` 的 url 缺陷已修脚本、线上那份待重传） |
| [v1.0.1 GitHub Release 正文](RELEASE-NOTES-v1.0.1.md) | v1.0.1 | 代码已并入 `main`（`679a2cb`）、远端标签已指向含版本推进的提交；**是否真的发布过 Release / 跑过 CI 未经核验** |

---

## 版本号落点（实读核验，2026-10-02 随 v1.0.2 推进更新；2026-10-03 复验 5 处一致；同日随 v1.0.3 再推进；2026-10-08 随 v1.0.4 再推进）

| 位置 | 当前值 | 说明 |
|------|--------|------|
| 根目录 `package.json` → `version` | **1.0.4** | **版本唯一来源**；`name = wb-bridge`、`private: true`、`type: module`、`engines.node >= 24`。根级脚本：`test`（`cargo test --manifest-path src-tauri/core/Cargo.toml`）/`test:prefs`、`test:manifest`、`test:updater-key`（三组 `node --test`，v1.0.2 新增，分别覆盖面板偏好、更新清单生成、签名注入，合计 30 用例：prefs 8 + manifest 9 + updater-key 13）/`rust:check`/`lint`（仅 eslint）/`dev`（`tauri dev`）/`vite:dev`/`vite:build`/`build`（= `vite:build && tauri:build && make:dmg`）/`tauri`/`tauri:dev`/`tauri:build`（= `node scripts/with-updater-key.mjs tauri build`，签名注入包装器）/`make:dmg`（`bash scripts/make-dmg.sh`，hdiutil 出 macOS 的 .dmg，非 mac 平台跳过）/`version:set`/`version:check`/`gen:latest`。Node 在此只服务面板构建与 `scripts/*.mjs` |
| `src-tauri/tauri.conf.json` → `version` | **1.0.4** | 打包与更新元数据读此值；`productName = WB Bridge`、`identifier = app.wbbridge.desktop`、`frontendDist = ../dist`、`bundle.externalBin = []`（核心已静态链接，无 sidecar）、`bundle.createUpdaterArtifacts: true` + `plugins.updater`（v1.0.2 起）；窗口默认 **1120 × 720**、最小 **860 × 560** |
| `src-tauri/Cargo.toml` → `[package] version` | **1.0.4** | 壳工程侧同步落点（`name = wbbridge`，`rust-version = 1.77`，`tauri = "2"`，本版另加 `tauri-plugin-updater` / `tauri-plugin-process`）；已在 `AGENTS.md`「当前基准版本」表登记 |
| `src-tauri/core/Cargo.toml` → `[package] version` | `0.1.0` | **内部库 crate 版本，有意与产品版本解耦**（crate `wbbridge-core`，`rust-version = 1.75`，`publish = false`），不是版本落点、不被 `version:check` 校验、不随产品版本递增 |
| `src-tauri/core/src/orchestration.rs`（`status.json` 内置） | `0.2.0` | 历史沿革值，沿自上游参考实现（参考 https://github.com/louchi1984-coder/ow-bridge），界面上可见；同处另写 `schemaVersion: 1`；非版本来源 |
| `src/core/bridge.js` 等面板代码 | 无版本字面量 | 面板为 **Vue 3 + Vite** 源码（`src/` → 构建到 `dist/`），版本号由 `vite.config.js` 在构建期从 `package.json` 注入 `__APP_VERSION__`；`src/core/` 是**前端内核**（IPC 边界），与已归档的 Node 后端无关 |
| `.github/workflows/release.yml` | **已写入，2026-10-03 已跑通完整一轮** | 六平台构建工作流已按 Rust 形态重写。同日**更早**的首次实跑停在 build 作业（`needs: test`，因此 test 作业已过）的 `tauri build` 生成 updater 产物那一步（私钥变量取到空值）；改用仓库级 **Variables** 后，tag `v1.0.2`（head `0e4a535`）的 run `37092915120` **8/8 全绿**，Release `v1.0.2` 已 Publish、**23 个资产**，标签闸门与 `update-manifest` 均已真实执行。🔴 但该轮 `latest.json` 的六条 `url` 全部 404（GitHub 把 Release 资产名里的空格规范化成 `.`，脚本却按本地文件名 `%20` 编码）——脚本与 CI 对账闸门已修，线上那份清单待维护者重传。**v1.0.3 尚未推标签、CI 未跑过该版本**，且新加的资产对账步骤本身还没在 CI 上实跑过（本机只跑过同段逻辑） |

> 版本号一致性由脚本核验：本地 `npm run version:check`（校验 5 处落点：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 与 `AGENTS.md` 的两行基准表）。`release.yml` 的测试 job 除同一脚本外还有一道**「触发标签 ↔ `tauri.conf.json` 版本」闸门**（v1.0.2 新增），该闸门与脚本**已在 2026-10-03 的 `v1.0.2` 完整一轮里真实跑过**（8/8 作业全绿），"CI 已核验"至此成立；但同轮暴露出 `latest.json` 的 url 空格/点缺陷（清单能生成、CI 全绿，六条下载 url 却全 404），所以**「CI 绿」不等于发布物可用**，人工复核仍建议在改动版本号后 `grep -rn "1\.0\.1"` 扫一遍文档（v1.0.2 那轮已把 `docs/wiki/*`、`docs/version/*` 的当前版本口径同步为 1.0.2；**2026-10-03 随 v1.0.3 推进再把「当前值」类表述改为 1.0.3**（`docs/wiki/版本与发布.md` 落点表与当前状态表、`docs/wiki/Home.md`、`docs/wiki/已知限制与未验证项.md`、本目录三处）；`RELEASE-v1.0.md` 与 `RELEASE-NOTES-v1.0.2.md` 的历史分节、`docs/validation.md` 各轮条目、以及**已实测产物文件名**（`WB Bridge_1.0.2_aarch64.dmg`、`WB Bridge_1.0.1_aarch64.dmg` 等）里残留的 `1.0.0` / `1.0.1` / `1.0.2` 属于**历史口径与真实测量记录，不改写**）。

---

## 约定

- **按主版本一个文件**：`RELEASE-v{主版本}.md`，次版本迭代作为分节追加到文件顶部（最新在前）
- **Release 正文必备小节（macOS 放行）**：每份 `RELEASE-NOTES-v{完整版本}.md` 必须含固定小节「macOS 首次打开（ad-hoc 签名放行）」——ad-hoc 签名、未做 Apple 公证的说明 + 放行命令 `xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"`（首次打开或更新后提示「已损坏」时，确认来源可信后执行）；新建版本正文时从上一版**原样复制**该小节，发布前检查不得缺失
- **发布前**：在对应主版本文件追加该次版本分节；仅当 `AGENTS.md`「当前基准版本」允许的唯一场景出现时，才同步递增版本号各落点
- **发布后**：分节状态标记 ✅ 已发布；本目录内已发布内容只增不改（历史日志不可篡改）
- 待发布内容先以 📝 待发布 状态记录，发布时更新状态与日期
- **当前发布状态说明（2026-10-03 更新，同日末轮再更正）**：标签 `v1.0.2`（head `0e4a535`）**已打出并触发过完整一轮 CI**，**Release `v1.0.2` 已 Publish、23 个资产**；仓库已有 103 个被跟踪文件、`dev` 分支已推到远端、`main` 上是 `679a2cb`（= 远端 `v1.0.1` 标签，含 1.0.1 版本推进）；**本地 `v1.0.1` 标签仍指 `f046208`（1.0.0）**，同步本地标签属维护者操作。截至本轮：**桌面 GUI 没有实机启动**；本机除 macOS aarch64 外的安装包没构建过（本机更早一次打包发生在 `1.0.1` 源码版本，只出过 macOS aarch64 的 dmg 且未运行 `.app`），其余五平台的包**由 CI 完整一轮产出并发布**（同样没在实机装过）。`release.yml` 同日**更早**的首次实跑曾停在签名一步（私钥变量取到空值），改用仓库级 **Variables**（`TAURI_SIGNING_PRIVATE_KEY` = 私钥全文、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 非空口令）后 8/8 全绿，闸门与 `update-manifest` 均已真实执行。🔴 但那轮 `latest.json` 的六条 `url` 因 GitHub 把 Release 资产名里的空格规范化成 `.` 而**全部 404**：`gen-latest-json.mjs` 已改为先把空格换成点再编码、`test:manifest` 断言同步、`update-manifest` 作业新增一道「清单 url ↔ 实际资产名」对账；**线上那份 v1.0.2 清单仍是坏的**，重传属共享状态、由维护者操作，**一次真实升级闭环从未走过**。分节状态是否按「发布后改 ✅」的约定改写由维护者决定（本目录已发布内容只增不改），Agent 不代改。2026-10-03 追加一轮渲染与轮询降耗（壳 `status.json` 轮询 `(mtime, 长度)` 前置过滤、核心去除未使用的 `brotli` 解压、面板帧级合并发布与行级 / 详情引用稳定），属同一未发布版本内的打磨，**版本号仍为 1.0.2**；同日复跑 `cargo test` **207** / 壳 `cargo test --lib` **9** / `test:prefs` **8** / `test:manifest` **9** / `clippy` **0 warning** / `eslint` **0 problem** / `vite:build` 成功；**同日两个独立 code-reviewer 复审后修掉两处会丢面板状态的缺陷**（mtime 不可得时不得启用轮询快路径、恢复运行分支须作废 stamp）并各补 1 项壳单测（壳基线 8 → 9），另有面板队列封顶与两处注释/文档口径更正，仍**不推进版本号**，未验证项不变。同日**再追加一轮签名/发布链路重写**：`scripts/with-updater-key.mjs` 从「本地手跑的签名自检工具」重写为 `npm run tauri:build`（= `node scripts/with-updater-key.mjs tauri build`）的**签名注入包装器**并接入 `build` 与 CI，撤销 `--check-only` 与「形态→配对→试签」前置；`plugins.updater.pubkey` 更正为**仅一条公钥**（`2B11F78BEA8A43F`，此前「两条＝轮换白名单」的结论被 `tauri-plugin-updater`/`minisign-verify` 源码推翻——只有第一条生效）；macOS 的 `.dmg` 改由 `npm run make:dmg`（hdiutil）生成、`bundle.targets` 从 `"all"` 改显式列表；CI 撤除 `tauri-apps/tauri-action`、Release 改由 `softprops/action-gh-release` 建（`tag_name` 现读 `tauri.conf.json`）。`npm run test:updater-key` 由 9 → **13**，三组 JS 套件合计由 26 → **30**；Rust 基线不变（核心 207、壳 9）。版本仍为 **1.0.2**，详见 `docs/validation.md` 与 `RELEASE-NOTES-v1.0.2.md`、`RELEASE-v1.0.md` 的同日条目
- **去重整理（沿用参考项目 discipline）**：同类问题多次修复的条目合并为一条，统一记述于最终修复版本；被合并的早期版本分节保留编号与合并指向（不删版本号、不重复正文）
- **文档类更新不推进版本号**：本目录建立（`docs/version/README.md` + `RELEASE-v1.0.md`）与本次 Node → Rust 迁移后的文档更正均属纯文档更新，按纪律**不推进版本号**（当时产品版本保持 **1.0.0**），只在对应分节留痕（见 `RELEASE-v1.0.md` v1.0.0 分节末条）。2026-10-01 第八轮**引入不同根因的安全/并发修复与面板新视图，且用户明确要求推进版本号**，遂 `1.0.0 → 1.0.1`（`npm run version:set -- 1.0.1`，5 处落点一致）。2026-10-02 的 `1.0.1 → 1.0.2` 同样满足两条前提（新增**不同类**的自动更新链路、发布门禁与偏好持久化；推进由**维护者本人**执行），Agent 未擅自改动版本号
- **界面调整同样不推进版本号**：2026-10-01 的面板布局改造（侧栏分组导航、详情改为右侧常驻分栏、默认窗口 980×680 → **1120×720**、侧栏 `--sidebar-w` 224 → **208px**、新增 `--muted-strong`；该轮留下的 4 个「规划中」入口已在同日第四轮实现为只读视图，同样不推进版本号）属同一未发布版本内的界面调整，**不推进版本号**；已在 `RELEASE-v1.0.md` 的 v1.0.0 分节以「面板布局改造说明」留痕，验证证据见 `docs/validation.md` 同轮条目
- **渲染 / 轮询降耗同样不推进版本号**：2026-10-03 的一轮降耗（壳侧 `status.json` 轮询加 `(mtime, 长度)` 前置过滤、核心 `reqwest` 去掉未使用的 `brotli` feature、面板 `core-status`/`core-activity` 帧级合并发布与行级 / 详情 props 引用稳定）不引入新功能，按纪律**不推进版本号**（保持 **1.0.2**）。同日两个独立 code-reviewer 复审该轮改动，查出并修掉**两处会丢面板状态的缺陷**（① `unwrap_or(UNIX_EPOCH)` 使 mtime 不可得时快路径永久跳过长度不变的改写；② 恢复运行分支只清 `last` 未作废 `last_stamp`，抵消了「重启后即使逐字节相同也要重推」的既有保障），并为快路径判定补 1 项壳单测（壳基线 **8 → 9**）；另有面板队列封顶（🟠）与两处注释口径更正（🟠🟡：合并语义只对订阅者等价、`base64` 重复只属核心 workspace、brotli 收益不等同整机产物）。这些属**同一未发布版本、同一批改动**的复审修正，按「同日对同一模块追加修复」纪律仍**不推进版本号**；已在 `RELEASE-v1.0.md` 的 v1.0.2 分节以「2026-10-03 追加」小节留痕，验证证据见 `docs/validation.md` 2026-10-03 条目
- **v1.0.3 的推进（2026-10-03，满足纪律的两条前提）**：`1.0.2 → 1.0.3` 由**维护者明确要求**，且内容与 v1.0.2 **不同类、不同根因**——新增多平台接入 Stage 1（`providers.rs` 注册表 + `providers.json` 凭据通道 + 三条 `provider-*` 管理动作，`ACTION_ROUTES` 5 → 8）与同日全量代码复审查出的**四个新根因修复**（一次性子进程携带宿主环境、探测路径的转写开关、`status.json` 非对象形状在 `panic = "abort"` 下整进程退出、并发 `modelResults` 互相吞写）。同日的 Stage 2（命名空间参数化）本身是**行为零变化的等价移植**，按纪律不单独构成推进理由，只是随本版一起入库。推进用 `npm run version:set -- 1.0.3`，`version:check` 实测 5 处一致；`src-tauri/Cargo.lock` 由 cargo 自动同步、须一并提交；`bump-version.mjs` 重写 `tauri.conf.json` 时把单行的 `bundle.targets` 展开成多行，属**纯格式副作用**。核心测试基线 207 → **227**（lib 205 + js_parity 11 + red_lines 11），`AGENTS.md` 的基线行随之更正。**发布前置仍未完成**：版本落点提交、`v1.0.3` 标签（必须打在该提交上）、CI 构建与 Publish 都由维护者执行
- **v1.0.4 的推进（2026-10-08，满足纪律的两条前提）**：`1.0.3 → 1.0.4` 由**维护者明确要求**，且内容与 v1.0.3 **不同类、不同根因**——新增第二个写入目标 CodeBuddy（`targets.rs` 目标定义与聚合、`codebuddy_config.rs` 对照定位、`orchestration.rs` 双目标分发与 `sync.targets`/`codeBuddyModelsFile` 状态形状、面板逐目标展示）。推进用 `npm run version:set -- 1.0.4`，`version:check` 实测 5 处一致；`src-tauri/Cargo.lock` 由 cargo 自动同步、须一并提交。核心测试基线 227 → **237**（lib 215 + js_parity 11 + red_lines 11，新增 10 项），`AGENTS.md` 的基线行随之更正。**发布前置仍未完成**：版本落点提交、`v1.0.4` 标签（必须打在该提交上）、CI 构建与 Publish 都由维护者执行
