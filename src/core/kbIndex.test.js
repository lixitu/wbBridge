// src/core/kbIndex.js 的单测：node --test。
// 覆盖 AC-4（IndexedDB 不可用降级且不抛）、版本/差异重建、缓存复用与写失败降级。
// Node 默认没有 indexedDB，天然覆盖「不可用」分支；「可用」分支注入一个最小 fake。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { INDEX_VERSION } from './bm25.js'
import { KNOWLEDGE_VERSION, knowledgeDocuments } from './kb.js'
import { TOOLS } from './tools.js'
import { indexSummary, initIndex, searchKnowledge } from './kbIndex.js'

// ── 最小 fake indexedDB（只实现 store.js 用到的 open/transaction/get/put/clear）──
// 用 queueMicrotask 触发 onsuccess：store.js 在 open / get / put 之后才同步挂载回调，
// 微任务保证回调已就位。所有「可用」测试共享同一实例（store.js 的 dbPromise 是模块级缓存），
// 通过重置 state.data / failWrites 隔离各用例。
function createFakeIndexedDb() {
  const state = { data: new Map(), failWrites: false }
  const later = fn => queueMicrotask(fn)

  function request(result, error = null) {
    return { result, error, onsuccess: null, onerror: null, onblocked: null, onupgradeneeded: null }
  }

  const store = {
    get(key) {
      const req = request(state.data.get(key))
      later(() => req.onsuccess?.())
      return req
    },
    put(value, key) {
      const req = request(value, state.failWrites ? new Error('quota') : null)
      later(() => {
        if (state.failWrites) { req.onerror?.(); return }
        state.data.set(key, value)
        req.onsuccess?.()
      })
      return req
    },
    clear() {
      const req = request(undefined)
      later(() => { state.data.clear(); req.onsuccess?.() })
      return req
    },
  }

  const db = {
    objectStoreNames: { contains: () => true },
    createObjectStore: () => {},
    transaction: () => ({ objectStore: () => store }),
    onversionchange: null,
    close: () => {},
  }

  const factory = {
    open() {
      const req = request(db)
      later(() => req.onsuccess?.())
      return req
    },
  }

  return { factory, state }
}

const fake = createFakeIndexedDb()

function withIndexedDb(value, fn) {
  const had = 'indexedDB' in globalThis
  const previous = had ? globalThis.indexedDB : undefined
  if (value) globalThis.indexedDB = value
  else delete globalThis.indexedDB
  try {
    // 必须 await：fn 是异步的，若不等它跑完就恢复 indexedDB，
    // 后续 initIndex 会在「不可用」分支上误判。
    return Promise.resolve(fn()).finally(() => {
      if (had) globalThis.indexedDB = previous
      else delete globalThis.indexedDB
    })
  } catch (error) {
    if (had) globalThis.indexedDB = previous
    else delete globalThis.indexedDB
    throw error
  }
}

// 第一个用例必须先于任何「可用」用例执行：此刻 store.js 的 dbPromise 还是 null，
// 且本用例只走「不可用」早退分支，不会把它初始化到某个 fake 上。
test('IndexedDB 不可用：降级为内存索引、如实给出原因、不抛异常', async () => {
  await withIndexedDb(null, async () => {
    const outcome = await initIndex({})
    assert.equal(outcome.degraded, true)
    assert.match(outcome.reason, /未持久化/)
    assert.ok(outcome.index && outcome.index.N > 0, '内存索引必须可用')
    // 内存索引同样能检索（AC-4：助手仍可用）。
    const hits = searchKnowledge(outcome.index, '模型检测')
    assert.ok(hits.length > 0)
  })
})

test('无索引 / 空查询时检索老实返回空数组', () => {
  assert.deepEqual(searchKnowledge(null, '模型'), [])
  assert.deepEqual(searchKnowledge({ N: 0 }, '模型'), [])
  assert.deepEqual(searchKnowledge({ N: 1, docs: [], postings: {} }, ''), [])
})

test('首次初始化：成功落盘索引（单记录、版本与知识版本一致）', async () => {
  fake.state.data.clear()
  fake.state.failWrites = false
  await withIndexedDb(fake.factory, async () => {
    const outcome = await initIndex({})
    assert.equal(outcome.degraded, false)
    const stored = fake.state.data.get('bm25')
    assert.ok(stored, '索引必须写入 IndexedDB')
    assert.equal(stored.version, INDEX_VERSION)
    assert.equal(stored.knowledgeVersion, KNOWLEDGE_VERSION)
  })
})

test('知识版本一致且内容无变化：直接复用缓存，不重建', async () => {
  await withIndexedDb(fake.factory, async () => {
    const first = await initIndex({})
    const second = await initIndex({})
    assert.equal(second.degraded, false)
    // 复用路径直接返回缓存对象（同一引用）；重建路径会生成新对象。
    assert.equal(second.index, first.index)
  })
})

test('索引结构版本不符：触发重建并覆盖脏索引', async () => {
  await withIndexedDb(fake.factory, async () => {
    fake.state.data.set('bm25', { version: 999, docs: [], postings: {} })
    const outcome = await initIndex({})
    assert.equal(outcome.degraded, false)
    assert.equal(outcome.index.version, INDEX_VERSION)
    assert.equal(fake.state.data.get('bm25').version, INDEX_VERSION)
  })
})

test('知识版本升级：即使内容未变也重建（updatedAt 口径变化）', async () => {
  await withIndexedDb(fake.factory, async () => {
    const first = await initIndex({})
    assert.equal(first.index.knowledgeVersion, KNOWLEDGE_VERSION)
    // 模拟旧版落盘：内容指纹未变，但知识版本落后。
    fake.state.data.get('bm25').knowledgeVersion = '2020-01-01'
    const outcome = await initIndex({})
    assert.equal(outcome.index.knowledgeVersion, KNOWLEDGE_VERSION)
    assert.notEqual(outcome.index, first.index, '知识版本变化必须重建')
  })
})

test('写入失败：降级为会话内可用，reason 来自 storageError，不抛异常', async () => {
  fake.state.data.clear()
  fake.state.failWrites = true
  await withIndexedDb(fake.factory, async () => {
    const outcome = await initIndex({})
    assert.equal(outcome.degraded, true)
    assert.match(outcome.reason, /quota/)
    assert.ok(outcome.index && outcome.index.N > 0)
  })
  fake.state.failWrites = false
})

test('indexSummary 不暴露倒排表本体', async () => {
  fake.state.data.clear()
  await withIndexedDb(fake.factory, async () => {
    const { index } = await initIndex({})
    const summary = indexSummary(index)
    assert.ok(summary.docs > 0)
    assert.equal(JSON.stringify(summary).includes('postings'), false)
  })
})

// T-12 / F10：知识文档与工具注册表同源，新增工具无需手改 kb.js 即自动进索引。
test('知识文档与工具注册表一一对应：每个工具都有同源知识文档', () => {
  const docs = knowledgeDocuments(TOOLS)
  for (const tool of TOOLS) {
    assert.ok(
      docs.some(doc => doc.id === `kb.tool.${tool.id}`),
      `工具 ${tool.id} 必须有同源知识文档`,
    )
  }
  // tool 类文档 = 总览 1 条 + 每工具 1 条；索引 type 统计与此口径一致。
  assert.equal(docs.filter(doc => doc.type === 'tool').length, TOOLS.length + 1)
})
