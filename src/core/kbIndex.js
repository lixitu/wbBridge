// 知识索引的组合接线层：把「知识文档（kb.js）→ 索引（bm25.js）→ 持久化（store.js）」
// 串成一条启动即可用的链路。视图与 assistant.js 都不直接触碰三者的组合逻辑，只依赖本模块。
//
// 纪律（继承 store.js 的「索引是加速器，不是功能主干」）：
// • IndexedDB 不可用 / 读写失败时**一律降级为内存索引重建**，绝不把存储异常冒到界面；
// • 降级不是静默失败：返回 `degraded` 与可读 `reason`，由视图顶部常驻提示（AC-4 / AC-8）；
// • 版本与内容差异判定沿用 bm25.js / kb.js 的既有语义，本模块不另起一套真相。
import { TOOLS } from './tools.js'
import { KNOWLEDGE_VERSION, knowledgeDocuments } from './kb.js'
import { INDEX_VERSION, buildIndex, describeIndex, diffDocuments, search } from './bm25.js'
import { indexedDbAvailable, readStoredIndex, storageError, writeStoredIndex } from './store.js'
import { MAX_RETRIEVED } from './assistant.js'

// IndexedDB 不可用时的固定降级文案：与「会话内可用」的承诺一致。
const DEGRADED_NO_IDB = 'IndexedDB 不可用，知识检索未持久化，本次会话内可用'

function build(docs, now) {
  const index = buildIndex(docs, { now })
  // 给索引附加知识版本：bm25.js 只按 title/tags/body 判内容变化，覆盖不了 kb.js 的
  // 人工维护日期（updatedAt）变化；这里额外记录一次，用于「纯内容不变但知识升级」的边界。
  index.knowledgeVersion = KNOWLEDGE_VERSION
  return index
}

// 初始化索引：返回 `{ index, degraded, reason }`。任何一条路径都不抛异常。
//   index    为 null 表示连内存索引都建不出来（例如文档集为空但 buildIndex 仍会成功，
//            因此正常情况不会为 null；保留 null 分支给调用方兜底）。
//   degraded 为 true 表示本轮用的是「未持久化」的内存索引，UI 需要如实告知用户。
//   reason   仅在 degraded 时非空，给用户看的可读原因。
export async function initIndex({ tools = TOOLS, now } = {}) {
  const docs = knowledgeDocuments(tools)

  // 1) IndexedDB 不可用（隐私模式 / 配额 / 环境不支持）：直接内存重建，不尝试任何读写。
  if (!indexedDbAvailable()) {
    return { index: build(docs, now), degraded: true, reason: DEGRADED_NO_IDB }
  }

  // 2) 没有可用缓存（缺失 / 损坏 / 索引结构版本不符）：重建并尝试落盘。
  const cached = await readStoredIndex(INDEX_VERSION)
  if (!cached) {
    const index = build(docs, now)
    return finalize(index)
  }

  // 3) 知识版本升级：即使文档内容逐字未变也要重建（updatedAt 等口径变了）。
  if (cached.knowledgeVersion !== KNOWLEDGE_VERSION) {
    const index = build(docs, now)
    return finalize(index)
  }

  // 4) 知识版本一致：按内容指纹判增删改；无变化则直接复用缓存（秒级恢复的关键路径）。
  const diff = diffDocuments(cached, docs)
  const changed = diff.added.length || diff.changed.length || diff.removed.length
  if (changed) {
    const index = build(docs, now)
    return finalize(index)
  }

  return { index: cached, degraded: false, reason: null }
}

// 重建后的收尾：优先落盘；落盘失败则降级为「会话内可用」，但内存索引照常返回。
async function finalize(index) {
  const saved = await writeStoredIndex(index)
  if (!saved) {
    return { index, degraded: true, reason: storageError() || '知识索引写入失败，本次会话内可用' }
  }
  return { index, degraded: false, reason: null }
}

// 检索：包一层 bm25.search，权限固定 public、默认条数复用 assistant.js 的 MAX_RETRIEVED。
// 无索引 / 空查询 / 无命中都返回空数组（不编造结果）。
export function searchKnowledge(index, query, { limit = MAX_RETRIEVED } = {}) {
  if (!index || typeof query !== 'string' || !query.trim()) return []
  return search(index, query, { perm: 'public', limit })
}

// 索引概况：只用于调试 / 展示，不暴露 postings / df 大对象（见 bm25.js::describeIndex）。
export function indexSummary(index) {
  return describeIndex(index)
}
