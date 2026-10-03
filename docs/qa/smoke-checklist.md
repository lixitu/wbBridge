# 冒烟清单：升级闭环 + GUI 基线（v1.0.2 → v1.0.3）

> 用途：1.0.3 的验收条件 M3（见 `docs/plans/2026-10-03-upgrade-roadmap-and-v1.0.3-closure-plan.md`）。
> 这是本项目**第一次**真的把升级链路走通，也是**第一次** GUI 实机启动。
> 预计 25 分钟（清单体检与备份 5 + 基线 10 + 升级闭环 10）。
> 纪律：**每格只填 ✅ / ⚠（附现象原文）/ ❌（附原文），不得凭印象打勾**；跑完把结果写成 `docs/qa/2026-10-03-v1.0.3-smoke.md`。

---

## 0. 先验清单，再验应用（3 分钟，不通过就别往下走）

线上 `latest.json` 曾因「GitHub 把资产名里的空格规范化成 `.`、脚本按本地文件名编码成 `%20`」而六条 url 全部 404。清单坏时后面每一步都会表现为「莫名其妙的失败」。

```bash
curl -sL https://github.com/trexwb/wbBridge/releases/latest/download/latest.json | tee /tmp/latest.json \
  | python3 -c 'import sys,json;m=json.load(sys.stdin);print("manifest version:",m["version"]);[print(v["url"]) for v in sorted(m["platforms"].items())]'
# 上面顺手把清单存到 /tmp/latest.json 供 0.2 对照；下面逐条探 url（只取首字节，不下载整包）
python3 -c 'import json;[print(v["url"]) for v in json.load(open("/tmp/latest.json"))["platforms"].values()]' \
  | while read -r u; do
      code=$(curl -s -o /dev/null -w '%{http_code}' -L -r 0-0 --max-time 15 "$u")
      case "$code" in 200|206) s=OK ;; *) s=BAD ;; esac
      printf '%s %s  %s\n' "$s" "$code" "${u##*/download/}"
    done
```

| # | 判据 | 结果 |
|---|---|---|
| 0.1 | 六个平台**全部** `OK 200`（`linux-*` 应是 `.AppImage`，`darwin-*` 应是 `.app.tar.gz`，`windows-*` 应是 `-setup.exe`） | |
| 0.2 | 清单 `version` = 你要升到的版本（不是 1.0.2 自己） | |
| 0.3 | 本机已把 `.env.local` 里的口令与 CI Variables 对齐（构建能签出 `.sig`，签名者 key ID = 配置 `plugins.updater.pubkey` 那条） | |

> 0.1 出现任何 `BAD`：停。属 M1 未完成（修复未进被 tag 的那条线），回去先合并与重跑 CI。
>
> ⚠ **状态码口径**（免得误判）：`404` = 清单 url 真的指错资产（就是那个空格/点缺陷）；`000` = **压根没连上**（超时、断网、需要代理），跟清单正确与否无关，换网或重试再看。本会话 2026-10-03 的实测记录：空格形式 `%20` → `404`（darwin-aarch64、windows-x64 两条），同一文件的点形式 → `200`。

## 1. 备份（可回滚的前提，2 分钟）

```bash
cd /tmp && D="smoke-backup-$(date +%Y%m%d-%H%M)" && mkdir -p "$D"
cp -R "$HOME/Library/Application Support/app.wbbridge.desktop" "$D/app-data" 2>/dev/null
cp "$HOME/.workbuddy/models.json" "$D/models.json"            # 路径以面板「WorkBuddy 集成」显示的为准
grep -c buddy-bridge-v1 "$HOME/.workbuddy/models.json"        # 记下这个数字，第 3.4 步要用
```

| # | 判据 | 结果 |
|---|---|---|
| 1.1 | `/tmp/smoke-backup-*` 里有 `app-data/` 与 `models.json` | |
| 1.2 | 记下了 `buddy-bridge-v1` 条目数：`____` | |

## 2. 装 v1.0.2（被升级的一方，2 分钟）

1. 从 Release `v1.0.2` 下载 `WB.Bridge_1.0.2_aarch64.dmg`（**ad-hoc 签名、未公证** → 首次必须右键 →「打开」）。
2. 拖进 `Applications`，启动。

| # | 判据 | 结果 |
|---|---|---|
| 2.1 | Gatekeeper 用右键打开能放行，应用真的起来了（**本项目 GUI 第一次启动**，这里任何异常都请截图/记原文） | |
| 2.2 | 窗口尺寸/布局正常，侧栏五入口都能点（模型与服务 / 运行日志 / 用量与额度 / WorkBuddy 集成 / 关于与更新），无「规划中」占位 | |

## 3. 基线 GUI 冒烟（升级前，10 分钟）

打开「关于与更新」，把这几行抄下来，作为升级后的对照：面板版本 / 核心状态版本 / OpenCode 版本 / 本机接口地址 / 数据目录。

| # | 判据 | 结果 |
|---|---|---|
| 3.1 | 面板版本 = `1.0.2`；本机接口地址形如 `http://127.0.0.1:<port>/v1`；数据目录 = `~/Library/Application Support/app.wbbridge.desktop` | |
| 3.2 | 「模型与服务」列出 `OC · ` 模型且有 ok 状态；点「刷新」后有反馈且状态变化（验 `core-activity` 帧级合并 + 详情栏逐模型状态真的会更新，长期未实测项） | |
| 3.3 | 用面板地址做健康检查（把 `/v1` 去掉）：`K=$(cat "$HOME/Library/Application Support/app.wbbridge.desktop/api-key"); curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $K" "http://127.0.0.1:<port>/health"` → 期望 `200`；**不带** header 期望 `401` | |
| 3.4 | 「WorkBuddy 集成」→ 点「导入」→ 出现成功反馈；`grep -c buddy-bridge-v1 models.json` 与 1.2 记录的量级一致 | |
| 3.5 | **在 WorkBuddy 里用 `OC · ` 模型真跑完一次对话**（全项目第一次端到端价值验证） | |
| 3.6 | 「用量与额度」：3.5 之后 `requests`/`ok` 增加；探测不计、客户端取消不计（`usage` 口径第一次实测） | |
| 3.7 | 「运行日志」能读到尾部且**看不到** `api-key`/`OPENCODE_SERVER_PASSWORD` 的值 | |
| 3.8 | 切到「运行日志」→ 完全退出 → 重开：仍停在「运行日志」（`localStorage` 在 WKWebView 里真的可写，长期未实测） | |
| 3.9 | 「关于与更新」关掉「启动后自动检查更新」→ 重启：开关仍为关、「上次检查」时间不变；打开后冷启动 ~5s 会出现检查结果 | |
| 3.10 | **关窗即退出**：点红色关闭按钮 → `pgrep -fl "WB Bridge"` 无输出（不再驻留托盘）；`app-data/service.pid` 不会让下次启动报「已在运行」 | |

## 4. 升级闭环（核心，10 分钟）

进「关于与更新」，展开「更新」区。

| # | 判据 | 结果 |
|---|---|---|
| 4.1 | 点「检查更新」→ 出现更新条「发现新版本 v1.0.3」＋ notes。**这一步是第一次真的打到 updater endpoint**；若报「更新失败」，把原文抄进结果文件再继续 | |
| 4.2 | 点「下载并安装」→ 按钮变「下载中」＋ spinner；进度文本给出百分比与 `已下载 X / Y MB`。若显示「总大小未知」说明上游没给 `contentLength`（记 ⚠，非缺陷） | |
| 4.3 | 下载完成后更新条文案变「v1.0.3 已下载完成」，按钮变「重启应用」 | |
| 4.4 | 点「重启应用」→ 窗口关闭并**自动重新起来**，全程不卡死、无僵尸进程（`RunEvent::ExitRequested` 走 `stop_core_bounded` 的效果，**唯一可信证据就是这一步**） | |
| 4.5 | 重启后「关于与更新」：面板版本 = `1.0.3`；OpenCode 版本非「未就绪」；模型列表非空（升级后的核心带已更新配置正常启动） | |
| 4.6 | 重跑 3.3 健康检查 + 3.5 的一次 WorkBuddy 对话，结果与升级前一致 | |
| 4.7 | `grep -c buddy-bridge-v1 models.json` **不小于** 1.2 的数字（升级/重启过程不得清空发布条目） | |
| 4.8 | 再点一次「检查更新」→ 显示「已是最新版本（1.0.3）」 | |

## 5. 失败预案（提前看好，才敢点 4.4）

| 现象 | 先查什么 | 处置 |
|---|---|---|
| 4.1 报「更新失败」 | 重跑 0.1 的脚本看 url；再看错误是否指向签名校验 | url 坏 → M1 未落地；签名不符 → `pubkey` 与实际签名私钥不是同一把（历史上真发生过，见 AGENTS.md「签名与发布」） |
| 4.2 卡住不动 | 只有 0 B 且无变化 | 先等 2 分钟（CDN 首包可能慢）；失败会给 `FeedbackBar` 原文，抄下来 |
| 4.4 后起不来 / 模型空 | 面板是否显示 `core-failed` 与原因、是否有重试入口（这条本身也是未实测项，顺手验） | 用第 1 步备份恢复 `app-data` 与 `models.json`，改手动装 `v1.0.3` 的 `.dmg`；把现象写进结果文件并**保留 ❌** |
| 任何数据丢失 | `models.json` 条目数、`api-key` 是否被重建 | 恢复备份优先；不要在工作区里临时改核心逻辑来「救」 |

## 6. 回填

1. 新建 `docs/qa/2026-10-03-v1.0.3-smoke.md`：把上面每格的实际结果贴进去（含错误原文、截图路径）。
2. 对通过的项，**如实**更新 `AGENTS.md`「核心架构与数据流」上方状态表的对应行（GUI、关窗即退出、偏好持久化、updater 下载侧、`stop_core_bounded`）；未通过的保持 ❌ 并写明卡在哪。
3. 同步 `docs/wiki/已知限制与未验证项.md`。
4. 提交与否由维护者决定；Agent 不自动 commit / push / tag。

> ⚠ 全平台口径：本清单只覆盖 **macOS aarch64 + 手动安装的 v1.0.2**。Windows / Linux 的升级路径（NSIS `-setup.exe`、裸 `.AppImage`）与其余五个平台的实机安装在本次仍不会被验证，结论不得外推到它们。
