# WB Bridge v1.0.0 验证记录

> 阅读顺序：最新记录在前。自 **2026-10-01** 起核心已从 Node.js sidecar 迁移为 Rust 库（静态链接进壳），
> 该日期之后的条目描述 Rust 形态；下方的 2026-09-30 条目属于**迁移前的 Node/sidecar 时代**，作为历史
> 保留原样（其 97 项测试、`src/core/`、`src-tauri/binaries/` 等结论已不再对应当前仓库）。

日期：2026-10-01（本机 macOS，Apple Silicon；Rust 核心 + Vue 面板）

## 全量代码审查与壳↔面板接缝修复（2026-10-01，同日第二轮）

### 已验证（本轮实跑数字）

- `cargo test`（`src-tauri/core/`）→ **197 通过 / 0 失败**：lib 单测 **179** + `js_parity` **11** + `red_lines` **7**。
  新增的 2 个 lib 单测在 `src-tauri/core/src/orchestration.rs::tests`：
  `concurrent_chain_registration_never_forks`（8 线程并发登记串行链，断言只有一个无前驱、
  predecessor 不重复——即写链不得分叉）与 `reused_slot_clone_shares_the_recorded_outcome`
  （复用进行中 refresh 时，后到者持有的 Slot 克隆必须读得到首个调用者的失败）。
- `cargo clippy --all-targets`（核心）→ **0 warning**；`cargo clippy --no-deps`（壳）→ **0 warning**。
- `npx eslint .` → 0 problems；`npm run version:check` → 5 处一致（**1.0.0**，本轮未推进）。
- `npm run build` → **exit 0，无构建错误**：vite 31 模块（`dist/` 78.76 kB js + 8.40 kB css）→
  cargo release → `bundle/macos/WB Bridge.app`（6.42 MiB）+ `bundle/dmg/WB Bridge_1.0.0_aarch64.dmg`
  （3.34 MiB），ad-hoc 签名、跳过公证（无 APPLE_ID/…）。

### 本轮修的缺陷（均为行为修复，逐条读过源码与归档 JS）

- **面板丢状态**：壳把 `activity` / `modelResults` 拆成 `core-activity` 发出，但 `src/core/bridge.js`
  从未监听它，`core-status` 收到的又是剥掉这两字段的轻量快照 → 逐模型明细与活动文案恒为空。
  现在 `bridge.js` 缓存最近一次完整状态并把两路合并后下发，`onState` 订阅时直接回放缓存
  （原先那段 `getCurrent…emit('wb-bridge/replay-request')` 调的是 Tauri 2 里不存在的 API、且全仓
  无监听者，已删）。事件名与 payload 形状未改。
- **系统代理开关必然失败**：面板发裸布尔，核心按 `docs/contract.md` 读 `{enabled}` →
  「代理开关必须是布尔值」。改为 `run('system-proxy', { enabled: $event })`（面板侧对齐契约，
  核心路由与字段名不动）。
- **refresh 的 spinner 永不显示**：`busyAction` 是布尔却被与字符串 `'refresh'` 比较。改为持有动作名，
  并把传给 Boolean prop 的 `:disabled` / `:busy` 统一 `!!busyAction`，避免 Vue 的 prop 类型告警。
- **托盘吞错**：`src-tauri/src/lib.rs` 两处 `let _ = admin_call(...)` 改为失败时 `eprintln!` 记录原因
  （GUI 打包后 stderr 不进终端，可观测性有限——面板侧可见的托盘错误提示需要新的展示位，未做）。
- **refresh 复用把失败说成成功**：后到者只 `wait_for(done)` 就返回 `Ok({count})`；JS 里大家 await 同一个
  promise、rejection 会传播给每个等待者。`Slot` 增设 `outcome`（Arc 共享，克隆可见），任务先写结果
  再置 done，复用路径原样返回该结果。
- **串行写链分叉**：`persist_status` / `chain_sync` 原本「读 predecessor」与「写回链头」分两次加锁，
  两线程可读到同一个 predecessor 并各自只等它 → 两条分支并发执行、后登记者覆盖先登记者的 Slot、
  `drain()` 等不到被覆盖的那条写。抽出 `reserve_chain_slot()` 在单次持锁内完成读+登记，
  status/sync/refresh 三条链统一走它。
- **文档口径纠正**：探测超时原写「整批共享 60s」，实际（与归档 JS 一致）是**每模型一份 deadline、
  重试共用**；已改 `AGENTS.md`（3 处）、`probe.rs` 模块注释与常量文档、`red_lines.rs` 断言文案。
  壳注释里指向已归档 `src/core/src/server.js` 的路径、`server.rs` 的「方案B 阶段二/三」过期术语同步更正。

### 未验证 / 未做（如实标注）

- **GUI 仍未实机启动**：上面的接缝修复全部靠源码与契约推导 + 编译/测试/打包通过，托盘、面板交互、
  代理开关的实际点击链路未经真实运行验证。
- **CI 仍未实跑**：`release.yml` 只是静态结构正确；本轮拆掉了它声称但不存在的自动更新产物
  （`**/*.sig` glob、`TAURI_SIGNING_*` 环境变量、`.tar.gz`），因为 `tauri.conf.json` 既无
  `bundle.createUpdaterArtifacts` 也无 `plugins.updater` 依赖。**自动更新功能本身仍未接线**。
- 仍存而未修的已知项：`pick_port()` 先 bind 再 drop 的端口抢占窗口；`[profile.release] panic = "abort"`
  下任何 panic 直接带走整个应用（无日志）；`.vue` 不在 `npm run lint` 覆盖范围（`eslint.config.js` 只匹配
  `**/*.{js,mjs}`）；CI 的 `node-version: 22` 与 `engines.node >= 24` 不一致。

---

## Node → Rust 核心迁移复验（2026-10-01）

### 已验证

- **测试全绿**：`src-tauri/core/` 下 `cargo test` → **195 通过 / 0 失败**（该轮基线；同日第二轮修复后为
  **197**，见上方条目），构成：lib 单测 **177** +
  `tests/js_parity.rs` **11** + `tests/red_lines.rs` **7**（根目录 `npm test` 转发到同一 `cargo test` 命令）。
  - `js_parity.rs` 不再需要 Node：期望值是迁移前用真实 JS 模块录制、冻结在 `src-tauri/core/tests/fixtures/*.json`
    的真相快照（**271 例 / 11 个 fixture 模块**：atomic 10、handoff 34、json 11、model_status 22、platform 10、
    protocol 59、reasoning 10、repair 41、sync 17、system_proxy 38、workbuddy_config 19），对拍只比较 Rust 实现与快照；重新录制需把归档的 `core/` 与
    `tests/js/` 放回原位再跑 `WB_PARITY_RECORD=1 cargo test --test js_parity`。
  - `red_lines.rs` 覆盖：全路由（含 `/health`）必须 Bearer 鉴权、任意非空 `Origin` → 403、
    体积/并发上限不放松、原生工具只允许 ask/deny、隔离配置 autoupdate=false 且两个 agent 存在、
    子进程 env 只透传白名单、同步只动 `OWNER = "buddy-bridge-v1"` 名下的条目。
- **Lint 干净**：`src-tauri/core/` 下 `cargo clippy --all-targets` → 0 warning；`src-tauri/` 下
  `cargo clippy --no-deps` → 0 warning（本次复验实测）。
- **结构迁移落位**：`src-tauri/tauri.conf.json` 的 `bundle.externalBin` 为 `[]`，`src-tauri/binaries/`
  与 Node `core/`（旧 `src/core/`）目录均已从仓库移除；核心以 path 依赖 crate `wbbridge-core`
  （lib `wbbridge_core`）静态链接进壳，编排入口 `src-tauri/core/src/orchestration.rs::run(StartOptions)`。
  壳侧 `src-tauri/src/lib.rs` 负责：专用 tokio 运行时装配核心、`pick_port()`（默认 41980，占用时回退
  系统分配端口）、`app_data_dir()` 注入数据目录、`admin_call()` 走回环 HTTP 驱动 `/admin/*`、轮询
  `status.json` 并 emit `core-status` / `core-failed`、`orchestration::set_exit_hook` 保证核心不能反向
  终止壳；面板 `src/` 为 Vue 3 + Vite（`frontendDist: ../dist`，dev 端口 41990）。
- **版本一致性**：根 `package.json` 为版本单一来源（**1.0.0**）；`node scripts/check-version.mjs` 校验
  5 处落点（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` / `AGENTS.md` 两行）全部
  通过。`src-tauri/core/Cargo.toml` 的内部 crate 版本 **0.1.0** 与产品版本**有意解耦**，不参与同步。
- **独立核心冒烟运行**（一次性临时数据目录，非 GUI）：运行时按需下载成功 → 隔离 OpenCode 起来并通过
  `/agent` 校验（`buddy-bridge` / `buddy-chat` 存在）→ 发现 8 个免费模型 → 逐模型探测 → 干净关停。
  数据目录约定（`api-key` / `status.json` / `service.pid` / `opencode.log` / `runtime/` / 隔离 XDG 目录）
  由 `src-tauri/core/src/platform.rs`、`orchestration.rs` 与相应单测覆盖，本次冒烟未逐项复核其权限位。

### 未验证（如实标注，不得当作已验证）

- **迁移改动尚未提交**：`src-tauri/core/`（Rust 核心整体）、Vue 面板新文件（`src/App.vue` 等）、
  `docs/contract.md`、`vite.config.js`、`eslint.config.js`、`scripts/{bump,check}-version.mjs` 都还是未跟踪状态；
  旧 JS 核心（HEAD 里的 `src/core/*.js`）、`src/ui/`、`scripts/build-sidecar.mjs`、`docs/brand/` 的删除也仅在工作区生效
  （`src-tauri/binaries/` 本就未被跟踪，随工作区删除即消失）；提交与否由用户决定。
- **GUI 桌面应用从未实机启动过**：迁移后只做过编译 + 上述独立核心冒烟；托盘、面板交互、壳侧
  `restart_core` / 优雅退出链路均未经过真实运行验证。
- **迁移后未产出过安装包**：`.app` / `.dmg` / Windows / Linux 包待 `npm run build` 与 CI 实跑后复验。
- **重写后的 `.github/workflows/release.yml` 从未在 CI 运行**：仅本地做过 YAML 结构校验。
- **`cargo fmt --check` 全仓不干净**，未纳入门禁；当前风格门禁只有 clippy 0 warning。

### 环境遗留结论

- 2026-09-30 记录的「本机 DNS 把 `opencode.ai` 解析到证书 SAN 不含该域名的地址，导致模型探测
  `ERR_TLS_CERT_ALTNAME_INVALID`」仍是同一环境问题：Rust 核心保持 TLS 校验，不改宽松。

---

日期：2026-09-30（Node/sidecar 时代基线，本机 macOS，Apple Silicon）

## 目录重构后复验（15:08）

- 用户将 core/ui 移入 `src/` 并新增版本一致性脚本后，`npm run build`（sidecar + tauri build）与单独 `tauri build` 各复验一轮，均产出 .app + .dmg（25.02 MiB）。
- `npm run version:check`：5 处版本号一致（1.0.0）；根 `npm test`：97/97。
- 期间用户报告过一次 `bundle_dmg.sh` 失败，重构后未复现；判定为上一次中断构建残留的瞬时 hdiutil 问题。复发时的处置：确认无 `/Volumes/WB Bridge` 残挂载后删除 `src-tauri/target/release/bundle/dmg/` 重新构建。

## 自动化测试

- `core/` 测试套件：**97/97 通过**（`npm test`，node --test）。
  - 自上游参考实现（https://github.com/louchi1984-coder/ow-bridge）迁移的测试 95 项（协议校验、导入与退出清理、目录能力映射、图片转发、推理档位、系统代理解析、活动文案等），其中 2 项读取 UI 源码的契约测试已指向新 `views/` 路径。
  - 新增 2 项：`POST /admin/shutdown` 先响应后触发一次优雅退出（含鉴权拒绝）；`BUDDY_PARENT_PID` 看门狗在壳进程消失后自行优雅退出并落 `stopped` 终态。
- Rust 壳 `cargo check` 通过（macOS aarch64，tauri 2.12 依赖树）。

## macOS 本机构建

- `npx tauri build` 成功：
  - `src-tauri/target/release/bundle/macos/WB Bridge.app`（64.17 MiB）
  - `src-tauri/target/release/bundle/dmg/WB Bridge_1.0.0_aarch64.dmg`（25.02 MiB）
- ad-hoc 签名（identity "-"），跳过公证（无 Apple 凭据）。
- sidecar `wbbridge-core` 随 .app 打包并随包签名。

## 实机冒烟（`npx tauri dev`，debug 构建）

| 步骤 | 结果 |
|---|---|
| 壳启动并拉起 sidecar | ✅ 进程链：wbbridge → wbbridge-core → opencode serve |
| 数据目录落位 `~/Library/Application Support/app.wbbridge.desktop/` | ✅ api-key / settings / status.json / opencode.log / runtime |
| OpenCode 运行时按需下载 | ✅ 官方 npm 源下载 1.18.33（约 1 分钟） |
| /health 健康检查（Bearer api-key） | ✅ phase=ready，endpoint=127.0.0.1:41980/v1 |
| 免费模型发现 | ✅ 发现 8 个免费模型 |
| 模型探测 | ⚠️ 全部返回 TLS 证书主机名不匹配（ERR_TLS_CERT_ALTNAME_INVALID）——**本机网络环境问题，见下** |
| 强杀壳进程 → 看门狗 | ✅ 核心在 3 秒内自行优雅退出，status.json phase=stopped，service.pid 清理，无孤儿进程 |
| 配置清理 | ✅ 退出路径执行（本机无 WorkBuddy models.json，同步为 skipped，未产生误写） |

## 本机网络环境说明（冒烟中的模型探测失败）

- 系统解析器（dscacheutil）对 `opencode.ai` 持续返回 `141.193.154.70`，该地址的 TLS 证书 SAN 仅为 IP 自身，不含 `opencode.ai`，导致 Node fetch（保持 TLS 校验）报 `ERR_TLS_CERT_ALTNAME_INVALID`。
- 运营商 DNS 与路由器 DNS 直接查询均返回正确的 Cloudflare 段地址（172.65.90.20–23，证书 SAN 含 opencode.ai），且本机 `/etc/hosts` 无相关条目；本机装有系统管理描述文件（ManagedSettings profile）。
- 结论：为该 Mac 的系统级 DNS 处理（过滤器/描述文件）所致的**环境问题**，与 WB Bridge 及上游参考实现的代码无关；同一环境下上游行为一致。TLS 校验失败而拒绝连接恰说明安全行为正常。换用正常 DNS 的网络（或在无过滤器的环境）即可通过探测。

## 交付边界（如实区分）

- 已验证：macOS ARM64 包的构建、启动、模型发现、生命周期与退出清理；全部核心测试。
- 未验证（本机无法执行）：Windows x64/ARM64 安装包、Linux x64/ARM64 AppImage/deb 的实机运行——由 GitHub Actions 构建产出后需在实际系统上冒烟。
- 未处理：Apple 公证与 Windows 发布者签名（与原版一致，发布说明中已注明放行方式）。
