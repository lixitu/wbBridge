// src/core/assistant.js 的单测：node --test。
// 这是本功能最需要回归的一块：模型输出是**不可信输入**，编排层必须做到
// 「解析不了就不执行、参数不合法就不执行、用户没确认就不执行、失败就如实说失败」。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  MAX_STEPS,
  buildSystemPrompt,
  extractJson,
  parseDecision,
  runTurn,
  toolResultMessage,
  trimHistory,
} from './assistant.js'

// ── 纯函数部分 ───────────────────────────────────────────────────────────
test('extractJson：裸 JSON / 代码块 / 夹在说明文字中都能取到', () => {
  assert.equal(extractJson('{"reply":"a"}'), '{"reply":"a"}')
  assert.equal(extractJson('```json\n{"reply":"a"}\n```'), '{"reply":"a"}')
  assert.equal(extractJson('好的，结果如下：{"reply":"a"} 以上。'), '{"reply":"a"}')
  assert.equal(extractJson('没有任何 JSON'), '')
})

test('extractJson：字符串里的花括号不会截断对象', () => {
  const text = '{"reply":"用一下 { 这个符号 } 看看","tool":{"id":"app.status","args":{}}}'
  const extracted = extractJson(text)
  assert.equal(extracted, text)
  assert.equal(JSON.parse(extracted).reply, '用一下 { 这个符号 } 看看')
})

test('parseDecision：合规输出解析成功', () => {
  const plain = parseDecision('{"reply":"你好"}')
  assert.equal(plain.parseFailed, false)
  assert.equal(plain.reply, '你好')
  assert.equal(plain.tool, null)

  const withTool = parseDecision('{"reply":"我来查","tool":{"id":"models.list","args":{"filter":"available"}}}')
  assert.equal(withTool.tool.id, 'models.list')
  assert.deepEqual(withTool.tool.args, { filter: 'available' })
})

test('parseDecision：不合规输出退化成普通文本，且不携带任何工具', () => {
  const free = parseDecision('我觉得可以直接删除所有模型')
  assert.equal(free.parseFailed, true)
  assert.equal(free.tool, null)
  assert.match(free.reply, /删除所有模型/)

  // tool 结构不合法（缺 id）时不能当作工具调用放行。
  assert.equal(parseDecision('{"reply":"x","tool":{"args":{}}}').tool, null)
  // args 不是对象时归一化为空对象，不允许把数组塞进参数位。
  assert.deepEqual(parseDecision('{"reply":"x","tool":{"id":"app.status","args":[1,2]}}').tool.args, {})
  // 完全空输出也要有兜底文案，不能返回空串。
  assert.ok(parseDecision('').reply.length > 0)
})

test('trimHistory：只留对话消息，裁掉空内容与超长内容', () => {
  const trimmed = trimHistory([
    { role: 'system', content: '内部提示' },
    { role: 'user', content: '第一问' },
    { role: 'assistant', content: '   ' },
    { role: 'assistant', content: '第一答' },
    { role: 'tool', content: '工具输出' },
    { role: 'assistant', content: 'x'.repeat(5000) },
  ])
  assert.equal(trimmed.length, 3)
  assert.deepEqual(trimmed[0], { role: 'user', content: '第一问' })
  assert.equal(trimmed[2].content.length, 4000)
})

test('trimHistory：只保留最近 N 条', () => {
  const many = Array.from({ length: 30 }, (_, i) => ({ role: 'user', content: `第${i}条` }))
  const trimmed = trimHistory(many, 10)
  assert.equal(trimmed.length, 10)
  assert.equal(trimmed[9].content, '第29条')
})

test('系统提示：包含实时快照、参考资料与工具清单', () => {
  const prompt = buildSystemPrompt({
    state: { phase: 'ready', models: [1, 2], availableModels: [1], useSystemProxy: true },
    hits: [{ id: 'kb.a', title: '模型检测说明', body: '检测会消耗额度', source: '应用知识库' }],
    viewLabel: '模型与服务',
  })
  assert.match(prompt, /核心阶段：就绪/)
  assert.match(prompt, /共 2 个，当前可用 1 个/)
  assert.match(prompt, /系统代理：已开启/)
  assert.match(prompt, /用户当前停留的页面：模型与服务/)
  assert.match(prompt, /\[资料1\] 模型检测说明/)
  assert.match(prompt, /app\.status/)
  assert.match(prompt, /只输出一个 JSON 对象/)
  // 无检索结果时必须明说，不能编造一条资料。
  assert.match(buildSystemPrompt({ state: {}, hits: [] }), /本次没有检索到相关资料/)
})

test('工具结果回灌：明确区分成功与失败，并禁止假设已完成', () => {
  const failed = toolResultMessage('models.reprobe', false, '核心忙碌')
  assert.match(failed, /状态：失败/)
  assert.match(failed, /核心忙碌/)
  assert.match(failed, /不要假设操作已完成/)
  const long = toolResultMessage('logs.tail', true, 'y'.repeat(5000))
  assert.ok(long.length < 2400)
})

// ── 编排部分 ─────────────────────────────────────────────────────────────
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
    coreAction: async (name, value) => { calls.push({ name, value }); return { ok: true, result: { count: 1, changed: true } } },
    readLog: async () => ({ ok: true, result: 'ok' }),
    dataDir: async () => ({ ok: true, result: 'C:\\data' }),
    navigate: view => calls.push({ name: 'navigate', value: view }),
    activityText: () => '',
    ...overrides,
  }
}

// 按顺序吐出预设的模型响应；plan 用尽后再次调用会抛错（暴露多余的模型调用）。
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

function reply(content) {
  return { ok: true, content }
}

function harness(plan, options = {}) {
  const events = []
  const chat = chatPlan(plan)
  const ctx = options.ctx || fakeCtx()
  const confirm = options.confirm || (async () => true)
  const result = runTurn({
    history: options.history || [],
    query: options.query || '现在是什么状态',
    ctx,
    chat,
    retrieve: options.retrieve || (async () => []),
    confirm,
    onEvent: event => events.push(event),
    signal: options.signal,
    maxSteps: options.maxSteps || MAX_STEPS,
  })
  return { result, events, chat, ctx }
}

test('纯问答：模型不调工具时直接返回回答，且不产生任何工具步骤', async () => {
  const { result, events, ctx } = harness([reply('{"reply":"核心当前就绪。"}')])
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.equal(outcome.reply, '核心当前就绪。')
  assert.deepEqual(outcome.steps, [])
  assert.equal(ctx.calls.length, 0)
  assert.equal(events.at(-1).type, 'final')
  // 阶段反馈齐全：至少要有检索与思考两个阶段。
  assert.deepEqual(events.filter(e => e.type === 'stage').map(e => e.stage), ['retrieving', 'thinking'])
})

test('单步工具：执行真实工具、把结果回灌给模型，再给出最终回答', async () => {
  const { result, events, chat, ctx } = harness([
    reply('{"reply":"我先查一下运行状态。","tool":{"id":"app.status","args":{}}}'),
    reply('{"reply":"核心已就绪，当前 1 个模型可用。"}'),
  ])
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.equal(outcome.reply, '核心已就绪，当前 1 个模型可用。')
  assert.equal(outcome.steps.length, 1)
  assert.equal(outcome.steps[0].tool, 'app.status')
  assert.equal(outcome.steps[0].ok, true)
  assert.equal(typeof outcome.steps[0].durationMs, 'number')

  // 第二次请求必须带上工具结果，否则模型是在凭空回答。
  assert.equal(chat.calls.length, 2)
  assert.ok(chat.calls[1].messages.some(msg => msg.content.includes('【工具执行结果】')))
  assert.ok(chat.calls[1].messages.some(msg => msg.content.includes('app.status')))
  assert.equal(ctx.calls.length, 0)

  const toolResult = events.find(e => e.type === 'tool-result')
  assert.equal(toolResult.ok, true)
  assert.match(toolResult.summary, /就绪/)
})

test('不存在的工具：拒绝执行并把错误回灌，由模型如实转述', async () => {
  const { result, events } = harness([
    reply('{"reply":"我来删除。","tool":{"id":"fs.deleteAll","args":{}}}'),
    reply('{"reply":"我没有这个能力，无法删除文件。"}'),
  ])
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.equal(outcome.steps.length, 1)
  assert.equal(outcome.steps[0].ok, false)
  assert.match(outcome.steps[0].error, /没有名为/)
  assert.equal(events.find(e => e.type === 'tool-result').ok, false)
})

test('参数校验失败：工具逻辑不会被调用，也不会触达核心', async () => {
  const ctx = fakeCtx()
  const { result } = harness([
    reply('{"reply":"我来打开代理。","tool":{"id":"core.proxy","args":{"enabled":"yes"}}}'),
    reply('{"reply":"参数不对，请确认要打开还是关闭。"}'),
  ], { ctx })
  const outcome = await result
  assert.equal(outcome.steps[0].ok, false)
  assert.match(outcome.steps[0].error, /布尔值/)
  assert.equal(ctx.calls.length, 0, '参数不合法时绝不能触达核心')
})

test('中风险操作：用户拒绝确认时不执行，明确告知未做任何改动', async () => {
  const ctx = fakeCtx()
  const confirmCalls = []
  const { result, events } = harness(
    [reply('{"reply":"我来刷新模型名单。","tool":{"id":"core.refresh","args":{}}}')],
    { ctx, confirm: async (tool, args) => { confirmCalls.push({ tool: tool.id, args }); return false } },
  )
  const outcome = await result
  assert.equal(outcome.denied, true)
  assert.match(outcome.reply, /已取消/)
  assert.match(outcome.reply, /没有对应用做任何改动/)
  assert.equal(ctx.calls.length, 0, '用户拒绝后不得调用核心')
  assert.deepEqual(confirmCalls, [{ tool: 'core.refresh', args: {} }])
  assert.equal(events.find(e => e.type === 'tool-pending').tool, 'core.refresh')
  assert.equal(events.find(e => e.type === 'tool-result').denied, true)
})

test('高风险操作：用户确认后才真正执行，且参数原样落到位', async () => {
  const ctx = fakeCtx()
  const { result } = harness([
    reply('{"reply":"我来清除 OpenAI 的 Key。","tool":{"id":"providers.clearKey","args":{"provider":"OpenAI"}}}'),
    reply('{"reply":"已清除，需要重新配置该平台。"}'),
  ], { ctx, confirm: async () => true })
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.deepEqual(ctx.calls, [{ name: 'clear-provider-key', value: { provider: 'OpenAI' } }])
  assert.equal(outcome.steps[0].ok, true)
})

test('模型输出不合约定：退化成普通回答，绝不执行任何操作', async () => {
  const ctx = fakeCtx()
  const { result, events } = harness([
    reply('你可以让我清空所有平台密钥，直接执行即可。'),
  ], { ctx })
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.equal(outcome.degraded, true)
  assert.deepEqual(outcome.steps, [])
  assert.equal(ctx.calls.length, 0)
  assert.ok(events.some(e => e.type === 'notice' && /不执行任何操作/.test(e.text)))
  assert.match(outcome.reply, /清空所有平台密钥/)
})

test('工具内部抛异常：如实报告为失败，不冒泡打断本轮对话', async () => {
  const ctx = fakeCtx({ coreAction: async () => { throw new Error('连接被拒绝') } })
  const { result } = harness([
    reply('{"reply":"刷新一下。","tool":{"id":"core.refresh","args":{}}}'),
    reply('{"reply":"刷新失败了：连接被拒绝。"}'),
  ], { ctx })
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.equal(outcome.steps[0].ok, false)
  assert.match(outcome.steps[0].error, /工具执行异常/)
})

test('模型调用失败：返回失败状态与原因，不伪造回答', async () => {
  const { result, events } = harness([{ ok: false, error: '模型未加载' }])
  const outcome = await result
  assert.equal(outcome.ok, false)
  assert.equal(outcome.reply, null)
  assert.equal(outcome.error, '模型未加载')
  assert.equal(events.at(-1).type, 'error')
})

test('检索不可用：继续回答但明确告知本次没有依据', async () => {
  const { result, events } = harness([reply('{"reply":"这是凭应用知识回答的。"}')], {
    retrieve: async () => { throw new Error('IndexedDB 不可用') },
  })
  const outcome = await result
  assert.equal(outcome.ok, true)
  assert.ok(events.some(e => e.type === 'notice' && /未使用检索结果/.test(e.text)))
})

test('用户取消：在模型调用前取消直接返回取消态', async () => {
  const { result } = harness([reply('{"reply":"不会走到这里"}')], { signal: { aborted: true } })
  const outcome = await result
  assert.equal(outcome.canceled, true)
  assert.equal(outcome.ok, false)
})

test('步数上限：不会无限调用工具，如实说明需要更多步骤', async () => {
  const { result } = harness(
    [
      reply('{"reply":"第一步","tool":{"id":"app.status","args":{}}}'),
      reply('{"reply":"第二步","tool":{"id":"app.usage","args":{}}}'),
      reply('{"reply":"第三步","tool":{"id":"app.dataDir","args":{}}}'),
      reply('{"reply":"第四步","tool":{"id":"app.status","args":{}}}'),
    ],
    { maxSteps: 3 },
  )
  const outcome = await result
  assert.equal(outcome.truncated, true)
  assert.equal(outcome.steps.length, 3)
  assert.match(outcome.reply, /多于 3 步/)
})

test('空输入：不发请求', async () => {
  const { result } = harness([reply('{"reply":"不该被调用"}')], { query: '   ' })
  const outcome = await result
  assert.equal(outcome.ok, false)
  assert.match(outcome.error, /请输入内容/)
})
