# WB Bridge v1.0.0 验证记录

日期：2026-09-30（本机 macOS，Apple Silicon）

## 自动化测试

- `core/` 测试套件：**97/97 通过**（`npm test`，node --test）。
  - 自上游参考实现（https://github.com/louchi1984-coder/ow-bridge）迁移的测试 95 项（协议校验、导入与退出清理、目录能力映射、图片转发、推理档位、系统代理解析、活动文案等），其中 2 项读取 UI 源码的契约测试已指向新 `ui/` 路径。
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
