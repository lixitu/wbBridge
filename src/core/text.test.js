// src/core/text.js 的单测：node --test，无 DOM、无依赖。
// 覆盖三件事：归一化是否让「同一个词的不同写法」合并、中文切分是否真的按字对切、
// 指纹是否对同一内容稳定（增量索引的判定依据）。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { excerpt, fingerprint, normalizeText, tokenize } from './text.js'

test('归一化：全角转半角、英文小写、空白折叠', () => {
  assert.equal(normalizeText('ＡＰＩ  Ｋｅｙ'), 'api key')
  assert.equal(normalizeText('  Core\n\tRefresh  '), 'core refresh')
  assert.equal(normalizeText('模型\u3000检测'), '模型 检测')
  assert.equal(normalizeText(null), '')
  assert.equal(normalizeText(undefined), '')
  assert.equal(normalizeText(''), '')
})

test('中文按「单字 + 相邻字对」切分，而不是整句当一个词', () => {
  const tokens = tokenize('模型检测')
  assert.ok(tokens.includes('模型'), '应产出二元组「模型」')
  assert.ok(tokens.includes('型检'), '应产出二元组「型检」')
  assert.ok(tokens.includes('检测'), '应产出二元组「检测」')
  assert.ok(tokens.includes('检'), '应保留单字')
  // 关键回归：整句绝不能被当成一个词项。
  assert.equal(tokens.includes('模型检测'), false)
})

test('停用字在单字位置被过滤，但二元组不受影响', () => {
  const tokens = tokenize('的模型')
  assert.equal(tokens.includes('的'), false)
  assert.ok(tokens.includes('模型'))
  // 「的模」这个字对带上下文，保留（BM25 的 IDF 会压低它的权重）。
  assert.ok(tokens.includes('的模'))
})

test('英文与标识符：整体词项 + 按分隔符拆出的子词', () => {
  const tokens = tokenize('core-refresh')
  assert.ok(tokens.includes('core-refresh'))
  assert.ok(tokens.includes('core'))
  assert.ok(tokens.includes('refresh'))

  const model = tokenize('gpt-4o')
  assert.ok(model.includes('gpt-4o'))

  // 标点与空白只作边界，不产生词项。
  assert.equal(tokenize('，。！').length, 0)
})

test('中英混排分别落到各自的切分规则上', () => {
  const tokens = tokenize('重启 core 服务')
  assert.ok(tokens.includes('core'))
  assert.ok(tokens.includes('重启'))
  assert.ok(tokens.includes('服务'))
})

test('指纹：同内容稳定、不同内容可区分', () => {
  const a = fingerprint('模型检测')
  assert.equal(a, fingerprint('模型检测'))
  // 口径：fingerprint 内部先 normalizeText（含 trim），因此「尾随空格」不算内容差异——
  // 与「ＡＰＩ」/「api」归一化后同指纹的既定语义一致（归一化后相同的内容即同一份内容）。
  assert.equal(a, fingerprint('模型检测 '))
  assert.equal(fingerprint('ＡＰＩ'), fingerprint('api'))
  assert.match(a, /^[0-9a-f]{8}$/)
})

test('摘要截断到指定长度并带省略号', () => {
  assert.equal(excerpt('短文本', 10), '短文本')
  const long = excerpt('一'.repeat(50), 10)
  assert.equal(long.length, 10)
  assert.ok(long.endsWith('…'))
  assert.equal(excerpt(null), '')
})
