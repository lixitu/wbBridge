// src/core/bm25.js 的单测：node --test。
// 覆盖：索引统计是否正确、中文查询能否命中正确文档、字段加权与排序、
// 权限过滤、增量差异判定，以及「无结果」时是否老实返回空数组。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { INDEX_VERSION, buildIndex, describeIndex, diffDocuments, search } from './bm25.js'

function sampleDocs() {
  return [
    { id: 'kb.detect', title: '模型检测说明', body: '检测会消耗少量免费额度，耗时数十秒。', tags: '检测', type: 'knowledge', source: '应用知识库' },
    { id: 'kb.logs', title: '运行日志说明', body: '日志位于应用数据目录，可在日志视图查看。', tags: '日志', type: 'knowledge', source: '应用知识库' },
    { id: 'kb.provider', title: '平台配置说明', body: '平台 Key 由用户手动填写，模型名单随之更新。', tags: '平台', type: 'knowledge', source: '应用知识库' },
  ]
}

test('索引统计：文档数、平均长度、倒排表与文档频次自洽', () => {
  const index = buildIndex(sampleDocs())
  assert.equal(index.version, INDEX_VERSION)
  assert.equal(index.N, 3)
  assert.equal(index.docs.length, 3)
  assert.ok(index.avgdl > 0)
  // 每个词项的 df 必须等于它在 postings 里出现的文档数（倒排与统计不能脱节）。
  for (const [term, df] of Object.entries(index.df)) {
    assert.equal(df, Object.keys(index.postings[term]).length, `词项 ${term} 的 df 与倒排不一致`)
  }
  // 标题加权：标题里的词项词频应高于只出现在正文里的词项。
  assert.ok(index.postings['检测']['kb.detect'] >= 3)
})

test('非法文档被跳过，而不是让索引进入脏状态', () => {
  const index = buildIndex([
    { id: 'ok', title: '正常文档', body: '内容' },
    { title: '缺 id' },
    { id: '' , title: '空 id' },
    { id: 'ok', title: '重复 id', body: '另一份内容' },
  ])
  assert.equal(index.N, 1)
  assert.equal(index.skipped, 3)
  assert.equal(index.docs[0].title, '正常文档')
})

test('中文查询命中正确文档并按相关性排序', () => {
  const index = buildIndex(sampleDocs())
  const hits = search(index, '模型检测', { limit: 5 })
  assert.ok(hits.length > 0)
  assert.equal(hits[0].id, 'kb.detect')
  // 分数必须单调不增。
  for (let i = 1; i < hits.length; i += 1) assert.ok(hits[i - 1].score >= hits[i].score)
})

test('标题命中优先于正文命中（字段加权生效）', () => {
  const index = buildIndex([
    { id: 'title-hit', title: '系统代理', body: '无关正文' },
    { id: 'body-hit', title: '无关标题', body: '这里提到一次系统代理' },
  ])
  const hits = search(index, '系统代理')
  assert.equal(hits[0].id, 'title-hit')
})

test('查询词项去重：同一个词重复出现不会把分数叠上去', () => {
  const index = buildIndex(sampleDocs())
  const once = search(index, '模型', { limit: 5 })
  const twice = search(index, '模型 模型', { limit: 5 })
  assert.equal(once.length, twice.length)
  assert.equal(once[0].score, twice[0].score)
})

test('limit 与权限过滤都在查询期生效', () => {
  const index = buildIndex([
    ...sampleDocs(),
    { id: 'kb.secret', title: '内部模型说明', body: '仅管理员可见的模型信息', perm: 'admin' },
  ])
  const limited = search(index, '模型', { limit: 1 })
  assert.equal(limited.length, 1)
  // perm=public 时不得返回 admin 文档。
  const publicHits = search(index, '模型', { limit: 10, perm: 'public' })
  assert.equal(publicHits.some(hit => hit.id === 'kb.secret'), false)
  // 索引里仍然保留该文档（过滤发生在查询期，口径变化不必重建索引）。
  assert.equal(index.N, 4)
})

test('没有命中时返回空数组，不编造结果', () => {
  const index = buildIndex(sampleDocs())
  assert.deepEqual(search(index, '区块链挖矿'), [])
  assert.deepEqual(search(index, ''), [])
  assert.deepEqual(search(null, '模型'), [])
})

test('增量差异：新增 / 修改 / 未变 / 删除四类判定', () => {
  const index = buildIndex(sampleDocs())
  const changed = sampleDocs()
  changed[0].body = '检测会消耗额度，这个说明已经被改写。'
  const next = [...changed.slice(0, 2), { id: 'kb.new', title: '新文档', body: '新增内容' }]
  const diff = diffDocuments(index, next)
  assert.deepEqual(diff.added, ['kb.new'])
  assert.deepEqual(diff.changed, ['kb.detect'])
  assert.deepEqual(diff.unchanged, ['kb.logs'])
  assert.deepEqual(diff.removed, ['kb.provider'])
  assert.equal(diff.total, 3)
})

test('索引概况不暴露倒排表本体', () => {
  const info = describeIndex(buildIndex(sampleDocs()))
  assert.equal(info.docs, 3)
  assert.ok(info.terms > 0)
  assert.equal(info.version, INDEX_VERSION)
  assert.deepEqual(Object.keys(info.types), ['knowledge'])
  assert.equal(JSON.stringify(info).includes('postings'), false)
})

test('索引可被结构化克隆（IndexedDB 写入的前提）', () => {
  const index = buildIndex(sampleDocs())
  const clone = structuredClone(index)
  assert.equal(clone.N, index.N)
  assert.deepEqual(search(clone, '运行日志')[0].id, 'kb.logs')
})
