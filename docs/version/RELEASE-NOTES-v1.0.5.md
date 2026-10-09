# WB Bridge v1.0.5 — TLS 拦截修复 + 探测回归修复 + 单模型重新检测

> **状态**: 📝 待发布（尚未构建、未打标签）
> **日期**: 2026-10-09
> **上一版本**: v1.0.4

## 问题背景

v1.0.4 升级后，全部模型探测变为「不可用」。经全量代码对比与运行时日志排查，确认根因有两层：

1. **TLS 拦截（主因）**：OpenCode (Bun 运行时) 向上游模型 API 及 `https://models.opencode.ai/api.json` 发 HTTPS 请求时，若用户网络存在 TLS 拦截（公司代理 / VPN / ZScaler 等），Bun 不读取 macOS 系统钥匙串，即使拦截证书已安装到系统钥匙串也不信任它，报 `"Error: self signed certificate"`。所有模型的探测在 `ai-sdk` 运行时向上游发请求这一步即失败（`nativeAttempts: 0`）。
2. **转写闸门误关**：v1.0.3 全量代码复审时，将 `chat_only_attempt` 的 meta 从 `json!({})` 改为 `probe_meta()`（`{ probe: true }`），关闭了探测降级路径的辅助模型转写。升级前靠转写兜底通过的模型，升级后也变为不可用。

## 本版修复

### 1. 修复 TLS 拦截导致的 "self signed certificate"（根因修复）

`runtime.rs` — `isolated_environment` 中新增 `NODE_TLS_REJECT_UNAUTHORIZED=0`，让 Bun 运行时跳过 TLS 证书验证。Bun 不读取 macOS 系统钥匙串，在网络存在 TLS 拦截时无法信任拦截证书；wbBridge 自身的 HTTP API 在 `127.0.0.1` 上不受影响。

### 2. 回退 `chat_only_attempt` 转写闸门

`orchestration.rs` — `chat_only_attempt` 的 meta 从 `probe_meta()` 回退为 `json!({})`，恢复 `backend.rs` 两处转写闸门开放。主探测路径 `probe_single_model` 仍带 `probe: true` 不变。

### 3. 修复 `start_probes_admin` 二次解包 bug

`orchestration.rs` — `start_probes_admin` 原先对服务端已提取的字符串值再调 `value.get("model")`，返回 `None`，导致单模型探测请求退化为全量探测。现直接透传 `start_probes(model, false, false)`。

### 4. 面板单模型重新检测按钮

- `ModelRow.vue`：外层 `<button>` 改为 `<div role="option" tabindex="0">`，不可用模型行内新增「重新检测」按钮
- `ModelList.vue`：透传 `probe-running` prop 和 `@reprobe` 事件
- `App.vue`：新增 `reprobeModel(modelId)` 调用 `POST /admin/probe { model: id }`

## 验证

- ✅ `vite:build` 编译通过
- ✅ `cargo build`（壳）编译通过
- 🔴 单模型重新检测的端到端 GUI 效果、TLS 修复后真实模型探测通过率须实机点一遍

## macOS 首次打开（ad-hoc 签名放行）

WB Bridge 为 ad-hoc 签名，**未做 Apple 公证**。首次打开（含更新后重新被拦截）提示「已损坏」时，确认来源可信后执行：

```bash
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```
