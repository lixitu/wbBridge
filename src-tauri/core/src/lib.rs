//! wbBridge core 的 Rust 实现 —— 方案B「阶段一：纯逻辑模块移植」。
//!
//! 本 crate 只覆盖 `core/src/*.js` 中**无网络、无并发**的纯逻辑模块，逐函数对齐 JS 原实现：
//!
//! | 本 crate 模块 | 对应 JS | 说明 |
//! |---|---|---|
//! | [`platform`] | `core/src/platform.js` | 数据目录派生、运行时包名（纯字符串/路径） |
//! | [`json`] | `core/src/json.js` | 剥 BOM 后解析 JSON，另提供 JS 语义辅助函数 |
//! | [`atomic`] | `core/src/atomic.js` | rename 遇 EPERM/EACCES/EBUSY 的退避重试（可注入 rename/sleep） |
//! | [`workbuddy_config`] | `core/src/workbuddy-config.js` | `models.json` 路径解析与格式校验 |
//! | [`reasoning`] | `core/src/reasoning.js` | OpenCode variant ↔ reasoning effort 映射 |
//! | [`model_status`] | `core/src/model-status.js` | 失败归类、请求元信息回填、客户端模型 ID |
//! | [`handoff`] | `core/src/handoff.js` | 原生工具调用 → 外部工具 schema 映射与校验 |
//! | [`protocol`] | `core/src/protocol.js` | 请求校验 / 信封解析 / completion / SSE 帧构造 |
//! | [`repair`] | `core/src/repair.js` | 修复提示词、材料打包、平衡括号取 JSON、重发提示 |
//!
//! 阶段二（「sync + server」）新增：
//!
//! | 本 crate 模块 | 对应 JS | 说明 |
//! |---|---|---|
//! | [`sync`] | `core/src/sync.js` | `models.json` 合并、文件锁、原子写、无变化判定 |
//! | [`system_proxy`] | `core/src/system-proxy.js` | 系统手动代理 → 子进程环境变量（macOS `scutil` / Windows 注册表） |
//! | [`server`] | `core/src/server.js` | axum HTTP 层：路由、Bearer 鉴权、Origin 限制、体积/并发上限、SSE |
//!
//! 阶段三~四（`runtime` / `backend` / 编排层）新增：
//!
//! | 本 crate 模块 | 对应 JS | 说明 |
//! |---|---|---|
//! | [`runtime`] | `core/src/runtime.js` | OpenCode 运行时定位/下载/校验/隔离启动 |
//! | [`backend`] | `core/src/backend.js` | OpenCode HTTP 客户端、事件流、原生审批拦截 |
//! | [`probe`] | `core/src/probe.js` | 模型探测协议与判定 |
//! | [`orchestration`] | `core/src/main.js` | 编排层：可独立运行，也可嵌入 Tauri 壳（`run` + `set_exit_hook`） |
//!
//! 迁移之后新增的模块没有 JS 前身：
//!
//! | 本 crate 模块 | 对应 JS | 说明 |
//! |---|---|---|
//! | [`providers`] | — | 多平台接入的注册表（id/标签/npm/baseURL）与 `providers.json` 凭据通道 |
//! | [`auto`] | — | WB · auto 合成模型名的纯函数路由（按请求内容在可用池里选实际模型 id） |
//!
//! 正确性由两层测试保证：
//! 1. 各模块内的 Rust 单测（`#[cfg(test)]`）锁定行为；
//! 2. `tests/js_parity.rs` 用 Node 真实加载 `core/src/*.js`，同输入对拍 JS 与 Rust 的输出。

pub mod atomic;
pub mod auto;
pub mod backend;
pub mod handoff;
pub mod json;
pub mod model_status;
pub mod orchestration;
pub mod platform;
pub mod probe;
pub mod protocol;
pub mod providers;
pub mod reasoning;
pub mod repair;
pub mod runtime;
pub mod server;
pub mod sync;
pub mod system_proxy;
pub mod targets;
pub mod codebuddy_config;
pub mod workbuddy_config;

pub use json::Env;
pub use protocol::BridgeError;

/// 方案B 阶段标识：阶段四已完成——Node 环境移除，核心作为 path 依赖静态编进 Tauri 壳。
pub const STAGE: &str = "stage-4-embedded-no-node";

/// 被移植 JS 源码在迁移前的仓库相对目录。该目录**已不在仓库内**，连同 `tests/js/describe.mjs`
/// 一起归档到仓库外的 `backup/wbBridge-node-20261001/`；对拍现在只比 `tests/fixtures/*.json`
/// 里冻结的 `expected` 快照，不再需要 Node。
pub const JS_SOURCE_DIR: &str = "core/src";

/// JS 版 sidecar 的 npm 包名（迁移前与归档 `core/package.json` 的 `name` 一致）。
pub const PACKAGE_NAME: &str = "wbbridge-core";

/// crate 版本号（`CARGO_PKG_VERSION` 的便捷别名）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
