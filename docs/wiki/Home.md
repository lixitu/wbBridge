# WB Bridge 文档

WB Bridge 是一个**跨平台托盘应用**（Tauri 2 桌面壳 + 同进程内嵌 Rust 核心），通过**隔离托管**的 OpenCode 运行时，为 [WorkBuddy](https://www.workbuddy.cn) 提供免费模型服务。

它做三件事：

1. **托管一个隔离的 OpenCode 运行时**——优先复用本机已有的 `opencode`，否则从 npm 官方源（失败回退国内镜像）下载官方 tarball，校验 `sha512-` 完整性后解包，用隔离环境变量 + 独立随机端口以 `serve --pure` 启动，不污染用户自己的 OpenCode 配置、数据、缓存与登录态。
2. **对外暴露 OpenAI 兼容 API**——在本地 `127.0.0.1:<port>` 提供 `/v1/models` 与 `/v1/chat/completions`（含 SSE 流式），强制 `Authorization: Bearer <api-key>` 鉴权，拒绝一切浏览器 Origin，最多 8 个并发请求（2026-10-10 由 4 调到 8，上限值由 `red_lines` 断言钉死）。
3. **把可用模型同步给 WorkBuddy**——自动发现免费模型 → 逐模型真实探测 → 只把探测通过的模型发布给 WorkBuddy（原子写 + 增量合并 `models.json`，只增删自己名下的条目）。

> 功能与界面参考上游实现（[louchi1984-coder/ow-bridge](https://github.com/louchi1984-coder/ow-bridge)，Electron 版）并优化，核心代理逻辑与之保持行为一致。

---

## 目录导航

| 页面 | 内容 |
|---|---|
| [快速开始](快速开始) | 环境要求、安装与放行、首次运行、模型读取与检测、导入 WorkBuddy |
| [使用指南](使用指南) | 界面分区与交互、模型状态与徽章含义、系统代理、关窗即退出与托盘 |
| [核心概念与功能](核心概念与功能) | 运行时隔离托管、免费模型发现与真实探测、OpenAI 兼容接口、模型配置合并写入 |
| [架构设计](架构设计) | 三层结构、壳与核心的边界、IPC 命令与事件契约、数据目录 |
| [开发指南](开发指南) | 环境要求、源码构建与调试、测试与质量基线 |
| [版本与发布](版本与发布) | 版本号落点与校验、发布流程、日志约定 |
| [常见问题与故障排查](常见问题与故障排查) | 启动失败、鉴权 401、配置未写入、探测失败、代理开关等 |
| [已知限制与未验证项](已知限制与未验证项) | 尚未验证的能力边界与已知遗留问题 |

---

## 当前状态一览（如实标注）

| 项 | 状态 |
|---|---|
| 核心测试（`cargo test`，**316 通过 / 0 失败** = lib 294 + `js_parity` 11 + `red_lines` 11） | 已执行并通过（2026-10-10 实测；lib 由 261 增至 288 = WB · auto 的 `auto.rs` 18 项 + `server.rs` router 级 9 项，v1.1.15 再 +6 = `sync.rs` 的插件条目写入） |
| WB · auto 智能路由（合成模型名，见[核心概念与功能](核心概念与功能)与 `docs/contract.md`） | ✅ **核心代码已落地（v1.1.14 路由 + v1.1.15 写入插件配置）、逻辑已测、真实请求已端到端跑通（2026-10-11 实机取证：累计 37 次真实请求、ok 32 / failed 5、跨 12 个模型，两个插件 `models.json` 均含 `WB · auto`）**。316 项单测仍跑在**注入的假后端**上作为回归保障；真实池里的分散度、图片请求路由到支持视觉模型、失败摘除与恢复整轮均在真实客户端 + 真实 OpenCode 下走通（见[已知限制与未验证项](已知限制与未验证项)第 20 条）。✅ **v1.1.15 起会写进 WorkBuddy 与 CodeBuddy 的 `models.json`**（此前 v1.1.14 的「绝不写入」已被用户裁定撤销，并已实机验证插件侧能识别并按它发请求）：只在发布集非空时追加一条、不计入 `sync.count`、不进 `status.json` 的模型列表。名字形态是 `WB · auto`（分隔符 = 空格 + U+00B7 + 空格），`WB.auto` 已全局改名、不再被接受。接入边界（回环、Bearer 鉴权、任何非空 `Origin` → 403、并发 ≤8、体 ≤8MB）一字未放宽 |
| 面板与脚本测试（**四组** `node --test`，合计 **41 通过 / 0 失败** = prefs 8 + ops 11 + manifest 9 + updater-key 13） | 已执行并通过；`npx eslint .` 退出 0、`npm run vite:build` 通过 |
| 核心独立进程冒烟（真实下载运行时 → 隔离启动 → 刷新官方 OpenCode 免费目录（当日 8 个）→ 探测 → 干净关停） | 已在一次性数据目录实测通过。⚠ 该数字是**接入多平台之前**单日实测值；现在的发现口径是「官方 OpenCode 免费目录 + 已配 Key 的平台聚合发现」，条数随上游目录与平台清单变化，**这一形态没有重跑过独立进程冒烟** |
| `cargo clippy`（核心 `--all-targets` / 壳 `--no-deps`） | 0 warning |
| 产品版本 | **1.1.15**（唯一来源：根 `package.json`；`npm run version:check` **7 处落点**一致——`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/core/Cargo.toml`、`AGENTS.md` 三行；2026-10-09 由误推进的 1.3.4 回退而来，1.2.x~1.3.x 从未构建发布）。⚠ **源码版本 = 已发布版本 `v1.1.15`**：`1.1.11` ~ `1.1.14` 未单独构建、未单独打标签（其改动并入 `v1.1.15`），`v1.1.15` 已随 Release 发布并实机安装运行（2026-10-11 取证），见下一行 |
| 发布通道现状 | ✅ **最新已发布版本就是 `v1.1.15`**（2026-10-11 由 `git ls-remote` 确认并实机取证）：标签 `v1.1.15` 在远端、Release 已 Publish、**23 个资产**，`latest.json` 六条 `url` 逐条实测**全部 200**；此前 `v1.1.10`（2026-10-10 由 `api.github.com` 实测：`published_at = 2026-10-10T04:10:03Z`、head `befe96e7`、23 个资产）亦已发布。远端标签共 `v1.0.0 / v1.0.1 / v1.0.2 / v1.0.3 / v1.0.5 / v1.1.0 / v1.1.10 / v1.1.15`（**没有 v1.0.4**，也没有 1.1.1~1.1.9 的标签——那几个版本号只推进过落点、内容并入 v1.1.10；`1.1.11~1.1.14` 同样未单独打标签，内容并入 v1.1.15）；`v1.1.0` / `v1.0.5` / `v1.0.3` / `v1.0.2` 均已 Publish 且各带 23 个资产，`v1.0.0` / `v1.0.1` 各 14 个。🔴 **仍未验证**：Windows / Linux 包仍待在真实机器上安装（macOS app 已随 v1.1.15 实机安装运行，2026-10-11 取证），一次真实的应用内升级闭环（客户端拉包 → 装完 `relaunch()` 带新核心起来）仍待走（计划 R-03） |
| 实际启动 GUI 并操作托盘与面板 | ✅ **已实机运行（2026-10-11 取证）**：`/Applications/WB Bridge.app`（1.1.15）已安装、进程存活并监听 `127.0.0.1:41980`、`status.json` 持续更新；逐视图版式 / 对比度核验仍属计划 R-05 |
| 迁移后产出安装包 | 本机只有 **macOS aarch64** 产出过（且是 1.0.2 那一轮）；其余五平台**由 CI 产出**，并随 Release `v1.0.2`（2026-10-03）、`v1.1.10`（2026-10-10）、`v1.1.15`（2026-10-11）各发布过一次 23 资产。**本机从未构建过 1.1.10 / 1.1.15 的包**（那两轮全部由 CI 产出）。本机上按 `npm run build` 的三环节跑通：updater 包 `WB Bridge.app.tar.gz`（3,596,811 B）+ 配对 `.sig`（428 B，签名者 key ID = 配置 pubkey 那条 `2B11F78BEA8A43F`），`.dmg` 由 `npm run make:dmg`（hdiutil）产出 `WB Bridge_1.0.2_aarch64.dmg`（约 3.9 MB，只读挂载核对 + `codesign --verify --deep --strict` 通过，**未公证**）；**macOS app 已随 v1.1.15 实机安装运行（2026-10-11 取证）**，Windows / Linux 包仍待在真实机器上安装与 GUI 冷启动。六平台产物名与签名形态见[版本与发布](版本与发布) |
| `.github/workflows/release.yml` 在 CI 跑通 | ✅ **已跑通三轮完整 CI**：2026-10-03（tag `v1.0.2`、head `0e4a535`、run `37092915120`、**8/8 作业全绿**、Release 已 Publish、23 个资产）、**2026-10-10（tag `v1.1.10`、head `befe96e7`、run `38023106905`、completed / success、23 个资产）**、**2026-10-11（tag `v1.1.15`、Release 已 Publish、23 个资产、`latest.json` 六条 url 实测全 200）**。更早的一轮曾止步于 updater 签名步骤（私钥变量取到空值），随后签名变量改走仓库级 **Variables**，构建改走 `npm run tauri:build`（签名注入包装器）+ `npm run make:dmg`，**不再用 `tauri-apps/tauri-action`**，Release 由 `softprops/action-gh-release` 以现读配置的 `tag_name: v<版本>` 创建（CI 不设私钥前置校验步骤）。⚠ `v1.0.2` 那轮的 `latest.json` 六条 `url` 因 GitHub 把资产名空格规范化成 `.` 而**全部 404**；脚本与 CI 的对账步骤当场已修，**并由 `v1.1.10` 这轮实证**——那道「校验清单 url 指向的资产在 Release 上真的存在」步骤实跑 success，线上清单六条 url 逐条实测**全部 200**（v1.0.2 那份坏清单已被 `releases/latest` 取代，无需重传）。🔴 **一次真实升级闭环仍未验证** |
| 代码签名 / 公证 | 未做（macOS 为 ad-hoc 签名，构建期那条 `Warn skipping app notarization …` 属**预期**） |

完整清单见[已知限制与未验证项](已知限制与未验证项)。

---

## 工程结构（速览）

```
src-tauri/core/  Rust 核心（crate wbbridge-core，20 个 lib 模块 + 独立入口 src/main.rs）
                 作为 path 依赖静态链接进壳；编排层 orchestration.rs 等价于原 Node 版 main.js
src/             控制面板（Vue 3 + Vite）：App.vue + views/（SideBar / ModelList / ModelDetails /
                 ServiceStatus / MetricsBar / FeedbackBar）+ components/ModelRow.vue
                 src/core/ 是前端内核（与壳的 IPC 唯一边界 bridge.js、activity.js），不是后端
src-tauri/       Tauri 2 壳：窗口 / 托盘 / 核心生命周期 / IPC 命令 / 状态推送（bundle.externalBin 为空）
scripts/         版本号脚本（bump-version.mjs / check-version.mjs）
docs/            接口契约（contract.md）、验证记录（validation.md）、版本日志（version/）、
                 上游调研（research/upstream-architecture.md）、本 wiki（wiki/）
```

---

## 相关仓库文档

本 wiki 是面向使用者的导航与说明层；仓库内更详细的原始文档包括：

- `README.md`——面向使用者的项目说明（安装、构建、已知限制）
- `AGENTS.md`——项目强制规范、命令清单、架构与红线（所有结论的事实来源）
- `docs/contract.md`——壳 ↔ 核心管理接口契约（面板动作 → 核心路由）
- `docs/validation.md`——验证记录（最新在前，含迁移前后两代基线）
- `docs/version/`——版本日志与发布说明
- `docs/research/upstream-architecture.md`——上游参考实现调研
