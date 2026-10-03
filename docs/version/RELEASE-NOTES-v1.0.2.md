# WB Bridge v1.0.2

> **GitHub Release 正文**（推送 `v1.0.2` 标签或手动运行 `release.yml` 时，可直接复制本文件内容作为 Release body）。
> 发布日期：待定 ｜ 上一版本：v1.0.1 ｜ 内部 crate `wbbridge-core` 版本仍为 `0.1.0`（与产品版本解耦，不随本次递增）

**本版主题：应用内自动更新上线 + 发布链路自证 + 面板偏好持久化。**

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

- ❌ **桌面 GUI 从未实机启动**：更新区、重启链路、偏好持久化都只有编译 / 单测 / 构建证据。
- ✅ **v1.0.2 的安装包已由 CI 真实产出并发布**（2026-10-03 末轮：tag `v1.0.2`、head `0e4a535`、run `37092915120`、作业 8/8 全绿；Release `v1.0.2` 已 Publish，**23 个资产** = 六平台安装包 + 各自 `.sig` + `latest.json`，另有 `.msi` / `.deb` 的 `.sig`）。本机上一次打包仍发生在 `1.0.1` 源码版本，macOS 之外的平台**没有本机产物**，且**六平台都没有实机安装过**。
- ⚠ **`.github/workflows/release.yml` 首次实跑发生在 2026-10-03，止步于 updater 签名步骤**：报 `failed to decode secret key: incorrect updater private key password: Missing comment in secret key`。本机 14 组形态复现证明**这句＝私钥变量取到空值**（当时私钥只配在 GitHub Secrets，而 build 作业没声明 `environment:`）。已改用仓库级 **Variables**；同日再把 `scripts/with-updater-key.mjs` **从本地自检工具重写为 build 链路上的签名注入包装器**（`npm run tauri:build` = `node scripts/with-updater-key.mjs tauri build`，已接入 `build` 与 CI；`--check-only`、试签、形态校验与配对阻断全部撤除，公私钥不符只告警），并撤除 CI 的私钥前置步骤。**同日末轮完整一轮（六平台产物 + `latest.json`）已跑通**（同上 run `37092915120`）；但那一份 `latest.json` 的六条 `url` 全部 404，成因见下方「url 里的空格必须写成 `.`」一条。
- ❌ **一次真实的「检查 → 下载 → 安装 → 重启 → 首启」从未走过**。Windows / Linux 产物名与签名已于同日 CI 首跑实测到（见「发布产物与更新清单」），但**线上 v1.0.2 那份 `latest.json` 仍是带 URL 缺陷的那份**（重传资产属共享状态，由维护者操作或等下一次发布覆盖写），所以客户端的消费路径至今仍未走通；同日新增的那道 CI 资产对账步骤**本身也还没在 CI 上实跑过**。
- ⚠️ **推标签之前必须先配好 GitHub 仓库级 Variables**：`TAURI_SIGNING_PRIVATE_KEY` ＝ **私钥全文**（CI runner 上没有本机的 `~/.tauri`，路径无效）+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` ＝ 当前这把钥的**非空口令**。缺任一项会在一整套 Rust 编译跑完后的打包那一步才失败（空值报 `Missing comment in secret key`、口令错报 `Wrong password`），白烧十几分钟——注入包装器只注入不前置校验，所以推标签前务必本地跑一次 `npm run tauri:build` 确认它打印出「已注入内联签名私钥 → 公钥配对 OK（`2B11F78BEA8A43F`）」。⚠ Variables 是**明文值**、日志不像 Secrets 那样打码——这是 2026-10-03 按维护者决定对齐参考项目所接受的暴露面，想换回只需把 `release.yml` 里的 `vars.` 改回 `secrets.`。
- ✅ 已实测（本机、源码版本 `1.0.1` 时）：签名构建产出 `bundle/macos/WB Bridge.app.tar.gz`（3,595,514 B）+ 配对 `.sig`，签名 key ID 与 `tauri.conf.json` 内嵌公钥一致（`126D4E208E0F17BA`）；`bundle/dmg/WB Bridge_1.0.1_aarch64.dmg`（3,720,766 B）只读挂载核对 + `codesign --verify --deep --strict` 通过（adhoc + hardened runtime，**未公证**）。
- ✅ 已实测（本轮）：核心 `cargo test` **207 通过 / 0 失败**、核心与壳 `cargo clippy` **0 warning**、壳 `cargo test --lib` **9 通过**、`npm run test:prefs` **8 通过**、`npm run test:manifest` **9 通过**、`npm run test:updater-key` **13 通过**（签名注入包装器逻辑的单测套件，2026-10-03 同日由 9 重写为 13）、`npx eslint .` **0 problem**、`npm run vite:build` 成功、`npm run version:check` **5 处一致（1.0.2）**。
- ✅ 已实测（2026-10-03 签名接线轮，**本轮的 `--check-only` 前置自检随后同日已移除**，保留作历史）：`node scripts/with-updater-key.mjs --check-only` 用**一次性新钥**跑出「形态 OK → 配对跳过 + 告警 → **试签 OK** → 退出 0」，同钥改路径形式时因 key ID 不在配置白名单判为「不匹配 → 退出 1」（失败关闭）；对**当前采用的 9-30 钥**跑出「配对 OK → 试签 OK → 退出 0」。⚠ 该轮记的「`2B11F78BEA8A43F` 本就在配置 pubkey 白名单内」**是误判**——当时配置里其实只有 `126D4E208E0F17BA`（`verify_signature` 只认第一条），故那次「配对 OK」并不成立、真实构建会告警 does-not-match；已按下方「签名注入链路重写」节把 `pubkey` 更正为仅 `2B11F78BEA8A43F`。这只证明**钥 + 口令可用**，不证明产物签名已被客户端验证。

---

## ✨ 自动更新（本版最大变化）

1. **壳侧接线**：新增官方 `tauri-plugin-updater` + `tauri-plugin-process`；`tauri.conf.json` 打开 `bundle.createUpdaterArtifacts`，`plugins.updater` 内嵌验签公钥并把端点指向 `https://github.com/trexwb/wbBridge/releases/latest/download/latest.json`。
2. **权限最小化**：capabilities 只加 `updater:default` 与 **`process:allow-restart`**，**没有**用 `process:default`——后者含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链（停核心 → 清理 WorkBuddy 配置）直接杀进程。
3. **面板更新区**（「关于与更新」）：冷启动 5 秒后**静默**检查一次，只有真发现新版本才出现更新条；静默失败完全不提示。下载进度百分比只在上游给出 `contentLength` 时才显示，拿不到就只显示已下载字节数——**不猜进度**。装完停在「待重启」，由用户点「重启应用」。
4. **重启不冻窗口**：`RunEvent::ExitRequested` 从「事件循环线程上无界停止」改为**有界停止**（`stop_core_bounded`，预算 8s）。`relaunch()` 也走这条分支，此前最坏十几秒的停止会让「重启」看起来像卡死。⚠️ 该项无单测覆盖，效果必须实机确认。
5. **联网发生在 Rust 侧**：更新器是插件命令，不经 WebView `fetch`，因此面板 CSP（`default-src 'self'`）无需放宽；自有 IPC 命令与事件**一个都没新增或改名**。

## 📦 发布产物与更新清单

- **`latest.json` 只有一个写者**：CI 末尾的 `update-manifest` 作业跑 `scripts/gen-latest-json.mjs`，平台键取自产物**目录名**里的 target triple，默认要求六平台齐全、缺一即退出 1。（2026-10-03 起 `tauri-apps/tauri-action` 已**整体撤除**——build 作业改跑 `npm run tauri:build`，Release 由 `softprops/action-gh-release` 创建、`tag_name` 现读 `tauri.conf.json`；三个 build 作业谁都不写清单，其自带的清单生成当初以 `includeUpdaterJson: false` 关闭，因为 6 个并发作业各写一次会互相覆盖、静默漏平台。）
- **macOS 更新包补架构后缀**：tauri 产出的就是 `WB Bridge.app.tar.gz`（不带版本、不带架构），两个 mac runner 会往同一 Release 传同名资产、后者静默覆盖前者；CI 因此把它改名为 `WB Bridge_<arch>.app.tar.gz`（`.sig` 同步改名——minisign 签的是内容不是文件名）。✅ 该步骤已在同日 CI 首跑实测生效：Release 上 `WB Bridge_aarch64.app.tar.gz(.sig)` 与 `WB Bridge_x86_64.app.tar.gz(.sig)` 各自独立、无同名覆盖。
- 🔴 **url 里的空格必须写成 `.`**：GitHub 上传时会把 **Release 资产名里的空格规范化成 `.`**（磁盘上 `WB Bridge_…` → API 里 `WB.Bridge_…`），而 `/releases/download/<tag>/<带空格的名字>`（含 `%20` 编码）**一律 404**。v1.0.2 那一轮六平台产物齐全、六条 `.sig` 的签名者 key ID 逐字节解出全为 `2B11F78BEA8A43F`、清单也生成成功、CI 全绿，`latest.json` 的六条 `url` 却全部不可下载——只有客户端走到下载那一步才暴露。修复：`scripts/gen-latest-json.mjs` 拼 url 前把空格换成点，`npm run test:manifest` 的成功路径断言改成点形式，`release.yml` 的 `update-manifest` 作业新增「校验清单 url 指向的资产在 Release 上真的存在」一步（用 API 逐条对账，不符即 `exit 1`）。
- **六平台目标**（`release.yml` 矩阵）：macOS `aarch64` / `x86_64`，Windows `x64` / `arm64`，Linux `x64` / `arm64`。✅ 同日 CI 首跑的资产清单已确认实际产物名：macOS `WB Bridge_<arch>.app.tar.gz(.sig)` + `WB Bridge_1.0.2_<arch>.dmg`、Windows `WB Bridge_1.0.2_{x64,arm64}-setup.exe(.sig)`（另有 `…_en-US.msi(.sig)`，不参与更新）、Linux `WB Bridge_1.0.2_{amd64,arm64}.AppImage(.sig)`（**更新包就是裸 `.AppImage`**，bundler 没有额外产出 `.AppImage.tar.gz`；另有 `.deb(.sig)`，不参与更新）。这些名字在 GitHub 页面上显示为点形式（见上一条）。

## 🔒 发布链路自证（防 v1.0.1 那次标签指错）

- **标签 ↔ 版本闸门**：CI 的 `test` 作业要求 `GITHUB_REF_NAME` 去掉前导 `v` 后**等于** `src-tauri/tauri.conf.json` 的 `version`，不一致直接 exit 1（`check-version.mjs` 只校验同一提交内部一致，抓不到「标签指向全体旧版本的提交」）。`workflow_dispatch` 触发时该步骤跳过。
- **`latest.json` 生成器有已入库的行为基线**：`scripts/gen-latest-json.test.mjs`（9 用例）用临时产物目录跑真脚本，钉住「平台键只由 artifact 目录名的 target triple 决定」「缺平台 / 缺 `.sig` / mac 资产名漏架构后缀 / 同平台两目录 / 同目录互不相干的两个已签名包 → 一律退出 1」「Linux 双层打包 → 取 `.tar.gz` 并告警」（实测：bundler 并不双层打包，取的是裸 `.AppImage`）。这类错误原本的暴露方式是「用户装上才发现平台缺席」，现在在 CI 里先红。
- **产物名留痕**：三个 build 作业上传前打印源码版本、触发引用与全部 bundle 文件名；`update-manifest` 汇总实际收到的安装包，按「文件名含本次版本 / 按设计不含版本（macOS 更新包）/ 待人工核对」三分类把结论写进 run summary。**待核对项不为 0 时不要点 Publish**（该产物名自检只警告不硬失败——非 macOS 的产物名已于 2026-10-03 首跑实测到，见上节的三分类因此已能对得上）。同日在其后**新增一步「校验清单 url 指向的资产在 Release 上真的存在」**：带 `GITHUB_TOKEN` 列出 Release 资产，把六条 `url` 末段与实际资产名逐条对账，任一不符 `exit 1`。⚠ 该步骤本身**尚未在 CI 上实跑过**（本机拿线上那份清单与修正后的清单各跑一遍同一段逻辑：线上 6/6 报错退出 1、修正后 6/6 OK 退出 0）。
- **顺序铁律**：先 `npm run version:set -- <x.y.z>` 改写 5 处落点并**提交**，再**在那一个提交上**打 `v<x.y.z>` 标签。
- **签名注入包装器 `scripts/with-updater-key.mjs`（`npm run tauri:build` 的入口，配套单测 13 用例）**：`npm run tauri:build` = `node scripts/with-updater-key.mjs tauri build`。它**只做签名注入、不做前置校验**——按「进程环境 > 仓库 `.env.local` > `.env` > `~/.tauri/wbBridge.env` > `~/.tauri/wbBridge-updater.env` > 兜底 `~/.tauri/wbBridge-updater.key`」汇齐私钥与口令（逐项打印来源文件名、`~` 由脚本展开、显式空口令压过文件），把 `.key` 路径读成内联全文并 trim 后注入 `tauri build` 只认的 `TAURI_SIGNING_PRIVATE_KEY`（删掉互斥的 `_PATH`），明文钥显式置空口令，然后 exec 目标命令。它**不判形态、不试签、公私钥配对不符也只告警不阻断**（此前的 `--check-only` 前置自检与试签已于 2026-10-03 同日撤除），全程只输出来源与公开的 key ID、绝不回显密钥与口令。它要缓解的暴露方式是「签名失败发生在构建末尾、报错只有一句 `Missing comment in secret key`，而真实成因是变量取到空值」——注入包装器把这一步前移到构建最开始，但私钥与口令是否在位仍由维护者自行确认（CI 亦不设私钥前置步骤）。

## 🧩 面板偏好持久化

- 新增 `src/core/prefs.js` 作为面板读写偏好的**唯一边界**：键前缀 `wb.`，**写入前做键白名单校验**，值再做字段投影，序列化后超过 2KB 一律拒写；`localStorage` 不可用、抛错或被禁用时静默回落默认值，不报错也不阻断启动。
- 持久化两项：**当前视图**（重启后回到上次视图）与**「启动后自动检查更新」开关**（在「关于与更新」里，关掉后冷启动不再自动打端点，手动「检查更新」仍可用）；自动检查按 **12 小时**节流，时间戳只在**成功**查到清单/确认已是最新后写入。
- **凭据零落盘**：`api-key`、`OPENCODE_SERVER_PASSWORD`、WorkBuddy 配置文件路径等在设计上无法进入 `localStorage`（白名单不含它们，且值投影会丢弃未知字段），由 8 项 `node --test` 用例钉住 → `npm run test:prefs`。

## ⚡ 渲染与轮询降耗（2026-10-03 追加）

探测 / 高频状态推送期间，面板与壳各有一段「稳态下白干活」的开销。本项只优化既有路径，**不新增任何功能、不改对外契约与数据口径**：

1. **面板：帧级合并发布**（`src/core/bridge.js`）。壳对同一帧变化会连发 `core-status` + `core-activity` 两个事件，此前各自立即 `publish`，根状态被赋值两次、整树渲染两趟（探测期间成倍）。改为把事件按到达顺序折算成状态操作入队（`status` = 替换、`activity` = 合并、`failed` = 替换），用 `requestAnimationFrame` **每帧最多 flush 一次**，渲染次数由「每事件一次」降为「每帧一次」（非浏览器环境回落 `setTimeout(…, 16)`）。**对订阅者而言合并语义与逐个执行等价**（FIFO 保序、后者覆盖前者），但 `lastState` 要到帧末才更新，事件到达与 flush 之间的同步读取点会读到旧一帧（目前是「导入 WorkBuddy」读的 `lastState.modelsFile` 与 `onState` 的首次回放，下一帧自愈）。替换型事件入队前先清空队列——它的 payload 整体替换状态，前面排队的 op 必然被丢弃，清空与逐条折叠等价，同时让队列长度封顶（否则窗口不可见、rAF 停摆期间会持续堆积持有整份 payload 的闭包）。
2. **面板：行级 / 详情 props 引用稳定**（`src/views/ModelList.vue`、`src/App.vue`）。无结果的行与详情共用同一个空对象（`EMPTY_RESULT`），不随父级重渲染换引用，`ModelRow` / `ModelDetails` 只在自身数据真变化时更新，不再被 `usage` 等无关字段的状态推送波及；「请求中」判断由逐行 `some()`（行数 × 活动数、每趟重渲染都重做）改为一次生成 `Set` 后 `Set.has()`。
3. **壳：`status.json` 轮询降耗**（`src-tauri/src/lib.rs`）。`watch_status` 每 500ms 原本都会 `read_to_string` 整份 `status.json`（实测 8.6 KB）再逐字节比对。改为先用 `(mtime, 长度)` 做便宜的前置过滤，两者都没变就直接跳过，只有变了才真正读内容；`stamp` 只在**成功读到内容之后**记录，避免「读失败的那一轮」被当成稳态跳过。快路径**只在拿得到 mtime 时启用**——文件系统不暴露 mtime 时一律回落读内容，否则长度不变的改写会被永久跳过；「核心恢复运行」分支同时作废 `last` 与 `stamp`，保住「重启后内容即使逐字节相同也要重推」这道既有保障。该判定有单测守卫（`status_read_needed_only_skips_when_mtime_is_known`）。
4. **核心：去掉未使用的 brotli 解压**（`src-tauri/core/Cargo.toml` + 两份 `Cargo.lock`）。`reqwest` 的 `brotli` feature 被拆除。2026-10-03 用 `curl -H 'Accept-Encoding: gzip, br'` 实读两个白名单源：registry **元数据**都回 `content-encoding: gzip`，而 **tarball**（npmjs 直连与 npmmirror 跳转后的 CDN 目标）是 `application/octet-stream` 且不带 `content-encoding`（`.tgz` 原样传输，gzip 由 `flate2` 解）；本地回环的 OpenCode 响应同样不压缩——即 brotli 在真实链路上从不被用到，去掉它也不改变 sha512 完整性校验的语义。核心依赖树随之移除 `brotli` + `brotli-decompressor` + `alloc-no-stdlib` / `alloc-stdlib`。⚠ 口径边界：这是**核心 crate 自身**依赖树的收益，壳的产物里 brotli 仍会经 `tauri-codegen`（前端资产压缩）引入，安装包总体积不会因此等量减少。同处留注说明 `base64 = "0.22"` 为何不动：**核心 workspace** 的依赖树里两份 base64 分别来自 `reqwest`(0.22) 与 `hyper-util`(0.23)，由上游固定，改本 crate 版本消不掉重复（壳 workspace 是三份，多出的一份来自 tauri 侧 `swift-rs`，与本行无关）。

**版本号未推进（仍为 1.0.2）**：本轮属同一未发布版本内的渲染 / 轮询降耗与同日复审修正，未引入新功能分支、也无新的根因修复，按仓库版本纪律（同日同模块追加、无新逻辑分支的打磨）不满足末位 +1 的前提。⚠️ 降耗的**实际收益**仍需在 GUI 实机（探测 + 高频推送）确认：面板帧合并没有 JS 单测，`watch_status` 整条循环也仍需真实 `AppHandle` 才能覆盖（新单测只守卫快路径的判定）；rAF 在 WKWebView 中最小化 / 隐藏时的真实停摆行为同样未实测。

## 🔐 签名注入链路重写 + 单条公钥 + hdiutil dmg（2026-10-03 追加，不推进版本号）

同日在「渲染与轮询降耗」之后，又对**签名/发布链路**做了一轮重写，并**更正**本版先前记录的两处事实：

- **`scripts/with-updater-key.mjs` 由「本地手跑的签名自检工具」重写为 `npm run tauri:build` 的签名注入包装器**：`npm run tauri:build` = `node scripts/with-updater-key.mjs tauri build`，`npm run build` = `vite:build && tauri:build && make:dmg`。它**只做注入**——按「进程环境 > 仓库 `.env.local` > `.env` > `~/.tauri/wbBridge.env` > `~/.tauri/wbBridge-updater.env` > 兜底 `~/.tauri/wbBridge-updater.key`」汇齐私钥与口令（逐项打印来源文件名、`~` 由脚本展开、显式空口令压过文件），把 `.key` 路径读成内联全文并 trim 后注入 `tauri build` 只认的 `TAURI_SIGNING_PRIVATE_KEY`（删掉互斥的 `TAURI_SIGNING_PRIVATE_KEY_PATH`；明文钥显式置空口令，否则无 TTY 下报 `Device not configured (os error 6)`），再 exec 目标命令。**不再判形态、不再试签、公私钥配对不符也只告警不阻断**；`--check-only` 及其前置校验已移除。密钥与口令全程不打印，只输出来源与公开 key ID。早前的 `resolveKey`/`checkKeyShape`/`configuredKeyIds`/`readConfiguredKeyIds`/`checkPairing`/`classifySignerError`/`dryRunSign` 等导出一并删除。
- 🔴 **一处被源码推翻的关键更正**：先前多处文档写「`plugins.updater.pubkey` 内嵌两条公钥（`126D4E208E0F17BA` / `2B11F78BEA8A43F`）＝minisign 轮换白名单，换钥无需改配置」——**错**。实读 `tauri-plugin-updater` 2.13 `verify_signature()` → `minisign-verify` 0.2.5 `PublicKey::decode()`：只解码并使用**第一条**公钥，后面的公钥框被静默丢弃；`tauri build` 的 does-not-match 告警比对的也是同一第一条。当时提交在配置里的其实只有 `126D4E208E0F17BA`（属已消失的 20261001 钥），而签名用的是当前这把 9-30 钥（`2B11F78BEA8A43F`），于是既告警、签出的 `.sig` 又会被客户端判无效。**修复＝`plugins.updater.pubkey` 现只嵌 `2B11F78BEA8A43F`**。规则：**换钥必须同一次发布同步换 pubkey**，老客户端只认它自己内嵌的那条。**本次替换不锁死任何已发布版本**——两个已打标签（v1.0.0 / v1.0.1）都早于 updater 接线，`126D4E208E0F17BA` 从未进入交付的二进制；v1.0.2 是第一个带自动更新通道的产物。
- **macOS 的 `.dmg` 改由 hdiutil 生成**：`bundle.targets` 从 `"all"` 改为显式 `["app","nsis","msi","appimage","deb"]`，`.dmg` 由 `npm run make:dmg`（`scripts/make-dmg.sh`：`hdiutil create -format UDZO`，产物 `bundle/dmg/<productName>_<version>_<arch>.dmg`，架构取 `TARGET_TRIPLE` 前缀、非 macOS 跳过）生成——Tauri 自带的 create-dmg 末尾要用 AppleScript，无 GUI 的 CI runner 上会失败。移植自参考项目 lockPass / fastenerTradeWorkbench。
- **CI**：撤除 `tauri-apps/tauri-action`；build 作业跑 `npm run tauri:build -- --target <triple>`（macOS 另加 `npm run make:dmg` 与补架构后缀改名）；Release 由 `softprops/action-gh-release` 建，`tag_name` 现读 `tauri.conf.json`；签名变量取自仓库级 **Variables**（**明文值**，工作流绝不 echo；换回 Secrets 只需 `vars.`→`secrets.`），**不设私钥前置步骤**（同前决定）。
- **告警甄别（均预期、无需修）**：`Warn skipping app notarization, no APPLE_ID & …`＝ad-hoc 签名、无公证凭据（参考项目同样带着出货，`spctl` 据此 rejected）；pubkey does-not-match 告警**已消失**（本机 2026-10-03 构建不再打印）；macOS 27 的 `hdiutil … deprecated, please use diskutil image` 只是警告，产物正常，刻意保留 hdiutil。
- **测试基线变化**：`npm run test:updater-key` 由 9 → **13** 用例（含注入语义与接线断言：`tauri:build` 经本包装器、`build` 链 `make:dmg`、`release.yml` 读 `vars.` 且调 `npm run tauri:build`（无 tauri-action、无前置步骤）、`pubkey` 恰好一条、`bundle.targets` 不含 dmg）；三组 JS 套件合计 **30**（prefs 8 + manifest 9 + updater-key 13），Rust 基线不变（核心 207、壳 9）；CI test 步骤改名「运行面板偏好、更新清单与签名注入单测」。
- **本机真实验证（2026-10-03）**：零环境变量下 `npm run tauri:build -- --bundles app` **退出 0**（从 `.env.local` 取私钥路径 + 口令，打印「已注入内联签名私钥（来自 `wbBridge-updater.key`）→ 公钥配对 OK（`2B11F78BEA8A43F`）→ 加密态私钥 + 已提供口令」），产出 `bundle/macos/WB Bridge.app.tar.gz`（3,596,811 B）+ `.sig`（428 B），把 `.sig` 逐字节解出的签名者 key ID = `2B11F78BEA8A43F` = 配置里唯一那条；`npm run make:dmg` 产出 `bundle/dmg/WB Bridge_1.0.2_aarch64.dmg`（约 3.9 MB），只读挂载内含 `WB Bridge.app` + `Applications`、`codesign --verify --deep --strict` 通过（adhoc / TeamIdentifier 未设）；`test:updater-key` **13** / `test:prefs` **8** / `test:manifest` **9**、核心 `cargo test` **207**、壳 `cargo test --lib` **9**、`eslint` **0 problem**、`vite:build` ✓ built、`version:check` **5 处一致（1.0.2）**、`release.yml` 解析通过、`bash -n scripts/make-dmg.sh` 干净。
- ❌ **仍未验证**（不得伪装）：一次真实升级闭环（客户端拉到包、装完、`relaunch()` 后带更新核心的重启）、发布后 `latest.json` 的下载/安装（线上那份仍是 URL 缺陷的那份）、六平台的实机安装、GUI 实机启动、新增那道 CI 资产对账步骤本身。~~完整 CI 一轮（六平台 + `latest.json`）、Windows/Linux 产物名与签名~~已于同日末轮实测到（tag `v1.0.2`、run `37092915120`、8/8 全绿、23 个资产、六条 `.sig` 的 key ID 均为 `2B11F78BEA8A43F`），详见「发布产物与更新清单」。版本保持 **1.0.2**，本轮为纯签名/发布链路与文档重写，未推进版本号。

## 🧾 文档

- `.env.example`：整体重写为「无需 `export`」的口径——`scripts/with-updater-key.mjs`（本地手跑的自检工具）自己按 **进程环境 > `.env.local` > `.env`** 的顺序取值，`~` 开头的路径由脚本展开（Node 与 tauri 都不展开 `~`，写 `~` 会被当成相对路径），显式空口令以「进程环境」为准；只含占位符，不含真实凭据。
- `docs/wiki/版本与发布.md`：新增「商业签名与公证（A7 预留位，**尚未接入**）」「失败路径与回滚」「标签纪律」；发布前检查清单改为「仓库级 **Variables** 配私钥全文 + 非空口令 → 推标签前本地手跑 `--check-only` 自检」，并补签名私钥的 **报错 → 成因** 对照表（含 2026-10-03 的 14 组复现结论）与 `with-updater-key.mjs` 自检脚本说明（注明它不在 build/CI 链路上）。〔**同日第二轮更正**：`--check-only` 已移除，`with-updater-key.mjs` 现为 `npm run tauri:build` 的签名注入包装器**并已接入 build/CI**（不再「不在链路上」）；推标签前的本地自检相应改为跑一次 `npm run tauri:build`，wiki 该节需按此同步。〕
- `docs/wiki/常见问题与故障排查.md`：补「更新装完重启后启动失败」「手动回滚」「Gatekeeper 拦截」「自动检查开关」四条，更正「找不到安装包」的真实状态，并新增「CI 构建在签名那步报 `Missing comment in secret key`」（现象 / 成因 / 为什么这么晚才报 / 处理）。〔同日末轮再更正：安装包一行改为六平台已由 CI 产出并发布（产物名为实测值，GitHub 页面上显示为点形式），并新增「清单查得到、下载那步却 404」一条。〕
- `docs/wiki/已知限制与未验证项.md`：未验证项与已知限制两表按本轮实测状态更新（安装包只有 macOS aarch64、偏好持久化与回滚链路 GUI 未实测、CI 首轮实跑止步于签名步骤、9-30 钥本地试签已通过等）。〔同日末轮再更新：CI 完整一轮已跑通、六平台产物名与签名已实测，`latest.json` 的 URL 空格→点缺陷及其修复与残留已如实记录。〕
- `AGENTS.md`：「签名与发布」一节整体更正（自检脚本语义与「不接入 build/CI」的边界、`--check-only`、~~配置公钥白名单**同时**含 `126D4E208E0F17BA` 与 `2B11F78BEA8A43F` 因此换钥无需改配置~~〔**该结论已于 2026-10-03 同日作废并更正**：`verify_signature` 只认 pubkey 的**第一条**，配置现只嵌 `2B11F78BEA8A43F` 一条，换钥**必须**同步改 `plugins.updater.pubkey`〕、14 组报错映射含两类「静默签坏」隐患、Variables 决定与其明文代价）；验证边界表、命令清单、目录树与模块表、测试规范同步补 `scripts/with-updater-key.mjs`（现为签名注入包装器）与其单测。

## 📊 测试与质量基线

| 项 | v1.0.1 | v1.0.2 |
|---|---|---|
| 核心 `cargo test` | 207（lib 187 + js_parity 11 + red_lines 9） | **207（同）** |
| 壳 `cargo test --lib` | 8 | **9**（+1：`status.json` 轮询快路径判定） |
| `cargo clippy --all-targets`（核心 / 壳） | 0 warning | **0 warning** |
| 面板偏好 `npm run test:prefs` | —（本版新增） | **8 通过 / 0 失败** |
| 更新清单生成 `npm run test:manifest` | —（本版新增，此前只用手写一次性夹具跑过、未入库） | **9 通过 / 0 失败** |
| 签名注入 `npm run test:updater-key` | — | **13 通过 / 0 失败**（2026-10-03 追加并按注入器语义重写，`node --test scripts/with-updater-key.test.mjs`；原自检版为 9） |
| `npx eslint .` / `npm run vite:build` | 0 problems / 成功 | **0 problems / ✓ built** |

三组 JS 测试都用 `node --test`（不引测试框架）、不联网（签名注入测试用临时目录与合成的假 base64 串，不调 tauri CLI、不碰 `~/.tauri`、不签真产物），并已加入 CI 的 `test` 作业（「运行面板偏好、更新清单与签名注入单测」一步，prefs 8 + manifest 9 + updater-key 13 ＝ 合计 30 用例）。

核心测试基线本版**未变**（改动集中在壳的插件注册与退出分支、面板与 CI/脚本层，核心行为无回归即可，仍按要求全量跑过并如实报数）；壳侧从 8 增至 **9**，多出的那一项守卫 2026-10-03 复审发现的轮询快路径缺陷。

## 🍎 macOS 首次打开（ad-hoc 签名放行）

应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截：先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

## 🧭 升级与兼容

- **v1.0.1 及更早用户**：那一版没有接线更新器（产物无 `.sig`、面板无更新区），**必须手动下载 v1.0.2 安装包覆盖安装一次**；此后应用内更新才可能自我生效。
- **对外契约未变**：HTTP 路由、错误码、`status.json` 字段、自有 IPC 命令与事件名全部与 v1.0.1 一致；`STATUS_SCHEMA_VERSION` 仍为 `1`。
- **数据目录与写入规则不变**：`api-key` 沿用；WorkBuddy `models.json` 仍只增删 `OWNER = buddy-bridge-v1` 名下条目。
- **macOS 仍需放行**：ad-hoc 签名、未公证；首次打开与更新后如提示「已损坏」，按上文「macOS 首次打开（ad-hoc 签名放行）」小节确认来源可信后执行放行命令。
- ⚠️ **未公证 + 更新器替换镜像**的组合可能让 Gatekeeper 在更新后重新拦截；真出现时按上一条处理或重新下载 `.dmg` 覆盖安装（回滚只需装回上一版安装包，**不需要清理数据目录**）。

## 🙋 反馈

Issues：<https://github.com/trexwb/wbBridge/issues>

---

*本说明由仓库实读结果与当日真实测试输出整理；未验证项已显式标注，不代表功能已在真实桌面环境验证通过。*
