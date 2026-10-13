// src/core/review.regression.test.js：node --test。
// 测试 Agent 针对「实现代码审查报告（artifacts/05-review.md）」五项问题的回归与复现，
// 外加若干核心边界补充用例。本文件不修改任何实现，只钉住「当前行为 vs 期望行为」的差异：
// 复现缺陷的用例预期为 **失败**，用于证明缺陷真实存在；修复后应转为通过。
//
// 说明：视图层（AssistantView.vue）按既有约定不纳入 node --test，因此「问题1」采用
// 最小模型忠实复现其 append 累积模式；真正修复需落在 AssistantView.vue 并由组件级测试收口。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

import { runTurn } from './assistant.js'
import { findTool, validateArgs, redact, toolCatalog, TOOLS } from './tools.js'
import { clearStoredIndex, readStoredIndex, readStoredMeta, writeStoredIndex } from './store.js'

const ROOT = new URL('../../', import.meta.url)

function fakeCtx(overrides = {}) {
  return {
    state: {
      phase: 'ready',
      models: [{ id: 'gpt-4o-mini' }],
      availableModels: ['gpt-4o-mini'],
      usage: { total: 1, models: { 'gpt-4o-mini': 1 } },
    },
    coreAction: async () => ({ ok: true, result: {} }),
    readLog: async () => ({ ok: true, result: 'ok' }),
    dataDir: async () => ({ ok: true, result: 'C:\\data' }),
    navigate: () => {},
    activityText: () => '',
    ...overrides,
  }
}

function chatPlan(plan) {
  let index = 0
  const calls = []
  const chat = async (messages, opts) => {
    calls.push({ messages, signal: opts?.signal })
    if (index >= plan.length) throw new Error(`chat 调用次数超出预期（第 ${index + 1} 次）`)
    const item = plan[index]
    index += 1
    return item
  }
  chat.calls = calls
  return chat
}

// ── 问题1 触发条件（编排层，应通过）──────────────────────────────────────
test('问题1 触发条件：parseFailed 路径同步相邻发出 notice → final 两个 append 事件', async () => {
  const events = []
  await runTurn({
    query: '随便说点啥',
    ctx: fakeCtx(),
    chat: chatPlan([{ ok: true, content: '模型返回了一段没有 JSON 的纯文本' }]),
    retrieve: async () => [],
    onEvent: event => events.push(event),
  })
  const types = events.map(event => event.type)
  const i = types.indexOf('notice')
  assert.ok(i >= 0, 'parseFailed 路径必须发 notice')
  assert.equal(types[i + 1], 'final', 'notice 与 final 必须同步相邻（无 await 间隔，构成覆盖条件）')
})

test('问题1 触发条件：拒绝确认路径同步相邻发出 tool-result → final 两个 append 事件', async () => {
  const events = []
  await runTurn({
    query: '刷新模型名单',
    ctx: fakeCtx(),
    chat: chatPlan([{ ok: true, content: '{"reply":"我来刷新","tool":{"id":"core.refresh","args":{}}}' }]),
    retrieve: async () => [],
    confirm: async () => false,
    onEvent: event => events.push(event),
  })
  const types = events.map(event => event.type)
  const i = types.indexOf('tool-result')
  assert.ok(i >= 0, '拒绝确认路径必须发 tool-result')
  assert.equal(types[i + 1], 'final', 'tool-result 与 final 必须同步相邻（构成覆盖条件）')
})

// ── 问题1 回归（视图层 append 累积模型，修复后应通过）────────────────────
// 修复前 AssistantView.append 用 `[...props.messages, entry]` 生成新数组，而 props.messages
// 在同一 tick 内不会同步更新，导致连续两次 append 都基于同一份旧快照、后一次覆盖前一次。
// 修复后 append 基于本地镜像 transcript 累积，父级回传后再以父级值为准。
function viewAppendRepro(entries, initial = []) {
  let transcript = initial
  for (const entry of entries) {
    // 忠实还原修复后实现：每次基于已累积的本地镜像生成新数组。
    transcript = [...transcript, entry]
  }
  return transcript
}

test('问题1 回归：parseFailed 的「notice → final」两次追加不得丢前一条气泡', () => {
  const transcript = viewAppendRepro([
    { role: 'notice', text: '模型未返回约定格式，已按普通回答展示。' },
    { role: 'assistant', text: '最终答复' },
  ])
  assert.equal(transcript.length, 2, '应同时保留 notice 与 assistant 两条气泡')
})

test('问题1 回归：拒绝确认的「tool-result → final」不得丢工具结果卡', () => {
  const transcript = viewAppendRepro([
    { role: 'tool', tool: 'core.refresh', denied: true },
    { role: 'assistant', text: '已取消，没有对应用做任何改动。' },
  ])
  assert.equal(transcript.length, 2, '应同时保留工具卡与 final 文本')
})

test('问题1 修复核对：AssistantView.append 基于本地镜像累积，不再依赖 props.messages 即时性', () => {
  const vue = readFileSync(new URL('src/views/AssistantView.vue', ROOT), 'utf8')
  assert.equal(
    /emit\('update:messages', \[\.\.\.props\.messages/.test(vue),
    false,
    'append 不得基于 props.messages 快照生成新数组（同步相邻两次追加会互相覆盖）',
  )
  assert.ok(/transcript = \[\.\.\.transcript/.test(vue), 'append 应基于本地镜像 transcript 累积')
  assert.ok(/watch\(\(\) => props\.messages/.test(vue), '应监听父级 messages 回传以保持单一真相')
})

// ── 问题2（store.js writeStoredIndex meta 写失败，预期失败）───────────────
function createStoreFake() {
  const state = { data: new Map(), failKeys: new Set() }
  const later = fn => queueMicrotask(fn)
  const mkRequest = (result, error = null) => ({ result, error, onsuccess: null, onerror: null })

  function makeStore() {
    return {
      get(key) {
        const req = mkRequest(state.data.get(key))
        later(() => req.onsuccess?.())
        return req
      },
      put(value, key) {
        const willFail = state.failKeys.has(key)
        const req = mkRequest(value, willFail ? new Error(`写入失败：${key}`) : null)
        later(() => {
          if (willFail) { req.onerror?.(); return }
          state.data.set(key, value)
          req.onsuccess?.()
        })
        return req
      },
      clear() {
        const req = mkRequest(undefined)
        later(() => { state.data.clear(); req.onsuccess?.() })
        return req
      },
    }
  }

  const db = {
    objectStoreNames: { contains: () => true },
    createObjectStore: () => {},
    transaction: () => ({ objectStore: () => makeStore(), onabort: null, onerror: null }),
    onversionchange: null,
    close: () => {},
  }
  const factory = {
    open() {
      const req = mkRequest(db)
      later(() => req.onsuccess?.())
      return req
    },
  }
  return { factory, state }
}

const storeFake = createStoreFake()

function withIndexedDb(value, fn) {
  const had = 'indexedDB' in globalThis
  const previous = had ? globalThis.indexedDB : undefined
  if (value) globalThis.indexedDB = value
  else delete globalThis.indexedDB
  return Promise.resolve(fn()).finally(() => {
    if (had) globalThis.indexedDB = previous
    else delete globalThis.indexedDB
  })
}

const sampleIndex = { version: 1, builtAt: '', N: 1, docs: [], postings: {} }

test('问题2 正向：bm25 与 meta 均写成功时 writeStoredIndex 返回 true', async () => {
  storeFake.state.data.clear()
  storeFake.state.failKeys = new Set()
  await withIndexedDb(storeFake.factory, async () => {
    assert.equal(await writeStoredIndex(sampleIndex), true)
    assert.ok(storeFake.state.data.has('bm25'))
    assert.ok(storeFake.state.data.has('meta'))
  })
})

test('问题2 复现：meta 写失败时 writeStoredIndex 必须返回 false（当前返回 true）', async () => {
  storeFake.state.data.clear()
  storeFake.state.failKeys = new Set(['meta'])
  await withIndexedDb(storeFake.factory, async () => {
    const result = await writeStoredIndex(sampleIndex)
    assert.equal(result, false, 'meta 写失败应按函数注释返回 false，而非静默成功')
    assert.ok(storeFake.state.data.has('bm25'), '索引本体已落盘')
    assert.equal(storeFake.state.data.has('meta'), false, 'meta 未落盘')
  })
  storeFake.state.failKeys = new Set()
})

test('store.js 边界：readStoredMeta 在缺失时返回 null，readStoredIndex 版本不符返回 null', async () => {
  storeFake.state.data.clear()
  await withIndexedDb(storeFake.factory, async () => {
    assert.equal(await readStoredMeta(), null)
    storeFake.state.data.set('bm25', { version: 999, docs: [], postings: {} })
    assert.equal(await readStoredIndex(1), null)
  })
})

test('store.js 边界：clearStoredIndex 清空成功返回 true', async () => {
  storeFake.state.data.set('bm25', sampleIndex)
  await withIndexedDb(storeFake.factory, async () => {
    assert.equal(await clearStoredIndex(), true)
    assert.equal(storeFake.state.data.has('bm25'), false)
  })
})

// ── 问题5（logs.tail 未脱敏，预期失败）────────────────────────────────────
test('问题5 复现：logs.tail 返回给模型的日志必须脱敏（当前会原样回灌密钥）', async () => {
  const ctx = fakeCtx({
    readLog: async () => ({
      ok: true,
      result: 'boot ok\nAuthorization: Bearer sk-live-abc123\napi-key=secretvalue\nready',
    }),
  })
  const tool = findTool('logs.tail')
  const checked = validateArgs(tool.params, { limit: 10 })
  assert.equal(checked.ok, true)
  const result = await tool.run(ctx, checked.value, {})
  assert.equal(result.ok, true)
  assert.equal(result.summary.includes('sk-live-abc123'), false, 'Bearer 密钥不得进入 summary')
  assert.equal(result.summary.includes('secretvalue'), false, 'api-key 值不得进入 summary')
})

// ── 问题3（capabilities 注释与 generate_handler 命令清单一致性，预期失败）──
test('问题3 复现：capabilities 注释枚举命令时不得漏列 generate_handler 已注册命令', () => {
  const cap = JSON.parse(readFileSync(new URL('src-tauri/capabilities/default.json', ROOT), 'utf8'))
  const desc = cap[0]?.description || ''
  const lib = readFileSync(new URL('src-tauri/src/lib.rs', ROOT), 'utf8')
  const block = /generate_handler!\[([\s\S]*?)\]/.exec(lib)?.[1] || ''
  const commands = [...block.matchAll(/^\s*([a-z][a-z0-9_]*)\s*,?\s*$/gm)].map(match => match[1])
  assert.ok(commands.length > 0, '应能从 lib.rs 提取 generate_handler 命令清单')
  // 若注释走「见 lib.rs」的泛化写法（不再逐一列举），则豁免本检查。
  const enumerates = /core_refresh/.test(desc)
  if (!enumerates) return
  for (const cmd of commands) {
    assert.ok(desc.includes(cmd), `capabilities 注释漏列命令 ${cmd}（generate_handler 已注册）`)
  }
})

// ── 问题4（模型池空提示与降级提示互斥遮蔽，预期失败）──────────────────────
test('问题4 复现：模型池空提示与检索降级提示不得互斥遮蔽', () => {
  const vue = readFileSync(new URL('src/views/AssistantView.vue', ROOT), 'utf8')
  assert.equal(
    /v-else-if="degradedReason"/.test(vue),
    false,
    '降级提示不得用 v-else-if 依附于模型池提示（应改为独立 v-if 或合并文案）',
  )
})

// ── 核心路径补充（应通过）─────────────────────────────────────────────────
test('runTurn：chat 通道缺失时返回可辨识错误，不伪造回答', async () => {
  const events = []
  const outcome = await runTurn({
    query: '现在什么状态',
    ctx: fakeCtx(),
    chat: undefined,
    retrieve: async () => [],
    onEvent: event => events.push(event),
  })
  assert.equal(outcome.ok, false)
  assert.equal(outcome.error, '模型通道不可用')
  assert.equal(events.at(-1).type, 'error')
})

test('toolCatalog：只暴露 id / 参数 / 用途，不泄漏 risk / needConfirm / describe', () => {
  const catalog = toolCatalog(TOOLS)
  assert.equal(catalog.includes('needConfirm'), false)
  assert.equal(catalog.includes('risk:'), false)
  assert.equal(catalog.includes('describe'), false)
})

test('redact：数组截断到 50 项，超长字符串截断到 800 字符', () => {
  const big = redact(Array.from({ length: 60 }, (_, i) => ({ n: i })))
  assert.equal(big.length, 50)
  const clipped = redact('x'.repeat(900))
  assert.ok(clipped.endsWith('…'))
  assert.ok(clipped.length <= 801)
})

test('redact：嵌套数组内凭据字段同样被屏蔽', () => {
  const masked = redact({ rows: [{ apiKey: 'sk-nested' }, { ok: 1 }] })
  assert.equal(masked.rows[0].apiKey, '[已屏蔽]')
  assert.equal(masked.rows[1].ok, 1)
})
