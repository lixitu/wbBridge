# AGENTS.md — WB Bridge 桥接服务 (wbBridge)

## ⚠ 强制规范（所有 Agent 必须遵守）

**本项目是「Tauri 桌面壳 + 同进程内嵌 Rust 核心」的桥接工具。核心（`src-tauri/core/`，crate 名 `wbbridge-core`）作为 `path` 依赖静态编进壳（`src-tauri/`）里，跑在壳专用 tokio 多线程运行时上，仍在 `127.0.0.1:41980`（端口被占则由壳另择端口）提供 OpenAI 兼容 API，把**隔离托管**的 OpenCode 免费模型发布给 WorkBuddy；控制面板 `src/`（Vue 3 SFC + Vite 构建，产物 `dist/`）由 Tauri WebView 加载，经 `invoke('core_action' | 'restart_core' | 'core_running' | 'data_dir_path' | 'read_log')` 与 `core-status` / `core-activity` / `core-failed` 事件驱动。**

> 🔴 **已彻底移除 Node 运行环境**：不再有 Node sidecar、`@yao-pkg/pkg`、esbuild 预打包，不再有 `src-tauri/binaries/`（`externalBin` 为空数组）。Node 仅用于前端构建（Vite）与 `scripts/*.mjs`。原 JS 核心与其测试已归档到仓库**之外**：
> `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`（详见该目录 README）。
>
> 🔴 **所有 Agent（包括 file-agent、browser-agent、computer-agent 等一切主 Agent、Sub-Agent、子代理）在本项目中执行任何任务时，必须无条件遵守本 `AGENTS.md` 定义的全部规则，不得以任何理由违反。**
>
> 🔴 **文件操作根目录**：本项目所有文件操作默认以 `/Users/wbtrex/website/localServer/node/trexwb/git/wbBridge` 为根目录，**不得偏离**。
>
> 🔴 **根 `package.json` 存在且是版本单一来源**：`npm run test`（转发 cargo）、`npm run test:prefs`（面板偏好单测）、`npm run test:manifest`（更新清单单测）、`npm run test:updater-key`（签名注入包装器单测）、`npm run lint`、`npm run vite:build`、`npm run build`（= `vite:build && tauri:build && make:dmg`；`tauri:build` 是 `node scripts/with-updater-key.mjs tauri build` 的签名注入包装器）、`npm run make:dmg`（hdiutil 生成 macOS 的 `.dmg`，非 mac 平台自动跳过）、`npm run tauri:dev|build`、`npm run version:set|check`、`npm run gen:latest`（更新清单）。Rust 侧命令一律 `--manifest-path src-tauri/core/Cargo.toml` 或进 `src-tauri/`；全部 Rust 代码在 `src-tauri/` 下（壳 = `src-tauri/src/`，核心 = `src-tauri/core/`）。**不存在** Node 版的 `src/core/` 与根级 `core/`（那是已归档的 JS 核心），严禁臆造不存在的脚本、命令与路径。
>
> 🔴 **实读优先、禁止猜测**：本文件所有结论均以仓库真实代码为准。修改任何模块前必须先 `read` 读取原文，禁止凭记忆推测函数名、常量名、路由、错误码与配置字段。
>
> 🔴 **最小改动优先**：只做针对性修复，禁止重构、禁止大范围重写、禁止"顺手优化"。
>
> 🔴 **密钥零泄漏**：不得读取、打印、回显、提交或推断 `api-key`、`.env.local`、`providers.json`（用户自持的平台 Key）、用户 `~/.workbuddy/models.json` 中的凭据字段。**允许位（2026-10-03 多平台接入后新增）**：核心可以把**用户自己提交进来**的平台 Key 写到数据目录的 `providers.json`（`0600`，唯一入口是 `POST /admin/set-provider-key` 的请求体，唯一读者是 `providers.rs`）；除这一处以外，任何凭据都不得落盘、出日志、进 `status.json`、进面板偏好、进子进程环境变量，或出现在任何响应体里（`provider-status` 只回「是否已配置」）。
>
> 🔴 **提交由用户决定**：Agent 完成改动后不得自动执行 `git commit` / `git push`，除非用户在本轮明确要求。

## 当前状态与验证边界（必须如实转述，不得伪装成已验证）

| 项 | 状态 |
|---|---|
| `src-tauri/core` 单元测试 + 对拍 + 红线 | ✅ 已执行并通过（见「测试基线」） |
| `src-tauri/core` 独立进程冒烟（真实下载 OpenCode → 隔离启动 → `/agent` 校验 → 刷新 8 个免费模型 → 探测通过 → 鉴权 401/403 → `/v1/models` → 优雅关停） | ✅ 已在一次性数据目录实测通过 |
| `cargo clippy --all-targets`（src-tauri/core）/ `cargo clippy --no-deps`（src-tauri） | ✅ 0 warning |
| 实际启动 GUI（`npm run tauri:dev` / 打包后的 .app）并操作托盘与面板 | ❌ **未实测**。壳改动只能以「编译通过 + 核心独立运行行为」为证据，必须显式告知用户未做 GUI 验证 |
| 本轮壳侧改动（async 命令 + `spawn_blocking`、启动失败反复播报 `core-failed`、`startup_error`、**关窗即退出 `quit_app`**） | ❌ **同样未做 GUI 实测**，证据只有 `cargo clippy --no-deps --all-targets` 0 warning 与 `cargo test --lib`（`src-tauri/`）9 通过。关窗行为、托盘驻留取消、macOS 红按钮语义都属**必须实机点一遍**的类别 |
| 轮询与渲染降耗（壳 `status.json` 的 `(mtime, 长度)` 前置过滤、核心去掉未使用的 `brotli` 解压、面板 `bridge.js` 帧级合并、列表/详情 props 引用稳定） | ⚠ **复审后已修掉两处会丢状态的缺陷并补单测**（2026-10-03 代码复审，见 `docs/validation.md`）：`meta.modified()` 取不到时**不再启用快路径**（原先 `unwrap_or(UNIX_EPOCH)` 会让长度不变的改写被永久跳过、面板状态冻结），恢复运行分支**同时作废 stamp**（原先只清 `last`，会吞掉「重启后内容可能与故障前逐字节相同也要重推」这道保障）。面板侧配套：替换型事件入队前先清空队列（窗口不可见、rAF 停摆时原先无上限增长），并把「合并语义完全等价」的注释更正为「订阅者等价、`lastState` 滞后一帧」。降耗效果与 rAF 在 WKWebView 的真实停摆**仍未实机验证** |
| 迁移后产出安装包 | ⚠ **只有 macOS aarch64 已实测**。2026-10-03 本机按 `npm run build` 的三个环节逐段跑通（`vite:build` → `tauri:build -- --bundles app` → `make:dmg`，退出码均 0）：updater 包 `bundle/macos/WB Bridge.app.tar.gz`（3,596,811 B，**文件名不带版本与架构**，正是 CI 里必须补架构后缀的那个实测事实）+ 配对 `.sig`（428 B），`.dmg` 由 `scripts/make-dmg.sh`（hdiutil）产出 `bundle/dmg/WB Bridge_1.0.2_aarch64.dmg`（约 3.9 MB），只读挂载后内含 `WB Bridge.app` + `Applications` 快捷方式，`codesign --verify --deep --strict` 通过（adhoc + hardened runtime，**未公证**，`spctl` 判定 rejected 属预期）。⚠ 本机 macOS 27 上 `hdiutil create/attach/detach` 会打一句 `deprecated, please use diskutil image …` 的**警告**，产物正常，暂不改造。其余五平台（macOS x86_64 / Windows / Linux）本机不具备交叉构建条件，**已由 2026-10-03 的 CI 完整一轮产出并随 Release `v1.0.2` 发布**（六平台资产名与 `.sig` 均已实测，见下一行），但**六个包都没在实机上装过** |。
| updater 接线（`tauri-plugin-updater` + `tauri-plugin-process`、`createUpdaterArtifacts`、`plugins.updater` 端点与公钥、面板更新区、CI 的 `.sig` glob 与 `update-manifest` 作业） | ⚠ **签名侧已验证、下载侧刚发现一个 P0 缺陷（已修脚本，清单尚未重传）**。已验证：壳编译与 clippy、`npm run test:manifest` 9 通过、workflow YAML 结构、本机真实签名构建，以及**首轮完整 CI（2026-10-03，tag `v1.0.2`，run `37092915120`，8/8 作业全绿，Release v1.0.2 已 Publish、23 个资产）**——实测拿到：六平台资产齐全（mac `WB.Bridge_1.0.2_{aarch64,x86_64}.dmg` + `WB.Bridge_{aarch64,x86_64}.app.tar.gz`、win `…_{x64,arm64}-setup.exe` + `.msi`、linux `…_{amd64,arm64}.AppImage` + `.deb`）、**Linux 的 updater 包就是裸 `.AppImage`（没有额外一层 `.AppImage.tar.gz`）**、`.sig` 与资产同名的配对形态、以及**六条清单签名的签名者 key ID 全部 = `2B11F78BEA8A43F`（= 配置里唯一那条公钥，`fmt=4544`、74 字节 Ed25519 盒）**。🔴 **同轮实测出的缺陷**：GitHub 把 Release 资产名里的**空格规范化成 `.`**（磁盘 `WB Bridge_…` → API `WB.Bridge_…`），而 `gen-latest-json.mjs` 按本地文件名 `encodeURIComponent` 拼出的 `%20` URL **六个平台全部 404**（浏览器同源 fetch 实测：空格形式 404、点形式 302 到 CDN）。已改为 `name.replace(/ /g, '.')` 后再编码，并把该规则写进 `test:manifest` 的成功路径断言；`release.yml` 的 `update-manifest` 作业新增一步「校验清单 url 指向的资产在 Release 上真的存在」，用 API 逐条对账资产名，对不上即 exit 1（这类错误清单自己生成得出来、签名也对得上，只有客户端下载到那一步才炸）。**仍未验证**：修正后的 `latest.json` 已在本机对 v1.0.2 实际资产名跑通对账（6/6 OK），但**线上那份仍是坏的**（重传属共享状态，待用户操作）、真实下载与安装、`relaunch()` 之后壳能否带已更新的核心正常重启（属必须实机点一遍的类别）。⚠ 2026-10-03 两处更正：① 那次本机签名用的 `wbBridge-updater-20261001.key` **已不在 `~/.tauri/`**，现按用户决定改用 9-30 的 `wbBridge-updater.key`（口令非空），CI 变量改走 Variables；② `plugins.updater.pubkey` 里**只有第一条公钥会被读取**（不存在「多条＝轮换白名单」），所以随换钥把配置里的公钥换成 `2B11F78BEA8A43F` 那一条，构建期 `does not match the public key` 告警随之消失、`.sig` 的签名者 key ID 也与配置一致；因为 v1.0.0/v1.0.1 两个标签都早于 updater 接线，这次替换不影响任何已交付的二进制。详见「签名与发布」 |
| 多平台接入 Stage 3 + Stage 5（Key 注入 + 聚合发现 + 空集闸门 + 面板「平台」视图，2026-10-09） | ⚠ **逻辑已测、GUI 与真实 Key 未实测**。✅ 已实测：同日上午四轮 /tmp 沙箱对照实验（真实 OpenCode 1.18.35，与核心同款隔离方式）证得：`/provider` 无条件返回全部 226 家 catalog（注入不是平台出现的前提，声明段只负责鉴权）；四家 cost 字段随 catalog 合并保留，CostZero 判定直接复用（免费模型 ModelScope 7 / SiliconFlow 3 / 腾讯 2 / 智谱 3）；最小声明段 `{npm, options:{baseURL, apiKey}}` 足够，**Stage 4 注册表模型清单不再需要**；`/provider` 不回显 apiKey；OPENCODE_CONFIG_CONTENT 通道下两个自定义 agent 可见。落地：`runtime.rs` `isolated_config(providers_section)` + `providers_section_for`（Key 只经 CONFIG_CONTENT 进子进程）、`backend.rs` `configured_providers` + 聚合发现（单平台失败不丢其他平台）、`orchestration.rs` `sync_published` 加 `allow_empty`（探测/导入 false，关停/清旧/换文件 true；空集提前拒绝写 sync.error）、面板 `ProvidersView.vue`（四家卡片 + password 输入保存后即清空不回显 + 官方申请入口已核实 + 重启提示）、VIEW_IDS/SideBar/App 接线、`ModelRow.vue` 改渲染 `model.id`（Stage 5 已知偏差清除）。核心 cargo test **243 通过 / 0 失败**（lib 221 + js_parity 11 + red_lines 11，新增 5 项）、两侧 clippy 0 warning、fixtures diff 为空、壳 9 通过、JS 8/9/13、eslint/vite:build/version:check 全绿；版本随 1.1.0 推进（维护者裁定 minor 位）。✅ 2026-10-09 晚补丁（1.1.2）：修复 modelscope 全模型报 `AI_APICallError: Model id : <id> , has no provider supported` ——六组沙箱对照实验收敛结论：对 models.dev 在册 provider，经 `OPENCODE_CONFIG_CONTENT` 注入的声明段**必须带非空 `models` 段**（无 models 键 → ai-sdk 层 provider 工厂解析失败；空对象 → catalog 合并被清空、发现 0 模型）。修复形态：注入锚点条目 `models: { "wbbridge-provider-anchor": {} }`（保留名，OpenCode 为其合成的幽灵模型 cost 全 0，由 `free_models_in` 按保留名过滤，新增 2 项单测钉住注入形状与过滤）。端到端已验证（新二进制 + 真实运行时 + modelscope 假 Key）：探测打到真实 API、返回上游鉴权错误（预期），`has no provider supported` 消失。🔴 **仍未验证**：GUI 实机（平台视图/保存/热生效链路）、真实 Key 端到端（探测通过/真实对话/发布）、Key 变更热生效、探测额度消耗（≈15 模型） |
| 重新检测全程「检测中」+ 面板反馈/节流补齐（2026-10-09，v1.1.2 轮内追加；原记 v1.2.0 已作废） | ⚠ **逻辑已测、GUI 未实测**。核心：`orchestration.rs` 新增 `single_probes` 集合与 `note_single_probe`（在 `/admin/probe` **响应之前**登记、spawn 的探测任务结束时撤销），状态顶层新增加法式字段 `singleProbes`（`STATUS_SCHEMA_VERSION` 保持 1）。之所以是独立顶层键而不是塞进 `probe`：`apply_patch` 对顶层键整体替换，批量探测每检完一个模型就重写整份 `probe`，放进去会被互相覆盖。面板：`ModelRow.vue` 的 `reprobing` prop 驱动行内 spinner + 「检测中」文案 + `aria-busy` + `cursor: progress`，按钮在忙态禁点；`App.vue` 用「核心 `singleProbes` ∪ 本地已点击集合」的并集，本地标记只在请求 ACK 前或请求失败时存在，因此**不可能卡死**，同一模型重复点击被去重；`run()` 的重入拒绝与探测拒绝（202 带 `{error}` / `{started:false}` 这类**不会被 `ok:false` 捕获**的响应）改为显式提示；`pendingText` 区分「发起重新检测」与整批探测；FeedbackBar 在除「模型与服务」外的所有视图都可见（此前代理开关在平台/日志/用量/关于里失败了没地方显示）；「导入」按钮的 spinner 改按 `busyAction === 'import'` 判定（原先任何动作在跑都点亮它）；`ProvidersView.vue` 的保存/清除/逐平台应用互相排斥并在按钮上给「应用中」。✅ 已实测：核心 `cargo test` **245 通过 / 0 失败**（lib 223 + js_parity 11 + red_lines 11，本轮新增 2 项：在飞集合的幂等增删、批量探测重写 `probe` 时 `singleProbes` 不受影响）、`git diff --stat src-tauri/core/tests/fixtures` 为空、两侧 clippy **0 warning**、壳 `cargo test --lib` 9 通过、三组 JS 单测 8 / 9 / 13、`npx eslint .` 退出 0、`vite:build` 通过、`version:check` 5 处一致（**1.1.2**；此前误判为 minor 并推进到 1.2.0，已按维护者裁定回退）。🔴 **未验证**：真实 GUI 里点击「重新检测」到探测结束的行内忙态、批量探测与单模型探测并发时的展示、`providers.json` 保存后「应用中」的自动 refresh 链路，都属必须实机点一遍的类别；60s 探测预算内的等待体验（是否还要进度）待实机后再定 |
| 启动沿用 + 定向重检 + 单份备份（v1.1.2 轮内追加，2026-10-09 用户裁定的三项优化；原记 v1.3.0 已作废） | ⚠ **逻辑已测、GUI 与真实 Key 未实测**。① **改一家 Key 只重取/重检那一家**：`/admin/probe` 的载荷扩展为 `{ model?: string, providers?: string[] }`（`ProbeFn` 因此收**整份请求体**，路由与壳的 `ADMIN_ROUTES` 未动），`orchestration.rs` 新增 `Scope{All,Platforms}` + `merge_scoped_models`/`strip_validated`/`probe_platforms`，`refresh`/`run_refresh`/`start_probes` 全程按范围分流：其余平台的目录与探测结果**一概不动**，只有本平台名下的「已校验」标记被摘掉重新探测；定向读取会把合并后的整份目录写进 `status.json`（全量路径才走逐条揭示）。面板 `ProvidersView.vue` 的保存/清除改发 `probe {providers:[id]}`，不再叠一轮全量 `refresh`（`applyAll` 作为兜底保留）。🔴 契约变化：`providers` 形态不对（非数组/空数组/未注册 id/与 `model` 同时给）一律**当场拒成 202 + `{error:{message,type:"invalid_request_error"}}`**，绝不静默降级成全量——后者会按整份目录消耗探测额度（每模型 60s 预算）；`probe_platforms` 在回应前也查「正在探测/正在读取」，否则用户会看到「已应用」却什么也没变。② **启动沿用上次结果**：`bootstrap` 用 `restored_models`/`validated_from_results` 把上一份 `status.json` 的目录、`modelResults`、`validated` **三份一起**恢复（只恢复其一会得到「面板有模型、请求全 404」），`reusable_previous`（目录非空且至少一个 `ok===true`）成立时 `startup_sequence` 跳过「清空发布集」与自动全量重读+重检，改为按**当前**端点与 Key 直接 `sync_published(None,false)` 重发一遍配置（顺带修掉端口漂移与 api-key 轮换后插件里仍是旧地址）；`shutdown` **不再清空**插件配置。代价如实记录：应用没在跑的那段时间，WorkBuddy/CodeBuddy 里本工具名下的模型指向已停掉的端点，请求会失败。首启/上一轮全败/`models` 非数组 → 完全不沿用，逐字节走老路径。③ **插件配置只留一份备份**：`sync.rs::prune_old_backups` 在每次成功写出备份后，按段白名单（`<file>.buddy-bridge-<纯 ASCII 数字>.bak`，跳过目录、`read_dir` 失败静默）删掉同族的旧备份，只留最新那一份；备份**文件名形态未变**（对拍夹具冻结了它，故不改名而是清理累积）。✅ 已实测：核心 `cargo test` **253 通过 / 0 失败**（lib 231 + js_parity 11 + red_lines 11，本轮新增 6 项）、`git diff --stat src-tauri/core/tests/fixtures` 为空、两侧 clippy **0 warning**、壳 `cargo test --lib` **9 通过**、四个 JS 套件 8 / 11 / 9 / 13、`npx eslint .` 退出 0、`vite:build` 通过、`version:check` 5 处一致（**1.1.2**）。🔴 **未验证**：GUI 实机（保存 Key 后只有那一家的模型变化、重启应用后面板直接是上次的列表且可用、插件配置文件不再堆积备份）、真实 Key 的端到端、沿用后的模型在核心重启过（端口/Key 变了）之后请求是否真通——都属必须实机点一遍的类别 |
| 地区不可用的 OpenCode 模型检测失败改为可读中文说明（2026-10-09，v1.1.2 轮内追加） | ⚠ **逻辑已测、GUI 未实测**。用户实测到 `This model is not available in your country`：这句**不是本项目产生的**（全仓 grep 无此文案），是上游（OpenCode 的 zen 网关与其背后提供方）按**出口 IP 的国家/地区**给的拒绝，经 `backend.rs::to_bridge_error` 原样透传成 `code = upstream_error`。既有行为本来正确——`upstream_error` 不在 `RETRYABLE_PROBE`（只 `probe_mismatch`/`no_action` 重试），所以不重复烧额度、该模型判不可用、不进发布集、不牵连其它已发布模型；难用的只是「面板显示一句没人看得懂的英文」。落地：`probe.rs` 新增 `region_unavailable_message`，`probe_failure` 在**未超时**分支上做一次文案改写（`code`/`status` 逐字保留，两条探测入口 `run_probe_batch`/`run_single_probe` 都过这个唯一漏斗），命中条件是「被拒词」与「地区词」同时在场（不限次序、间隔 ≤60 字符）或上游给出机器码 `unsupported_country_region_territory`；改写后的文案保留上游原文并给出唯一可操作入口（侧栏「运行设置 → 使用系统代理」换出口后重新检测；`ENV_ALLOW` 不含代理变量，所以在终端 `export https_proxy` **不会**透传给子进程，只有该开关有效）。判定刻意收紧以免误伤：`429 Too Many Requests`、`invalid api key`、`context length exceeded`、`模型只返回了文本…`、`The model is not available right now` 都不得被说成地区问题。🔴 **已知边界（本轮未做，属用户未要求的范围）**：若同一句地区拒绝是以**正常文本内容**回传（不是 HTTP 错误），`judge_probe` 会得到 `no_action` → 走「按仅对话发布」那条分支，模型仍会被发布并在真实请求里继续失败；今日用户看到的是英文原文出错的错误路径，故未动该分支。✅ 已实测：核心 `cargo test` **255 通过 / 0 失败**（lib 233 + js_parity 11 + red_lines 11，本轮新增 2 项，均在 `probe.rs`）、`cargo clippy --all-targets` **0 warning**、`git diff --stat src-tauri/core/tests/fixtures` 为空、`version:check` 5 处一致（**1.1.2**）。🔴 **未验证**：面板里这条中文文案的实际排版（详情栏宽度下的换行）、以及打开系统代理后这些模型是否真能检测通过——后者取决于代理出口地区，属必须实机点一遍的类别 |
| 「不支持函数调用」的上游拒绝改走 chatOnly 降级（2026-10-09，v1.1.2 轮内追加；2026-10-10 扩到 OpenCode：兜底 chat-only 也以同理由被拒时确认仅对话，并放行被 `StructuredOutputError` 吞掉的同义文案） | ⚠ **逻辑已测、GUI 与真实 Key 未实测**。用户实测到 SiliconFlow 若干模型检测报 `Bad Request: Function call is not supported for this model.`，OpenCode 部分模型随后也复现该句。**这不是模型坏了**：探测请求带工具目录，而目录里的 `capabilities.toolcall` 是 models.dev 的**声明**值（发现阶段来自 OpenCode `/provider` 无条件返回的静态 catalog，见「多平台接入 Stage 3 + Stage 5」行记录的 2026-10-09 沙箱实测结论），声明为真、平台实际不给调用时就回这句——语义恰好等于「只能对话」。此前它的 code 是 `model_error`/`upstream_error`（`to_bridge_error` 的默认归宿，backend.rs:1778），既不在 `format_unsupported` 的码表里也匹配不上那条 `tool_choice` 正则，于是走「直接判不可用」分支：**一个能正常对话的模型被整个丢弃**。而目录声明 `toolcall=false` 的模型走的是 `invalid_tool_call` → 同一个 chatOnly 降级，两条入口归宿不一致就是本次要补的差集。落地：`probe.rs` 新增 `tool_call_unsupported`，并到 `format_unsupported` 的判定里（两个探测入口 `run_probe_batch`/`run_single_probe` 共用这一个漏斗，未新增分支）；命中后由既有 `chat_only_attempt` 向该模型发**一次真实的纯对话请求**，通了才按 `chat_only` 发布、面板注记「模型不支持函数调用，已按仅对话发布」（不再回显吓人的上游原文）；**兜底 chat-only 请求也以「不支持函数调用」被拒时同样按 `chat_only` 发布**——主探测与兜底指向同一结论，OpenCode 的 chat-only 通道对该模型仍走工具路由，模型本就只服务于对话通道，不得再整个丢弃。仅当兜底报的是**别的**错（鉴权/404/限流…）才照旧记失败并保留上游错误——所以判定即使误伤也不会凭文案把模型发出去。另：`backend.rs` 的 `info.error` 处理曾对 `name == "StructuredOutputError"` 整段跳过（交给下游 decode/repair），OpenCode 把「不支持函数调用」包成 `StructuredOutputError` 时会被吞成泛化的「未返回信封」、使 `tool_call_unsupported` 永远拿不到原文、降级触发不了；现已在「实质是不支持函数调用」时放行给降级逻辑（`probe.rs::tool_call_unsupported` 复用于此处判定）。判定收紧在「必须是 `function|tool` + `call|calling|use` 的复合形态」且与「不支持」措辞同时在场、间隔 ≤40 字符：`invalid api key`、`429`、`context length exceeded`、地区拒绝、`Chat-only model attempted native tool use; execution blocked` 都逐条断言不得命中。行为不变项：这句仍**不重试**（`upstream_error`/`model_error` 都不在 `RETRYABLE_PROBE`）、仍不撤销其它已发布模型、探测主路径仍带 `probe: true`（禁辅助模型转写）。✅ 已实测：核心 `cargo test` **258 通过 / 0 失败**（lib 236 + js_parity 11 + red_lines 11，本轮新增 3 项：probe.rs 2 + 兜底确认 1）、`cargo clippy --all-targets` **0 warning**、`git diff --stat src-tauri/core/tests/fixtures` 为空、`version:check` 5 处一致（**1.1.2**）。🔴 **未验证**：这些模型在 WorkBuddy/CodeBuddy 里作为「仅对话」模型被真实使用的表现（无工具能力，插件侧发起的工具调用会走 `native_tool_activity`）、面板注记文案在详情栏宽度下的排版、以及 SiliconFlow 是否还有别的措辞形态未被覆盖——都属必须实机点一遍的类别。另：`functions are not supported`（复数单独成词）这一形态**刻意不覆盖**，放宽到裸 `functions`/`tools` 会误伤鉴权与限流文案 |
| 提供方撤架模型的上游失败改为可读中文说明（2026-10-09，v1.1.2 轮内追加） | ⚠ **逻辑已测、GUI 未实测**。用户实测到 `Model exo-free has been deprecated.`。**归类本来就正确**，缺的只是可读性：目录的 `status` 只在**逐字等于** `"deprecated"` 时被 `free_models_in` 过滤（`backend.rs:131`），而 models.dev 的声明落后于提供方撤架，这类模型因此仍进探测队列并在这里失败；`model_error`/`upstream_error` 不在 `RETRYABLE_PROBE` → 不重试、不发布、不牵连其它已发布模型，真实请求路径还会把它从 `validated` 摘掉（永久失效，正该撤下）。落地：`probe_failure` 收拢成**上游英文原文的唯一改写漏斗**（地区拒绝之后接新函数 `deprecated_model_message`，`code`/`status` 逐字保留，两个探测入口 `run_probe_batch`/`run_single_probe` 都过它、无新分支），改写后给出「这不是 Key 或本工具的问题：它不会被发布，也不会重复消耗探测额度」。判定要求 `deprecat(ed|ing|ion)` 与「`model`/`provider`/`endpoint`/`version` 类主语」**同时**在场且间隔 ≤60 字符：`This config field is deprecated, use the new one`、`invalid api key`、`429 Too Many Requests`、`context length exceeded`、地区拒绝、`Function call is not supported for this model.` 都逐条断言不得命中。✅ 已实测：核心 `cargo test` **259 通过 / 0 失败**（lib 237 + js_parity 11 + red_lines 11，本轮新增 2 项，均在 `probe.rs`）、`cargo clippy --all-targets` **0 warning**、`git diff --stat src-tauri/core/tests/fixtures` 为空、`version:check` 5 处一致（**1.1.2**）。🔴 **未验证**：这条文案在详情栏宽度下的排版、`exo-free` 之外的撤架模型是否还有别的措辞形态未被覆盖、以及 OpenCode 目录会不会在后续版本把它标成 `deprecated` 从而在发现阶段直接消失——都属必须实机点一遍的类别 |
| 旧版攒下的插件配置备份：改为**完全不写备份**并清扫存量（2026-10-10，v1.1.2；上一轮的「收敛到最新一份」作废） | ⚠ **逻辑已测、GUI 与真实插件目录未实测**。用户反馈「几天下来光备份文件都有几十个」后追问了一句「我认为都不用备份 models.json」，并选定「**完全不写，只清理存量**」+ 存量 `.bak`「**删掉**」。⚠ 上一轮的做法（`prune_old_backups` 写出备份后只留一份、`prune_to_latest_backup` 无变化时收敛到最新一份）**本轮整体作废**，两个函数与 `SyncOutcome.backup` 一并删除。落地：`sync.rs` 写路径**不再有备份区块**，保护只剩「文件锁 + 写前二次读取 + 原子替换」（`atomic_write_with` 先写同目录临时文件再 `rename`，不可能写出半个文件）；`sweep_old_backups(file)` 在**两条出口都调用**（`atomic_write_with` 成功之后、以及 `changed: false` 早退之前）——启动沿用让「无变化」成为常态，只挂在写盘之后那些存量文件永远删不掉；判定仍是**逐段白名单**（`backup_prefix` + `backup_of`：文件名以 `<配置文件名>.buddy-bridge-` 开头、以 `.bak` 结尾、中段全是 ASCII 数字、必须是普通文件，跳过目录、`read_dir` 失败静默），用户自己命名的 `*.bak`、`*.tmp`、乱码中段一律不碰，**绝不放宽成通配删除**；WorkBuddy 与 CodeBuddy 共用这条写路径，两边一起生效。🔴 契约变化：`sync_models` 返回值不再有 `backup` 键，`status.json` 的 `sync.targets.<目标>` 在 `ok` 时只带 `count/changed`（`backup` 键消失）——**加法式变更，`STATUS_SCHEMA_VERSION` 保持 1**，面板从未读过该键（`src/` 全量 grep 无引用），旧壳忽略未知/缺失键。🔴 对拍夹具按用户裁定**如实分叉**：`tests/fixtures/sync.json` 的两个写盘用例去掉 `expected.value.backup` 与 `.bak` 文件条目、用例名改为「（Rust 侧不再留 .bak）」，`doc` 一句写明「JS 留备份、Rust 不再产生任何备份」是有意的行为分叉而不是为了让测试通过——**因此本轮 `git diff --stat src-tauri/core/tests/fixtures` 不为空**，这是预期结果，不得据此判失败。✅ 已实测：核心 `cargo test` **259 通过 / 0 失败**（lib 237 + js_parity 11 + red_lines 11；删掉 2 项「写备份/保留最新一份」的旧断言、新增 1 项「有变化的同步不写备份且把同族三份存量一次清干净、不匹配白名单的三个文件一个不少」，另 2 项原有断言改按新行为收紧、无变化收敛那项保留）、`cargo clippy --all-targets` **0 warning**、壳侧 `cargo clippy --no-deps --all-targets` **0 warning**、壳 `cargo test --lib` **9 通过**、`version:check` 5 处一致（**1.1.2**）。🔴 **未验证**：真实插件目录里几十个存量备份被一次同步清干净的效果、**必须重新构建并安装 1.1.2 才生效**（旧包仍在继续写备份）、以及删掉备份后用户对「写坏了能不能恢复」的真实反馈——都属必须实机点一遍的类别。⚠ 风险提示（已如实告知用户）：从此插件配置被本工具写坏时**没有自动回滚点**，唯一的兜底是原子替换本身 |
| 插件目录已在、但 `models.json` 缺失时补建空配置（2026-10-10，v1.1.2 轮内追加） | ⚠ **逻辑已测、真实插件目录未实测**。场景：装了 WorkBuddy/CodeBuddy 但从未保存过自定义模型，目录在而配置文件不在，此前被判「未检测到」、永远等不到发布。落地：`workbuddy_config::ensure_models_file` + `EMPTY_MODELS_FILE_TEXT`（`[]\n`——与 `sync_models` 对「文件不存在」的既有认定一致，且能被 `validate_models_file` 接受），由两个 `resolve_models_file` 的**默认发现分支**调用（`workbuddy_config.rs` / `codebuddy_config.rs` 各一处，逐行对照）。三道闸门：① 文件已存在绝不改写（只新建、不截断）；② 父目录不存在绝不建目录——目录不在＝插件没装，替它建目录会凭空造出「已安装」的假象；③ 任何 IO 失败静默返回 `false`，检测链不因补建失败而报错、也不把「补建失败」说成「已安装」。**显式位置（`BUDDY_MODELS_FILE` / `BUDDY_CODEBUDDY_MODELS_FILE` / saved）一律不补建**——那里失效必须照旧返回 `None`（既有铁律，新增单测钉住）。✅ 已实测：核心 `cargo test` **263 通过 / 0 失败**（lib 241 + js_parity 11 + red_lines 11，本轮新增 4 项：目录在→补建且内容为空数组且既有内容不被改写、目录不在不造目录 + 显式位置失效不补建、CodeBuddy 同款对称行为、`detect_targets` 下两个空插件目录都被检出）、`cargo clippy --all-targets` **0 warning**。🔴 **未验证**：真实插件目录里补建出来的文件能否被插件自身正确读入（插件可能期望 `{"models":[]}` 对象形态，本工具按既有语义用数组形态——文件原本不存在，无从得知插件偏好）、以及补建后首次同步的真实写盘 |
| 构建磁盘占用：依赖不生成调试信息 + 一次性清理（2026-10-10，v1.1.5） | ⚠ **改动本身已生效、`tauri:dev` 未再实测**。用户要求「合并 `src-tauri/target` 与 `src-tauri/core/target` 以省磁盘」，实测**前提不成立**：两棵树的 `release/deps` 文件名交集为 **0**（同版本同依赖也各自编一份、但内容并非逐字节相同），18G 里真正重复的只有 ~0.19G，合并省不了多少却要共享一个 target 目录（并发构建互相清缓存、`cargo clean` 一边删掉另一边的产物）。按用户裁定的 A 方案做**不改架构**的那一半：`src-tauri/Cargo.toml` 与 `src-tauri/core/Cargo.toml` 各加 `[profile.dev.package."*"] debug = false`（第三方依赖不出 split-debuginfo，本机实测占 deps 的 39%~89%），壳侧另加 `[profile.dev.package.wbbridge-core] debug = true`——核心是 **path 依赖、不是 workspace 成员**，会被上面的 `*` 命中，不显式保就会丢掉全部调试帧。`[profile.release]` 一字未动。一次性清理（都在用户确认 `tauri dev` 已退出之后才做）：删 `src-tauri/target/debug`、删两侧 `target/*/incremental`，**`release/` 全程未动**（里面有已签名的交付物）。结果 14.1G → 2.8G，`*.rcgu.o` 从 46,579 个 / 7.65G 降到 754 个 / 0.15G，`.app.tar.gz` + `.sig` + `.dmg` 逐个核对仍在。🔴 **未做**：`incremental = false`（用户未批准，只在本次删了缓存）；合并 target 的方案。🔴 **未验证**：删树后**新的 dev profile 还一次都没编过**（下一次 `npm run tauri:dev` 才是首次执行），因此在 `tauri:dev` 里断点调试核心是否仍然可用、以及依赖关调试信息后首次冷编译的耗时，都要实机看。本轮另需如实记录：被指控「改造导致隔离服务启动不了」的那次故障，时间线证明与本次改动无关（故障发生在清理之前，且按新 profile 构建的应用在此之前两次启动成功），真实成因见下一行的出口/负载证据 |
| `opencode --version` 回读超时改为重试一次 + 可读中文说明（2026-10-10，v1.1.5） | ⚠ **逻辑已测、真实超时与 GUI 未实测**。用户实测到 `…/runtime/1.18.35/opencode timed out`。定位：卡住的是 `start_backend` 在 spawn `serve` **之前**的第一次 `<file> --version` 回读（`run_command` 的 15s 预算），不是下载、不是健康轮询；本机空闲复现同一句只需 0.26–0.51s，因此**不是代码回归**。环境证据：models.dev 请求 12–20s 超时、npmjs 与 opencode.ai 解析到同一 IP 并给出自签证书（DNS 被拦截/重定向）、系统代理里**没有**任何配置，同时用户本机正在跑 release 构建（CPU/磁盘占满）。落地（`runtime.rs`）：新增 `probe_version(program, run)` + `VERSION_PROBE_TIMEOUT = 15s`（**预算未放宽**，按用户裁定）+ `VERSION_PROBE_ATTEMPTS = 2` + `should_retry_version_probe`（**只有超时才重试**——退出码非 0 / 不可执行这类失败重试只会把启动耗时整整翻倍，且候选随后会被 `find_runtime` 正常判为不可用继续走下一个来源），两处一次性 `--version` 调用（`default_probe` 与 `start_backend` 的回读）都改走它；`version_probe_timeout_message` 在两次都超时后给出中文说明。🔴 契约：迁移前 JS 的 `{program} timed out` **逐字保留在句首**（下游若按这句匹配仍成立），其后追加说明；不改 `code`/`status`/`status.json` 形状，`STATUS_SCHEMA_VERSION` 保持 1；全仓 grep 确认没有任何测试、夹具或面板文案按整句匹配，故包装它是安全的。**文案按用户裁定不得把系统代理说成唯一出路**——本工具正常出网不需要代理（用户自己才有 VPN，普通用户没有），主路径是「等 CPU/磁盘空下来后点面板状态条上的「重试」」，侧栏「运行设置 → 使用系统代理」只作为「网络本来就必须走代理」时的选项，并顺带说明终端 `export` 的代理变量不会透传给子进程（`ENV_ALLOW` 不含代理变量）。✅ 已实测：新增 3 项 `runtime.rs` 单测（超时→重试一次并成功、非超时失败不重试且原样透传、两次都超时才是 2 次调用且文案把重试排在代理之前），核心 `cargo test` **271 通过 / 0 失败**（lib 249 + js_parity 11 + red_lines 11；lib 里除本轮 3 项外还有另一轮 `probe.rs` 工作带来的 2 项，不计为本轮）、`cargo clippy --all-targets` **0 warning**、`git diff --stat src-tauri/core/tests/fixtures` 为空、`version:check` 5 处一致（**1.1.5**）。🔴 **未验证**：真的发生 15s×2 卡住时的端到端表现（单测用注入的 future，没有真子进程）、这句长文案在面板里的排版、以及换出口/等编译结束后是否真能起来；壳侧本轮**没有**跑 `cargo clippy --no-deps --all-targets` 与 `cargo test --lib`——本轮未改壳代码，且跑它会把刚清掉的 ~3G debug 树重新编出来，需要用户点头 |
| 两侧 `rust-version` 统一为 `1.90`（2026-10-10，用户裁定「照你说的改，两处都设为 1.90」） | ⚠ **只是声明值，不改任何构建行为**。实读依据：核心自己依赖图的下限是 1.88（`icu_*` ← `idna_adapter` ← `url` ← `reqwest`），壳侧 tauri 2.12 一线是 1.90；`rust-version` 是**逐包声明**，cargo 从不拿它去比对依赖，所以把核心从 1.88 抬到 1.90 对本机与 CI 的构建**一个字节都不影响**（CI 用 `dtolnay/rust-toolchain@stable`，四个作业都如此）。🔴 **未验证**：本机只有 rustc 1.98.1，仓库也**没有 MSRV 作业**，因此 1.88/1.90 这两个下限都没被任何构建实证过；若日后要在旧工具链上出货，必须先把 1.90 真的跑一遍。代码侧唯一相关点：壳 `lib.rs` 用的 `usize::is_multiple_of` 需要 ≥1.87，取 1.90 后余量充足 |
| ModelScope 改按「对方网关实际在册的清单」发布 + 上游三种拒绝分开成句（2026-10-10，v1.1.6 ~ v1.1.10 落地的核心改动） | ⚠ **逻辑已测、真实 Key 与 GUI 未实测**。① `Provider` 新增 `models: &'static [DeclaredModel]` = 该平台的**权威清单**：非空时同时约束两处——注入段只写清单内的模型（`runtime::providers_section_for`）、发现阶段只放行清单内的 key（`backend::free_models_in`）。之所以只声明不够：OpenCode 把注入段与 models.dev 的 catalog **合并**而非替换，catalog 里那批过期 id 仍会进探测队列。② modelscope 清单 13 → **9**：真实 Key 实测削掉 3 条 `PaddlePaddle/ERNIE-4.5-*-PT`（HTTP 401「没有访问权限」，需先在对方控制台开通）与 `meituan-longcat/LongCat-Flash-Lite`（HTTP 400，网关把它映射到自己也不承接的 `LongCat-Flash-Chat`）；留着只是在面板上常驻红色行并重复烧额度。其余三家清单为空＝沿用 catalog + 锚点占位。③ `probe_failure` 这个唯一文案漏斗再加三种可读中文说明（`code`/`status` 逐字保留、不触发重试、不撤销其它已发布模型）：`has no provider supported`（未承接）、`The model or service ID … does not exist`（服务 id 不在册）、以及**同一句上游拒绝按措辞分流**成「不在在线清单里」与「这把 Key 没有该模型的访问权限」（`unknown_service_id_message` 两分支）。🔴 **未验证**：这 9 条逐一探测是否通过、发布后在 WorkBuddy/CodeBuddy 里的真实表现；清单是**随版本手工维护**的（不做远程拉取），对方网关一改口径就得跟版本；`cost: 0` 是本工具的主动断言（依据对方「每日 2000 次免费调用」的推广口径），不是接口回读到的值。✅ 已实测：核心 `cargo test` **278 / 0**、两侧 clippy **0 warning**、壳 `cargo test --lib` **9 / 0**、四组 JS 套件 8/11/9/13、`version:check` 7 处一致（**1.1.10**）、对拍夹具 diff 为空 |
| 本轮（2026-10-10 文档同步 + 一处等价改写，v1.1.10） | ⚠ **纯文档轮，代码只有一处等价改写**。代码：`src-tauri/src/lib.rs:308` 的 `ticks % 8 == 0` → `ticks.is_multiple_of(8)`（那是 HEAD 上**真实存在的 1 条 clippy warning**，此前文档里「两侧 clippy 0 warning」的说法在壳侧不成立，本轮更正；行为逐字等价，故障播报的每 ~4s 重播节律不变）。文档：AGENTS.md / README.md / `docs/version/*` / `docs/wiki/*` / `docs/validation.md` 全部对齐到实测现状（测试基线 278/256、版本落点 7 处、`sync.rs` 不再写备份、`Provider.models`、地区/撤架/未承接/未知 id/无访问五种文案、`--version` 重试、空 `models.json` 补建、rust-version 1.90）。按「版本号规则」本轮属**纯文档 + 等价改写**，**不递增**（沿用 1.1.10）。✅ 门禁本轮**全部实跑**：核心 278/0、核心 clippy 0（touch `src/lib.rs` 强制重检）、壳 clippy 0（同样强制重检）、壳 `cargo test --lib` 9/0、JS 8/11/9/13、eslint 0、`vite:build` 通过、`version:check` 7 处 1.1.10、夹具 diff 为空。🔴 **未做**：远端 Release 状态复核（本机出网被拦截/重定向，`api.github.com` 本轮不可达），远端标签只以 `git ls-remote --tags origin` 为凭 |
| `RunEvent::ExitRequested` 改走 `stop_core_bounded`（原为事件循环线程上无界的 `graceful_stop`） | ❌ **无单测覆盖**（需要真实 `AppHandle`）。依据仅是 `stop_core` 最坏 ~17s 的既有预算与 `STOP_BUDGET = 8s` 的复用；重启/退出时窗口不再冻住的**效果必须实机看** |
| `.github/workflows/release.yml`（cargo 化后） | ✅ **2026-10-03 已跑通完整一轮**：tag `v1.0.2`（head `0e4a535`）触发的 run `37092915120` **8/8 作业全绿**，Release v1.0.2 已发布、23 个资产（六平台安装包 + 各自 `.sig` + `latest.json`），标签↔版本闸门、签名注入单测、hdiutil `.dmg`、mac 更新包补架构后缀、`update-manifest` 全部按新链路执行。此前那次「止步于签名步骤」的失败（私钥变量取到空值 → `Missing comment in secret key`）已由改用仓库级 **Variables** + `npm run tauri:build` 注入器解决。⚠ 但该轮产出的 `latest.json` **六条 url 全部 404**（GitHub 资产名把空格规范化成 `.`，脚本却按本地文件名 `%20` 编码）——脚本与 CI 已修，见「updater 接线」行与「签名与发布」。**仍未验证**：一次真实的升级闭环（客户端拉到包、装完 `relaunch()` 起来） |
| updater 签名注入 `scripts/with-updater-key.mjs`（`tauri:build` 的包装器：**只做注入、不做前置校验**，2026-10-03 完全对齐参考项目 fastenerTradeWorkbench / lockPass 后重写） | ✅ **本机全链路实测（2026-10-03）**：不显式设任何环境变量，脚本从仓库 `.env.local` 取到 `~/.tauri/wbBridge-updater.key`（`~` 形式）与其口令 → 读成内联私钥、删掉互斥的 `_PATH` → 打印「公钥配对 OK（`2B11F78BEA8A43F`）」→ exec `tauri build --bundles app` **退出 0**，产出的 `.sig` 逐字节解出签名者 key ID **= 配置 pubkey 里那条**；此前那条 `does not match the public key from plugins > updater > pubkey` 告警**已消失**（详见「签名与发布」）。它**不判形态、不试签、配对不符也只告警**，私钥与口令全程不打印。逻辑由 `npm run test:updater-key` **13 通过 / 0 失败**钉死。⚠ **尚未验证**：CI 里配好 Variables 之后整条 `tauri build` 是否真能签出可被客户端验签的 `.sig`（属下一轮 CI 才能确认的事） |
| 面板偏好持久化（`src/core/prefs.js`：当前视图 + 「启动后自动检查更新」开关 + 12h 节流） | ⚠ **逻辑已测、GUI 未实测**。白名单 / 凭据字段投影 / 坏数据回落 / 存储不可用降级 / 超大值拒写由 `npm run test:prefs`（`node --test`）**8 通过 / 0 失败**钉死，eslint 与 `vite build` 均通过；但 `localStorage` 在真实 WebView 里的读写、跨重启恢复视图、关掉开关后冷启动确实不再打端点，都**没在 Tauri GUI 验证过** |
| 多平台接入 Stage 2（命名空间参数化：`split_namespace`/`join_namespace`/`free_models_in`/`model_target`，`client_model_id` 前缀按命名空间推导） | ✅ **行为零变化，已由既有测试自证**：核心 `cargo test` **222 通过 / 0 失败**（lib 201 + js_parity 11 + red_lines 10），其中 **`js_parity` 11 全绿 + `git diff --stat src-tauri/core/tests/fixtures` 为空**就是「输出逐字节不变」的证明；两侧 clippy **0 warning**、壳 `cargo test --lib` **9 通过**、`version:check` 5 处一致（**1.0.2 未推进**——等价移植属禁止推进情形）。🔴 过程中被既有测试抓到的一处真实回归：原方案想让「未知命名空间原样当前缀」，但 `sync.rs` 的 4 个既有测试（和对拍夹具）用的是 `vendor/gpt` 这类**合成命名空间**，前缀一变输出就变 → 现规则收紧为**只有注册表平台换 `label`，其余一律 `OC`**。`model_target` 对无命名空间 id 的回落（`opencode` + 空 `modelID`）与旧的「截掉前 9 字节」不同，该形态今日不可达，已用单测把两种写法在全部真实 id 上钉成同值 |
| 多平台接入 Stage 1（`providers.rs` 四平台注册表 + `providers.json` 凭据通道 + 三条 `provider-*` 管理动作） | ⚠ **只有核心与壳侧链路，端到端无效**。已实测：核心 `cargo test` 落地时 **218 通过 / 0 失败**（lib 197 + js_parity 11 + red_lines 10；Stage 2 后为 222/201，见上）、壳 `cargo test --lib` **9 通过**、两侧 clippy **0 warning**、`npm run version:check` 5 处一致（1.0.2）。🔴 **未验证且当前为真**：① 写进 `providers.json` 的 Key **还没有任何消费者**（`isolated_config()` 注入与 `free_models` 注册表驱动属 Stage 3），四平台现在**不会多出一个模型**；② 面板没有入口（Stage 5）；③ 上游 OpenCode 是否把注入的 `provider.<id>` 回显进 `providers.all[]` **仍未实测**，这是 Stage 3 能否复用 `free_models` 的唯一前提；④ 真实 Key 从未在 GUI 输入过、`0600` 只在 unix 断言过。详见 `docs/plans/2026-10-03-upgrade-roadmap-and-v1.0.3-multi-provider-plan.md` 与 `docs/validation.md` 顶部 |
| 2026-10-03 全量代码复审后的四项最小修复（红线 7 子进程环境、红线「探测路径不得转写」、`status.json` 形状、`modelResults` 并发丢写） | ⚠ **逻辑已测、GUI 与真实并发未实测**。① `runtime.rs` 新增 `allowed_environment`，`default_probe` 与 `start_backend` 里「安装后回读 `--version`」两处一次性子进程调用改为先过 `ENV_ALLOW` 白名单——此前它们走 `run_command(env: None)`，而该分支**不做 `env_clear`**，等于把宿主完整环境（含一切凭据类变量）透传给下载的 OpenCode 进程；`isolated_environment` 提用同一函数，行为不变。② `orchestration.rs::probe_meta()` 让两个探测入口都带 `probe: true`（`chat_only_attempt` 原先传空对象，`backend.rs` 的两处转写闸门都以该标记为开关，探测路径因此**可以**启用辅助模型转写，违反数据红线）。③ `restored_model_results` 把上一份 `status.json` 里**非对象**的 `modelResults` 回落成空表（原先只挡缺失，`"x"` 这类形状会让后续一次写入 panic，而发布配置是 `panic = "abort"` → 整个 GUI 进程没）。④ `apply_patch` 改成只写调用方自己那一键（原先 `record()` 先读整份快照、整体塞回 patch，两个并发请求会互相吞掉对方的 `modelResults`），`update()` 与 `update_with_usage()` 的既有语义不变。✅ 已实测：核心 `cargo test` **227 通过 / 0 失败**（lib 205 + js_parity 11 + red_lines 11，本轮新增 5 项）、两侧 clippy **0 warning**、壳 `cargo test --lib` **9 通过**、三组 JS 单测 **8 / 9 / 13** 通过、eslint 与 `vite:build` 通过、`version:check` 5 处一致（**1.0.3**，按用户要求推进）。🔴 **未验证**：白名单生效后的真实子进程表现与并发写盘效果都要在实机看（属"必须点一遍"类别）；`tests/fixtures` 未改（`git diff --stat` 为空即对拍仍逐字节等价）。⚠ 复审中被**源码推翻**的两条指控不得"顺手修"：serde_json 1.0.151 默认 `remaining_depth: 128` 且未开 `unbounded_depth`，超深请求体是 `TooDeep` 错误而非栈溢出；非流式与流式两条链路在记录前都查了 `signal.is_aborted()`（`server.rs:972`、`server.rs:1074`），socket 关闭时 hyper 还会直接丢弃 handler，`on_result` 不会执行，因此不存在"取消记成功" |
| 模型发布多插件写入（双目标 WorkBuddy + CodeBuddy：`targets.rs` 目标定义/检测/聚合、`codebuddy_config.rs` 对照定位、`orchestration.rs` 双目标分发与 `sync.targets`/`codeBuddyModelsFile` 状态形状、面板逐目标展示） | ⚠ **逻辑已测、GUI 与真实 CodeBuddy 环境未实测**。✅ 已实测：核心 `cargo test` **238 通过 / 0 失败**（lib 216 + js_parity 11 + red_lines 11，本轮新增 11 项）、`git diff --stat src-tauri/core/tests/fixtures` 为空（对拍仍逐字节等价）、核心 clippy **0 warning**、壳 `cargo test --lib` **9 通过**、三组 JS 单测 **8 / 9 / 13**、eslint 与 `vite:build` 通过、`version:check` 5 处一致（**1.0.5**，按用户要求推进）。🔴 **未验证**：CodeBuddy 默认目录 `~/.codebuddy/models.json` 是比照 WorkBuddy 约定的假设（仓库此前无 CodeBuddy 线索；实际不同时改 `codebuddy_config.rs::DEFAULT_DATA_FOLDER` 或用 `BUDDY_CODEBUDDY_MODELS_FILE` 指定）；`sync.targets` 与顶层 `codeBuddyModelsFile` 属加法式变更（schemaVersion 保持 1）；GUI 逐目标展示与真实双插件写入效果必须实机点一遍；安装包尚未构建 |
| 1.0.5 探测回归修复 + 单模型重新检测（`start_probes_admin` 二次解包 bug、`chat_only_attempt` 转写闸门回退、面板单模型重新检测按钮） | ⚠ **逻辑已修、GUI 未实测**。① `orchestration.rs::start_probes_admin` 原先对服务端已提取的字符串值再调 `value.get("model")`，返回 `None` → 单模型探测请求退化为全量探测；现直接透传 `start_probes(model, false, false)`。② `orchestration.rs::chat_only_attempt` 的 meta 从 `probe_meta()`（`{ probe: true }`）回退为 `json!({})`——升级时该函数被加上 `probe: true` 后关闭了 `backend.rs:1575` 与 `backend.rs:1619` 两处转写闸门，主探测失败降级到 chat-only 路径后无辅助模型兜底 → 全部模型不可用；回退后转写闸门恢复开放。③ 面板 `ModelRow.vue` 外层 `<button>` 改为 `<div role="option">`，不可用模型行内新增「重新检测」按钮，`@reprobe` 事件经 `ModelList.vue` 转发到 `App.vue::reprobeModel()` 调用 `POST /admin/probe { model: id }`。✅ 已实测：`vite:build` 与 `cargo build`（壳）编译通过。🔴 **未验证**：单模型重新检测的端到端 GUI 效果、回退后真实模型探测通过率须实机点一遍 |。
| `cargo fmt --check` 全绿 | ❌ 未达成（本仓库不以 fmt 为准，勿在无关文件上顺手格式化） |
| 本轮（2026-10-10 全量代码审查 + 缺陷修复，v1.1.11 / **v1.1.12** 同一轮） | ⚠ **逻辑已测、GUI 一次都没启动过**。三路审查（核心 / 壳 / 面板）合并后**只落地被源码证实的缺陷**，不做重构、不新增功能。核心 3 项：① `backend.rs` 新增 `CompleteGuard`（:371 结构体 + :381 `impl Drop` + :1378 构造点）——客户端断连时 hyper **直接丢弃 handler future**，写在 `.await` 之后的清理永远不执行，在飞的 250ms 权限轮询任务与 `pending_permissions` 会话状态因此泄漏；`Drop` 里用 `Handle::try_current()` 取运行时句柄补发停止与清态（`panic = "abort"` 的 release 下，这类泄漏是长期驻留而非崩溃）。② `server.rs` 的 `read_body`（:1203）拆出 `read_body_bounded(body, budget)`（:1210）并给 `/admin/*` 补上此前**完全没有**的请求体预算（`REQUEST_BODY_TIMEOUT = 20s`，:57），非管理路由的既有行为逐字不变。③ `backend.rs` 会话 id 取回处（:1295-1301）把「缺 `id`」与「`id` 为空串」**分开**：空串过去会被当作合法 id 拼进后续 `/session/{id}/message`，现在当场报 `OpenCode session response has no session id`。面板 7 项（对比度与可读性 + 键盘 + 忙态 + 定时器 + ARIA）：`--orange` 浅色 `#bc752c`→`#a4581a`（暗色 `#e9ad70` 已达标、未动）、`--switch-off` `#aab2ae`→`#7f8a86`（AA 达标值由 `variables.css` 实色算出，非目测）；十处小字号 `--muted`→`--muted-strong`（`App.vue` `.subtitle`/`footer`、`ModelRow.vue` `.duration`、`ModelList.vue` `.empty`、`MetricsBar.vue` `.metrics span`、`ModelDetails.vue`、`ProvidersView.vue` `.provider-id`/`.guide-url`/`.note`、`AboutView.vue` `.notes`）；`App.vue` 的 Esc 不再从输入框里吞掉「收起详情」；重启/导入的 spinner 改按 `busyAction` 精确判定（原先任一动作在跑都点亮导入）；`ProvidersView.vue` 的 `applyFeedbackTimer` 在 `onUnmounted` 里 `clearTimeout`；`#import` 补 `aria-busy`；`ModelList.vue` 空列表时不再挂 `role="listbox"`（`listbox` 只允许 `option` 直接子元素）。🔴 **刻意未改**（避免超出「缺陷修复」范围）：`ModelRow.vue` 的 `<div role="option">` 外层可点区（改它会动 1.0.5 刚定的单模型重新检测交互）、`--details-w` 的 `clamp(300px, 45%, 360px)`。新增 3 项单测（`backend.rs` 2 + `server.rs` 1）；`server.rs` 那条原本想用 `#[tokio::test(start_paused = true)]` 做负向对照，但 tokio 未开 `test-util` feature ⇒ 改为**注入预算参数**（真实 50ms 预算）+ 常量断言，**不为此引依赖**（E0599 是当时的实测报错）。🔴 **两项核心修复的「撤掉修复就会失败」变异验证都没做成**：本轮多次尝试临时回退守卫（`std::mem::forget`）与会话 id 检查，全部被工具的安全分类器拦下（不得在无用户确认时临时回退一处修复）。所以文档只声明测试实际钉住的内容；「没有守卫就会漏」是按读码得出的论证（future 被弃 ⇒ 内联 `finally` 不执行 ⇒ `abort_hits`/`delete_hits` 会是 0、轮询计数继续增长），**不是实测过的失败**。要做真变异须先取得用户授权。⚠ **测试基线更正**：文档此前写 278 = lib 256，HEAD 实测属性数是 **258**（那 2 项差在本轮之前），本轮 +3 → **283 = lib 261 + js_parity 11 + red_lines 11**。✅ 门禁本轮**全部实跑**（`touch` 后强制重检，不复用缓存）：核心 `cargo test` **283 / 0**、核心 clippy **0 warning**、壳 clippy `--no-deps --all-targets` **0 warning**、壳 `cargo test --lib` **9 / 0**、JS **8 / 11 / 9 / 13**（全 0 失败）、`npx eslint .` 退出 **0**、`vite:build` **✓ 129ms**、`git diff --stat src-tauri/core/tests/fixtures` **为空**、`npm run version:check` **7 处一致（1.1.12）**、两侧 `Cargo.lock` 由 cargo 同步到 1.1.12。🔴 **未验证**：应用一次都没启动过；核心三项跑在**注入的假 OpenCode 服务**上（`start_mock_session`），不是真实 OpenCode；`/admin/*` 的 20s 预算在真实慢客户端下的表现、面板每处改动的真实排版，都属必须实机点一遍的类别。🔴 **未做**：构建安装包、打标签、提交；`1.1.11` 与 `1.1.12` 属同一轮（前者从未构建/打标签）。审查中被核实但**本轮未修**的九项（A~I）逐条记在 `docs/validation.md` 顶部，其中 **A = `runtime.rs:968` 既有的 `NODE_TLS_REJECT_UNAUTHORIZED=0`**（早于本轮、本轮一字未动，属安全面待决策项，不得当作已修复） |
| WB.auto 智能路由模型（对外暴露合成模型名 `WB.auto`，后端按请求内容自动选适配模型：含图片 → 支持图片输入的模型、含工具 → 支持 toolcall 的模型、纯文本 → 从可用池随机选；响应 `model` 字段回填实际模型名；不写进插件 `models.json`） | 📝 **设计已定稿（`docs/plans/2026-10-10-wb-auto-smart-router.md`）、代码待落地**。新增 `src-tauri/core/src/auto.rs`（纯函数 `auto_select` + `has_images` + `has_tools`）、`server.rs` 两处拦截（`/v1/models` 追加合成条目 + `chat()` 里 `WB.auto` 改写为实际模型）。🔴 **本轮无任何代码落地**（`src-tauri/core/src/auto.rs` 不存在，全仓 grep `WB.auto` / `auto_select` 无命中），因此**不占用 minor 位**：2026-10-10 曾被本轮推进到 `1.2.0`，已由维护者裁定回退，当前 7 处落点为 **1.1.12**（`npm run version:check` 实测一致；`1.2.x~1.3.x` 按上文裁定仍属「不得再占用」）。🔴 **未做**：核心单测落地（计划 ≥8 项）、端到端（含图片/工具请求的路由选择）、GUI 实机——属必须实机点一遍的类别 |

## 项目概述与定位

WB Bridge 是一个**跨平台托盘工具**，通过**隔离的 OpenCode 技术**为 WorkBuddy 提供免费模型服务。它做三件事：

1. **托管一个隔离的 OpenCode 运行时**：优先复用本机已有的 `opencode` 可执行文件，否则从 npm registry（官方源 → 国内镜像）下载官方 tarball，校验 `sha512-` 完整性后解包到数据目录，用**隔离环境变量 + 独立随机端口**以 `serve --pure` 方式启动，绝不污染用户 OpenCode 的配置、数据、缓存与登录态。
2. **对外暴露 OpenAI 兼容 API**：本地 `127.0.0.1:<port>` 提供 `/v1/models` 与 `/v1/chat/completions`（含 SSE 流式），强制请求携带鉴权头（Bearer 方案，值为数据目录 `api-key` 文件的内容）鉴权，拒绝一切浏览器 Origin，最多 4 个并发请求。
3. **把可用模型同步给 WorkBuddy**：自动发现免费模型 → 逐模型探测 → 只把探测通过的模型发布给 WorkBuddy（原子写 + 增量合并其 `models.json`，只增删自己名下的条目）。

三层结构：

| 层 | 位置 | 技术 | 现状 |
|---|---|---|---|
| 核心 | `src-tauri/core/src/`（crate `wbbridge-core`，lib `wbbridge_core`） | Rust + tokio + axum + reqwest + serde_json（`preserve_order`） | 已入库，测试齐全；既可独立成进程，也嵌入壳 |
| 控制面板 | `src/`（Vue 3 SFC）+ `vite.config.js` → 产物 `dist/` | Vue 3 + Vite，无 CDN、无外部请求 | 已入库 |
| 桌面壳 | `src-tauri/`（crate `wbbridge`，lib `wbbridge_lib`） | Tauri 2（Rust），托盘 + IPC + 状态轮询 | 已入库，直接 `cargo build` 出成品（无 sidecar） |

## 常用命令（安装 / 开发 / 构建 / 测试 / 签名发布）

### 环境前置

- **Rust** stable（`src-tauri/core/Cargo.toml` 与 `src-tauri/Cargo.toml` 均声明 `rust-version = "1.90"`，取锁定依赖图的实际下限；本机 `rustc 1.98.1`，CI 用 `dtolnay/rust-toolchain@stable`）。
- **Node.js >= 24**（根 `package.json` 的 `engines`）——只为 Vite 前端构建与 `scripts/*.mjs` 服务，核心运行不依赖它。本机由 nvm 管理，**非登录 shell 的 PATH 中可能没有 `node`**，执行 npm 命令前先载入：
  ```bash
  export NVM_DIR="$HOME/.nvm"; . "$NVM_DIR/nvm.sh"
  ```
- Linux 构建需系统包：`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev`。

### 命令清单

| 目的 | 命令 | 执行目录 | 说明 |
|---|---|---|---|
| 安装前端依赖 | `npm install` | 仓库根 | 根 `package.json`（devDeps：`@tauri-apps/cli`、`vite`、`@vitejs/plugin-vue`、`eslint`、`@eslint/js`；deps：`vue`） |
| 运行全部核心测试 | `cargo test`（或 `npm run test`） | `src-tauri/core/` | 基准 **283 通过 / 0 失败**：lib 261 + `js_parity` 11 + `red_lines` 11（约 1.1s，另有 bin/doc-test 0 用例）；壳侧另有 `cargo test --lib`（`src-tauri/`）**9 通过 / 0 失败**（其中 `shell_action_routes_match_the_core_contract` 逐项对齐 8 条动作） |
| 运行面板偏好测试 | `npm run test:prefs` | 仓库根 | = `node --test src/core/prefs.test.js`，基准 **8 通过 / 0 失败**（白名单/凭据字段投影/视图清洗/坏数据回落/存储不可用降级/超大值拒写/静默检查节流）；只测 `src/core/prefs.js`，不联网、不编译 Rust |
| 运行操作守卫测试 | `npm run test:ops` | 仓库根 | = `node --test src/core/ops.test.js`，基准 **11 通过 / 0 失败**（成功反馈文案、在飞互斥、完成冷却、拒绝节流、时钟回拨）；只测 `src/core/ops.js` 纯函数，不联网、不编译 Rust |
| 运行更新清单生成测试 | `npm run test:manifest` | 仓库根 | = `node --test scripts/gen-latest-json.test.mjs`，基准 **9 通过 / 0 失败**；用临时产物目录跑真脚本，钉住六平台成功路径与缺平台/缺签名/漏架构后缀/歧义产物等失败路径。**严禁**改成真实联网或读仓库 `src-tauri/target/` 下的产物 |
| 运行 updater 签名注入测试 | `npm run test:updater-key` | 仓库根 | = `node --test scripts/with-updater-key.test.mjs`，基准 **13 通过 / 0 失败**；钉住 env 文件取值优先级（进程环境 > `.env.local` > `.env` > `~/.tauri/wbBridge{,-updater}.env`，**显式空口令压过文件里的口令**、`~/…` 先展开）、内联位的两种形态（单行 base64 全文 vs `.key` 文件路径 → 读成全文并 **trim**，尾部换行会让 tauri 报 `Invalid symbol 10`）、路径注入后删除与之互斥的 `_PATH`、明文钥显式置空口令、加密钥缺口令**只告警不阻断**、公钥配对**只认配置里第一条且只告警**、`buildCommand` 的三种入参形态、CLI 在**相对路径**调用下确实执行且**任何输出都不回显密钥**。接线断言：`tauri:build` 确实经本包装器、`build` 链 `make:dmg`、`release.yml` 把私钥读成 `vars.` 且调用 `npm run tauri:build`（不再有 `tauri-action`、不再有私钥前置步骤）、`plugins.updater.pubkey` **恰好一条公钥**、`bundle.targets` **不含 dmg**。**不联网、不调 tauri CLI、不读 `~/.tauri`、不碰真实私钥**（用合成的假 base64 串与临时目录） |
| 静态检查 | `cargo clippy --all-targets` | `src-tauri/core/` | 必须保持 0 warning |
| 启动核心（独立进程，调试用） | `cargo run --manifest-path src-tauri/core/Cargo.toml --bin wbbridge-core` | 仓库根 | 监听 `127.0.0.1:41980`（`BUDDY_PORT` 覆盖），数据目录走平台默认值 |
| 开发桌面应用 | `npm run tauri:dev` | 仓库根 | `beforeDevCommand = npm run vite:dev`（`http://localhost:41990`），壳内嵌启动核心 |
| 构建前端产物 | `npm run vite:build` | 仓库根 | 输出 `dist/`（`tauri.conf.json` 的 `frontendDist` 指向 `../dist`） |
| 构建桌面应用 | `npm run build`（= `vite:build && tauri:build && make:dmg`） | 仓库根 | 产物 `src-tauri/target/*/release/bundle/`。`tauri:build` 经签名注入包装器 `scripts/with-updater-key.mjs`（把 `.env.local` / `~/.tauri` 里的私钥汇齐并归一成 `tauri build` 只认的内联 `TAURI_SIGNING_PRIVATE_KEY`）；`createUpdaterArtifacts` 为 `true`，私钥取不到时整套 Rust 编译跑完才在打包那步失败。`bundle.targets` 不含 `dmg`，macOS 的 `.dmg` 由 `make:dmg` 用 **hdiutil** 生成（非 mac 平台自动跳过） |
| 仅生成 macOS 的 .dmg | `npm run make:dmg` | 仓库根 | `scripts/make-dmg.sh`：从 `tauri.conf.json` 读 `productName`/`version`，架构取 `TARGET_TRIPLE` 的前缀（CI 交叉目标必须传，否则 `uname -m` 反映的是 runner 架构）；产物 `bundle/dmg/<productName>_<version>_<arch>.dmg` |
| 版本一致性校验 | `npm run version:check` | 仓库根 | `scripts/check-version.mjs`：7 处版本号必须一致，否则退出 1（2026-10-10 起含核心 crate） |
| 版本推进 | `npm run version:set -- <x.y.z>` | 仓库根 | `scripts/bump-version.mjs` 同步改写；执行口径见下方「版本号规则」（2026-10-09 起为「每次修改必须递增」，豁免情形除外） |
| 健康检查 | `curl --oauth2-bearer "$(cat "<数据目录>/api-key")" http://127.0.0.1:41980/health` | 任意 | `/health` **同样需要 Bearer 鉴权**（无 key 返回 401；`--oauth2-bearer` 即发送 Bearer 头） |
| 列出已发布模型 | `curl --oauth2-bearer "<api-key 文件内容>" http://127.0.0.1:41980/v1/models` | 任意 | 返回客户端可见模型（id 形如 `OC · 名称`） |

> ⚠ `<数据目录>` 取决于运行方式：**壳运行时**是 Tauri 的 `app_data_dir`（macOS `~/Library/Application Support/app.wbbridge.desktop`，identifier `app.wbbridge.desktop`）；**独立进程运行时**是平台默认目录（macOS `~/Library/Application Support/Buddy Bridge`，见下）。两套目录不要混为一谈。

### 签名与发布

- **自动更新已接线**：`src-tauri/Cargo.toml` 依赖 `tauri-plugin-updater` + `tauri-plugin-process`，`lib.rs` 的 builder 链注册两者，`tauri.conf.json` 有 `bundle.createUpdaterArtifacts: true` 与 `plugins.updater`（`pubkey` 内联公钥串 + `endpoints` 指向 `https://github.com/trexwb/wbBridge/releases/latest/download/latest.json`）。因此**本地构建必须给出签名私钥**，否则打包在生成 updater 产物那一步失败并报「A public key has been found, but no private key」。构建入口 `npm run tauri:build` = `node scripts/with-updater-key.mjs tauri build`（2026-10-03 完全对齐参考项目 fastenerTradeWorkbench / lockPass 后的写法）：包装器**只做注入**——按「进程环境 → 仓库 `.env.local` / `.env` → `~/.tauri/wbBridge.env`、`~/.tauri/wbBridge-updater.env` → 兜底 `~/.tauri/wbBridge-updater.key`」汇齐私钥与口令，把 `.key` 路径（含 `~/…`，Node 与 tauri 都不展开 `~`）读成内联全文并 trim，删掉与内联值互斥的 `TAURI_SIGNING_PRIVATE_KEY_PATH`，明文钥**显式**置空口令，然后 exec 目标命令。它**不判形态、不试签、公私钥配对不符也只告警不阻断**；全程只输出来源文件名与公开的 key ID，绝不回显密钥与口令。产物每平台额外得到 `<安装包>.sig`：macOS `*.app.tar.gz`、Windows `*-setup.exe`（MSI 不参与更新）、Linux `*.AppImage`（✅ 2026-10-03 v1.0.2 首跑实测：就是裸 `.AppImage`，bundler **没有**额外产出 `.AppImage.tar.gz`；`.deb` 也带 `.sig` 但不参与更新）。
  - ✅ 本机实测（2026-10-03，当前这把 9-30 钥）：`npm run tauri:build -- --bundles app` 在**不设任何环境变量**的情况下打印「已注入内联签名私钥（来自 wbBridge-updater.key）→ 公钥配对 OK（`2B11F78BEA8A43F`）→ 加密态私钥 + 已提供口令」，构建退出 0，产出 `WB Bridge.app.tar.gz`（3,596,811 B）+ `.sig`（428 B）；把 `.sig` 的 base64 解出来逐字节取签名者 key ID = **`2B11F78BEA8A43F`**，与 `plugins.updater.pubkey` 里那个（唯一、也是唯一生效的）key ID 一致。此前那条 `The updater secret key from TAURI_SIGNING_PRIVATE_KEY does not match the public key from plugins > updater > pubkey` 告警**已消失**（成因见下条）。构建期仍会打 `Warn skipping app notarization, no APPLE_ID & …` ——**预期、无需修**：ad-hoc 签名（`bundle.macOS.signingIdentity: "-"`）、无 Developer ID 证书与公证凭据，两个参考项目同样带着它出货。
  - 🔴 **一条被源码推翻的旧结论（必须知道）**：文档里曾反复写「`plugins.updater.pubkey` 内嵌两条公钥＝minisign 轮换白名单，换钥无需改配置」——**错**。实读 `tauri-plugin-updater` 2.13 的 `verify_signature()` → `minisign-verify` 0.2.5 的 `PublicKey::decode()`：它只读解码文本的**前两行**，后面的公钥框被静默丢弃，所以**只有第一条生效**；`tauri build` 那条 does-not-match 告警比对的也正是同一第一条。当时提交在 `tauri.conf.json` 里的其实**只有** `126D4E208E0F17BA` 一条（属已丢失的 20261001 钥），而签名用的是 `2B11F78BEA8A43F` 那把，于是既告警、签出的 `.sig` 又会被客户端判无效。修复 = 把 `pubkey` 换成**真正签名那把**的公钥（已完成，现配置只有 `2B11F78BEA8A43F`）。
    - ⚠ 因此**换钥必须同步换 `pubkey`**，且老客户端只认它自己内嵌的那条 → 换钥那一次发布必须能让老版本先收到最后一更。
    - ✅ 本次替换**不影响任何已发布版本**：`git show v1.0.1:src-tauri/tauri.conf.json` 里 `grep -c updater` = 0、`Cargo.toml` 无 `tauri-plugin-updater`、`bundle.createUpdaterArtifacts` 缺席 —— 唯一打过的两个标签（v1.0.0 / v1.0.1）都**早于 updater 接线**，`126D4E208E0F17BA` 从未进入任何交付给用户的二进制。（v1.0.2 将是第一个带自动更新通道的产物，其后的版本只要还用这把钥签名就能被它验签升级。）
  - ⚠ 变量分工（易错，实测过）：`tauri build|bundle` **只读 `TAURI_SIGNING_PRIVATE_KEY`**（值可为私钥全文或文件路径）；`TAURI_SIGNING_PRIVATE_KEY_PATH` 只对 `tauri signer sign` 生效（等价 `-f`）；`signer sign -k` 要的是**私钥字符串**，误传路径会报 `failed to decode base64 secret key: Invalid symbol 46`。
  - ⚠ 空值 → 报错文案的映射（2026-10-03 本机 14 组形态逐条复现）：私钥取到**空值**时 tauri 报 `failed to decode secret key: incorrect updater private key password: Missing comment in secret key`（看着像口令错，其实不是）；尾部多换行/中间空格 → `Invalid symbol 10 / 32, offset …`；多包一层 base64、或误存公钥 → `Missing encoded key in secret key`；`.pub` 当私钥 → `failed to fill whole buffer`；加密钥 + 口令错/空 → `Wrong password for that key`。⚠ 另有两类损坏**会被静默接受**（截掉尾部到 344 字符、或去掉开头 4 字符）——注入器**不拦**这类损坏（2026-10-03 对齐参考项目后撤掉前置试签），签坏了只有客户端验签时才暴露，所以取值来源那几行日志要人工看一眼。
  - ⚠ 本机权限隐患（**属待用户决策项，Agent 不得自行改动 `~/.tauri` 下的文件，也不得擅自 chmod 仓库里的凭据文件**）：仓库根 `.env.local`（`0644`，1200 B，`git ls-files` 确认未入库）存着当前这把私钥的**路径与口令**，同机其他用户可读；`~/.tauri/wbBridge.env` 与 `~/.tauri/EdtibBooks.env` 同样是 `0644`。私钥文件本身 `~/.tauri/wbBridge-updater.key` 是 `0600`。⚠ 2026-10-03 实读：仓库根**已无 `.env`**，只有 `.env.local`（此前多处文档写的是 `.env`，已按实读更正）。建议 `chmod 600 .env.local`，但该文件属用户所有，由用户决定。`.env.example` 只含占位符。
  - ⚠ macOS 的更新包名**不含版本也不含架构**（就是 `WB Bridge.app.tar.gz`），两个 mac runner 会往同一 Release 传同名资产、后者静默覆盖前者；CI 的「给 macOS 更新包补架构后缀」步骤因此把它改名为 `WB Bridge_<arch>.app.tar.gz`（`.sig` 同步改名——minisign 签的是内容不是文件名）。
- `latest.json` **只有一个写者**：CI 末尾的 `update-manifest` 作业跑 `scripts/gen-latest-json.mjs`（`node scripts/gen-latest-json.mjs --dir artifacts --out latest.json`），平台键取自 artifact **目录名**里的 target triple（`macos-aarch64-…-bundles` → `darwin-aarch64`；不能靠文件名，见上），默认要求六平台齐全、缺一即退出 1。三个 build 作业**谁都不写清单**（2026-10-03 起连 `tauri-action` 都已撤掉，它自带的清单生成当初用 `includeUpdaterJson: false` 关闭——6 个并发作业各写一次会互相覆盖、静默漏平台）。Draft 未 Publish 前该 URL 返回 404 属预期。
  - 🔴 **url 里的空格必须写成 `.`**（2026-10-03 v1.0.2 首跑实测）：GitHub 在上传时把 Release 资产名里的空格规范化成点（磁盘上 `WB Bridge_1.0.2_x64-setup.exe` → API 里 `WB.Bridge_1.0.2_x64-setup.exe`），而 `/releases/download/<tag>/<空格名>`（含 `%20`）**一律 404**。脚本因此先 `name.replace(/ /g, '.')` 再 `encodeURIComponent`；紧跟其后的 CI 步骤「校验清单 url 指向的资产在 Release 上真的存在」用 API 把六条 url 末段与实际资产名逐条对账，对不上即失败。这类缺陷清单本身看不出来（六平台齐全、`.sig` 也配得上公钥），只有客户端下载到那一步才炸。
- **macOS 的 `.dmg` 不经 Tauri**：`bundle.targets` 从 `"all"` 改成显式列表 `["app", "nsis", "msi", "appimage", "deb"]`（本平台不适用的目标由打包器跳过），`.dmg` 改由 `scripts/make-dmg.sh`（`npm run make:dmg`，已接进 `npm run build` 链尾）用 **hdiutil** 生成 —— Tauri 自带的 create-dmg 末尾要用 AppleScript 美化窗口，无 GUI / 无 Finder 自动化授权的 runner 上会失败。脚本从 `tauri.conf.json` 读 `productName`/`version`，架构取 `TARGET_TRIPLE` 前缀（CI 交叉目标**必须传**，否则 `uname -m` 反映的是 runner 架构，会拿错 bundle 目录并把 x86_64 的包标成 aarch64），产物 `bundle/dmg/<productName>_<version>_<arch>.dmg`；非 macOS 直接跳过返回 0。
- 环境变量模板：`.env.example` → 复制为 `.env.local`（`.env.local` 已被 `.gitignore` 忽略，**严禁提交**）。这是唯一模板落点（`src-tauri/updater-signing.env.example` 与其逐字节相同、已删除）。包装脚本按 **进程环境 > `.env.local` > `.env` > `~/.tauri/wbBridge{,-updater}.env`** 逐项取值，**逐项打印来源**（钥来自环境、口令来自 `.env.local` 这种混搭出问题时 tauri 只报「口令不对」，看不出哪项错）。
  - `TAURI_SIGNING_PRIVATE_KEY`：私钥全文**或私钥文件路径**（本地推荐路径形式，密钥内容不进环境；`~/…` 会被展开）。CI 用 GitHub **Variables** 存私钥全文。
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥口令。**当前采用的这把（9-30 的 `~/.tauri/wbBridge-updater.key`）口令非空**，CI 必须另配该变量，留空即「Wrong password」。显式给空串表示这把钥确实无口令——环境变量里只要**存在**该变量就按显式处理，不再回落 `.env*`。
  - ⚠ CI 侧 2026-10-03 按用户决定从 **Secrets 改为 Variables**（对齐参考项目 fastenerTradeWorkbench），代价如实记录：**Variables 是明文值**（设置页可见、日志不打码），所以工作流绝不 echo 这两个变量，包装脚本也只输出来源文件名与公开 key ID。想换回 Secrets 只需把 `vars.` 改回 `secrets.`，脚本与命令都不用动。
  - 🔴 私钥纪律：私钥绝不入库、不进日志、不进 `status.json`；公钥是公开值，内嵌配置即可
- 密钥生成（`@tauri-apps/cli` 已在根 devDependencies，现可执行）：
  ```bash
  npm run tauri -- signer generate -p '' -w ~/.tauri/wbBridge-updater-<日期>.key
  # -k/--private-key 是「私钥字符串」，读文件要用 -f/--private-key-path；
  # 单独签文件：npm run tauri -- signer sign -f <私钥路径> -p '' <产物路径>
  ```
- CI：`.github/workflows/release.yml`（推 `v*` tag 或手动 dispatch）——test 作业跑 `cargo test` + `check-version.mjs` + 三组 `node --test`（prefs 8 / manifest 9 / updater-key 13）+ 标签↔版本闸门；三个平台作业跑 `npm ci` → `npm run tauri:build -- --target <triple>`（签名变量取自 `vars.TAURI_SIGNING_PRIVATE_KEY{,_PASSWORD}`，**不设私钥前置校验步骤**，2026-10-03 用户决定）；macOS 另加两步：`npm run make:dmg`（带 `TARGET_TRIPLE`）与「给 macOS 更新包补架构后缀」；`update-manifest` 作业 = 生成 latest.json → **「校验清单 url 指向的资产在 Release 上真的存在」**（用 API 逐条对账资产名，防的就是下节的空格/点问题）→ 产物版本自检 → 上传 `latest.json`。Release 由 `softprops/action-gh-release` 创建（**不再用 `tauri-apps/tauri-action`**），`tag_name` 由「读取版本号」步骤现读 `tauri.conf.json` 拼成 `v<版本>`，免得 workflow_dispatch 时把分支名当标签开出第二个 Release。
  - ⚠ **首次 CI 实跑（2026-10-03 上午）已在签名步骤失败**，报 `failed to decode secret key: incorrect updater private key password: Missing comment in secret key`；本机 14 组形态复现证明这句**就是「变量取到空值」**（当时私钥只配在 GitHub Secrets，而 build 作业没声明 `environment:`，环境级 Secret 对作业不可见 → 展开成空串）。修复 = 按用户决定改用仓库级 **Variables** 并同时配齐口令。
  - ✅ **同日第二轮（tag `v1.0.2`，run `37092915120`）8/8 作业全绿**，Release v1.0.2 已 Publish、23 个资产。实测确认的产物形态：macOS 两个架构各自 `.dmg`（hdiutil）与补了架构后缀的 `WB Bridge_<arch>.app.tar.gz(+.sig)`；Windows `…_x64/arm64-setup.exe(+.sig)` 与 `.msi(+.sig)`；**Linux 的 updater 包是裸 `.AppImage`（bundler 没有额外产出 `.AppImage.tar.gz`）**，`.deb` 也带 `.sig`（不参与更新）；六条清单签名的签名者 key ID 全部 `2B11F78BEA8A43F`，与 `plugins.updater.pubkey` 里唯一那条一致。
  - ❌ **仍未验证**：一次真实升级闭环（客户端拉包、安装、`relaunch()` 起来）。另外**线上那份 `latest.json` 的六条 url 当时全是 404**（空格 vs 点，见上条与「签名与发布」），脚本与 CI 闸门已修，但 v1.0.2 那份清单的**重传属共享状态、只能由用户操作**。

### 环境变量（核心）

| 变量 | 默认值 | 作用 |
|---|---|---|
| `BUDDY_PORT` | `41980` | HTTP 端口；非 1024–65535 的整数直接失败退出（嵌入壳时由壳显式传入 `StartOptions.port`，不读环境变量） |
| `BUDDY_DATA_DIR` | 平台数据目录（见下） | 覆盖数据目录（嵌入壳时由 `StartOptions.data_dir` 注入 app_data_dir） |
| `BUDDY_MODELS_FILE` | macOS/Linux `~/.workbuddy/models.json` | 覆盖 WorkBuddy 配置文件路径（Windows 走"已保存值 → 发现"流程） |
| `BUDDY_CODEBUDDY_MODELS_FILE` | 无 | 覆盖 CodeBuddy 的 models.json 路径（优先级最高；另有对称的 `CODEBUDDY_CONFIG_DIR` / `CODEBUDDY_DATA_FOLDER_NAME`，默认目录 `~/.codebuddy`） |
| `BUDDY_NO_SYNC` | 未设置 | `=1` 时跳过启动导入与后续模型同步 |
| `BUDDY_OPENCODE_PATH` | 无 | 首选 OpenCode 可执行文件路径（优先级最高） |
| `BUDDY_PARENT_PID` | 无 | 父进程 pid；父进程消失则核心自行优雅关停 |

> 嵌入壳时**禁止**用 `std::env::set_var` 改变进程级配置（不安全、与并发冲突）；必须走 `orchestration::run(StartOptions { data_dir, port, handle_signals })`。

数据目录（`src-tauri/core/src/platform.rs::data_directory`，目录名常量 `DATA_DIR_NAME = "Buddy Bridge"`）：macOS `~/Library/Application Support/Buddy Bridge`、Windows `%APPDATA%\Buddy Bridge`、Linux `${XDG_CONFIG_HOME:-~/.config}/Buddy Bridge`。壳运行时改用 `app_data_dir`。目录内文件：

| 文件 / 目录 | 用途 |
|---|---|
| `api-key` | 32 字节随机 hex（权限 `0600`），所有 HTTP 请求的 Bearer 令牌 |
| `providers.json` | 用户自持的**平台 Key**（形态 `{ "<注册表里的 provider id>": "<Key>" }`，即请求体的 `apiKey` 落盘后的键是平台 id；权限 `0600`，经 `sync::atomic_write` 原子替换）。只由 `providers.rs` 读写；**绝不进日志 / `status.json` / 面板偏好 / 子进程环境 / 任何响应体**，`provider-status` 只回「是否已配置」 |
| `service.pid` | 单实例锁；进程仍存活即视为已在运行（独立进程按退出码 2 结束） |
| `settings.json` | 持久化 `useSystemProxy`、`workBuddyModelsFile` |
| `status.json` | 服务状态快照（UI 与外部读取的唯一状态源），串行 + 原子写 |
| `opencode.log` | OpenCode 子进程 stdout/stderr；超过 5MB 轮转为 `opencode.log.previous` |
| `runtime/<version>/opencode` | 托管下载的 OpenCode 运行时 |
| `opencode/{config,data,cache,state,project}` | 隔离的 OpenCode XDG 目录（`0700`） |

## 目录结构说明

```
wbBridge/
├── AGENTS.md                     ← 本文件（所有 Agent 的规则来源）
├── README.md                     ← 面向使用者的项目说明
├── package.json                  ← 根级便利脚本 + 版本单一来源（非 npm 工作区）
├── package-lock.json             ← 入库：CI 的 npm ci 依赖它
├── vite.config.js                ← 前端构建（root: src，outDir: ../dist，dev 端口 41990）
├── eslint.config.js              ← 扁平 ESLint（只开能真报错的规则）
├── src/                          ← 控制面板（Vue 3 + Vite；root: src）
│   ├── index.html  main.js  App.vue
│   ├── core/bridge.js            ← 与壳的唯一边界：invoke + listen → onState/onDismiss/action（另有只读 readLog/dataDir + updater/process 三调用）
│   ├── core/ops.js               ← 操作守卫唯一实现：成功反馈文案 + 在飞互斥 + 完成冷却 + 拒绝节流（纯函数，node --test 直测）
│   ├── core/ops.test.js          ← `npm run test:ops`（node --test，11 用例）
│   ├── core/update.js            ← 更新状态机（静默检查、下载进度、重启生效）；AboutView 只渲染
│   ├── core/prefs.js             ← 面板偏好的唯一读写边界（localStorage 键白名单 + 值字段投影 + 坏数据静默回落）
│   ├── core/prefs.test.js        ← `npm run test:prefs`（node --test，8 用例）
│   ├── core/activity.js          ← 活动文案（托盘与面板共用，禁止两套文案）
│   ├── views/                    ← SideBar.vue（分组导航 + 运行设置）、ModelList.vue、ModelDetails.vue（详情右栏）、
│   │                                ServiceStatus.vue、MetricsBar.vue、FeedbackBar.vue、
│   │                                LogsView.vue、UsageView.vue、IntegrationView.vue、AboutView.vue（4 个只读视图）
│   ├── components/ModelRow.vue   styles/{variables,base}.css  public/logo.svg
├── src-tauri/                    ← 全部 Rust 代码都在这里（壳 + 核心）
│   ├── Cargo.toml                ← 壳 crate wbbridge；依赖 wbbridge-core = { path = "core" } + tokio(rt-multi-thread)
│   ├── tauri.conf.json           ← frontendDist ../dist、bundle.externalBin **空**、CSP
│   ├── capabilities/default.json ← 只声明插件侧权限（自有命令不经 capability 授权）
│   ├── build.rs
│   ├── src/{main.rs,lib.rs}      ← 托盘、IPC 命令、状态轮询、核心生命周期
│   ├── icons/                    ← 壳/托盘图标
│   └── core/                     ← Rust 核心（crate wbbridge-core，**独立 workspace**：自有 Cargo.lock 与 target）
│       ├── Cargo.toml            ← 核心 crate 版本（2026-10-10 起随产品版本同步，勿再手动改；status.json 的 0.2.0 仍是历史沿革值）
│       ├── src/
│       │   ├── orchestration.rs  ← 原 main.js 的编排：启动时序、status/settings、探测调度、关停、单实例锁、StartOptions、set_exit_hook
│       │   ├── main.rs           ← 独立进程入口（--help / --version；信号自处理）
│       │   ├── lib.rs            ← 模块导出 + STAGE / PACKAGE_NAME / VERSION
│       │   ├── server.rs         ← axum 路由、Bearer 鉴权、Origin 拒绝、并发/体积上限、SSE + 心跳、/admin/*
│       │   ├── protocol.rs       ← OpenAI 兼容协议层：prepare / decode / completion / send_sse / BridgeError
│       │   ├── backend.rs        ← OpenCode HTTP 客户端：会话、事件流、原生工具审批、免费模型发现、转写钩子
│       │   ├── runtime.rs        ← 运行时发现/下载/sha512 校验/解包/隔离 env/启动/关停
│       │   ├── probe.rs          ← 模型探测（工具调用与纯文本两类；每模型各 60s 预算，重试共用）
│       │   ├── providers.rs      ← 平台注册表（四家，唯一真相）+ providers.json 凭据通道（0600、只回是否已配置）
│       │   ├── repair.rs         ← 信封/工具格式修复与辅助模型转写（含 CLIENT_CONVENTIONS）
│       │   ├── handoff.rs        ← 原生工具 handoff：构建客户端动作、拒绝反馈、动作校验
│       │   ├── sync.rs           ← WorkBuddy models.json 原子写 + 增量合并（OWNER 标记、锁；**不写备份**，只清扫存量同族 `.bak`）
│       │   ├── workbuddy_config.rs ← models.json 定位与校验（不猜测、不创建文件）
│   ├── codebuddy_config.rs ← CodeBuddy models.json 定位（对照 workbuddy_config.rs）
│   ├── targets.rs ← 写入目标：检测、分发、聚合
│       │   ├── system_proxy.rs   ← 系统代理解析（macOS scutil / Windows 注册表）与子进程环境
│       │   ├── reasoning.rs      ← 推理档位映射与 WorkBuddy 回退策略
│       │   ├── model_status.rs   ← 模型状态构造、请求元信息、客户端展示 ID（`OC · 名称`）
│       │   ├── platform.rs       ← 数据目录、路径/平台原语、运行时包名
│       │   ├── atomic.rs         ← 原子替换（重试 → 等待 → 换名，Windows 共享冲突友好）
│       │   └── json.rs           ← 容错 JSON 解析 + JS 语义原语（truthy/js_stringify/…）
│       └── tests/
│           ├── js_parity.rs      ← JS↔Rust 对拍：与 fixtures 里冻结的 expected 比较（默认无需 Node）
│           ├── red_lines.rs      ← 运行期红线守卫（鉴权/Origin/权限表/隔离配置/限额/环境白名单）
│           └── fixtures/*.json   ← 11 个模块共 271 个用例，每个带 expected 黄金快照
├── scripts/                      ← bump-version.mjs / check-version.mjs / gen-latest-json.mjs / gen-latest-json.test.mjs / make-dmg.sh
│                                    with-updater-key.mjs（tauri:build 的签名注入包装器）/ with-updater-key.test.mjs（`npm run test:updater-key`）/ make-dmg.sh（hdiutil 出 macOS 的 .dmg）
├── docs/                         ← contract.md、validation.md、version/*、research/upstream-architecture.md
├── .github/workflows/release.yml ← cargo 化 CI（无 sidecar 步骤）
├── .env.example / .env.local     ← 签名环境变量（.env.local 严禁提交）
└── .gitignore                    ← 忽略 node_modules、dist/、src-tauri/target|gen、.zwork/、src-tauri/binaries(防误加回)
```

> **为什么核心是 `src-tauri/core/` 而不是并进壳的单个 crate**：核心仍是独立 crate（`wbbridge-core`，
> 且是**独立 workspace**），壳通过 `path = "core"` 依赖把它静态编进同一进程。这样核心的 283 个测试
> 不必编译 tauri/webkit 依赖图（CI 的 Linux 测试任务因此无需装 libwebkit2gtk），`wbbridge-core`
> 也能单独构建出可执行文件做进程级冒烟；同时全部 Rust 代码物理位置都在 `src-tauri/` 下。
> 合并成单 crate 会把这三点全部丢掉，故不采用。

## 核心架构与数据流

### 进程模型

```
┌──────────────── Tauri 壳（单一进程）─────────────────┐
│  WebView: dist/（Vue）↔ invoke(core_action/…) / core-status │
│  托盘菜单 · 状态轮询 status.json · 退出流程              │
│  专用 tokio Runtime ──► orchestration::run(StartOptions)│
│                          │                              │
│  回环 HTTP 127.0.0.1:<port>/admin/*（壳→核心，Bearer）  │
└───────────────────────────┬─────────────────────────────┘
                            ▼
             核心编排（同进程，函数调用 + HTTP）
        ├── HTTP 服务 127.0.0.1:<port>（Bearer 鉴权、拒绝 Origin、并发 ≤4）
        ├── status.json（串行 + 原子写）→ 壳/UI 读取
        ├── WorkBuddy models.json（原子写 + 增量合并）
        └── OpenCode 子进程（隔离 env + 随机回环端口 + serve --pure）
```

- **核心不得单方面结束进程**：嵌入壳时 `set_exit_hook` 把「退出」翻译成记录 `core_code` / `core_stopped`，真正退出只由壳的**关窗 / 托盘退出 / `RunEvent::ExitRequested`** 驱动。
- **关闭窗口即退出应用（全平台一致）**：`on_window_event` 的 `CloseRequested` 在非退出态下 `prevent_close()` + `hide()`，随后在后台线程执行 `quit_app`（= `stop_core_bounded` → `cleanup_before_exit` → `exit(0)`）；**不再隐藏到托盘驻留**，用户无需二次退出。`show_main` 在 `quitting` 置位后直接返回，退出收尾期间不会被托盘左键唤回。
- **壳保留回环 HTTP 调用路径**（`admin_call` + `ACTION_ROUTES`），因此鉴权、Origin 拒绝、并发上限等红线与迁移前完全一致，`core_action` 契约未变。
- 核心可重启：`restart_core` 关停现有实例后按新的 `StartOptions` 重新装入（`APP` 为可重置的进程级全局）。

### 启动时序（`src-tauri/core/src/orchestration.rs`）

1. 端口校验（非 1024–65535 直接失败退出，独立进程按 1）。
2. 数据目录（`StartOptions.data_dir` > `BUDDY_DATA_DIR` > 平台默认），`create_dir_all` + `0700`。
3. **单实例锁**：读 `service.pid`，若指向存活进程且不是自己 → `ALREADY_RUNNING`（独立进程退出码 2）；否则写回自身 pid。
4. 读/建 `api-key`（缺失时生成 32 字节随机 hex，`0600`）。
5. 读 `settings.json`（容错解析）、上一份 `status.json`（延续 `modelResults`）、日志 >5MB 轮转。
6. 解析 models 文件路径（`BUDDY_MODELS_FILE` → settings 已保存值 → WorkBuddy 配置目录发现；失败返回 null，**绝不静默回退**）。
7. `find_runtime` 定位或下载 OpenCode 运行时 → `start_backend` 启动子进程 → axum HTTP 服务开始监听。
8. `/agent` 校验：确认隔离配置中的 `buddy-bridge`、`buddy-chat` 两个自定义 agent 存在，否则 `phase` 不进入 `ready`。
9. 系统代理（`useSystemProxy`）：macOS `scutil --proxy`、Windows 注册表解析注入子进程 env；**解析失败回退为关闭，不阻断启动**。
10. 启动导入（`BUDDY_NO_SYNC !== "1"`）：**先清除旧 own 条目并 `drain_sync().await`，再逐模型探测（每模型各 `PROBE_TIMEOUT_MS = 60000` 预算，重试共用同一 deadline）**，仅 `ok` 的模型进入发布集。
11. 同步发布集到 WorkBuddy `models.json`，启动状态轮询/活动计时/刷新与探测后台任务；服务已在监听时启动失败会保留进程，由壳经 `/admin/shutdown` 结束这次启动。

### 请求链路（`/v1/chat/completions`）

```
WorkBuddy
  → server.rs：Bearer 鉴权（timing-safe）→ 拒绝 Origin（403）→ 限流（>4 并发 429 busy）→ 读体（>8MB 413）
  → protocol::prepare(body, models)：模型匹配 / messages 校验 / n=1 / tool 定义唯一性 / 图片 mime 白名单（png·jpeg·webp·gif）
  → backend::complete(...)：OpenCode 会话 + 消息 + 事件流
        ├─ 原生工具调用 → native_permissions（'*': 'ask'，question/websearch/codesearch/webfetch/task/plan_enter/plan_exit/todowrite: 'deny'）
        │    → 阻塞并交回客户端（handoff），模型不得自行执行本地动作
        └─ chatOnly 模型（buddy-chat）遇原生工具 → 502 native_tool_activity
  → protocol::decode(text, prepared)：校验信封 {content, calls}、校验工具与参数
        ├─ 一次格式修正（resend_prompt：只纠正格式）
        └─ 仍失败 → repair()：由辅助模型（translator，TRANSLATOR_ORDER 中挑，**排除刚失败的模型**）转写一次；探测路径禁止转写
  → completion() / send_sse()：非流式 JSON 或 SSE（先校验后发送，含 10s 心跳注释行）
  → 【断连兜底】backend::CompleteGuard（Drop）：放行 AbortController → 停掉每 250ms 的审批轮询 → 清 `usage_by_session`/`pending_approvals`/会话表条目 → 表空时 `stop_events()` → 派发 abort + DELETE。客户端断开时 hyper **丢弃整个 future**，写在 `.await` 之后的收尾不执行，所以清理必须挂在这上面而不是函数末尾
  → on_result：写入 status.json 计数与最近请求（客户端取消的请求绝不记为成功）
```

### 模型发布链路

```
models.json → free_models(providers)（免费判定：输入/输出/缓存全 0、输出支持文本、非 deprecated）
  → 逐模型探测（probe_model：工具调用类 + 纯文本类，失败可重试 1 次）
  → 仅 ok 的模型进入发布集（客户端 ID = `OC · <name>`）
  → sync_models（原子写）→ merge_models（保留非本工具条目，只清理/更新 OWNER = 'buddy-bridge-v1' 的条目）
```

### 运行时隔离链路

```
find_runtime
  ├─ 优先级：BUDDY_OPENCODE_PATH > 托管目录 runtime/<version>/ > ~/.opencode/bin/（含 Homebrew 等）> Windows npm 全局 shim
  │   （拒绝 .cmd/.bat/.ps1 启动器脚本；版本必须形如 x.y.z）
  └─ 与 registry 最新版比对：本地不落后则复用；否则下载
       registry 顺序：https://registry.npmjs.org → https://registry.npmmirror.com
       校验：metadata.name/version 合法 + dist.integrity 必须为 `sha512-` + tarball 必须来自白名单 registry
       安装：下载 → sha512 校验 → 解包只取 `package/bin/<binary>` → 原子替换 → chmod 755 → 回读版本必须一致
start_backend
  → env 白名单（ENV_ALLOW：PATH/HOME/USER/LANG/TMPDIR/SHELL/SSL_CERT_FILE/NODE_EXTRA_CA_CERTS 与 Windows 必需项）
  → XDG_* 指向数据目录 + 独立随机 OPENCODE_SERVER_PASSWORD
  → 关闭 OPENCODE_DISABLE_AUTOUPDATE / _PROJECT_CONFIG / _CLAUDE_CODE / _EXTERNAL_SKILLS
  → OPENCODE_CONFIG_CONTENT = isolated_config()（权限全 ask/deny、autoupdate:false、share:disabled、两个自定义 agent）
  → 启动 `serve --pure --hostname 127.0.0.1 --port <随机空闲端口>` → 轮询 /global/health（≤120 次 × 500ms，版本必须一致）
  → 关停：SIGTERM → 等待 ≤4s → SIGKILL
```

## 关键模块与函数清单（修改前必须确认）

| 模块 | 关键公开项 | 职责 |
|---|---|---|
| `orchestration.rs` | `run(StartOptions)`、`StartOptions{data_dir,port,handle_signals}`、`set_exit_hook`、`HELP`、`ALREADY_RUNNING`；内部 `update_and_persist`、`sync_published`、`drain_sync`、`record`、`usable_models`、`attach_translator`、`start_probes`、`drop_pending`、`refresh`、`read_models`、`import_models`、`shutdown`、`startup_sequence`、`bootstrap`、`watch_runtime`、`spawn_parent_watchdog`、`note_activity`、`update_with_usage`（第三参 `model_result` **只写调用方自己那一键**）、`apply_patch`、`update`、`probe_meta`（探测元信息，`probe: true` 是探测路径禁转写的开关点）、`restored_model_results`（非对象形状回落空表）、`restored_models`/`validated_from_results`/`reusable_previous`（启动沿用上轮结果的三份数据一起恢复）、`Scope{All,Platforms}`+`covered`/`covers`/`labels`（一轮读取+探测的作用范围，按命名空间判定）、`merge_scoped_models`/`strip_validated`（定向范围只替换本平台的目录段与通过标记）、`probe_platforms`（定向重取+重检的后台入口，走 `/admin/probe`）、`requested_providers`（`providers` 请求体解析，形态不对一律当场拒绝）、`accumulate_usage`、`fresh_usage`、`resolve_api_key`、`write_secret_file`、`providers_io`、`provider_status`、`set_provider_key`、`clear_provider_key`、`provider_id`、`provider_key_input` | 编排与生命周期（对应旧 `main.js`；`record` 经 `update_with_usage` 串行写盘，并累加 `status.json` 顶层 `usage`；三个 `provider-*` 动作的处理器把同步 IO 挪进 `spawn_blocking`，入参校验留在外壳之前以便映射成 400） |
| `server.rs` | `Server::new(key)` 链式注入 → `build() -> (Router, ServerControl)`、`serve`、`ACTION_ROUTES`（**8 条**）、`route_for`/`method_for`、`MAX_BODY_BYTES`、`MAX_CONCURRENT_REQUESTS`、`DEFAULT_HEARTBEAT`、`REQUEST_BODY_TIMEOUT`、`AbortController/AbortSignal`、`ResultRecord`、`Handlers`、`BoxFuture`、类型别名 `CompleteFn/ModelsFn/AdminFn/…` | HTTP 路由与鉴权：`GET /health`、`GET /v1/models`、`POST /admin/{probe,system-proxy,import,refresh,shutdown,provider-status,set-provider-key,clear-provider-key}`、`POST /v1/chat/completions`。三个 `provider-*` 动作的处理器类型都是 `AdminFn`（收整份请求体），`probe` 的类型是 `ProbeFn`（v1.1.2 起同样收**整份**请求体 `{ model?, providers?[] }`，因为路由必须在解析失败时原样回送拒绝体、不能把形态不对的请求静默降级成全量探测），其中 `provider-status` 与 `refresh` 一样**不读请求体**。请求体读取统一走 `read_body` → `read_body_bounded(body, REQUEST_BODY_TIMEOUT)`（`:1203`/`:1210`，2026-10-10 起 `/admin/*` 与 chat 路径**共用同一份 20s 预算**；此前管理路由没有它，而管理路由不受 `MAX_CONCURRENT_REQUESTS` 限制，一个只发头不发体的连接能无限期占住任务与缓冲），超时映射 408 + `code = "timeout"` |
| `protocol.rs` | `BridgeError`（`with`/`status`/`code`）、`prepare`、`PreparedRequest`、`decode`、`completion`/`completion_with`、`send_sse`、`random_hex_id`/`random_uuid`、`parse_image_data_url` | OpenAI 兼容入参校验、信封解码、响应组装、SSE |
| `backend.rs` | `Backend`（`complete`、`set_translator`）、`native_permissions()`、`free_models`（= `free_models_in(providers, OPENCODE_NAMESPACE)` 的包装）、`free_models_in(providers, namespace)`、`model_target(model)`、`shrink_permission`、`to_bridge_error`、私有的 `CompleteGuard`（**断连兜底守卫**：`Drop` 里放行 `AbortController`、清 `usage_by_session` / `pending_approvals`、必要时 `stop_events()`，异步 abort+DELETE 经构造时抓到的 `Handle` 派发；`finished` 标记使内联 `finally` 与 `Drop` 幂等） | OpenCode HTTP 客户端、事件流、原生审批拦截、免费模型发现（命名空间已参数化）。⚠ **清理只能挂在 `Drop` 上**：客户端断开时 hyper 直接丢弃 handler future，写在 `.await` 之后的任何收尾都不会执行 |
| `runtime.rs` | `find_runtime`、`isolated_config()`、`isolated_environment`、`allowed_environment`（`ENV_ALLOW` 白名单过滤，**每一次 spawn 都必须先过它**，含 `--version` 这类一次性调用）、`ENV_ALLOW`、`start_backend`/`Started`、`stop_backend`、`runtime_candidates`、`compare_versions`、`generate_password`、`RuntimeOptions`、`FetchFn/LatestFn/…` | 运行时定位/下载/校验/启动与隔离配置 |
| `probe.rs` | `PROBE_TIMEOUT_MS`、`probe_tools`、`probe_body`、`judge_probe`、`format_unsupported`、`tool_call_unsupported`（上游明说「该模型不支持函数/工具调用」→ 归入格式类失败、走 chatOnly 降级）、`chat_only_fallback_confirms_chat_only`（兜底的纯对话请求也以同理由被拒 → 确认仅对话，不得整个丢弃）、`retryable_probe_codes`、`probe_model`、`probe_failure`（**上游英文原文的唯一文案改写漏斗**：地区拒绝 → 撤架 → 未承接服务 id → 未知服务 id → 模型已下线，`code`/`status` 逐字保留、不触发重试）、`region_unavailable_message`（上游按出口 IP 的地区拒绝 → 可读中文文案）、`deprecated_model_message`（提供方撤架 → 可读中文文案）、`unserved_model_message`（ModelScope 网关 `has no provider supported` → 说明该 id 未被在线推理承接）、`unknown_service_id_message`（同一句上游拒绝按**措辞**分流两种说明：「不在在线清单里」与「这把 Key 没有该模型的访问权限」）、`should_retry` | 模型探测协议与判定 |
| `providers.rs` | `PROVIDERS`（**四家平台的复核过集合**：`modelscope` / `siliconflow-cn` / `tencent-tokenhub` / `zhipuai`）、`Provider{id,label,npm,base_url,models}`、`DeclaredModel{id,name,context,output,tool_call}`、`PROVIDERS_FILE`（`providers.json`）、`MAX_KEY_CHARS`、`find`/`check_key`/`read_keys`/`status`/`set_key`/`clear_key` | 多平台接入的注册表（唯一真相，随版本发布、不做远程拉取）与 `providers.json` 凭据通道：读盘容错（坏文件＝没配过）、写盘走 `sync::atomic_write`（临时文件 `0600` 独占创建再 rename）、`status()` 只回 `id/label/configured`。`Provider.models` 是**该平台的权威模型清单**——非空即替代 models.dev 的 catalog 声明，同时约束注入段（`runtime::providers_section_for`）与发现阶段的放行集合（`backend::free_models_in`）；空切片＝沿用 catalog + 锚点占位。目前只有 `modelscope` 带清单（9 条，2026-10-10 实测自其公开 `/v1/models` 并按真实 Key 剔除打不通的条目），其余三家为空 |
| `repair.rs` | `REPAIR_SYSTEM`、`client_conventions`、`raw_material`、`tool_catalog`、`repair_body`、`extract_json`、`translator_request`、`resend_prompt`、`RepairDeps`、`repair` | 格式修复与辅助模型转写 |
| `handoff.rs` | `build_handoff`、`handoff_input`、`reject_feedback`、`validate_action`、`has_category` | 原生工具 handoff 协议 |
| `sync.rs` | `OWNER`（`'buddy-bridge-v1'`）、`LOCK_STALE_MS`、`atomic_write`、`merge_models`、`sync_models`/`sync_models_with`、`SyncOptions`、`SyncOutcome`、`SyncIo`、`SyncError`；私有的 `sweep_old_backups`/`backup_prefix`/`backup_of`（**写路径不再产生任何备份**，这三个只负责清扫旧版攒下的同族 `<file>.buddy-bridge-<毫秒>.bak` 存量，两条出口都调用） | WorkBuddy 配置写入与增量合并 |
| `workbuddy_config.rs` | `validate_models_file`、`ensure_models_file`、`EMPTY_MODELS_FILE_TEXT`（`"[]\n"`）、`resolve_models_file`、`resolve_models_path`、`ConfigError`、`INVALID_PATH_MESSAGE`、`MODELS_FILE_NAME`/`DEFAULT_DATA_FOLDER` | models.json 定位与校验：插件目录已在而配置文件缺失时补建空配置（文件已存在绝不改写、父目录不存在绝不建目录、显式位置失效一律不补建） |
| `codebuddy_config.rs` | `DEFAULT_DATA_FOLDER`（`.codebuddy`）、`MISSING_MESSAGE`、`resolve_models_file` | CodeBuddy `models.json` 定位的对照实现（优先级与空串假值语义与 WorkBuddy 版逐行对照、复用同一校验器、失效绝不静默回退） |
| `targets.rs` | `Target{WorkBuddy,CodeBuddy}`、`Target::ALL`、`resolve_target_models_file`、`detect_targets`、`validate_selected_models_file`（导入链对称包装，保留原始错误）、`missing_reason`、`aggregate_sync` | 模型发布写入目标的唯一真相：「已安装」只以定位成功判定；`aggregate_sync` 产出 `sync.targets` 形状（count 为各成功目标之和、顶层 error 仅在全部定位目标失败时出现） |
| `system_proxy.rs` | `parse_system_proxy`、`system_proxy_environment`、`environment_from_output`、`parse_windows_proxy`、`js_number`、`ProxyError` | 系统代理发现与子进程 env 映射 |
| `reasoning.rs` | `EFFORT_LEVELS`、`reasoning_efforts`、`work_buddy_reasoning` | 推理档位与回退策略 |
| `model_status.rs` | `model_result`/`model_result_at`、`with_request_meta`、`client_model_id`、`OPENCODE_NAMESPACE`、`join_namespace(ns, key)`/`split_namespace(id)`（按**第一个** `/` 切，无 `/` 时命名空间为空串）、`CATEGORY_*`、`now_iso8601` | 状态记录与客户端展示 ID（前缀 = 注册表平台的 `label`，其余一律 `OC`）、全限定模型 id 的唯一拆拼处 |
| `platform.rs` | `data_directory`/`data_directory_with`、`DATA_DIR_NAME`、`runtime_package`、`host_platform`/`host_arch`、路径原语 | 平台路径与运行时包名 |
| `atomic.rs` | `replace_with_retry`/`replace_with_retry_with`、`ReplaceError`、`DELAYS`、`TRANSIENT_CODES`、`node_code_for_io` | 原子替换（Windows 共享冲突重试） |
| `json.rs` | `parse_json`、`Env`、`truthy`、`strict_eq`、`js_stringify`/`js_stringify_pretty`、`number_from_f64`、`type_of` 等 | 容错 JSON + JS 语义等价原语 |
| `src-tauri/src/lib.rs` | `core_action`、`restart_core`、`core_running`、`data_dir_path`、`read_log`、`open_external`（`generate_handler` 六命令；前两个是 **async 命令**，阻塞的 key 轮询与回环 HTTP 走 `spawn_blocking`；`read_log` 无参数、只读数据目录内运行日志的尾部，返回 `{text, truncated, bytes}`；`open_external`（1.1.0 新增）用系统浏览器打开 https 外链——Tauri WebView 里 `target=_blank` 默认静默失败，申请 Key 的官方页必须经壳转发，安全边界 = 仅 https + RFC 3986 字符白名单，应用自有命令不经 capability 授权）、事件 `core-status`（轻量快照）/ `core-activity`（activity + modelResults）/ `core-failed`（**故障期间每 ~4s 重播**，面板晚注册监听也能收到）、`start_core`/`stop_core`/`watch_status`/`service_down`/`restart_core_with`/`build_tray`/`quit_app`/`stop_core_bounded`/`show_main`/`admin_call`（`on_window_event` 的 `CloseRequested` → `quit_app`，即**关窗即退出**，全平台一致）、`status_read_needed`（`watch_status` 的 `(mtime, 长度)` 前置过滤判定，**只在拿得到 mtime 时**才允许跳过读内容；有单测守卫） | 托盘壳：生命周期、IPC、状态轮询、退出预算 |
| `src/core/bridge.js` | `action(name, value)`、`onState(cb)`、`onDismiss(cb)`、`readLog()`、`dataDir()`、`checkUpdate()`、`downloadUpdate(onProgress)`、`relaunchApp()` | 面板与壳/插件的唯一边界（invoke + listen + 插件全局绑定）；`readLog`/`dataDir` 是**只读**调用，不接受路径入参、不写文件。`check()` 返回的 Update 句柄只留在模块内（组件拿的是可序列化快照 `{version, notes}`）。三个 `core-*` 事件走**帧级合并**（`requestAnimationFrame` 每帧最多 flush 一次；替换型事件入队前先清空队列），因此对订阅者语义等价、但 **`lastState` 滞后一帧**——新增**同步读 `lastState`** 的代码（如 `action('import')` 里的 `lastState.modelsFile`）必须容忍这一帧延迟 |
| `src/core/update.js` | `subscribe(cb)`、`check({silent})`、`install()`、`restart()`、`dismiss()`、`setAutoCheck(enabled)`、`startSilentCheck()` | 更新状态机（`idle`/`checking`/`available`/`downloading`/`ready`/`uptodate`/`error`）；冷启动 5s 后**按偏好与 12h 节流**静默检查，静默失败不打扰用户，上次检查时间戳只在**成功**打到端点后写入。视图不得自行持有状态 |
| `src/core/prefs.js` | `VIEW_IDS`、`DEFAULTS`、`SILENT_CHECK_MIN_INTERVAL_MS`、`isAllowedKey(key)`、`readPref(key)`、`writePref(key, value)`、`loadView()`/`saveView(id)`、`loadUpdatePref()`/`saveAutoCheck(enabled)`/`saveCheckedAt(ts)`、`shouldSilentCheck(pref, now, minInterval)`、`sanitizeView`/`sanitizeUpdate` | 面板偏好的**唯一**读写边界：键前缀 `wb.` + 写入前键白名单 + 值字段投影 + 序列化后 2KB 上限 + 存储不可用/抛错一律静默回落默认值。**凭据类字段（`api-key`、`OPENCODE_SERVER_PASSWORD`、models 文件路径）不在白名单内，结构上落不进 `localStorage`** |
| `src/core/activity.js` | `activityText(...)` | 活动文案统一（托盘与面板共用） |
| `scripts/*.mjs` | `bump-version.mjs`、`check-version.mjs`、`gen-latest-json.mjs`（+ 其 `gen-latest-json.test.mjs`）、`with-updater-key.mjs`（导出 `KEY_CONTENT_NAME`/`KEY_PATH_NAME`/`KEY_PASSWORD_NAME`、`ROOT`、`DEFAULT_KEY_PATH`、`ENV_FILES`、`parseEnvFile`、`expandHome`、`loadEnvFiles`、`firstConfiguredKeyId`、`keyIdFromPubFile`、`isInlineKeyShape`、`injectKey`、`buildCommand`、`main(argv)`；早前的 `resolveKey`/`checkKeyShape`/`configuredKeyIds`/`checkPairing`/`classifySignerError`/`dryRunSign`/`--check-only` 已随 2026-10-03 的注入器重写一并移除，**不要再按旧名调用**）+ 其 `with-updater-key.test.mjs`、`make-dmg.sh`（bash，hdiutil） | 版本单一来源同步与校验；更新清单生成（平台键由 artifact **目录名**的 target triple 推导，默认要求六平台齐全）；**updater 签名注入**（汇齐私钥与口令 → 归一成 `tauri build` 只认的内联 `TAURI_SIGNING_PRIVATE_KEY` → exec 目标命令；不判形态、不试签、配对不符只告警；密钥与口令只经环境传子进程、绝不打印。**已接入 `npm run build` 与 CI 的 `tauri:build`**）；macOS `.dmg` 生成 |

### 关键常量（改动前必须确认语义）

| 常量 | 值 | 位置 |
|---|---|---|
| 默认端口 | `41980`（`BUDDY_PORT` 覆盖，范围 1024–65535） | `orchestration.rs`；壳 `pick_port()` |
| 请求体上限 | `MAX_BODY_BYTES = 8MB` → 413 | `server.rs` |
| 并发上限 | `MAX_CONCURRENT_REQUESTS = 4`（超出 429 busy） | `server.rs` |
| 请求体读取超时 | `REQUEST_BODY_TIMEOUT = 20s` | `server.rs` |
| SSE 心跳 | `DEFAULT_HEARTBEAT = 10s` | `server.rs` |
| 探测超时 | `PROBE_TIMEOUT_MS = 60_000`（每个模型一份，该模型的重试共用同一 deadline） | `probe.rs`；`orchestration.rs::start_probes` |
| 日志轮转阈值 | `5MB` | `orchestration.rs` |
| 活动去抖 | `1000ms`（紧急文案除外） | `orchestration.rs::note_activity` |
| 同步锁过期 | `LOCK_STALE_MS = 5min` | `sync.rs` |
| 健康轮询 | `120` 次 × `500ms` | `runtime.rs` |
| 关停窗口 | SIGTERM → `4s` → SIGKILL | `runtime.rs::stop_backend` |
| 权限文本截断 | `shrink_permission(value, limit)`，调用侧固定传 `400` | `backend.rs` |
| 同步归属标记 | `OWNER = "buddy-bridge-v1"` | `sync.rs` |
| status schema | `STATUS_SCHEMA_VERSION = 1` | `orchestration.rs` |
| 状态内置版本 | `"0.2.0"`（写入 `status.json`，历史沿革值） | `orchestration.rs` 状态初值 |
| 格式类失败集合 | `REQUEST_SHAPED_FAILURES`（4 项） | `orchestration.rs` |
| 转写首选名单 | `TRANSLATOR_ORDER`（4 项，须排除刚失败模型） | `orchestration.rs` |

## 编码规范

> 以下规范适用于本项目所有代码改动，所有 Agent 新增或修改代码时必须遵守。

### 语言与依赖（Rust 核心）

- `src-tauri/core/` 是**纯 Rust、无 Node 依赖**；edition 2021，`rust-version = "1.90"`（与壳一致；下限由锁定依赖图决定——`icu_*` ← `idna` ← `url` ← `reqwest` 需要 1.88，壳侧 tauri 2.12 一线需要 1.90）。
- 跨语言语义对齐靠 `json.rs` 的 JS 等价原语（`truthy`、`strict_eq`、`js_stringify`、`Env`）与 `serde_json` 的 `preserve_order`；**不得改用无序 map**，键序是 `models.json` 合并语义的一部分。
- 新增运行期依赖必须说明理由（体积 + 供应链可信度双重成本）；能用标准库或既有依赖完成的绝不引包。禁止把任何 Node 运行时、pkg、esbuild 重新引入核心链路。
- 异步一律 `async/await` + tokio；共享可变状态用 `Arc<Mutex<..>>` / 原子量，串行写链（`Slot` + `CHAIN_SEQ`）不得改成"看起来更快"的并发写。
- 禁止 `std::env::set_var` 配置核心实例；一切走 `StartOptions`。
- 禁止在未确认生命周期安全的前提下使用 `unwrap()`/`expect()` 于错误路径之外的 panic 风险点；`Mutex` 取值统一 `.lock().unwrap()` 仅用于「中毒即不可恢复」的既有约定写法。

### 错误与注释

- 业务错误统一 `BridgeError { message, status, code }`（`protocol.rs`），HTTP 层据此映射状态码与 `error.type`；不得吞掉错误或返回无码错误。
- 注释写**为什么**（约束、坑、平台差异、红线由来），不写"是什么"；现有源码已建立该风格。
- 平台差异集中在 `platform.rs` / `system_proxy.rs` / `runtime.rs`（`#[cfg(unix)]` / `#[cfg(windows)]`），业务代码不得散落平台判断。
- 日志只写非敏感信息；**严禁把 `api-key`、`OPENCODE_SERVER_PASSWORD`、用户凭据写入日志或 `status.json`**。

### 文件与平台

- 文件写入优先走 `atomic.rs::replace_with_retry` 或 `sync.rs::atomic_write`，禁止裸 `rename` 覆盖；Windows 需考虑 `.exe` 与不能替换运行中镜像。
- 新建目录/敏感文件显式权限（目录 `0700`、密钥文件 `0600`）。
- 路径拼接使用 `platform.rs` 提供的原语，禁止硬编码分隔符。

### 测试规范

- 核心测试全部用 Rust：`cargo test`（`src-tauri/core/`）。基线 **283 通过 / 0 失败**（lib 261 + js_parity 11 + red_lines 11；2026-10-10 本轮实测。⚠ 该基线行此前长期写 278/lib 256，而按测试属性逐个数 HEAD 实为 **258** 个 lib 测试——先前某轮补测试时没回头改基线行，失真 2 个，本轮一并更正）：
  - lib 单元测试 256（含 `src/*.rs` 内 `#[cfg(test)]`；其中 `targets.rs` 7 项覆盖双目标检测（只装一个/两个都装）、CodeBuddy env 覆盖优先级、坏文件不回退、导入链对称包装（合法通过、坏形状保留原始契约文案）、聚合的 count 求和与「全部定位目标失败才报顶层 error」、契约字段名自检；`codebuddy_config.rs` 4 项与 `workbuddy_config.rs` 测试逐条对照（env > saved > 默认、目录变量覆盖、空串假值回落、显式位置失效不回退）；其中 `providers.rs` 7 项覆盖注册表唯一性、空目录状态、set/read/clear 往返与只删指定平台、状态与落盘文件都不含 Key、坏输入不落盘、损坏或外来形态按「未配置」读；Stage 2 另加 4 项：`model_status.rs` 的「前缀只跟注册表走、`opencode`/无命名空间一律 `OC`」与「拆合无损且与旧的定长截串逐个同值」、`backend.rs` 的「`free_models_in` 选对命名空间且包装同值」与「`model_target` 拆 `{providerID, modelID}`」；2026-10-03 代码复审修复另加 4 项，均在 `orchestration.rs`：探测元信息必须带 `probe` 标记、上一份 `modelResults` 非对象时回落空表、并发请求各写自己那一键不互相吞、合并时对非对象 map 自愈；2026-10-09 单模型重新检测另加 2 项，同在 `orchestration.rs`：在飞集合的重复登记/撤销只动自己那一 id、`probe` 对象被批量探测整份重写时 `singleProbes` 不受影响；2026-10-09（v1.1.2 轮）另加 6 项——`orchestration.rs` 5 项（上一份目录+通过集+结果三者一起沿用的判定、`providers` 请求体只认去重后的注册表平台且形态不对绝不降级成全量、作用范围只覆盖自己的命名空间、定向重取只替换本平台的条目、重检一家只摘它名下的通过标记 + `/admin/probe` 的歧义与未知平台必须在碰到编排状态之前被拒）与 `sync.rs` 1 项（重复发布后只保留最新那一份备份）；同轮另加 `probe.rs` 2 项（上游按出口 IP 的地区拒绝改写成可操作中文文案且**不动 code/status/不触发重试**、「被拒」与「地区」两类词不同时就绪不得被判成地区问题）；同轮另加 `probe.rs` 2 项（上游明说「该模型不支持函数/工具调用」时按既有 chatOnly 降级通道处理、而不是直接判不可用；地区拒绝/鉴权失败/限流/上下文超长/chatOnly 越权都不得被误判成这一类）；同轮另加 `probe.rs` 2 项（提供方撤架的上游原文改写成可操作中文说明且**同样不动 code/status、不触发重试**；与模型无关的弃用提示、地区拒绝、鉴权失败、限流、函数调用不支持都不得被说成模型已下线）；同轮另加 `sync.rs` 1 项（内容无变化的同步也要把旧版攒下的同族备份收敛到最新那一份，同时不得新建备份、不得改动正在使用的配置、不得碰用户自己的 `*.bak`）；2026-10-10 另加 4 项——`workbuddy_config.rs` 2 项（插件目录已在而 `models.json` 不在时补建空配置、且既有内容绝不被改写；目录不在不造目录、显式位置失效也不补建）、`codebuddy_config.rs` 1 项（同款对称行为）、`targets.rs` 1 项（`detect_targets` 下两个空插件目录都被检出并各自补建）；2026-10-10（`--version` 回读超时）另加 `runtime.rs` 3 项（超时后重试一次并可成功、非超时失败绝不重试且原样透传、两次都超时的文案把「重试」排在系统代理之前且不把代理说成唯一入口）。⚠ 上面这份枚举是**沿革记录**，逐轮的加减项不再逐条追平（同名测试被改写/合并会让「新增 N 项」与净增量对不上）；以 `cargo test` 实测为准——2026-10-10 之后又补进了 ModelScope **权威清单**（`providers.rs` 清单形态与命名空间归属、`backend.rs` 发现阶段只放行清单内的 id）、上游措辞分流（`probe.rs` 的未承接服务 id / 未知服务 id /「这把 Key 没有访问权限」三类各自成句且不改变判定、chatOnly 兜底以同理由被拒时确认仅对话）与退出时的空同步只删本工具名下条目（`sync.rs`）
  - `tests/js_parity.rs` 11（每模块一组，比较 `tests/fixtures/*.json` 冻结的 `expected`）；**等价重构的验证方式就是以这 11 项全绿 + `git diff --stat src-tauri/core/tests/fixtures` 为空为准，不得改夹具**；
  - `tests/red_lines.rs` 11（运行期红线守卫；`provider_registry_is_reviewed_and_status_echoes_no_key_material` 钉死四平台 id 集合、https-only、`npm` 形态，以及写入真实 Key 后 `status()` 序列化里既无 Key 也无 `apiKey`；`version_probe_child_environment_only_passes_the_allow_list` 钉死 `--version` 这类一次性子进程调用同样只透传 `ENV_ALLOW` 白名单）。
  壳（`src-tauri/`）另有 `cargo test --lib` **9 通过**：日志尾部读取 4 项 + `shell_action_routes_match_the_core_contract`
  （断言 `ADMIN_ROUTES` 与核心 `ACTION_ROUTES` 逐项一致）+ 退出链路 3 项（`repeated_quit_requests_stop_only_once`、
  `a_quit_requested_shutdown_is_not_reported_as_failure`、`service_down_reports_only_real_failures`）
  + `status_read_needed_only_skips_when_mtime_is_known`（`status.json` 轮询快路径：只在拿得到 mtime 且 `(mtime, 长度)` 未变时才允许跳过读取）。
- **JS 侧只有四个测试套件，都用 `node --test`（不引测试框架、不依赖 DOM，基线合计 41 通过 / 0 失败）**：
  - `npm run test:prefs` = `node --test src/core/prefs.test.js` → **8 通过**：`prefs.js` 的键白名单、值字段投影（凭据类字段绝不落 `localStorage`）、坏数据回落默认值、存储不可用/抛错时静默降级、超大值拒写、静默检查 12h 节流。测试用 `globalThis.localStorage` 取值 + `withStorage(fakeStorage(...), fn)` 注入假存储来跑。
  - `npm run test:ops` = `node --test src/core/ops.test.js` → **11 通过**：操作守卫模块 `src/core/ops.js` 的常量合法性、终态动作集合与文案表、在飞互斥文案稳定性（连点不闪烁反馈条）、完成冷却窗口与时钟回拨处理、成功反馈文案的存在性与视图自有反馈的边界（import 与平台 Key 动作不被代写）。改动 `ops.js` 的文案/守卫阈值/冷却集合，**必须同步改对应测试**。
  - `npm run test:manifest` = `node --test scripts/gen-latest-json.test.mjs` → **9 通过**：`latest.json` 唯一写者的六平台成功路径（平台键只由 artifact **目录名**的 target triple 决定、url 空格编码、签名内容取自配对 `.sig`）与七类必须失败/告警的路径（缺平台、缺 `.sig`、mac 资产名漏架构后缀、同平台两目录、判不了平台的目录、Linux 双层打包、同目录互不相干的两个已签名包）。用 `spawnSync` 跑真脚本 + `mkdtempSync` 临时产物目录，**不联网、不碰真实产物**，版本号现读 `tauri.conf.json` 因此不随版本推进失效。
  - `npm run test:updater-key` = `node --test scripts/with-updater-key.test.mjs` → **13 通过**：签名注入包装器 `with-updater-key.mjs` 的取值优先级（进程环境 > 仓库 `.env.local` / `.env` > `~/.tauri/wbBridge{,-updater}.env`；**显式空口令压过文件里的口令**；`~/…` 先展开）、内联位两种形态的判定与归一（单行 base64 全文 vs `.key` 文件路径 → 读成全文并 **trim**）、路径注入后删掉互斥的 `_PATH`、明文钥显式置空口令 / 加密钥缺口令**只告警不阻断**、公钥配对**只认配置第一条且只告警**、`buildCommand` 三种入参形态、CLI 在**相对路径**调用下确实执行且**任何输出都不回显密钥**。另有「接线」一项钉住：`tauri:build` 确实经本包装器、`build` 链 `make:dmg`、`release.yml` 把私钥读成 `vars.` 且调用 `npm run tauri:build`（**无** `tauri-action`、**无**私钥前置步骤）、`plugins.updater.pubkey` **恰好一条公钥**、`bundle.targets` **不含 dmg**。测试只用合成的假 base64 串 + `mkdtempSync` 临时目录，**不联网、不调 tauri CLI、不读 `~/.tauri`、不碰真实私钥**。
  三组都已进 CI 的 `test` 作业（「运行面板偏好、更新清单与签名注入单测」一步）。改 `prefs.js` 的白名单/投影逻辑、`gen-latest-json.mjs` 的平台识别与失败判定、或 `with-updater-key.mjs` 的取值/内联归一/口令/配对判定，**必须同步改对应测试**。
- **JS↔Rust 对拍已快照化**：`tests/fixtures/*.json` 每个用例带 `expected`（迁移前由 JS 实现录制、已抹平随机 id / `created` / `ms` / sync 文案；沙箱绝对路径在比较前还原成 `$BASE` 占位符，快照因此不绑定机器与目录布局）。默认不启动 Node。需要重新录制时，把仓库外归档 `backup/wbBridge-node-20261001/` 的 `core/` 与 `core-rs-tests-js/`（归档内的目录名，放回后即 `tests/js/`）放回原位，再 `WB_PARITY_RECORD=1 cargo test --test js_parity`；**禁止**在没有 JS 真相的情况下手工编辑 `expected` 来"让测试通过"。
- **测试严禁真实联网、真实下载**：网络与运行时行为必须通过注入点（`RuntimeOptions` 的 `FetchFn`/`LatestFn`/`ProbeFn`、`SyncIo`、`atomic::replace_with_retry_with`）替换。
- 新增/修改行为必须补测试；测试名要描述被保护的行为。触碰安全/供应链/数据红线时，优先在 `tests/red_lines.rs` 补断言而不是只写文档。
- 修改 `src-tauri/core/src/` 后必须运行 `cargo test` 并报告**实际**通过/失败数量，**不得以"应该能过"代替执行**；`cargo clippy --all-targets` 必须保持 0 warning。

### UI 规范

- 面板 = Vue 3 SFC + Vite 构建产物 `dist/`，由 Tauri WebView 加载；**不得引入 CDN 或任何外部请求**，必须满足 `tauri.conf.json` 的 CSP（`default-src 'self'`、`script-src 'self'`、`connect-src ipc: http://ipc.localhost`）。
- 与壳的通信只走既有契约：`src/core/bridge.js` 的 `action(name, value)` / `onState(cb)` / `onDismiss(cb)`，底层是 `invoke('core_action' | 'restart_core' | 'core_running' | 'data_dir_path' | 'read_log')` + 事件 `core-status`（轻量快照，含顶层 `usage`）/ `core-activity`（`activity` + `modelResults` 明细，面板必须与最近一次轻量快照合并后再下发，否则逐模型状态永远为空）/ `core-failed`。新增只读命令时须同步 `src-tauri/src/lib.rs` 的 `generate_handler`；`capabilities/default.json` 对**应用自有命令无需改动**（自有命令不经 capability 授权），但**插件命令必须在此声明**——updater/process 已加 `updater:default` + `process:allow-restart`（刻意不用 `process:default`：它含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链硬杀进程）。**不得新增或改名 IPC 命令/事件**，除非同步更新 `src-tauri/src/lib.rs` 与 `src-tauri/capabilities/default.json`。
- 面板为只读展示 + 动作触发：不得在面板内直接读写文件、直接访问网络、直接调用 OpenCode。**唯一例外是自动更新**：`checkUpdate/downloadUpdate/relaunchApp`（`src/core/bridge.js`）调的是 Tauri 官方 updater/process 插件，联网发生在 **Rust 侧**（插件命令），不经 WebView `fetch`，因此 CSP 的 `default-src 'self'` 无需放宽；更新状态机集中在 `src/core/update.js`，视图只渲染与触发。
- 动作执行期间必须置忙（按钮禁用 + spinner + 结果反馈），失败必须显示原因。
- **面板偏好只走 `src/core/prefs.js`（2026-10-02 固化）**：视图不得直接碰 `localStorage`。新增一项偏好 = 在 `SANITIZERS` 里加一个键 + 加一个**只做字段投影**的 `sanitize*` 函数 + 补对应单测；**严禁**把 `api-key`、`OPENCODE_SERVER_PASSWORD`、WorkBuddy models 文件路径、完整 `status.json`、完整配置对象放进 `localStorage`（白名单是唯一的守卫点，值投影负责丢弃调用方多塞的字段）。存储不可用（WebView 禁用、配额满、抛错）时一律**静默回落默认值**，不得抛到界面。**当前范围**：只持久化「当前视图」与「启动后自动检查更新」开关（+ 上次成功检查时间戳，12 小时节流）；详情栏收起状态与窗口尺寸/位置**刻意未做**——后者要引 `tauri-plugin-window-state`，其与 `quit_app` 关停时序的交互在没有 GUI 实测的条件下无法确认，属待用户决策项。
- **交互反馈分层（2026-10-01 UI 优化后固化；2026-10-09 补强防抖/节流）**：等待用 `spinner` / `.skeleton` / `.busy-bar`，结果用 `FeedbackBar`（pending / error / success 三态；pending 态**不给关闭按钮**，防误判操作已结束）。**进度只许来自壳推送的真实数据**：`probe` 可显示「已完成/总数」（由 `probe.pending` 队列长度与当前模型数推算，只反映当帧快照，缺任一项即不显示），其余动作前端拿不到真实进度，一律只给文字、**不写数字**。
- **操作守卫（2026-10-09 固化，实现唯一在 `src/core/ops.js`）**：每一个触发后台动作的入口都必须让用户知道「点了之后发生了什么」——① **成功反馈**：refresh / restart / system-proxy / probe / import 成功后由 `App.vue::run()` 统一落成功文案（文案唯一来源 `ops.js::successMessage`），5 秒（`FEEDBACK_VISIBLE_MS`）自动消隐，失败文案不消隐；② **在飞互斥（防抖）**：同一动作在飞时重复触发拒绝并提示（`rejectionOf`），文案逐字稳定使连点不闪烁反馈条；只有渲染层 `disabled` 的入口（打开申请页、刷新日志/状态）必须另加同步在飞标记，双击在重渲染前会穿透模板 disabled；③ **完成冷却（节流）**：终态动作（`COOLDOWN_ACTIONS` = refresh / restart / system-proxy / import）成功后 5 秒（`ACTION_COOLDOWN_MS`）内重复触发拒绝并提示「刚完成」，失败不进冷却；④ 受控开关（侧栏系统代理）被拒绝时必须回弹 DOM 视觉状态，不得停留在假位置。改动守卫判定或文案必须同步 `ops.test.js`。
- **图标一律内联 SVG**（`stroke="currentColor"`）：禁止字符符号（▦▤◔⇄ⓘ 之类随系统字体变化、基线不齐、无法统一线宽），也禁止外部图标资源或图标字体（CSP `default-src 'self'`）。
- **样式硬约束**：颜色 / 时长 / 阴影一律取 `src/styles/variables.css` 的 token（交互层含 `--dur-*`、`--ease-*`、`--focus-ring`、`--on-primary`、`--shadow-s/m`、`--nav-hover-bg` / `--nav-active-bg` / `--nav-active-ring`），组件内不得写死数值；`prefers-reduced-motion` 下位移与淡入必须取消，但 `spinner` / `.skeleton` / `.busy-bar` 的循环**必须保留**（否则「正在加载」失去唯一载体）；所有可点元素必须有 hover、`focus-visible` 焦点环与 `disabled` 态。

#### 布局与窗口（2026-10-01 面板改造后的当前实现）

- **布局**：外层 `.shell` 为左右两段——左侧栏固定宽 `var(--sidebar-w)` + 右侧主区；主区默认单列，选中模型时 `.content.is-split` 变为
  `grid-template-columns: minmax(0, 1fr) var(--details-w)`（`src/App.vue`）。
- **详情面板是右侧常驻分栏，不是浮层**：无遮罩、不 `position: fixed/absolute`、不覆盖列表；**任何窗口宽度都不降级为上下堆叠**。
  禁止新增按宽度堆叠的媒体查询或 `<900px` 降级分支——`src/` 内现存的媒体查询只有 `prefers-color-scheme`（`variables.css`）与
  `prefers-reduced-motion`（`base.css`）两处。收起详情有三条等价路径：`Esc`（`App.vue::onKeydown`）、详情头部「收起详情」按钮、
  窗口失焦（`onDismiss`）；收起即清空选中，列表占满主区宽度。
- **侧栏是分组导航**：分组为「模型 / 运行 / 集成 / 其他」，底部为「运行设置」（系统代理开关 + 版本号）。**5 个入口都是已实现的视图**（视图状态由 `src/App.vue` 的 `view` 持有，侧栏按 `current` 高亮并带 `aria-current="page"`）：
  模型与服务（`.top` + `.content`，保留主从两栏）、运行日志（`src/views/LogsView.vue`，经 `read_log` 只读拉取日志尾部）、用量与额度（`src/views/UsageView.vue`，渲染 `status.json` 顶层 `usage`）、
  WorkBuddy 集成（`src/views/IntegrationView.vue`，只读展示配置定位与最近发布结果，唯一动作是复用既有 `import`）、关于与更新（`src/views/AboutView.vue`，版本与数据目录只读 + **自动更新区**：启动后静默检查、发现新版本才出现更新条、由用户点「下载并安装」，进度百分比只在上游给出 `contentLength` 时显示，装完点「重启应用」经 `process.relaunch` 走 `ExitRequested` 的有界停止）。
  **禁止**再出现「规划中」标签、禁用占位或可点击却无响应的假入口。
- **`usage` 口径（`status.json` 顶层字段）**：`{"since": ISO, "total": {requests, ok, failed}, "models": {"<clientModelId>": {requests, ok, failed, lastMs, avgMs}}}`；只累计 `source == "request"` 的真实客户端请求——探测（`probe`）不计；客户端取消（连接断开）走 `server.rs` 的两条早退路径，**整条不计入**（`requests` 也不 +1，不是记为失败）；启动时从上一份 `status.json` 读回，与 `modelResults` 同法跨重启延续。
- **`schemaVersion` 升级口径**：向后兼容的**加法式**顶层字段（如本次新增的 `usage`，旧壳忽略未知字段即可）**不**递增两侧 `STATUS_SCHEMA_VERSION`，本次保持 `1`；只有删除 / 改名顶层字段、或改变既有字段语义（非兼容变更）时，才把 `src-tauri/src/lib.rs` 与 `src-tauri/core/src/orchestration.rs` 两侧常量**同步 +1**（规则原文见两处注释）。
- **设计 token**：尺寸与颜色一律取自 `src/styles/variables.css`；小字号说明文字（副标题、页脚说明、耗时行等）用 `--muted-strong`，
  普通次要文字用 `--muted`，不得在组件里写死颜色或宽度。

| 布局常量 | 值 | 位置 |
|---|---|---|
| 默认窗口 | `1120 × 720` | `src-tauri/tauri.conf.json` → `app.windows[0]` |
| 最小窗口 | `860 × 560`（`minWidth` / `minHeight`） | 同上 |
| 侧栏宽 | `--sidebar-w: 208px` | `src/styles/variables.css` |
| 详情栏宽 | `--details-w: clamp(300px, 45%, 360px)` | 同上 |
| 宽度断点 | **无**（不按窗口宽度堆叠，见上） | `src/` 内无宽度媒体查询 |

### 命名规范

| 类别 | 规则 | 示例 |
|---|---|---|
| Rust 文件/模块 | snake_case，单词表意 | `workbuddy_config.rs`、`model_status.rs` |
| Rust 函数/变量 | snake_case，动词开头表意 | `usable_models()`、`sync_published()`、`resolve_models_file()` |
| Rust 常量 | SCREAMING_SNAKE_CASE | `PROBE_TIMEOUT_MS`、`TRANSLATOR_ORDER`、`MAX_BODY_BYTES`、`OWNER` |
| Rust 类型 | CamelCase | `BridgeError`、`PreparedRequest`、`SyncOptions` |
| 对外契约 | 保持 JS 侧原名（snake_case 化） | 路由 `/v1/chat/completions`、`/admin/system-proxy`；错误码 `invalid_model_output` |
| Vue 组件文件 | PascalCase；视图放 `src/views/`，通用组件放 `src/components/` | `views/SideBar.vue`、`views/ModelDetails.vue`、`components/ModelRow.vue` |

## Git 提交规范

提交格式：`type(scope): content`

| type | 说明 |
|---|---|
| `feat` | 新功能 |
| `fix` | 修复 |
| `docs` | 文档 |
| `style` | 格式调整（不影响逻辑） |
| `refactor` | 重构 |
| `perf` | 性能优化 |
| `test` | 测试 |
| `chore` | 构建 / 工具 / 依赖 |

`scope` 约定：`core`（`src-tauri/core/`）、`ui`（`src/` 面板与 Vite）、`tauri`（壳与 `src-tauri/icons/`）、`scripts`（版本脚本）、`docs`（文档）、`ci`（工作流）。

示例：`refactor(core): 移除 Node sidecar，核心改为 Rust 内嵌`

提交纪律：

- 提交前必须运行 `cargo test`（`src-tauri/core/`）；测试失败禁止提交。
- **标签纪律（v1.0.1 事故教训，发布必须遵守）**：① 先 `npm run version:set -- <x.y.z>` 改写 5 处落点并**提交**，再**在该提交上**打 `v<x.y.z>` 标签——标签绝不允许指向"不含本次版本推进"的提交；② CI 的 test 作业有闸门：`GITHUB_REF_NAME` 去掉前导 `v` 必须等于 `tauri.conf.json` 的 `version`，不一致 exit 1（`check-version.mjs` 只看同一提交内部一致，抓不到标签指错）；③ 三个 build 作业上传前打印产物名，`update-manifest` 作业汇总六平台安装包名并把「产物版本 = tag 版本」自检结论写进 run summary，**待人工核对项不为 0 时不得点 Publish**；④ 删除/移动**已推送**的标签属共享状态变更，只能用户执行。
- 一次提交只包含一类改动，禁止把源码改动与大批二进制/图标混在一起。
- **严禁提交**：`api-key`、`.env.local`、`status.json`、`settings.json`、`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/core/target/`、`src-tauri/gen/`、`.zwork/`、任何私钥文件、以及仓库外的 Node 归档目录。
- 仓库现状（2026-10-01 实读）：分支 `dev`，远端 `origin = https://github.com/trexwb/wbBridge.git`（另有 `main`），`git ls-files` 有 **103** 个被跟踪文件（早期「只有 README.md 被跟踪」的说法已过时）；工作区仍有目录重组留下的删除记录，删除不等于丢数据（原 JS 核心在仓库外归档）。提交前务必 `git status` 复核实际纳入内容。

## 红线与限制

### 安全红线（不得移除、不得放松、不得绕过；由 `tests/red_lines.rs` 部分钉死）

1. **鉴权**：所有路由（含 `/health`）必须校验鉴权头（Bearer 方案，值 = 数据目录 `api-key` 文件内容），比较走 `server.rs::constant_time_eq`（长度不等直接 false，等长时用 `subtle::ConstantTimeEq`；**不得**换成手写异或累加，后者可能被优化成提前短路）；`api-key` 文件权限 `0600`，**缺失或内容为空白时随机生成并强制 `0600`**（沿用空白文件等于把期望头退化成空 Bearer 值）；`authorized` 对空密钥一律拒绝；裸 key / 非 Bearer 方案一律 401。
2. **拒绝浏览器来源**：请求带任何非空 `Origin` 头一律 403。WorkBuddy 只经其原生运行时访问本服务。
3. **仅监听回环地址**：`TcpListener::bind(("127.0.0.1", port))`（`orchestration.rs`），禁止改为 `0.0.0.0` 或对外开放。
4. **限流与体积**：并发 ≤4、请求体 ≤8MB，不得放宽。
5. **原生工具严禁开放给模型**：`native_permissions()` 必须保持 `'*': 'ask'`，且 `question`/`websearch`/`codesearch`/`webfetch`/`task`/`plan_enter`/`plan_exit`/`todowrite` 为 `deny`；`isolated_config()` 的 `autoupdate: false`、`share: "disabled"` 不得改动。**模型绝不允许直接执行本地动作**，原生动作只能以 handoff 交回客户端。
6. **chatOnly 模型不得使用工具**：纯文本 agent（`buddy-chat`）遇原生工具活动必须报 `native_tool_activity`，不得降级为静默执行。
7. **密钥零泄漏**：子进程环境只透传 `ENV_ALLOW` 白名单（其中不得出现任何凭据类变量名）；`OPENCODE_SERVER_PASSWORD` 只能用核心自己生成的值；密钥不得写入日志、`status.json` 或仓库。**唯一的允许位（2026-10-03 多平台接入后新增）**：`providers.rs` 可把**用户自己提交**的平台 Key 原子写到数据目录 `providers.json`（`0600`），入口只有 `POST /admin/set-provider-key` 的请求体；该 Key 不得出日志、出 `status.json`、出任何响应体——`/admin/provider-status` 的返回形态只有 `{ providers: [{ id, label, configured }] }`，**连掩码尾字符都不返回**（由 `providers.rs::status_never_contains_key_material`、`stored_file_holds_the_key_but_the_status_shape_does_not` 与 `red_lines` 的注册表守卫三条钉死）。后续 Stage 3 注入子进程时同样只能走 `OPENCODE_CONFIG_CONTENT`，**不得**把 Key 放进 `ENV_ALLOW` 或独立环境变量。

### 供应链红线（OpenCode 运行时）

1. 只能从**白名单 registry**（`registry.npmjs.org`、`registry.npmmirror.com`）获取运行时元数据与 tarball；元数据必须满足 `name` 匹配、语义化 `version`、`dist.integrity` 以 `sha512-` 开头、tarball 来源属于白名单。
2. **下载后必须校验 sha512 完整性**，校验失败不得使用、不得"跳过校验重试"；镜像与官方字节不一致时必须报错（测试已固化该行为）。
3. 解包只提取 `package/bin/<binary>` 单个文件，禁止整包解开到运行时目录。
4. 安装后必须**回读 `--version` 与元数据版本一致**，不一致即失败；禁止把无法确认版本的候选当作运行时。
5. 拒绝 `.cmd` / `.bat` / `.ps1` 启动器脚本作为运行时二进制。
6. 不得把运行时降级为"直接调用系统 `opencode` 的共享实例"——隔离（独立 XDG 目录、独立端口、独立密码、独立配置）是本项目的核心价值。

### 数据红线

1. 写 WorkBuddy `models.json` 时：**只清理/更新 `OWNER='buddy-bridge-v1'` 名下的条目，其他条目与对象元数据必须原样保留**；手动条目 ID 冲突时不得擅自覆盖。
2. `status.json` 写入必须串行 + 原子；探测中的临时状态不得覆盖已完成状态。
3. `service.pid` 单实例锁不得删除或绕过；重复启动必须失败（独立进程 `exit(2)`）而不是抢占端口。
4. 格式类上游失败（`invalid_model_output` / `invalid_tool_call` / `native_tool_activity` / `output_truncated`）**不得撤销已发布模型**；探测路径**不得启用辅助模型转写**；转写候选**不得包含刚失败的模型本身**。
5. 客户端取消的请求不得记录为成功。

### 工程红线

1. **禁止重构**：只做针对性最小改动；不得重命名已有导出、路由、错误码、IPC 命令与事件名。
2. **禁止重新引入 Node 运行期到核心**：不得为 `src-tauri/core` 加 sidecar/pkg/esbuild 链路，也不得新建第二个核心实现。
3. **禁止在仓库内写入运行时数据**：所有运行期文件（`api-key`、`status.json`、日志、runtime、opencode 目录）只能写入平台数据目录（或壳的 `app_data_dir`）。
4. **禁止编造已验证状态**：GUI 实机启动与 CI 跑通在本仓库**尚未实测**；签名链路只验证到**本机产出一个 key ID 与配置公钥配对的 `.sig`**，Windows/Linux 产物、发布后的 `latest.json` 与一次真实的升级闭环都**未实测**。涉及这些的说明必须标注"未验证"。
5. **禁止大规模二进制入库**：`src-tauri/binaries/` 已随 sidecar 方案废弃（目录已删除，`.gitignore` 仍保留守卫条目），不得复活该约定。
6. **版本推进必须依规执行、不得遗漏**：见下方「版本号规则」——自 2026-10-09 起，每次完成的修改都必须按规则递增版本号；唯一豁免是修复上一轮因理解错误造成的问题（返工）。
7. **Rust 代码只能放在 `src-tauri/` 下**：壳 = `src-tauri/src/`，核心 = `src-tauri/core/`（独立 workspace、壳以 `path = "core"` 依赖）。不得在仓库根重建 `core-rs/` 之类的第三个 Rust 位置，也不得把核心并进壳的单个 crate（那会让核心测试被迫编译 tauri/webkit 依赖图、失去独立二进制）。

## 当前基准版本

| 项 | 值 | 来源 |
|---|---|---|
| 版本单一来源 | **1.1.12** | 根 `package.json` 的 `version` |
| 壳工程同步落点 | **1.1.12** | `src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 的 `[package] version` |
| 核心 crate 版本 | **1.1.12** | `src-tauri/core/Cargo.toml`（`wbbridge-core --version` 输出；自 2026-10-10 起随产品版本同步，`version:set`/`version:check` 均覆盖） |
| 状态内置版本 | `0.2.0` | `src-tauri/core/src/orchestration.rs` 写入 `status.json` 的 `version`（沿自上游参考实现，界面上可见） |
| 上游调研基线 | `0.2.5` | `docs/research/upstream-architecture.md`（⚠ 2026-10-10 该文件已被并行轮次从工作区删除、尚未提交，git HEAD 里仍有） |
| 测试基线 | **283 通过 / 0 失败**（lib 261 + js_parity 11 + red_lines 11，约 1.1s）；壳 `cargo test --lib` 9 通过；JS 侧 `test:prefs` 8 + `test:ops` 11 + `test:manifest` 9 + `test:updater-key` 13 通过（合计 41） | `src-tauri/core/` 下 `cargo test`、`src-tauri/` 下 `cargo test --lib`、仓库根四个 `npm run test:*` |
| 迁移前 JS 基线 | 97 通过 / 0 失败（node v24.21.0） | 仓库外归档 `backup/wbBridge-node-20261001/core/test/` |
| 运行时基线 | OpenCode 版本由 registry 最新版决定（不固定）；核心不再需要 Node | `src-tauri/core/src/runtime.rs` |

**版本号规则（写死，所有 Agent 必须遵守）**：

- 版本号以**根 `package.json`** 为唯一来源；`src-tauri/core/Cargo.toml` 自 2026-10-10 起随产品版本同步（用户指令，`version:set`/`version:check` 均覆盖，共 7 处落点）；`status.json` 的 `0.2.0` 是历史沿革值，**仍不得在无用户指令的情况下静默改动**。
- `npm run version:check` 校验 7 处一致（`package.json` / `tauri.conf.json` / `src-tauri/Cargo.toml` / `src-tauri/core/Cargo.toml` / `AGENTS.md` 三行）；推进版本一律用 `npm run version:set -- <x.y.z>`。
- **强制递增规则（2026-10-09 用户设定，2026-10-10 落点扩为 7 处，所有 Agent 必须执行，取代本节旧有的「且用户明确允许」前提）**：**每一次完成的修改落盘后，都必须按规则递增版本号**，无需逐轮请示——运行 `npm run version:set -- <x.y.z>` 同步 7 处落点（含核心 crate `src-tauri/core/Cargo.toml`）、`npm run version:check` 通过，并在回复中如实报出新版本号与校验结果。档位按改动性质裁定：新功能或对既有功能同类范围的扩展 = minor（y+1）；缺陷修复（含同一自然日对同一模块/同一类 bug 的追加修复）= patch（z+1）。
- **唯一豁免（不递增）**：修复**上一轮因理解错误**造成的问题（同一问题的返工性重做）；此时版本号保持不变，并在回复中注明豁免原因。
- **以下情形仍不递增**（强制递增规则之外）：
  1. 用户明确要求"不修改版本号 / 回退到 X.Y.Z"（用户指令永远优先）；
  2. 纯 `docs/`、`README.md`、本 `AGENTS.md` 等不触碰代码行为的文档类改动——默认不递增，用户明确要求时从其指令；
  3. 纯文案、注释、日志措辞、去抖等不引入新逻辑分支的打磨——同上；
  4. 移植/迁移过程中的等价实现（行为未变即版本未变），除非用户对该次改动明确指示递增。
- 用户要求"回退到 X.Y.Z"时，所有落点必须一致改写为用户指定值，本回合内不得再以"我刚改了代码所以 +1"为由推进。
- **2026-10-09 版本回退（维护者裁定，已执行）**：`1.3.4 → 1.1.1（2026-10-10 随第 7 条优化推进为 1.1.2）`。依据：v1.1.0 之后的各轮改动（重新检测忙态与面板反馈、启动沿用/定向重检/单份备份三项优化、三条上游失败文案改写、备份收敛）都属**同一未发布版本内的补丁与打磨**，此前被逐轮推进成 `1.2.0` 与 `1.3.0~1.3.4`，属**错误推进，已作废**。五处落点（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`AGENTS.md` 两行）实测 `npm run version:check` 一致为 **1.1.2**；各轮条目内的 `1.2.x` / `1.3.x` 标注已同步改为「v1.1.2 轮内追加（原记 X 已作废）」。`1.2.x~1.3.x` **从未构建、从未打标签，不得再占用**。
- 承上条：`v1.1.2` 构建与打标签之前的追加修复**默认不递增版本号**（沿用「同一未发布版本内的追加修复不推进」纪律），用户另有指令时从其指令。

## 变更与交付约定（面向 Agent）

1. **改动前**：`read` 读取目标文件原文；涉及 `orchestration.rs` 编排、`server.rs` 路由、`protocol.rs` 校验、`runtime.rs` 下载的逻辑，必须同时阅读相关测试（含 `tests/fixtures/*.json` 的快照断言）确认既有行为边界。
2. **改动中**：一次只改一个独立区块/函数，避免大范围替换；不修改与任务无关的文件。
3. **改动后**：
   - 必须运行 `src-tauri/core/` 的 `cargo test` 与 `cargo clippy --all-targets`，并在回复中给出**真实的**通过/失败与 warning 数量；
   - 改动 `src/**`（面板）或 `scripts/*.mjs` 后必须运行 `npm run test:prefs`、`npm run test:ops`、`npm run test:manifest` 与 `npm run test:updater-key`（四组合计 41 用例）、`npx eslint .` 与 `npm run vite:build`，同样**如实报数**；改壳（`src-tauri/src/`）后补 `cargo test --lib`（`src-tauri/`）；改 `release.yml` 后必须 `python3 -c "import yaml;yaml.safe_load(open('.github/workflows/release.yml'))"` 并复述各作业步骤名；改 `scripts/make-dmg.sh` 后必须 `bash -n` 它；
   - 必须用 `grep` / `read` 验证改动已落盘；
   - **版本步骤（2026-10-09 起强制；2026-10-10 起落点 7 处）**：改动落盘并验证后，按「版本号规则」执行 `npm run version:set -- <x.y.z>` 递增并 `npm run version:check` 确认 7 处一致，在回复中报出新版本号；仅返工豁免（修复上一轮理解错误）与纯文档类改动不递增；
   - 不得自动 `git commit` / `git push`；
   - 若改动涉及对外契约（路由、错误码、`status.json` 字段、`models.json` 写入格式、IPC 命令/事件），必须在回复中显式列出并提示用户影响面；
   - 若改动触及红线，必须同步更新 `tests/red_lines.rs` 的对应断言。
4. **遇到不确定**：宁可向用户询问，也不要凭推测修改安全、供应链、隔离相关代码。
