# WB Bridge

基于 Tauri 2 的跨平台托盘应用，通过隔离的 OpenCode 运行时为 [WorkBuddy](https://www.workbuddy.cn) 提供免费模型。功能与界面参考上游实现（https://github.com/louchi1984-coder/ow-bridge，Electron 版）并优化，核心代理逻辑与之保持行为一致。

## 工作方式

- 启动时按需下载官方 OpenCode 运行时（失败自动尝试 npmmirror 镜像，下载后校验版本），以隔离配置启动，不使用用户已有的 OpenCode 数据。
- 自动发现 OpenCode 免费模型，向每个模型发送简短真实请求检测可用性与工具调用能力（会消耗少量免费额度）。
- 在本地 `127.0.0.1` 提供 OpenAI 兼容接口（Chat Completions + SSE），只发布检测通过的模型。
- 将可用模型写入 WorkBuddy 的 `models.json`（只修改本应用拥有的条目，写入前备份；退出时清理，保留用户手动配置）。
- 关闭窗口后常驻托盘；从托盘退出时优雅停止服务并完成配置清理。壳进程意外被杀时，核心看门狗会自动优雅退出，不会留下占端口的孤儿进程。

## 下载与安装

| 系统 | 状态 | 安装包 |
|---|---|---|
| macOS 10.15+（Apple Silicon） | 已本机构建并实机冒烟验证 | `WB Bridge_1.0.0_aarch64.dmg` |
| Windows 10/11 x64 | CI 构建，待实机验证 | `WB Bridge_1.0.0_x64-setup.exe` |
| Linux x64 | CI 构建，待实机验证 | `.AppImage` / `.deb` |

推送 `v*` 标签后 GitHub Actions 自动构建六平台（macOS ARM/Intel、Windows x64/ARM、Linux x64/ARM）安装包并发布 Release。

### macOS 首次打开

应用使用 ad-hoc 签名（未做 Apple 公证）。首次打开若被拦截：先尝试打开，再到「系统设置 → 隐私与安全性」点击「仍要打开」；若提示「已损坏」，确认来源可信后执行：

```sh
xattr -dr com.apple.quarantine "/Applications/WB Bridge.app"
```

## 使用

1. 先安装并登录 WorkBuddy，在 WorkBuddy 中保存一个自定义模型（生成 `models.json`）。
2. 启动 WB Bridge：自动准备运行时、扫描并检测免费模型，找到有效配置后自动导入。
3. 找不到配置时点「导入 WorkBuddy」选择 `models.json`；Windows 托盘菜单「选择 WorkBuddy 配置…」可更换位置。
4. 「使用系统代理」开关控制运行时下载与模型请求是否走系统 HTTP/HTTPS 代理。

## 从源码构建

依赖：Node.js 22+、Rust stable、各平台 Tauri 系统依赖（Linux 需 webkit2gtk 等）。

```sh
npm install
npm run sidecar      # 构建本机核心 sidecar（--targets=all 构建全部六平台）
npm test             # 运行核心测试（97 项）
npm run build        # 桌面应用构建（产物在 src-tauri/target/release/bundle/）
```

## 工程结构

```
src/
  core/        Node 代理核心（协议转换/模型探测/配置同步），打包为 sidecar 单文件可执行
  ui/          控制面板前端（原生 HTML/CSS/JS，Tauri WebView 加载）
src-tauri/     Tauri 2 壳：窗口/托盘/sidecar 生命周期/IPC 代理/状态推送
scripts/       sidecar 构建脚本（@yao-pkg/pkg）
docs/research/ 上游参考实现行为规格书（移植依据，https://github.com/louchi1984-coder/ow-bridge）
docs/brand/    logo 设计源文件与图标生成脚本
```

## 与上游实现（Electron 版）的差异

- 壳从 Electron 换为 Tauri 2：安装包从约 100 MB 降至 25 MB（dmg），内存占用显著降低；核心 Node 进程以 sidecar 形式随包分发，用户无需安装 Node。
- 新增 `POST /admin/shutdown` 管理接口与 `BUDDY_PARENT_PID` 父进程看门狗：Windows 下 SIGTERM 不可靠、壳可能被强杀，两者共同保证退出时总能优雅清理 WorkBuddy 配置。
- 托盘、导入、系统代理、模型检测与自动导入等行为与原版一致；界面保留原设计系统并优化了层级、留白、悬停反馈与深浅色主题。
- logo 全新设计（悬索桥 + W 形缆线），全平台图标由 `docs/brand/` 源文件生成。

## 已知限制

- 免费模型名单、额度与可用性由上游 OpenCode 决定，本应用不控制也不缓存额度。
- 未做商用代码签名/公证：Windows 可能提示未知发布者，macOS 见上方放行说明。
- Windows ARM64 与 Linux 包由 CI 构建，尚未实机验证。
- 仅 PAC/SOCKS 代理暂不支持（与原版一致）。

## 验证记录

见 `docs/validation.md`。
