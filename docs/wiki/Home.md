# WB Bridge 文档

WB Bridge 是一个**跨平台托盘应用**（Tauri 2 桌面壳 + 同进程内嵌 Rust 核心），通过**隔离托管**的 OpenCode 运行时，为 [WorkBuddy](https://www.workbuddy.cn) 提供免费模型服务。

它做三件事：

1. **托管一个隔离的 OpenCode 运行时**——优先复用本机已有的 `opencode`，否则从 npm 官方源（失败回退国内镜像）下载官方 tarball，校验 `sha512-` 完整性后解包，用隔离环境变量 + 独立随机端口以 `serve --pure` 启动，不污染用户自己的 OpenCode 配置、数据、缓存与登录态。
2. **对外暴露 OpenAI 兼容 API**——在本地 `127.0.0.1:<port>` 提供 `/v1/models` 与 `/v1/chat/completions`（含 SSE 流式），强制 `Authorization: Bearer <api-key>` 鉴权，拒绝一切浏览器 Origin，最多 4 个并发请求。
3. **把可用模型同步给 WorkBuddy**——自动发现免费模型 → 逐模型真实探测 → 只把探测通过的模型发布给 WorkBuddy（原子写 + 增量合并 `models.json`，只增删自己名下的条目）。

> 功能与界面参考上游实现（[louchi1984-coder/ow-bridge](https://github.com/louchi1984-coder/ow-bridge)，Electron 版）并优化，核心代理逻辑与之保持行为一致。

---

## 目录导航

| 页面 | 内容 |
|---|---|
| [快速开始](快速开始) | 环境要求、安装与放行、首次运行、模型读取与检测、导入 WorkBuddy |
| [使用指南](使用指南) | 界面分区与交互、模型状态与徽章含义、系统代理、托盘常驻 |
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
| 核心测试（`cargo test`，202 通过 / 0 失败） | 已执行并通过 |
| 核心独立进程冒烟（真实下载运行时 → 隔离启动 → 发现 8 个免费模型 → 探测 → 干净关停） | 已在一次性数据目录实测通过 |
| `cargo clippy`（核心 `--all-targets` / 壳 `--no-deps`） | 0 warning |
| 产品版本 | **1.0.0**（唯一来源：根 `package.json`） |
| 实际启动 GUI 并操作托盘与面板 | **未实测**（仅编译、独立核心冒烟、面板在浏览器引擎内经 CDP 实测） |
| 迁移后产出安装包 | **未产出** |
| `.github/workflows/release.yml` 在 CI 跑通 | **未跑通**（仅做过本地 YAML 结构校验） |
| 代码签名 / 公证 | 未做（macOS 为 ad-hoc 签名） |

完整清单见[已知限制与未验证项](已知限制与未验证项)。

---

## 工程结构（速览）

```
src-tauri/core/  Rust 核心（crate wbbridge-core，16 个 lib 模块 + 独立入口 src/main.rs）
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
