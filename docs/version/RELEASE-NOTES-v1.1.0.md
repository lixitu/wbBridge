# WB Bridge v1.1.0 — 多平台免费模型接入（自带 Key）

> **状态**: 📝 待发布（尚未构建、未打标签）
> **日期**: 2026-10-09
> **上一版本**: v1.0.5

## 本版主题

WB Bridge 不再只有 OpenCode 自带的免费模型：新增「平台」视图，用户在面板里填入自己持有的平台 Key，即可把 **ModelScope（魔搭）**、**SiliconFlow（硅基流动）**、**腾讯混元 TokenHub**、**智谱** 四家平台的免费模型一起发现、探测并发布给 WorkBuddy / CodeBuddy。每张平台卡片内置三步申请指引与「打开官方申请页」按钮（系统浏览器打开），Key 只保存在本机数据目录（`providers.json`，权限 0600），面板不回显、不做掩码、不进任何日志。

不填任何 Key 时行为与上一版本完全一致（既有 `OC · ` 模型照常发布）。

## 新增功能

### 1. 面板「平台」视图

- 侧栏「模型」组新增入口，四家平台卡片恒常显示（不依赖核心在线），含「已配置 / 未配置」状态徽章；
- Key 输入框为密码型，保存后立即清空——面板不留任何副本；**保存 / 清除后自动热生效**：面板自动触发「读取免费模型」（只重启隔离的模型子进程，不动应用窗口），新 Key 随即注入并重新发现模型，无需重启应用；
- 每张卡片内置「申请 Key（三步）」指引与官方申请页直达按钮（经壳的新 `open_external` 命令用系统浏览器打开；Tauri WebView 里 `target=_blank` 默认静默失败，外链必须经壳转发）；
- 底部「读取免费模型（重新应用）」按钮为兜底手动入口：自动应用失败（如网络抖动）时可重试，语义与「模型与服务」页的「读取免费模型」一致。

### 2. 多平台模型发现与发布（核心）

- **Key 注入**：启动时从 `providers.json` 读出已配置平台，以最小声明段 `{ npm, options: { baseURL, apiKey } }` 注入隔离配置（`OPENCODE_CONFIG_CONTENT` 通道）；Key 绝不进环境变量白名单、不进日志；
- **聚合发现**：在既有 `opencode` 命名空间之外，逐个已配置平台发现免费模型（CostZero 判定复用：输入/输出/缓存全 0 + 支持文本输出 + 未下线）；**单平台失败不丢其他平台**（失败只记日志）；
- **展示前缀**：新模型按平台显示，如 `ModelScope · …`、`智谱 · …`；模型行直接渲染真实 client id（顺带修复了此前模板硬编码 `OC · ` 的已知偏差）；
- **空发布集闸门**：一轮探测全部失败时拒绝清空 WorkBuddy / CodeBuddy 配置里本工具名下的条目并显式报错（关停 / 启动清旧 / 换配置文件三处用户意图清空不受影响）。

### 3. 各平台免费模型（2026-10-09 实测 catalog，随上游变动）

| 平台 | 免费模型数 | 代表模型 |
|---|---|---|
| ModelScope | 7 | GLM-4.6、Qwen3-235B 系列、Qwen3-Coder-30B |
| SiliconFlow | 3 | Qwen3.5-4B、DeepSeek-OCR、PaddleOCR-VL |
| 腾讯 TokenHub | 2 | Hy3、Hy3-preview |
| 智谱 | 3 | glm-4.5-flash、glm-4.7-flash、glm-4.6v-flash |

> 额度与可用性由各平台决定，本应用不缓存额度；探测与对话都会消耗平台侧额度（免费模型为 0 标价，仍受平台限额约束）。上游新增免费模型不会自动出现，随本应用注册表更新。

## 安全与凭据纪律（不变量）

- Key 只经「平台」视图的保存动作进入核心，落盘 `providers.json`（0600，原子写）；除该文件外任何凭据不落盘、不进日志、不进 `status.json`、不进面板偏好（localStorage 白名单不含它）、不进子进程环境变量；
- 「平台」视图只显示「已配置 / 未配置」，明文与掩码都不回显；
- 新增 `open_external` 壳命令仅接受 https + RFC 3986 合法字符的 URL，用系统浏览器打开，不引入任何 WebView 侧网络请求（CSP 不放宽）。

## 验证

- ✅ 核心 `cargo test` **243 通过 / 0 失败**（lib 221 + js_parity 11 + red_lines 11，本版新增 5 项）
- ✅ 核心 `cargo clippy --all-targets` 0 warning；壳 `cargo test --lib` 9 通过；壳 clippy 0 warning
- ✅ JS 三组单测 8 / 9 / 13 全绿；eslint 0 problem；`vite:build` 通过；`version:check` 5 处一致（1.1.0）
- ✅ 对拍夹具零改动（`git diff --stat src-tauri/core/tests/fixtures` 为空）
- ✅ 沙箱实测（2026-10-09，真实 OpenCode 1.18.35）：四家平台 catalog 回显与 CostZero 判定、`/provider` 不回显 apiKey、`OPENCODE_CONFIG_CONTENT` 通道下自定义 agent 可见、`provider-status` / `set-provider-key` 端到端往返（假 Key）
- 🔴 **未验证**：真实 Key 的端到端闭环（录入 → 重启 → 探测通过 → 在 WorkBuddy 里真实对话）需实机确认；GUI 实机操作（平台视图、保存、重启链路）未点过；探测对已配置平台的额度消耗需使用后观察

## macOS 首次打开（ad-hoc 签名放行）

WB Bridge 为 ad-hoc 签名，**未做 Apple 公证**。首次打开（含更新后重新被拦截）提示「已损坏」时，确认来源可信后执行：

```bash
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```
