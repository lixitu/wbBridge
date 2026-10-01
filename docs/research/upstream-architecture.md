# 上游参考实现架构规格书（Tauri 移植调研）

- 参考仓库：`https://github.com/louchi1984-coder/ow-bridge`（只读快照），版本基线 `package.json` version **0.2.5**（package.json:3）；注意 `src/main.js:39` 内嵌 `version: '0.2.0'` 写入 status.json，两者不一致，移植时以 package.json 为准并在状态里注明来源。
- 本文所有结论标注 `文件:行号`（相对仓库根）。无法确认的点显式标注「未确认」。
- 用途：在不再回读原仓库的情况下忠实复刻其行为；第 10 章为移植后验收清单，第 11 章为风险排序。
- ⚠ **阅读须知（2026-10-01 更新）**：本文件记录的是**上游 Node + Electron 参考实现的形态**，与当前仓库的形态已不同——本仓核心已重写为 Rust（`src-tauri/core/`，crate `wbbridge-core`，静态链接进 Tauri 壳），面板为 Vue 3 + Vite（`src/`）。因此**第 8 / 9 章的窗口与界面数值均属上游原实现**（窗口 1040×740 / 最小 880×620、侧栏 218px、详情为模型行就地展开、`renderer.js` 的 Escape + 面板外点击关闭），**不代表本仓当前界面**；本仓界面现状见 `AGENTS.md`「UI 规范」与 `docs/validation.md`「面板布局改造」（详情为右侧常驻分栏 `clamp(300px, 45%, 360px)`、默认窗口 1120×720 / 最小 860×560、侧栏 208px、无宽度堆叠降级）。第 10 章的 JS 测试清单同样见下方说明。

---

## 1. 系统总览

### 1.1 进程模型

上游参考实现是一个**单进程 Node 服务 + 一个可选 Electron 桌面壳**的两层结构：

- **服务进程**（`node src/main.js`）：启动 OpenCode 托管运行时（独立子进程，见第 2 章）、提供 OpenAI 兼容 HTTP API（默认 127.0.0.1:41980）、维护 status.json、管理 WorkBuddy models.json 同步与模型探测。桌面壳不是必需的：服务可以独立运行，桌面壳只是 status.json 的轮询读者 + /admin 控制端（desktop/main.cjs:56-61,111）。
- **桌面壳进程**（Electron main + renderer）：读 `status.json` 展示模型面板，通过 IPC 调用服务的 `/admin/*` 接口（desktop/main.cjs:63-75）。壳与服务之间**没有直接 RPC**，全部通过 HTTP + 文件轮询通信。

### 1.2 端口与入口（src/main.js）

- 监听端口：`process.env.BUDDY_PORT || 41980`（main.js:17-18）。
- 单实例锁：数据目录下 `service.pid`；启动时读取，若 `kill(pid, 0)` 成功说明已有实例在跑，**直接 exit(2)**（main.js:20-27）。写自己的 pid 进锁文件。
- API key：首次启动生成 `randomBytes(32).toString('hex')` 写入数据目录 `api-key` 文件；已有则复用（main.js:28-31）。所有 HTTP 请求（/v1 与 /admin）都必须携带 Bearer 认证头（令牌值为 api-key 文件内容；bridge.test.js:112）。
- 日志：OpenCode 子进程 stdout/stderr 追加写入 `opencode.log`，**超过 5MB 先轮转**（重建文件）（main.js:47-48）。

### 1.3 status.json：字段全集与语义

state 对象初始字段全集（main.js:39）与各处更新：

| 字段 | 语义 | 出处 |
|---|---|---|
| `version` | 服务版本，硬编码 `'0.2.0'`（与 package.json 0.2.5 不一致） | main.js:39 |
| `phase` | `starting` → `ready`；就绪判定含 /agent 校验 | main.js:39,289-291 |
| `startedAt` | ISO 启动时间 | main.js:39 |
| `models` | 当前发布给客户端的模型目录（freeModels 产物） | main.js:39,207-212 |
| `availableModels` | models.json 中可见模型 id 列表 | main.js:39 |
| `modelResults` | 每模型探测结果 map（id → modelStatus） | main.js:39,150-171 |
| `probe` | `{ running, startedAt?, finishedAt?, error? }` 探测批次状态 | main.js:39,130-177 |
| `lastRequest` | 最近一次 /v1 请求观察 `{ code, model?, at? }` | main.js:85-98 |
| `requests` | 计数与计时（成功/失败、上游耗时） | main.js:85-98；server 计时测试 bridge.test.js:253-273 |
| `useSystemProxy` | 系统代理开关（settings.json 持久化） | main.js:39,229-243 |
| `proxySource` | 代理解析来源描述（scutil/注册表/manual/none） | system-proxy.js 返回值；main.js:229-243 |
| `modelsFile` | 当前生效的 WorkBuddy models.json 路径 | workbuddy-config.js resolve 结果；main.js:189-198 |

写盘纪律：status.json **串行化写入**（promise 链防交错，main.js:41-46），原子替换写（atomic.js writeJson）。

### 1.4 生命周期全流程

**启动（main.js:269-298）**：
1. 取数据目录（platform.js dataDirectory）、检查 pid 锁（1.2）、读写 api-key。
2. 读 `settings.json`（数据目录内，`useSystemProxy`、`workBuddyModelsFile` 持久化字段；lifecycle.test.js:79,98）。
3. 解析 modelsFile：`BUDDY_MODELS_FILE` > settings.json 保存值 > WorkBuddy 配置目录发现（workbuddy-config.js:15-19）。
4. 启动 OpenCode 运行时（runtime.js，见第 2 章），起 HTTP server。
5. **/agent 校验**：向 OpenCode 查询 agent 列表，确认 `buddy-bridge` 与 `buddy-chat` 两个自定义 agent 存在，失败则 phase 不进 ready（main.js:289-291）。
6. 若 `useSystemProxy` 开启，先解析系统代理并应用到运行时 env（main.js:269-283；解析失败回退为关闭，不阻断启动）。
7. `BUDDY_NO_SYNC!=0` 时执行启动导入：**先清除旧的 owned 条目再探测**（lifecycle.test.js:41），随后批量探测。
8. 启动后台活动定时器（1.5）与事件订阅。

**探测编排 startProbes（main.js:130-177）**：
- 串行逐模型探测（同一时间只有一个 in-flight 请求打到 OpenCode），**整批共享一个 60 秒 deadline**（probe.js PROBE_TIMEOUT=60000，probe.js:4；预算分配见第 5 章）。
- 每个模型结果经 `record()` 落入 state.modelResults；**格式类失败不撤销已发布模型**（main.js:85-98：`invalid_model_output` 只记 lastRequest.code，modelResults.ok 仍为 true——测试 lifecycle.test.js:59-65）。
- 批次结束：把通过探测的模型 merge 进 models.json（syncModels）、更新 phase=ready、发布模型目录。

**refresh（main.js:189-198 + POST /admin/refresh）**：重新拉取 OpenCode 模型目录（`models opencode --refresh`）、重跑探测批次；**从不写 WorkBuddy 配置**（lifecycle.test.js:71-74）。

**import（main.js:215-224 + POST /admin/import）**：把当前已发布的模型目录显式写入 modelsFile（可传 `{ modelsFile }` 切换目标文件）；切换位置时**只清理之前管理的条目**（lifecycle.test.js:92-101）。

**setSystemProxy（main.js:229-243）**：开关系统代理，持久化到 settings.json；网络模式切换**不触发导入**（lifecycle.test.js:75-81）。

**退出/shutdown（main.js:249-263）**，顺序固定：
1. `syncModels(file, [], …, { allowEmpty:true })`——从 models.json **移除全部 owned 条目**（退出清理；bridge.test.js:223-234 证明保留用户模型与 metadata）；
2. `runtime.stop()` 停 OpenCode 子进程；
3. 等待进行中的 probeTask 结束；
4. 最后一次 status.json 落盘；
5. 删除 pid 锁文件。
桌面壳触发的退出走 IPC `shutdown`（lifecycle.test.js:104 用 `child.send('shutdown')` 模拟，Node IPC channel；lifecycle.test.js:25 spawn 时带 `'ipc'` stdio）——即服务进程支持 **process.on('message') 的 'shutdown' 消息**作为优雅退出通道（main.js:249 处注册；进程间消息通道为未在桌面壳代码中直接出现的细节，桌面壳用 taskkill/SIGKILL 兜底，desktop/main.cjs:131-137）。

### 1.5 活动上报（main.js:99-129）

- `record()`：每次 /v1 请求完成记录 lastRequest/requests；**格式失败（invalid_model_output）不撤销模型**（main.js:85-98）。
- 5 秒周期定时器产出 heartbeat 活动事件，活动事件带 **1 秒节流**；activity 回调消费者为 SSE 客户端与桌面壳状态（main.js:99-129）。activity 语义测试：content 类事件 vs heartbeat 区分、busy 不覆盖 repair stage、文案诚实（activity.test.js 全部条目，见第 10 章）。

### 1.6 HTTP 面（src/server.js）

- 路由：`GET /v1/models`、`POST /v1/chat/completions`（流式/非流式）、`POST /admin/{probe,refresh,import,system-proxy}`、`GET /global/health`（透传给 OpenCode）。
- 鉴权：Bearer key 不符 → 401（server.js:20-24；bridge.test.js:112）。
- Origin 校验：非空 Origin 且非本机来源 → 403（bridge.test.js:113；server.js:25-30）。
- 模型选择：请求 model 必须命中当前发布目录（`OC · Test` 这类短 id），失败 400（bridge.test.js:247,281）；失败模型从 /v1/models 消失后，缓存的调用方再请求得到 400 而不是打到上游（bridge.test.js:236-251）。
- 失败语义：上游/格式错误 → 502（bridge.test.js:61,245,269）；`invalid_model_output` 记入 lastRequest（lifecycle.test.js:62）。
- SSE 细节见第 4.3 节。

---

## 2. OpenCode 运行时管理（src/runtime.js、src/platform.js）

### 2.1 产物形态（对问题 (a) 的回答）

- 运行时是 npm 上的**平台专用包**：包名 `opencode-<platform>-<arch>`（如 `opencode-darwin-arm64`），包内是**单个原生可执行文件** `opencode`（Windows 为 `opencode.exe`）（platform.js:11-15）。
- **完全不依赖系统 Node.js**：下载并解包后直接 spawn 该二进制；`runtime.js` 只用 Node 自身做下载/校验/启动编排。移植到 Rust 后可照搬「下载 tarball → 校验 → 解出单文件 → spawn」模型，无需任何 JS 运行时。
- 安装布局：`<dataDir>/managed/runtime/<版本>/<binary>`；测试断言安装路径为 `install/runtime/1.18.33/opencode`（runtime.test.js:80,107,146）。

### 2.2 下载源、镜像回退与代理

- 主源 `https://registry.npmjs.org/<pkg>/latest`（metadata，含 `dist.integrity` 与 `dist.tarball`），tarball 从 metadata 给出的 URL 下载（runtime.test.js:58,70,81）。
- **镜像回退**：主源失败（超时/网络错）→ `https://registry.npmmirror.com/<pkg>/latest` → 对应镜像 tarball（runtime.js:19；测试序列 `official/latest → mirror/latest → mirror tarball`，runtime.test.js:109-111）。
- **镜像字节仍按官方校验**：tarball 回退时保留 official 的 integrity，镜像字节不同 → `checksum mismatch` 拒绝且不落盘（runtime.test.js:167-193）。
- 代理：配置了 `HTTPS_PROXY` 时，metadata 与 tarball 请求都走代理 agent（runtime.test.js:94-108 `calls.every(call => call.proxied)`）。
- 元数据可信性：tarball URL 必须在 registry 域名下，外部 URL（`https://example.com/runtime.tgz`）→ 拒绝「安装信息不可信」（runtime.test.js:152-160）。

### 2.3 版本判定与更新策略

1. 列出候选（runtime.js:33-40；platform.js 提供 candidates）：既有 `managed/runtime/*` 版本目录 + 外部候选路径（如系统里已装的 opencode）。
2. 候选探测版本：执行 `--version`；**`.cmd/.bat/.ps1` shim 直接拒绝**（launcher script，runtime.js:131；测试 runtime.test.js:113-126），版本输出不可解析 → 报「unrecognized version output」而不是静默忽略（runtime.test.js:125）。
3. 查询 npm latest；**查询失败 → 回退复用本地**（runtime.test.js:27-42，日志含 `using local`）。
4. 本地版本 **≥ latest → 复用**（runtime.test.js:45-59）；本地更旧 → 下载新版（runtime.test.js:61-82）。
5. 下载后**解包验证版本**：执行包内二进制的 `--version`，与 metadata 版本不符 → `version mismatch` 拒绝（runtime.test.js:128-150）。
6. 复用已准备好的运行时会报告 `using local`（runtime.test.js:39-41）。
- Windows 差异：版本探测在测试中以读文件内容代替（POSIX shebang 脚本在 Windows 不可执行）（runtime.test.js:16-21）；生产探测方式对 Windows 的适配细节未在 src 中另见特殊分支——未确认。

### 2.4 解压与校验

- sha512（`dist.integrity`，SRI 格式 `sha512-<base64>`）校验下载字节 → tar 解包，只取 `package/bin/<binary>`，`strip: 2`（runtime.js:75-116）。
- 校验失败/元数据不可信时**不得留下半成品**：测试断言失败后 `runtime/<version>/<binary>` 不存在（runtime.test.js:162-164,188-190）。

### 2.5 启动参数、环境变量、健康判定

- 启动序列（runtime.js:188-192）：先 `opencode models opencode --refresh --pure`（拉模型目录），再 `opencode serve --pure --hostname 127.0.0.1 --port N`。
- 隔离 env：**白名单继承** + 重定向 XDG 目录到数据目录 + 注入 `OPENCODE_CONFIG_CONTENT`（内嵌配置，含 buddy-bridge/buddy-chat agent 与 StructuredOutput 工具定义）（runtime.js:172-186）。具体配置 JSON 内容未在本调研中全文摘录——未确认（需移植时从 runtime.js:172-186 原文抄录）。
- 健康判定：轮询 `GET /global/health`，**最多 120 次 × 500ms（60s）**，且要求返回版本与本地版本一致（runtime.js:203-216）。
- 子进程 stdout/stderr → `opencode.log`（5MB 轮转，main.js:47-48）。

### 2.6 GET /event 事件流（backend.js:67-103）

- Backend 启动后订阅 `GET /event`（SSE），**断线 1 秒后重连**（backend.js:67-103）。
- 事件载荷形状：`{ directory, payload: { type, properties } }`（bridge.test.js:681,687-688）。
- 用途：`permission.asked`（原生动作审批，携带 callID）→ 进入 handoff 短路（第 3.3 节）；`session.status` retry → 上游重试进度上报（attempt/message，其他 session 忽略，bridge.test.js:661-697）；`message.updated` → 记录 session 级 token 用量（usageBySession，bridge.test.js:1405-1427）；`request.done` → 清理进行中条目（bridge.test.js:694）。
- 订阅生命周期：probe / complete 前确保在订阅，最后一个 session 结束后释放（watchEvents/stopEvents；bridge.test.js:1445-1460）。

---

## 3. 后端请求流（src/backend.js）

### 3.1 complete() 全流程（backend.js:250-415）

输入是 prepare() 产物（第 4 章）：`{ model, text, tools?, images?, variant?, chatOnly?, toolChoice? … }`。步骤：

1. **创建会话**：`POST /session`，body 含 `permission: [{ permission: '*', pattern: '*', action: 'ask' }]`——**一切原生动作都必须经过询问**（bridge.test.js:99）。
2. **发消息**：`POST /session/<id>/message`，payload 含 `agent`（工具模型 = `buddy-bridge`，chatOnly = `buddy-chat`）、`parts`（用户文本 + 图片 parts）、`system`（图片匹配指令等）、`format`（StructuredOutput envelope schema，仅工具路径；chatOnly 不带 format，bridge.test.js:313）、`variant`（推理档位，命中 variants 时）。**不发 tools 字段**——工具目录已编码进 system/format（bridge.test.js:97-98）。
3. **等待完成**：同时（a）轮询 `GET /permission`（**250ms 间隔**）取待审批动作；（b）消费 /event 流。控制请求（permission 轮询）有自己的短 deadline，**模型生成本身不受其约束**——上游重试、5 分钟以上静默推理都合法，只有客户端取消才终止（bridge.test.js:544-580,1226-1255）。
4. **取结果**：message 完成后从响应提取 envelope，三通道按序（见 3.4）。
5. **轮次预算**：整个 complete 最多 **3 轮** message（bridge.test.js:510-513：修正 1 + 翻译尝试 1 + 显式重试 1）；探测模式只花 **2 轮**（格式修正 1 次，不做翻译/重试，bridge.test.js:1191-1202）。
6. **finally 清理**：成功与失败都必须 `DELETE /session/<id>`；失败/取消先 `POST /session/<id>/abort` 再 DELETE（bridge.test.js:100-103,577-578）；会话清理同时清 pendingApprovals 与 usageBySession（bridge.test.js:1362,1422-1426）。
7. 用量：`GET /session/<id>/message?limit=1` 取最后一条 assistant 的 tokens（在 abort **之后**查，bridge.test.js:1402）；查不到则回退 usageBySession 中该 session 的 `message.updated` 记录（bridge.test.js:1405-1427）。
8. 取消语义：客户端 abort → 转发为上游 abort，错误码保持 `ABORT_ERR`；上游连接错保持 `ECONNRESET`；**被取消的请求不得记为成功**（onResult 不触发，bridge.test.js:630-649）。

### 3.2 错误分类（model-status.js modelResult）

| category | 判定 | 测试 |
|---|---|---|
| `quota` | message 含 `insufficient_quota`（如 429） | bridge.test.js:213,738 |
| `rate_limit` | 「Too many requests」429 | bridge.test.js:214,739 |
| `access` | 「Free tier only within OpenCode」403 | bridge.test.js:215,740 |
| `timeout` | probe 超时（`Model probe timed out`/无 message 的 abort） | bridge.test.js:216,727 |
| `error` | 其余（含 no_action 的文本回复 502） | bridge.test.js:217,726 |
| `available` | ok=true | bridge.test.js:218,741 |

- `withRequestMeta`：单次请求的观察（tools/calls/nativeAttempts/steps）只作观测记录，**不改判模型能力标签**（noAction 不出现在请求级 meta，bridge.test.js:598-605）。
- server 层把 meta（tools、calls、nativeAttempts、steps、permissions[]）递给记录器（bridge.test.js:607-628）。

### 3.3 原生动作拦截与 handoff

- OpenCode 自身工具（bash/read/write/glob/grep/skill…）被 `permission: ask` 拦下后出现在 `GET /permission` 列表或 `permission.asked` 事件里；Bridge **永远不批准**（reply 一律 `reject`，bridge.test.js:179,852），而是尝试**映射为本次请求提供给客户端的外部工具**（handoff），把该调用作为 tool_calls 返回给客户端执行。
- **映射表**（handoff.js:10-28）：`bash → Bash/PowerShell`、`read filePath → file_path`、`write`、`edit` 家族、`glob → Glob/LS`、`grep → Grep/Search`、`skill → Skill`。
- 映射规则（全部有测试锁定）：
  - 只填目标 schema **声明过的参数名**；required 填不满 → 放弃（null），绝不发明参数（bridge.test.js:761-778,946-974；glob→只有 path 的 LS 只传 path，bridge.test.js:955-957）。
  - 外部没有等价工具 → 按名拒绝并继续请求（`rejectFeedback` 消息点名工具、说明 cannot be mapped、指示放回 calls 数组；不 abort，bridge.test.js:780-789,821-856）。
  - 参数来源：tool part 的真实 input 优先；pending（尚未有 input）时用审批 metadata（filepath/patterns/command…）兜底（handoffInput，bridge.test.js:858-869,968-970）。事件流里的 toolParts 缓存可补齐 approval 只有 callID 的情形（bridge.test.js:810,1354-1356）。
  - 遵守本次 `tool_choice`：none → 不 handoff 不翻译；具名 function → 只映射该工具（bridge.test.js:1257-1286）。
- **handoff 短路**：一旦拿到可映射动作 → `POST /session/<id>/abort` 停止生成 → 直接以该 tool_calls 作为响应，**不跑第二轮模型**（bridge.test.js:791-819,892,1364）。
- permission 轮询的容错：查询异常（连接重置/非数组/400 BadRequest）只记日志、继续轮询，不中断推理、不丢 handoff（bridge.test.js:1288-1314,1338-1366）；轮询彻底不可用时保留取消/连接错误的原始错误码（bridge.test.js:1316-1336）。
- 审批载荷瘦身：`shrinkPermission` 截断超长 metadata.content 为 `<前 N 字>…[total chars]`（bridge.test.js:651-659）。
- 无 callID 的审批：按 permission 名拒绝（bridge.test.js:158-184）；**原生动作在修正后的结构化响应被接受前先被拒绝**（bridge.test.js:186-209）。

### 3.4 envelope 提取与修复（repair.js，20s deadline backend.js:223-248 + repair.js:121-146）

envelope = `{ content, calls: [{name, arguments}] }`。三通道按序：
1. `info.structured`（模型按 format 直接返回）；
2. **已完成的 `StructuredOutput` 工具 part**：`state.status === 'completed'` 时 `state.input` 即 envelope，即使没有 info.structured（bridge.test.js:976-996）；
3. 文本 parts 中解析 JSON（含 ```json 围栏）。

- 空 parts/全空 → 报「三者都为空」（invalid_model_output，bridge.test.js:998-1011）；`content:null, calls:[]` = 空回答而非格式错；两字段都缺 → 报错并点名字段；类型错 → 报 `content=number` 这类字段与类型（bridge.test.js:1013-1030）。
- 等价格式归一化：`{content:null,calls:[...]}`、缺 content、arguments 为 JSON 字符串、OpenAI 风格 `tool_calls` 均接受；扁平化 `content+name+arguments` 冒充纯文本 → 拒绝（bridge.test.js:518-535,1027-1029）；**calls 数组整体 JSON 字符串编码**也解码（bridge.test.js:1429-1443）。
- 校验：calls 里的名字必须在请求工具清单内（unlisted 拒）；`tool_choice:'none'` 时出现 calls 拒；`'required'` 时空 calls 拒；arguments 非法 JSON 拒但保留 `{}`（bridge.test.js:22-31,28）。
- **修复阶梯**（每类失败的处理，轮次预算见 3.1 第 5 条）：
  - envelope 格式错/单个 call 条目畸形 → **一次 format-only 修正**：重发同 format，附「No external tool has been executed」等系统文本（bridge.test.js:452-493）。
  - 无翻译器时再显式重试一次：把诊断原文给模型（「补齐诊断指出的缺失项后重发」），`meta.repaired.envelope.reason='no translator available'`（bridge.test.js:1167-1189）。
  - 配置了 `translator`（TRANSLATOR_ORDER 指定的对话模型）：在**独立 session**、agent=buddy-chat 下翻译；翻译结果必须通过同一校验（越清单 → 保留原错误 reason=invalid_tool_call），material 携带外部对话（material.conversation）、工具描述（tools[].description）与约定（conventions 含 complete supplied file content）（bridge.test.js:1032-1075,1144-1165）。
  - 被输出上限截断（finish=length）→「要求压缩重发」（缩短说明和推理、保留完整参数）（bridge.test.js:1204-1224）；截断与原生活动**不重试**（native 直接失败，1 轮；其余 3 轮，bridge.test.js:495-515）。
  - 结构化输出 JSON 解析失败（invalid 工具 part 带 error）按**格式问题**走翻译路径，绝不当成原生活动（bridge.test.js:1105-1142）。
  - 不可映射的原生动作也可走翻译：handoffCheck.native 记录原因，翻译出外部调用（bridge.test.js:1077-1103）。
  - 修复流程整体 20s deadline（backend.js:223-248；repair.js:121-146）。

### 3.5 handoff 与修复的观察记录

- meta.handoffCheck（为何无法映射/为何翻译）、meta.repaired.{envelope,action}（{ok, model?, reason?}）、meta.handoff（最终工具名）都会写入请求 meta 供记录（bridge.test.js:914-917,1054-1055,1073-1074,1098-1100,1187）。

### 3.6 免费模型发现（freeModels，backend.js:11-20）

- 判定 **cost.input === 0 && cost.output === 0**，价格缺失（undefined）→ 排除；名字含 free 但 output>0 → 排除（bridge.test.js:42-49）。
- 额度耗尽（remaining:0）的免费模型**保留在目录**（bridge.test.js:219-220）。
- 保留 provider 原名：catalog name 缺失时回退 id 尾段（bridge.test.js:289-299）。

---

## 4. 协议转换（src/protocol.js、json.js、reasoning.js、model-status.js、repair.js、handoff.js）

### 4.1 OpenAI 请求 → OpenCode 字段映射（protocol.js:11-79 prepare）

- 鉴权模型解析：请求 model（短 id `OC · Test`）→ 客户端模型名映射回上游 `opencode/test`（clientModelID 规则见 4.5）；不在免费目录 → 「available free」错误（bridge.test.js:29,281）。
- messages 历史：role 保留、tool 结果保留 `tool_call_id`（bridge.test.js:16-21）；assistant 的 tool_calls 归一为 envelope（content 置 ''，bridge.test.js:592）；外部工具执行失败的原样保留作为 tool 观察（bridge.test.js:537-542）。
- 图片：仅接受 `data:image/*;base64` URL；file:// / https / 非 image MIME / 非法 base64 全拒（bridge.test.js:447-449）；模型不支持图片 → 「does not declare image input」拒（bridge.test.js:20）。文本占位 `message-<轮>-image-<序>.png` 按出现顺序编号，base64 不进 text（bridge.test.js:439-441）；图片以独立 parts 追加在文本 part 后，system 注入「Match each attachment filename」指令（bridge.test.js:443-445）。
- tool_choice 透传约束：'none'/'required'/具名 function 均由 decode 校验执行（bridge.test.js:22-31）；对 OpenCode 不发 tools 字段（bridge.test.js:97-98）。
- 推理档位 → variant：见 4.4。
- chatOnly 模型收到工具请求 → 「仅支持普通对话」拒（bridge.test.js:303）；chatOnly 请求不带 format（bridge.test.js:313）。

### 4.2 错误对象形状

- BridgeError：`{ message, status, code }`；server 层转 `{ error: { message, type: code||'upstream_error', code } }`，HTTP 状态取 e.status || 502（server.js:75-79）。特殊：413 请求超 8MB（server.js:14）、429 busy 并发上限 4（server.js:42）、401 鉴权（server.js:24）、403 带 Origin 头（server.js:26）、404 其他路由（server.js:41）。
- SSE 中途失败：headers 已发则发 `data: {"error":{…}}` 块（server.js:78）。
- TimeoutError 名义错误 → message 统一「Model request timed out」/「Model probe timed out」（server.js:75；probe.js:77）。

### 4.3 SSE 块序列与起始块规则（protocol.js:152-162 + server.js:48-70）

1. 请求受理后立刻发注释行 `: validating model response before emission\n\n`（busy 不算模型输出，bridge.test.js:137）；此后每 10s 发 `: waiting` 心跳（server.js:51）。
2. **起始块规则**：只在首个「带 content 的 receiving 活动」时发 role 块 `{id, created, object:'chat.completion.chunk', model, choices:[{index:0, delta:{role:'assistant'}, finish_reason:null}]}`（server.js:52-57）；id=`chatcmpl-<uuid>`，created 秒级（bridge.test.js:139-140）。
3. 最终 sendSSE：若已发过起始块则**复用同一 id/created/model**，后续块不带 role（bridge.test.js:147-148）；未发过（非流式内容路径）则 role-only 起始块在首个内容事件时发（server.js:48-60）。
4. 内容与 tool_calls 整块输出（content 块、tool_calls 带 index 映射），finish_reason：有 calls → 'tool_calls'，否则 'stop'（bridge.test.js:149-151）。
5. usage 块：仅 `stream_options.include_usage` 时附，最后 `data: [DONE]\n\n` 恰好一次（bridge.test.js:152-153,39-40）。
6. usage 缺失则整段不出现 usage 字段（bridge.test.js:1377-1384）。
7. usage 合成（protocol.js:137-148 completion）：`prompt_tokens = input + cache.read + cache.write`、`completion_tokens = output + reasoning`、total 不重复计数、`prompt_tokens_details.cached_tokens`、`completion_tokens_details.reasoning_tokens`（bridge.test.js:1368-1376）。

### 4.4 推理档位 → variant 映射（reasoning.js + protocol.js）

- 目录侧：variants 中 `disabled:true` 剔除；每 variant 的 `reasoningEffort` 生成 effort→variant 名映射（bridge.test.js:331-345）。
- 请求侧：`reasoning_effort`（OpenAI 风格）或 `reasoning.effort`（WorkBuddy 风格）→ 命中映射表 → `variant` 字段发给 OpenCode；'none'/'max'/未知值 → 「reasoning effort」错误；模型无 reasoning 能力同样拒（bridge.test.js:352-357,380-382）。
- 固定推理模型（reasoning:true 但 variants 为空）：任何合法 effort 都不发 variant（走默认模式），面板只报 supportsReasoning+空 efforts（bridge.test.js:360-383,346-350）。
- 导入侧：onlyReasoning=true、canDisableThinking=false、supportedEfforts=[映射后 keys]；无 variants 时 supportedEfforts=[]（bridge.test.js:340-349）。

### 4.5 clientModelID 规则（model-status.js clientModelID；bridge.test.js:276-286）

- 导入条目 id = `OC · <provider原名>`（OC、空格、间隔号、空格），name 同 id；手工同名冲突保留手工条目；legacy owned 条目（旧 id 格式）在合并时被替换为新格式（bridge.test.js:284-285）。
- `OC · X` 反查上游 id 用目录（prepare 阶段），目录撤销后缓存调用方 400（bridge.test.js:236-251）。

### 4.6 repair/handoff/json/model-status 补充

- json.js：安全 JSON 解析/字符串化工具（envelope 与 calls 的容错解码，含 JSON 字符串编码的 calls 数组，bridge.test.js:1429-1443）。
- repair.js：修复材料构造（material.conversation / tools[].description / conventions）与 20s deadline（bridge.test.js:1144-1165；repair.js:121-146）。
- handoff.js：映射表 handoff.js:10-28；rejectFeedback 文案（点名+cannot be mapped+放回 calls 数组，bridge.test.js:780-789）；handoffInput 优先级 bridge.test.js:858-869。
- model-status.js：modelResult 分类（3.2 表）、clientModelID、withRequestMeta（bridge.test.js:598-605）。

---

## 5. 模型探测（src/probe.js + main.js:130-177）

- **请求构造** probeBody（probe.js:19-26）：用户消息 `Read /external/probe-<token>.txt and report its contents.`（token=randomBytes(8).hex）；**5 个工具** Read/Write/Bash/Glob/WebSearch（probe.js:10-16）；`parallel_tool_calls:false`；**不设 tool_choice**（不强制工具调用，bridge.test.js:705-708）。
- **判定树** judgeProbe（probe.js:29-40）：
  - 无 tool_calls → `no_action`（502，「模型只返回了文本，没有产生任何动作」）→ 最终归类 error→chat-only（bridge.test.js:710-711,726）。
  - calls≠1 或名字≠Read 或 file_path 不含 token → `probe_mismatch`（502，点名收到的工具名，bridge.test.js:717-719）。
  - 通过 → 返回该 call（bridge.test.js:713-715）。
- **formatUnsupported**（probe.js:46-49）：`invalid_model_output`/`invalid_tool_call` 或 「only auto is supported for tool_choice」消息 → 真格式不支持 → 走 30s 纯对话复测后标 chatOnly；probe_mismatch/no_action/native_tool_activity/timeout 都**不是**格式问题（bridge.test.js:748-759）。
- **超时**：PROBE_TIMEOUT=60000（probe.js:8），批次共享 deadline（main.js:130-177）；超时 → `probeFailure` 造 `TimeoutError`/504/`timeout`（probe.js:75-80；bridge.test.js:721-724）。
- **重试**：RETRYABLE_PROBE={probe_mismatch,no_action}，重试 1 次（新 token）；格式失败与超时不重试（probe.js:55-71；bridge.test.js:920-944）。
- **工具能力检测方式**：整条链路 = 真实工具 schema + 自由选择 + 校验「必须产生指向 token 文件的 Read 动作」（probe.js:4-7 注释：旧版强制 tool_choice 只验证了传输与格式，文本-only 模型会假通过）。
- **批处理后**：chatOnly 判定模型导入时 supportsToolCall=false（lifecycle.test.js:89-91）；manual probe/refresh 从不写 WorkBuddy（lifecycle.test.js:70,74）；探测通过才允许 import 落盘（lifecycle.test.js:82-91）。

---

## 6. WorkBuddy 配置同步（src/sync.js、workbuddy-config.js、atomic.js）

- **OWNER**=`'buddy-bridge-v1'`（sync.js:9）：bridge 写入的条目带 `buddyBridgeOwner: OWNER`；清理/合并只动 owned 条目，用户条目与对象其他字段（availableModels、任意 metadata）原样保留（bridge.test.js:50-71,223-234）。
- **mergeModels**（sync.js:20-41）：保留非 owned 条目；owned 条目按新目录重建；条目形状含 id/name/apiKey/url/availableModels 维护；手工 ID 冲突保留手工；无法识别的配置形状（如数组顶层的非对象）→「Unrecognized」拒，**绝不重置用户配置**（bridge.test.js:72-76）；能力字段从目录填：supportsToolCall/supportsImages/supportsReasoning/maxInputTokens（limit.input||limit.context）/maxOutputTokens（bridge.test.js:319-328,404-417）。
- **syncModels**（sync.js:43-66）：锁文件 `<file>.buddy-bridge.lock`，**5 分钟视为 stale** 可抢（bridge.test.js:56-59）；写入前备份 `<file>.buddy-bridge-<ts>.bak`；**写前 re-read** 检测外部修改，冲突则放弃本次（不覆盖）；空目录需 `{allowEmpty:true}`（退出清理用），否则「Empty model」拒（bridge.test.js:68-69）；无变化时 changed=false 不写（bridge.test.js:67）。备份计数验证：一次启动=清理 1+导入 1 共 2 个 .bak（lifecycle.test.js:44-45）。
- **validateModelsFile**（workbuddy-config.js:6-13）：数组、元素为对象、有 id（其余字段宽容）。**resolveModelsFile 优先级**（workbuddy-config.js:15-19）：`BUDDY_MODELS_FILE` env > settings.json 保存值 > `WORKBUDDY_CONFIG_DIR` > `WORKBUDDY_DATA_FOLDER_NAME` > `~/.workbuddy`；Windows 对发现路径要求文件已存在（requireExisting），另需处理 BOM；**发现逻辑从不创建文件、不回退到别的路径**（workbuddy-config.test.js 全部条目，第 10 章）。
- **原子写**（atomic.js）：临时文件 + rename；Windows 上 rename 遇 EPERM/EACCES/EBUSY 按 [50,100,200,400,800]ms 退避重试（atomic.test.js：重试序列与 6 次 EPERM 后成功、读者持锁不失败）；writeJson 供 status.json/settings.json/models.json 共用。
- **平台差异**：Windows 路径解析/占用的额外容错全在 atomic.js 重试与 requireExisting；macOS 无特殊分支（未确认有更多平台分支）。

---

## 7. 平台与代理（src/platform.js、system-proxy.js）

- **数据目录 dataDirectory**（platform.js:4-9）：macOS `~/Library/Application Support/…`、Windows `%APPDATA%/…`、Linux XDG 数据目录；运行时目录 `<dataDir>/managed`（candidates runtime.js:33-40）。具体常量字面量未逐字摘录——未确认（移植时抄 platform.js:4-9）。
- **runtimePackage**（platform.js:11-15）：`opencode-<platform>-<arch>` + `opencode[.exe]`。
- **候选路径**（platform.js 测试 platform.test.js：3 条，第 10 章）。
- **系统代理解析**（system-proxy.js:5-48）：
  - macOS：`scutil --proxy` 读 HTTP/HTTPS/SOCKS；**HTTPS 必须可用才采用**；SOCKS-only 拒绝（platform.test.js）。
  - Windows：注册表 `HKCU/software/microsoft/windows/currentversion/internet settings` 经 PowerShell 读取；ProxyServer 形如 `host:port`（无 scheme 视为 HTTP）或 `http=…;https=…` 分流；socks= 前缀拒绝（platform.test.js Windows 条目）。
  - 结果 `{ url, source }`（manual/scutil/registry/none），仅 HTTPS 代理被接受；开关状态持久化 settings.json（main.js:229-243）。

---

## 8. 桌面壳（desktop/main.cjs、preload.cjs、activity.cjs）

- **窗口**：1040×740，min 880×620，`sandbox:true` + `contextIsolation:true`（main.cjs:18-19）；单实例；关窗=隐藏到托盘（main.cjs:24），真正退出走 before-quit → stopService（20s 超时，Windows taskkill /T /F，POSIX SIGKILL）（main.cjs:131-137）。
- **服务管理**：壳负责 spawn 服务进程（node src/main.js，继承 env），**500ms 轮询 status.json** 且校验其中 pid 与已记录 pid 一致（main.cjs:56-61,111）；pid 不符/文件缺失视为服务丢失并重启。
- **托盘菜单全集**（main.cjs:45-54）：显示/隐藏面板、打开数据目录、（代理开关状态项）、退出。各项行为对应 show/hide、shell.openPath 数据目录、app.quit()。
- **IPC**（preload.cjs + main.cjs:63-75,151-156）：
  - `window.buddy.action(name, payload)`：白名单 6 个 action——`probe`、`refresh`、`import`、`setSystemProxy`、`openDataDir`、`quit`（main.cjs:63；与 /admin 路由一一对应，openDataDir/quit 为壳本地行为）。返回统一 `{ok:true,result}|{ok:false,error}`（main.cjs:151-156）。
  - `window.buddy.onState(cb)`：壳轮询 status.json 后推送 `{...state, actionBusy}`（main.cjs 载荷组装；renderer 侧消费）。
  - `window.buddy.onDismiss(cb)`：窗口 blur 时触发（main.cjs:25），renderer 收到后隐藏面板。
- **activity.cjs**：把服务活动事件转状态文案（含 content/heartbeat 区分、repair stage 不被 busy 覆盖、文案诚实性三组测试，activity.test.js）。
- **renderer 交互**见第 9 章。

---

## 9. 界面规格（desktop/index.html、style.css、renderer.js）

### 9.1 布局

- index.html（23 行）：左侧栏（品牌/服务状态行/指标卡）+ 右主区（模型列表）+ 底部反馈条；详情面板为模型行展开区；spinner 与徽章为行内元素。
- 侧栏固定宽 **218px**（style.css）；主区纵向滚动。

### 9.2 设计 token（style.css 已格式化提取；light/dark 成对）

| token | light | dark |
|---|---|---|
| --bg | #ffffff | #202422 |
| --panel | #f3f5f4 | #272d2a |
| --row | #f6f7f7 | #2a302d |
| --text | #202826 | #e3ebe6 |
| --muted | #78817e | #a0ada5 |
| --line | #e0e7e3 | #3b4540 |
| --green | #266e5c | #8cc9ad |
| --green-bg | #e8f1ed | #2a4035 |
| --orange | #bc752c | #e9ad70 |

- 徽章附加色：reasoning #815a99 / #eee8f3；images #477bad / #e7eff9（同为前/后景）。
- 圆角：7/8/9/6px（面板 8、按钮 7、徽章 9、输入 6 量级）。字体：14px，`-apple-system` 栈 + Microsoft YaHei。

### 9.3 组件清单

服务状态行（phase + pid/端口）、指标卡（模型数/请求数/耗时）、模型行（名称+徽章+状态）、徽章（可用·仅对话/检测超时/额度不足/请求受限/访问受限/reasoning/images）、详情面板（模型行展开，支持工具/图片/推理/上下文/输出上限）、反馈条（lastRequest 错误文案）、spinner（probe running）。
- 状态文案表（renderer.js，测试 bridge.test.js:743-745）：`timeout:'检测超时' quota:'额度不足' rate_limit:'请求受限' access:'访问受限'`，chatOnly 显示「可用 · 仅对话」。

### 9.4 交互细节（renderer.js）

- **模型排序**：rank(可用=0 < 等待/未知=1 < 不可用=2) 再 name localeCompare（renderer.js:14,26）。
- **渲染去重**：对整个渲染载荷算 JSON 签名，相同签名跳过重绘（renderer.js:21-22）。
- **折叠详情**：点击模型行切换展开；**Escape / 点击面板外部**关闭详情或隐藏面板（renderer.js:100-101，配合 onDismiss blur 时机）。
- 模型行徽章与详情字段由 modelResults + models 目录合成；actionBusy 期间禁用按钮。

---

## 10. 测试基线（test/*.test.js 逐文件验收清单）

> ⚠ **历史清单（2026-10-01 标注）**：本章清单属于**迁移前已归档的 JS 核心**（连同其 12 个测试文件整体移出仓库到 `/Users/wbtrex/website/localServer/node/trexwb/backup/wbBridge-node-20261001/`），仓库内已不可执行，保留原样作为行为覆盖的对照依据。**当前仓库的测试基线**是 Rust：`src-tauri/core/` 下 `cargo test` → **197 通过 / 0 失败**（lib 179 + `tests/js_parity.rs` 11 + `tests/red_lines.rs` 7），其中 `js_parity.rs` 以冻结在 `src-tauri/core/tests/fixtures/*.json` 的 JS 真相快照（**271 例 / 11 个 fixture 模块**）对拍，不需要 Node；详见 `docs/validation.md` 与 `docs/version/RELEASE-v1.0.md`。

**system-proxy.test.js（2 条）**
A1. macOS scutil 解析（HTTPS 必须，SOCKS-only 拒绝）。A2. Windows 注册表代理共享与 http/https 分流、socks 拒绝。

**atomic.test.js（3 条）**
B1. Windows 重试退避序列 [50,100,200,400,800]ms。B2. 连续 6 次 EPERM 后最终成功。B3. 读者持锁期间写入不失败（原子替换）。

**workbuddy-config.test.js（1 条大）**
C1. 发现优先级 env>saved>WORKBUDDY_CONFIG_DIR>WORKBUDDY_DATA_FOLDER_NAME>~/.workbuddy；BOM 容错；从不创建文件；找不到不回退别的路径；Windows requireExisting。

**platform.test.js（3 条）**
D1. dataDirectory 三平台路径。D2. runtimePackage 包名/二进制名。D3. Windows 代理共享与分流 + socks 拒绝（与 A2 呼应）。

**activity.test.js（2 条）**
E1. content 类活动 vs heartbeat 区分。E2. busy 不覆盖 repair stage；文案诚实性（不夸大状态）。

**repair.test.js（3 条）**
F1. 46K 修复材料完整携带（不截断对话）。F2. unrepairable 时把原错误交回原模型（不吞）。F3. blocked 动作的两轮处理（先拒后修）。

**runtime.test.js（8 条）**
G1. npm 查询失败回退复用本地（含外部候选拷贝进 managed，日志 using local）。G2. 本地≥latest 复用（只打 /latest 一次）。G3. 本地过期下载新版。G4. 代理 dispatcher 全程生效 + npmmirror 回退序列。G5. shim/坏版本候选报错不静默。G6. 首装解析 latest 并验证解包版本，mismatch 拒绝。G7. 非官方元数据（外部 tarball URL）与校验和不匹配拒绝且不落盘。G8. tarball 回退保留官方校验和，镜像坏字节拒绝。

**lifecycle.test.js（1 条端到端大）**
H1. 启动清旧 owned→探测→导入 2 条 .bak→translator 校验→格式失败 502 且不撤模型→manual probe/refresh 不写配置→代理开关不导入→import 增删→chatOnly 导入 supportsToolCall=false→切换 modelsFile 只清旧 owned→旧文件删除不阻塞再选择→shutdown 清理所选配置。

**bridge.test.js（72 条，编号关键项）**
I1 历史角色与 tool id 保留+图片声明拒。I2 tool_choice none/required/unlisted/参数容错校验。I3 SSE 结构化调用不泄漏 envelope+usage+[DONE]。I4 free 判定按价格。I5 sync 保留用户条目/元数据+stale 锁+备份。I6 手工冲突保留/坏配置不重置。I7 原生审批必须 ask+成功失败都清 session。I8 HTTP 鉴权/Origin/SSE/模型选择。I9 SSE 起始块时机与单 role。I10 无 callID 审批按名拒。I11 原生动作先于修正响应被拒。I12 错误分类五分。I13 撤空 owned 保留用户。I14 失败模型从 API 消失且缓存调用 400。I15 计时含成功与失败。I16 短 id 映射与 legacy 迁移。I17 目录名保留。I18 chatOnly 纯文本+拒工具。I19 导入填能力与限额。I20 reasoning variants 映射与导入。I21 固定推理模型 effort 走默认。I22 reasoning 到达 OpenCode（工具+纯聊）。I23 目录 limit input/context 分离驱动导入。I24 图片历史映射+system 指令+非法 URL 拒。I25 envelope 畸形一次格式修正。I26 单条 call 畸形同待遇。I27 修正有界（3 轮；native 1 轮）。I28 等价格式归一化+冒充拒绝。I29 外部执行失败保留为观察。I30 取消传播 abort+DELETE。I31 Write 缺参错误回模型。I32 请求 meta 不判能力。I33 meta 到记录器含 blocked。I34 取消不记成功。I35 shrinkPermission 截断。I36 上游重试事件上报。I37 探测判定树+超时名义。I38 每类目有面板文案。I39 仅格式不支持降级 chat-only。I40 bash/read handoff 映射+required 校验。I41 无等价按名拒。I42 handoff 短路（轮询恢复+abort）。I43 不可映射拒绝后请求仍完成。I44 审批 metadata 兜底参数。I45 无 tool part 的 bash 审批仍 handoff。I46 handoff 失败原因上报。I47 语义 miss 重试一次/格式不重试。I48 glob/grep/skill 映射+metadata patterns。I49 StructuredOutput 完成态 envelope。I50 空响应报「三者都为空」。I51 null 字段默认不误报。I52 翻译独立 session+校验。I53 越清单翻译弃用。I54 不可表达动作走翻译。I55 解析失败=格式问题非原生活动。I56 不可解析调用不报原生活动。I57 翻译材料含对话/工具/约定。I58 无翻译器显式重试带诊断。I59 探测不花额外轮（2 轮）。I60 截断压缩重发。I61 5 分钟静默不设代理层截止。I62 handoff/翻译遵守 tool_choice。I63 权限轮询瞬断恢复。I64 轮询不可用保留原始错误码。I65 权限事件在列表不可序列化时仍 handoff+清 pendingApprovals。I66 usage 含 cache/reasoning 不重复计数。I67 usage 缺失不报零。I68 handoff 返回会话用量（abort 后查询）。I69 用量回退事件流+完成后清除。I70 JSON 编码 calls 解码不弱化校验。I71 探测订阅工具事件并在结束后释放。

**shutdown.test.js（1 条，本仓新增）**
J1. `POST /admin/shutdown` 先回 `{ok:true}` 再触发恰好一次优雅退出（与信号走同一关停路径）；错误 Bearer 返回 401。

**watchdog.test.js（1 条，本仓新增）**
K1. `BUDDY_PARENT_PID` 指向的壳进程消失后，sidecar 自行优雅退出（exit 0）并落 `status.json` phase=stopped，不留守端口的孤儿进程（30s 超时保护）。

> 覆盖度：`activity` / `atomic` / `bridge` / `lifecycle` / `platform` / `repair` / `runtime` / `system-proxy` / `workbuddy-config` 9 个文件对照上游行为，合计 95 条；`shutdown` / `watchdog` 2 条为本仓库新增（上游无对应行为）。**11 个文件、97 条**（`npm test` 实测，node v24.21.0；此为 JS 核心归档前的实测值，当前仓库测试基线见本章开头注）。

---

## 11. 移植风险清单（Rust 重写，按风险从高到低）

1. **修复阶梯 + 轮次预算的状态机**（3.1/3.4）：3 轮=修正+翻译+显式重试、探测 2 轮、native 1 轮、截断不重试——轮次语义交织（I25-I27,I47,I58-I60），Rust 里最容易写成「简单重试 N 次」而丢失「每轮内容各不相同」的语义。
2. **handoff 短路与审批容错**（3.3）：250ms 轮询、事件流双通道、metadata 兜底、toolParts 缓存、永远 reject、tool_choice 约束、轮询异常不中断（I36,I40-I46,I62-I65）。异步竞态多，Rust 需要等价的 select/interval 结构。
3. **SSE 起始块/usage/id 复用规则**（4.3）：注释行先行、content 触发 role 块、id/created 复用、单 role、[DONE] 恰一次（I9,I3,I66-I67）。任何偏差直接破坏 OpenAI 客户端兼容。
4. **envelope 归一化与错误文案**（4.2/3.4）：等价格式接受集、错误消息内嵌字段类型（content=number）、中文文案（三者都为空/补齐诊断…）被测试正则锁定（I50-I51,I58），文案必须逐字保留否则验收失败。
5. **reasoning/variant 双字段入口**（4.4）：reasoning_effort 与 reasoning.effort、固定推理模型不发 variant、非法值拒绝清单（I20-I22）。映射表必须按目录动态构建。
6. **原子写与 Windows 重试**（第 6 章）：rename 退避序列与 stale 锁判定是数据安全边界（B1-B3, I5）；Rust 需自行实现等价重试，不可用「直接覆盖」。
7. **运行时下载校验链**（2.2-2.4）：官方校验和用于镜像字节、失败不落半成品、shim 拒绝、版本复验（G1-G8）。安全属性，逐条对应。
8. **退出清理顺序**（1.4）：sync([])→stop→等 probe→落盘→删锁的顺序保证不丢状态不留 owned 残留（H1, I13）。
9. **模型目录一致性**：失败模型即时下架 + 缓存调用 400 + 格式失败不撤模型（I14, lifecycle H1）三者的边界容易混。
10. **低风险**：桌面壳 IPC/status 轮询（第 8 章）在 Tauri 中由原生机制替代，行为可简化但需保留 action 白名单与 {ok,result}/{ok,error} 形状的兼容层。
