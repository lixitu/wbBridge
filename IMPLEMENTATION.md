# wbBridge「AI 助手」功能深化 — 实现记录

> 依据：`artifacts/02-tasks.md`、`artifacts/03-architecture.md`。
> 本文记录本轮已完成任务、变更清单、验证方式与未验证项。代码已落盘到 `D:\nodejs\code\wbBridge`。

## 一、结论

- **P0 任务全部完成**：T-01 ~ T-06、T-07、T-08 ~ T-10、T-13、T-14。
- **P1 任务全部完成**：T-11（会话生命周期）、T-12（知识/工具同源，实现已存在，补齐回归用例）。
- **P2 不展开**：F11 会话历史持久化 / F12 模型通道手动选择 / F13 检索溯源展示 属后续 backlog，未实现。
- 本轮**零新增第三方依赖**，前端沿用 Vue 3 + `node --test`，壳沿用 `ureq`。

## 二、任务完成状态

| 任务 | 内容 | 状态 | 落点 |
|---|---|---|---|
| T-01 | 修复 `text.test.js` 指纹用例 | ✅ | `src/core/text.test.js` |
| T-02 | 修复 `bm25.test.js` 索引概况用例 | ✅ | `src/core/bm25.test.js` |
| T-03 | Rust 壳新增 `ai_chat` 命令 | ✅（静态核对） | `src-tauri/src/lib.rs` |
| T-04 | `bridge.js` 暴露 `aiChat(messages,{signal})` | ✅ | `src/core/bridge.js` |
| T-05 | 检索持久化接线 | ✅ | `src/core/kbIndex.js`（新） |
| T-06 | IndexedDB 降级与如实告知 | ✅ | `src/core/kbIndex.js` + `AssistantView.vue` |
| T-07 | 入口与视图注册 | ✅ | `prefs.js` / `SideBar.vue` / `App.vue` |
| T-08 | `AssistantView.vue` 骨架 + 对话流渲染 | ✅ | `src/views/AssistantView.vue`（新） |
| T-09 | 工具确认/取消弹窗 + 工具结果展示 | ✅ | `src/views/AssistantView.vue` |
| T-10 | 停止进行中的对话 | ✅ | `src/views/AssistantView.vue`（AbortSignal） |
| T-11 | 会话状态视图层持有 | ✅ | `App.vue` 持有 `assistantMessages`（状态提升） |
| T-12 | 知识文档与工具同源更新 | ✅（回归） | `kb.js` 已实现 + `kbIndex.test.js` 回归用例 |
| T-13 | 视图层/集成层测试补充 | ✅ | `kbIndex.test.js` / `assistant.flow.test.js`（新） |
| T-14 | 测试收口 + `IMPLEMENTATION.md` | ✅ | 本文档 |

## 三、变更清单

### 新增文件（4 个）

| 文件 | 职责 |
|---|---|
| `src/core/kbIndex.js` | 知识索引组合接线：`knowledgeDocuments` → `buildIndex` → `read/writeStoredIndex`；降级信号与检索封装。 |
| `src/views/AssistantView.vue` | 对话主视图：事件流渲染、确认弹窗、停止、清空、空模型池/降级提示。 |
| `src/core/kbIndex.test.js` | AC-4 降级、版本/差异重建、缓存复用、写失败降级、T-12 同源回归。 |
| `src/core/assistant.flow.test.js` | mock `chat` 覆盖闭环 / 确认取消 / 通道失败 / 停止（AC-6/7/8/10/11）。 |

### 修改文件（6 个）

| 文件 | 改动 |
|---|---|
| `src/core/text.test.js` | 指纹用例：尾随空格改为「同指纹」断言（T-01 口径）。 |
| `src/core/bm25.test.js` | `sampleDocs()` 三项补 `type:'knowledge'`（T-02 口径）。 |
| `src/core/prefs.js` | `VIEW_IDS` 末尾增 `'assistant'`。 |
| `src/core/bridge.js` | 新增 `aiChat(messages,{signal})`，形状对齐 `runTurn` 的 `chat` 契约。 |
| `src/views/SideBar.vue` | `ICONS` 增 `assistant` 图标；`groups` 末尾增「助手」分组。 |
| `src/App.vue` | 引入 `AssistantView`、`assistantMessages` 状态提升、模板接线。 |
| `src-tauri/src/lib.rs` | 新增 `AI_CHAT_TIMEOUT`、`chat_call`、`ai_chat` 命令，并注册进 `generate_handler!`。 |

> `capabilities/default.json` 的插件权限未改（应用自有 IPC 命令不经 capability 授权），仅按 05 审查结论补全描述里的命令枚举（加入 `ai_chat`，与 `lib.rs::generate_handler` 对齐）；`tools.js::NAVIGABLE_VIEWS` 未加 `assistant`（自指无意义，`tools.test.js` 已钉住 `nav.open('assistant')` 被拒）。

## 四、验证方式与结果

### 4.1 前端内核测试（统一 `node --test`）

```bash
node --test src/core/*.test.js
```

真实执行结果：**99 例全部通过，0 失败**（`# pass 99` / `# fail 0`）。

含 05 代码审查 5 项问题的回归用例（`review.regression.test.js`）：修复前 98 例中 6 例失败
（缺陷复现），修复后全部转绿并新增 1 例源码核对。

按文件：

```bash
node --test src/core/text.test.js
node --test src/core/bm25.test.js
node --test src/core/kbIndex.test.js
node --test src/core/assistant.flow.test.js
node --test src/core/review.regression.test.js
```

既有 `assistant.test.js` / `tools.test.js` / `prefs.test.js` / `ops.test.js` 回归不动，全绿。

### 4.2 静态检查与前端构建

依赖已安装（`npm install`），以下均实跑通过：

```bash
npm run lint        # eslint .：0 error / 0 warning
npm run vite:build  # vite v8.3.4：58 modules transformed，构建成功
```

审查发现的 2 处 `no-useless-assignment`（`assistant.js` / `store.js`）与 2 个缺失全局
（`structuredClone` / `queueMicrotask`）已修；`deploy/**`（压缩后的发布产物）已加入 ESLint ignores。

### 4.3 运行方式

```bash
npm install
npm run dev          # tauri dev（需本机 Rust 工具链）
npm run vite:dev     # 仅前端 Vite（部分 bridge 调用需壳）
npm run test:prefs   # 已有脚本
node --test src/core/*.test.js
```

## 五、未验证项（AC-14）

1. **Rust 命令仅静态核对**：本机无 `cargo`，`ai_chat` / `chat_call` 未执行 `cargo check` / `cargo test`。
   - 已严格复用 `admin_call` 的 `ureq` 调用与错误解析模式；`AUTO_MODEL_ID` 复用 `wbbridge_core::auto::AUTO_MODEL_ID` 单一真相。
   - 前端侧以 mock `chat`（形状即 `aiChat` 返回值）锁定编排契约，不虚构外部 API。
2. **GUI 交互未实机验证**：弹窗、流式渲染、真实 IPC 链路需 `npm run dev` + 真实核心联调（Vue 视图层不纳入 `node --test`，属既有约定）。
3. **前端构建/lint**：已在依赖安装后实跑通过（`npm run lint` 零告警 / `npm run vite:build` 构建成功，见 4.2），本项已闭环。

## 六、关键取舍与口径说明

- **T-01 口径**：`fingerprint` 内部先 `normalizeText`（含 trim），「归一化后同内容即同指纹」；
  「模型检测」与「模型检测 」尾随空格差异不算内容变化，与 `ＡＰＩ`/`api` 同指纹断言一致。
- **T-02 口径**：`describeIndex` 不泄 `postings`；`types` 统计与样例 `type` 一致（单一真相），
  样例补 `type:'knowledge'`，不改 `buildIndex` 默认 `type:'doc'` 的实现语义。
- **IPC 软取消（R1）**：`aiChat`/视图停止只中止 UI 等待与 `runTurn` 的 aborted 判定，
  壳内已发出的 `ureq` 请求无法从 JS 硬取消，副作用为最多一次已发请求。
- **会话不持久化（R4）**：本轮只保证视图切换不丢 + 手动清空，跨重启清空（P2 F11 backlog）。
