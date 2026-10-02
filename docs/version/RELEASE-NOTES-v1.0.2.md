# WB Bridge v1.0.2

> **GitHub Release 正文**（推送 `v1.0.2` 标签或手动运行 `release.yml` 时，可直接复制本文件内容作为 Release body）。
> 发布日期：待定 ｜ 上一版本：v1.0.1 ｜ 内部 crate `wbbridge-core` 版本仍为 `0.1.0`（与产品版本解耦，不随本次递增）

**本版主题：应用内自动更新上线 + 发布链路自证 + 面板偏好持久化。**

---

## ⚠ 发布状态（请如实阅读）

- ❌ **桌面 GUI 从未实机启动**：更新区、重启链路、偏好持久化都只有编译 / 单测 / 构建证据。
- ❌ **v1.0.2 的安装包尚未构建**；`.github/workflows/release.yml` **从未在 CI 实际跑通**（仅本地 YAML 结构校验 + 清单脚本的合成产物单测）。
- ❌ **一次真实的「检查 → 下载 → 安装 → 重启 → 首启」从未走过**；`latest.json` 上传到真实 Release 后的消费路径、Windows / Linux 产物名与签名同样未实测。
- ⚠️ **推标签之前必须先配好 GitHub Secrets**（`TAURI_SIGNING_PRIVATE_KEY` + 空的密码），否则三个平台作业都会在生成 updater 产物那一步失败。
- ✅ 已实测（本机、源码版本 `1.0.1` 时）：签名构建产出 `bundle/macos/WB Bridge.app.tar.gz`（3,595,514 B）+ 配对 `.sig`，签名 key ID 与 `tauri.conf.json` 内嵌公钥一致（`126D4E208E0F17BA`）；`bundle/dmg/WB Bridge_1.0.1_aarch64.dmg`（3,720,766 B）只读挂载核对 + `codesign --verify --deep --strict` 通过（adhoc + hardened runtime，**未公证**）。
- ✅ 已实测（本轮）：核心 `cargo test` **207 通过 / 0 失败**、核心与壳 `cargo clippy` **0 warning**、壳 `cargo test --lib` **8 通过**、`npm run test:prefs` **8 通过**、`npm run test:manifest` **9 通过**、`npx eslint .` **0 problem**、`npm run vite:build` 成功、`npm run version:check` **5 处一致（1.0.2）**。

---

## ✨ 自动更新（本版最大变化）

1. **壳侧接线**：新增官方 `tauri-plugin-updater` + `tauri-plugin-process`；`tauri.conf.json` 打开 `bundle.createUpdaterArtifacts`，`plugins.updater` 内嵌验签公钥并把端点指向 `https://github.com/trexwb/wbBridge/releases/latest/download/latest.json`。
2. **权限最小化**：capabilities 只加 `updater:default` 与 **`process:allow-restart`**，**没有**用 `process:default`——后者含 `allow-exit`，会让 WebView 绕过 `quit_app` 的优雅关停链（停核心 → 清理 WorkBuddy 配置）直接杀进程。
3. **面板更新区**（「关于与更新」）：冷启动 5 秒后**静默**检查一次，只有真发现新版本才出现更新条；静默失败完全不提示。下载进度百分比只在上游给出 `contentLength` 时才显示，拿不到就只显示已下载字节数——**不猜进度**。装完停在「待重启」，由用户点「重启应用」。
4. **重启不冻窗口**：`RunEvent::ExitRequested` 从「事件循环线程上无界停止」改为**有界停止**（`stop_core_bounded`，预算 8s）。`relaunch()` 也走这条分支，此前最坏十几秒的停止会让「重启」看起来像卡死。⚠️ 该项无单测覆盖，效果必须实机确认。
5. **联网发生在 Rust 侧**：更新器是插件命令，不经 WebView `fetch`，因此面板 CSP（`default-src 'self'`）无需放宽；自有 IPC 命令与事件**一个都没新增或改名**。

## 📦 发布产物与更新清单

- **`latest.json` 只有一个写者**：CI 末尾的 `update-manifest` 作业跑 `scripts/gen-latest-json.mjs`，平台键取自产物**目录名**里的 target triple，默认要求六平台齐全、缺一即退出 1。三个构建作业的 `tauri-action` 全部关闭自带清单生成（6 个并发作业各写一次会互相覆盖、静默漏平台）。
- **macOS 更新包补架构后缀**：tauri 产出的就是 `WB Bridge.app.tar.gz`（不带版本、不带架构），两个 mac runner 会往同一 Release 传同名资产、后者静默覆盖前者；CI 因此把它改名为 `WB Bridge_<arch>.app.tar.gz`（`.sig` 同步改名——minisign 签的是内容不是文件名）。
- **六平台目标**（`release.yml` 矩阵）：macOS `aarch64` / `x86_64`，Windows `x64` / `arm64`，Linux `x64` / `arm64`。除 macOS aarch64 外的文件名仍是迁移前口径，**待 CI 首跑的资产清单确认**。

## 🔒 发布链路自证（防 v1.0.1 那次标签指错）

- **标签 ↔ 版本闸门**：CI 的 `test` 作业要求 `GITHUB_REF_NAME` 去掉前导 `v` 后**等于** `src-tauri/tauri.conf.json` 的 `version`，不一致直接 exit 1（`check-version.mjs` 只校验同一提交内部一致，抓不到「标签指向全体旧版本的提交」）。`workflow_dispatch` 触发时该步骤跳过。
- **`latest.json` 生成器有已入库的行为基线**：`scripts/gen-latest-json.test.mjs`（9 用例）用临时产物目录跑真脚本，钉住「平台键只由 artifact 目录名的 target triple 决定」「缺平台 / 缺 `.sig` / mac 资产名漏架构后缀 / 同平台两目录 / 同目录互不相干的两个已签名包 → 一律退出 1」「Linux 双层打包 → 取 `.tar.gz` 并告警」。这类错误原本的暴露方式是「用户装上才发现平台缺席」，现在在 CI 里先红。
- **产物名留痕**：三个 build 作业上传前打印源码版本、触发引用与全部 bundle 文件名；`update-manifest` 汇总实际收到的安装包，按「文件名含本次版本 / 按设计不含版本（macOS 更新包）/ 待人工核对」三分类把结论写进 run summary。**待核对项不为 0 时不要点 Publish**（该自检目前只警告不硬失败，因为非 macOS 产物名尚未实测）。
- **顺序铁律**：先 `npm run version:set -- <x.y.z>` 改写 5 处落点并**提交**，再**在那一个提交上**打 `v<x.y.z>` 标签。

## 🧩 面板偏好持久化

- 新增 `src/core/prefs.js` 作为面板读写偏好的**唯一边界**：键前缀 `wb.`，**写入前做键白名单校验**，值再做字段投影，序列化后超过 2KB 一律拒写；`localStorage` 不可用、抛错或被禁用时静默回落默认值，不报错也不阻断启动。
- 持久化两项：**当前视图**（重启后回到上次视图）与**「启动后自动检查更新」开关**（在「关于与更新」里，关掉后冷启动不再自动打端点，手动「检查更新」仍可用）；自动检查按 **12 小时**节流，时间戳只在**成功**查到清单/确认已是最新后写入。
- **凭据零落盘**：`api-key`、`OPENCODE_SERVER_PASSWORD`、WorkBuddy 配置文件路径等在设计上无法进入 `localStorage`（白名单不含它们，且值投影会丢弃未知字段），由 8 项 `node --test` 用例钉住 → `npm run test:prefs`。

## 🧾 文档

- `.env.example`：写清三个签名变量的真实分工（`tauri build|bundle` **只读** `TAURI_SIGNING_PRIVATE_KEY`，值可为私钥全文或绝对路径；`TAURI_SIGNING_PRIVATE_KEY_PATH` 只对 `tauri signer sign` 生效）。
- `docs/wiki/版本与发布.md`：新增「商业签名与公证（A7 预留位，**尚未接入**）」「失败路径与回滚」「标签纪律」，扩充发布前检查清单（含 Secrets 前置、`test:prefs`）。
- `docs/wiki/常见问题与故障排查.md`：补「更新装完重启后启动失败」「手动回滚」「Gatekeeper 拦截」「自动检查开关」四条，并更正「找不到安装包」的真实状态。
- `docs/wiki/已知限制与未验证项.md`：未验证项与已知限制两表按本轮实测状态更新（安装包只有 macOS aarch64、偏好持久化与回滚链路 GUI 未实测等）。

## 📊 测试与质量基线

| 项 | v1.0.1 | v1.0.2 |
|---|---|---|
| 核心 `cargo test` | 207（lib 187 + js_parity 11 + red_lines 9） | **207（同）** |
| 壳 `cargo test --lib` | 8 | **8** |
| `cargo clippy --all-targets`（核心 / 壳） | 0 warning | **0 warning** |
| 面板偏好 `npm run test:prefs` | —（本版新增） | **8 通过 / 0 失败** |
| 更新清单生成 `npm run test:manifest` | —（本版新增，此前只用手写一次性夹具跑过、未入库） | **9 通过 / 0 失败** |
| `npx eslint .` / `npm run vite:build` | 0 problems / 成功 | **0 problems / ✓ built** |

两组 JS 测试都用 `node --test`（不引测试框架）、不联网，并已加入 CI 的 `test` 作业（「运行面板偏好与更新清单单测」一步）。

核心与壳的测试基线本版**未变**：改动集中在壳的插件注册与退出分支、面板与 CI/脚本层，核心行为无回归即可（仍按要求全量跑过并如实报数）。

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
