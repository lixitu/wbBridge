// src/core/assistant.flow.test.js：node --test。
// 用 mock `chat`（返回值形状即 bridge.aiChat 的契约）覆盖「视图层要依赖的」闭环编排，
// 与 assistant.test.js 的纯编排单测互补：这里把 kbIndex 的检索、确认决议与取消接入一起验证。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { runTurn } from './assistant.js'
import { initIndex, searchKnowledge } from './kbIndex.js'

function fakeCtx(overrides = {}) {
  const calls = []
  return {
    calls,
    state: {
      phase: 'ready',
      models: [{ id: 'gpt-4o-mini' }],
      availableModels: ['gpt-4o-mini'],
      usage: { total: 3, models: { 'gpt-4o-mini': 3 } },
    },
    coreAction: async (name, value) => { calls.push({ name, value }); return { ok: true, result: {} } },
    readLog: async () => ({ ok: true, result: 'ok' }),
    dataDir: async () => ({ ok: true, result: 'C:\\data' }),
    navigate: view => calls.push({ name: 'navigate', value: view }),
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

const ok = content => ({ ok: true, content })

test('闭环：检索真实命中 → 模型返回工具调用 → 确认后执行 → 最终答复回显', async () => {
  // 默认无 indexedDB：initIndex 降级为内存索引（AC-4），但检索能力必须完好。
  const { index, degraded } = await initIndex({})
  assert.equal(degraded, true)
  const hits = searchKnowledge(index, '模型检测')
  assert.ok(hits.length > 0, '知识检索必须真实命中，而非返回空/假数据')

  const chat = chatPlan([
    ok('{"reply":"我先查一下运行状态。","tool":{"id":"app.status","args":{}}}'),
    ok('{"reply":"核心就绪，1 个模型可用。"}'),
  ])
  const ctx = fakeCtx()
  const events = []

  const outcome = await runTurn({
    query: '模型检测相关：现在运行正常吗',
    ctx,
    chat,
    retrieve: query => searchKnowledge(index, query),
    confirm: async () => true,
    onEvent: event => events.push(event),
  })

  assert.equal(outcome.ok, true)
  assert.equal(outcome.reply, '核心就绪，1 个模型可用。')
  assert.equal(outcome.steps[0].tool, 'app.status')
  // 系统提示里必须带上真实检索到的资料，而不是「没有资料」的兜底文案。
  assert.ok(chat.calls[0].messages[0].content.includes('[资料1]'))
  assert.equal(events.some(event => event.type === 'tool-result' && event.ok === true), true)
})

test('确认/取消：拒绝后不执行工具，回显已取消且未做任何改动', async () => {
  const chat = chatPlan([
    ok('{"reply":"我来刷新模型名单。","tool":{"id":"core.refresh","args":{}}}'),
  ])
  const ctx = fakeCtx()
  const events = []
  const confirmCalls = []

  const outcome = await runTurn({
    query: '刷新模型名单',
    ctx,
    chat,
    retrieve: async () => [],
    confirm: async (tool, args) => { confirmCalls.push({ tool: tool.id, args }); return false },
    onEvent: event => events.push(event),
  })

  assert.equal(outcome.denied, true)
  assert.match(outcome.reply, /已取消/)
  assert.match(outcome.reply, /没有对应用做任何改动/)
  assert.equal(ctx.calls.length, 0, '用户拒绝后不得触达核心')
  assert.deepEqual(confirmCalls, [{ tool: 'core.refresh', args: {} }])
  assert.equal(events.find(event => event.type === 'tool-result').denied, true)
})

test('通道失败不静默：aiChat 失败形状如实回传，不伪造回答', async () => {
  const chat = chatPlan([{ ok: false, error: '模型池为空' }])
  const events = []

  const outcome = await runTurn({
    query: '现在能对话吗',
    ctx: fakeCtx(),
    chat,
    retrieve: async () => [],
    onEvent: event => events.push(event),
  })

  assert.equal(outcome.ok, false)
  assert.equal(outcome.reply, null)
  assert.equal(outcome.error, '模型池为空')
  assert.equal(events.at(-1).type, 'error')
})

test('停止：模型调用期间 abort 立即回落取消态，不产生最终答复', async () => {
  const controller = new AbortController()
  const chat = chatPlan([ok('{"reply":"不该被展示"}')])
  // 模拟「发送后用户点停止」：模型调用返回前信号被中止。
  const originalChat = chat
  const abortingChat = async (messages, opts) => {
    controller.abort()
    return originalChat(messages, opts)
  }
  const events = []

  const outcome = await runTurn({
    query: '检查状态',
    ctx: fakeCtx(),
    chat: abortingChat,
    retrieve: async () => [],
    onEvent: event => events.push(event),
    signal: controller.signal,
  })

  assert.equal(outcome.canceled, true)
  assert.equal(outcome.ok, false)
  assert.equal(events.some(event => event.type === 'final'), false)
})
