// BM25 检索：索引构建（纯函数）与相关性打分。
//
// 定位：这是 AI 助手「查得到依据」的能力底座，不是全文搜索引擎。文档来源是
// 应用自身的功能说明、工具定义、业务规则与运行态快照（见 kb.js），规模在百篇量级，
// 因此索引整体可序列化后直存 IndexedDB（见 store.js），无需倒排分片。
//
// 打分公式（标准 BM25，k1=1.2 / b=0.75）：
//     score(q, d) = Σ_t IDF(t) · tf(t,d)·(k1+1) / ( tf(t,d) + k1·(1 − b + b·|d|/avgdl) )
//     IDF(t) = ln(1 + (N − df(t) + 0.5) / (df(t) + 0.5))
// 采用 ln(1+…) 形式，保证 df 超过半数文档时 IDF 仍为正（不会出现负分项）。
//
// 字段加权用「词频重复」实现：标题计入 3 次、标签 2 次、正文 1 次。
// 这是 BM25 上最省事且不改变公式语义的字段加权近似（等价于把标题拼三遍）。
// 代价是文档长度统计包含重复项——对所有文档一视同仁，排序仍是自洽的。
//
// 本模块是纯函数：不碰 IndexedDB、不碰 DOM，可在 node --test 下直接验证。
import { fingerprint, tokenize } from './text.js'

export const INDEX_VERSION = 1
export const BM25_K1 = 1.2
export const BM25_B = 0.75
export const TITLE_BOOST = 3
export const TAG_BOOST = 2
// 单字段长度上限：异常超长文档（例如误把日志整篇塞进来）不应把 avgdl 拉飞。
export const MAX_FIELD_CHARS = 8000

function clip(value, limit = MAX_FIELD_CHARS) {
  if (typeof value !== 'string') return ''
  return value.length > limit ? value.slice(0, limit) : value
}

// 文档 → 词项序列（含字段加权重复）。title/tags/body 三种来源都可缺失。
export function tokenizeDocument(doc) {
  const tokens = []
  const title = clip(doc?.title, 200)
  const tags = clip(doc?.tags, 200)
  const body = clip(doc?.body)
  for (let i = 0; i < TITLE_BOOST; i += 1) tokens.push(...tokenize(title))
  for (let i = 0; i < TAG_BOOST; i += 1) tokens.push(...tokenize(tags))
  tokens.push(...tokenize(body))
  return tokens
}

// 构建索引。入参为文档数组，每项至少要有稳定且唯一的 `id`；
// 缺 id 或重复 id 的文档会被跳过（而不是让索引进入「同一 id 两条记录」的坏状态）。
//
// 返回结构刻意只用普通对象 / 数组（无 Map / Set / 函数），
// 这样 Structured Clone 能把它原样写进 IndexedDB、也能在测试里深比较。
export function buildIndex(documents, options = {}) {
  const now = typeof options.now === 'number' ? options.now : Date.now()
  const list = Array.isArray(documents) ? documents : []
  const docs = []
  const docById = Object.create(null)
  const df = Object.create(null)
  const postings = Object.create(null)
  let totalLength = 0
  let skipped = 0

  for (const raw of list) {
    if (!raw || typeof raw.id !== 'string' || !raw.id) {
      skipped += 1
      continue
    }
    if (docById[raw.id] !== undefined) {
      skipped += 1
      continue
    }
    const tokens = tokenizeDocument(raw)
    const index = docs.length
    docById[raw.id] = index
    totalLength += tokens.length
    docs.push({
      id: raw.id,
      title: typeof raw.title === 'string' ? raw.title : '',
      body: typeof raw.body === 'string' ? raw.body : '',
      source: typeof raw.source === 'string' ? raw.source : '',
      type: typeof raw.type === 'string' ? raw.type : 'doc',
      perm: typeof raw.perm === 'string' ? raw.perm : 'public',
      updatedAt: typeof raw.updatedAt === 'string' ? raw.updatedAt : '',
      len: tokens.length,
      fingerprint: typeof raw.fingerprint === 'string'
        ? raw.fingerprint
        : fingerprint(`${raw.title || ''}\u0000${raw.tags || ''}\u0000${raw.body || ''}`),
    })
    const counts = Object.create(null)
    for (const token of tokens) counts[token] = (counts[token] || 0) + 1
    for (const [term, tf] of Object.entries(counts)) {
      if (!postings[term]) {
        postings[term] = Object.create(null)
        df[term] = 0
      }
      postings[term][raw.id] = tf
      df[term] += 1
    }
  }

  const N = docs.length
  return {
    version: typeof options.version === 'number' ? options.version : INDEX_VERSION,
    builtAt: new Date(now).toISOString(),
    N,
    avgdl: N ? totalLength / N : 0,
    skipped,
    df,
    postings,
    docById,
    docs,
  }
}

// 单文档打分：抽出文档侧与查询无关的部分，避免在候选循环里重复计算。
function scoreTerm(index, tf, docLength, idf) {
  const norm = BM25_K1 * (1 - BM25_B + BM25_B * (docLength / (index.avgdl || 1)))
  return idf * ((tf * (BM25_K1 + 1)) / (tf + norm))
}

// 检索。`perm` 是对调用方可见的权限标签：文档 perm 与之不符则不参与打分
// （索引里保留全部文档，过滤发生在查询期，这样权限口径变化不必重建索引）。
export function search(index, query, options = {}) {
  const limit = Number.isInteger(options.limit) && options.limit > 0 ? options.limit : 5
  const minScore = Number.isFinite(options.minScore) ? options.minScore : 0
  const perm = typeof options.perm === 'string' ? options.perm : ''
  if (!index || !index.N || !Array.isArray(index.docs) || !index.postings) return []

  // 查询词项去重：重复出现的同一个词不该让分数线性叠加（BM25 的查询模型是词项集合）。
  const terms = [...new Set(tokenize(query))]
  if (!terms.length) return []

  const scores = new Map()
  for (const term of terms) {
    const posting = index.postings[term]
    if (!posting) continue
    const df = Number(index.df?.[term]) || Object.keys(posting).length
    const idf = Math.log(1 + (index.N - df + 0.5) / (df + 0.5))
    if (!(idf > 0)) continue
    for (const [docId, tf] of Object.entries(posting)) {
      const docIndex = index.docById?.[docId]
      const doc = Number.isInteger(docIndex) ? index.docs[docIndex] : null
      if (!doc) continue
      if (perm && doc.perm !== perm) continue
      const delta = scoreTerm(index, tf, doc.len, idf)
      const current = scores.get(docId)
      if (current) current.score += delta
      else scores.set(docId, { doc, score: delta })
    }
  }

  return [...scores.values()]
    .filter(hit => hit.score > minScore)
    // 同分时按 id 升序，保证同一查询的结果顺序稳定（便于测试与结果对比）。
    .sort((a, b) => (b.score - a.score) || (a.doc.id < b.doc.id ? -1 : 1))
    .slice(0, limit)
    .map(hit => ({
      id: hit.doc.id,
      score: Math.round(hit.score * 1000) / 1000,
      title: hit.doc.title,
      body: hit.doc.body,
      source: hit.doc.source,
      type: hit.doc.type,
      perm: hit.doc.perm,
      updatedAt: hit.doc.updatedAt,
    }))
}

// 增量更新的判定依据：返回「内容变了 / 是新增」的文档与「已失效」的 id。
// 索引按整份重建（百篇量级重建成本可忽略），但**跳过未变化文档**的重新分词，
// 因此这里返回的是可复用的旧文档统计（len / fingerprint）。
export function diffDocuments(index, documents) {
  const next = Array.isArray(documents) ? documents : []
  const known = new Map()
  for (const doc of (index?.docs || [])) known.set(doc.id, doc)
  const seen = new Set()
  const added = []
  const changed = []
  const unchanged = []
  for (const doc of next) {
    if (!doc || typeof doc.id !== 'string' || !doc.id) continue
    seen.add(doc.id)
    const fp = fingerprint(`${doc.title || ''}\u0000${doc.tags || ''}\u0000${doc.body || ''}`)
    const prev = known.get(doc.id)
    if (!prev) added.push(doc.id)
    else if (prev.fingerprint !== fp) changed.push(doc.id)
    else unchanged.push(doc.id)
  }
  const removed = [...known.keys()].filter(id => !seen.has(id))
  return { added, changed, unchanged, removed, total: seen.size }
}

// 供界面展示的索引概况（不暴露 postings / df 这类大对象）。
export function describeIndex(index) {
  if (!index || !index.N) return { docs: 0, terms: 0, avgdl: 0, builtAt: '', version: 0, types: {} }
  const types = {}
  for (const doc of index.docs) types[doc.type] = (types[doc.type] || 0) + 1
  return {
    docs: index.N,
    terms: Object.keys(index.postings || {}).length,
    avgdl: Math.round((index.avgdl || 0) * 10) / 10,
    builtAt: index.builtAt || '',
    version: index.version || 0,
    types,
  }
}
