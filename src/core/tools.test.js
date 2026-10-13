// src/core/tools.js 的单测：node --test。
// 覆盖两件关键事：① 注册表本身的自洽与安全边界（不允许出现「写密钥」类工具）；
// ② 参数校验与脱敏是否真的拦得住模型的不可信输入。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { NAVIGABLE_VIEWS, TOOLS, confirmPrompt, findTool, redact, toolCatalog, toolIds, validateArgs } from './tools.js'

function fakeCtx(overrides = {}) {
  const calls = []
  return {
    calls,
    state: {
      phase: 'ready',
      message: '',
      version: '1.0.0',
      useSystemProxy: false,
      models: [
        { id: 'gpt-4o-mini', images: false, reasoning: false, context: 128000 },
        { id: 'qwen-vl', images: true, reasoning: false, context: 32000 },
      ],
      availableModels: ['gpt-4o-mini'],
      modelResults: { 'gpt-4o-mini': { ok: true, ms: 1234 } },
      usage: { total: 7, models: { 'gpt-4o-mini': 7 } },
      probe: { running: false },
    },
    coreAction: async (name, value) => { calls.push({ name, value }); return { ok: true, result: {} } },
    readLog: async () => ({ ok: true, result: 'boot ok\nmodel probe failed\nready' }),
    dataDir: async () => ({ ok: true, result: 'C:\\data\\wb-bridge' }),
    navigate: view => calls.push({ name: 'navigate', value: view }),
    activityText: () => 'gpt-4o-mini · 等待上游返回',
    ...overrides,
  }
}

// 按真实链路执行工具：先在注册表里找，再走参数校验，最后才调用 run。
async function invoke(toolId, args, ctx) {
  const tool = findTool(toolId)
  assert.ok(tool, `工具 ${toolId} 应存在于注册表`)
  const checked = validateArgs(tool.params, args)
  if (!checked.ok) return { ok: false, error: checked.error, rejected: true }
  return tool.run(ctx, checked.value, {})
}

test('注册表自检：id 唯一、字段齐全、风险等级与确认要求自洽', () => {
  const ids = toolIds()
  assert.equal(new Set(ids).size, ids.length, '工具 id 不得重复')
  for (const tool of TOOLS) {
    assert.equal(typeof tool.run, 'function', `${tool.id} 必须有真实执行逻辑（禁止只有名字的占位工具）`)
    assert.ok(tool.summary && typeof tool.summary === 'string', `${tool.id} 缺少 summary`)
    assert.ok(tool.describe && typeof tool.describe === 'string', `${tool.id} 缺少 describe`)
    assert.ok(['query', 'action', 'navigate'].includes(tool.kind), `${tool.id} 的 kind 非法`)
    assert.ok(['low', 'medium', 'high'].includes(tool.risk), `${tool.id} 的 risk 非法`)
    // 中高风险操作一律要求确认：这是「模型不能代替用户决定」的守门条件。
    if (tool.risk !== 'low') assert.equal(tool.needConfirm, true, `${tool.id} 中高风险必须要求确认`)
  }
})

test('安全边界：注册表里不允许存在任何需要密钥参数的写操作', () => {
  for (const tool of TOOLS) {
    for (const name of Object.keys(tool.params || {})) {
      assert.equal(
        /(api[-_]?key|apikey|token|secret|password)/i.test(name),
        false,
        `${tool.id} 不允许接收密钥类参数 ${name}——密钥不得经过模型`,
      )
    }
  }
  // 正向确认：平台相关能力只有「查看状态」与「清除 Key」两项，
  // 且删除是不可逆的高风险操作，必须走确认。
  assert.deepEqual(
    TOOLS.filter(tool => tool.id.startsWith('providers.')).map(tool => tool.id),
    ['providers.list', 'providers.clearKey'],
  )
  const clear = findTool('providers.clearKey')
  assert.equal(clear.risk, 'high')
  assert.equal(clear.needConfirm, true)
  const list = findTool('providers.list')
  assert.equal(list.risk, 'low')
})

test('参数校验：必填、类型、枚举、范围、未知参数一律拒绝', () => {
  const schema = {
    model: { type: 'string', required: true, max: 10 },
    limit: { type: 'number', required: false, min: 1, max: 20, default: 5 },
    enabled: { type: 'boolean', required: false },
    view: { type: 'string', required: false, enum: ['models', 'logs'] },
  }
  assert.equal(validateArgs(schema, { model: 'a' }).ok, true)
  assert.deepEqual(validateArgs(schema, { model: 'a' }).value, { model: 'a', limit: 5 })

  assert.equal(validateArgs(schema, {}).ok, false, '缺必填应拒绝')
  assert.equal(validateArgs(schema, { model: 1 }).ok, false, '类型不符应拒绝')
  assert.equal(validateArgs(schema, { model: 'x'.repeat(11) }).ok, false, '超长应拒绝')
  assert.equal(validateArgs(schema, { model: 'a', limit: 0 }).ok, false, '越界应拒绝')
  assert.equal(validateArgs(schema, { model: 'a', limit: 21 }).ok, false, '越界应拒绝')
  assert.equal(validateArgs(schema, { model: 'a', enabled: 'yes' }).ok, false, '布尔必须是布尔')
  assert.equal(validateArgs(schema, { model: 'a', view: 'about' }).ok, false, '枚举外取值应拒绝')
  // 未知参数必须报错而不是静默忽略——否则模型会以为参数生效了。
  const unknown = validateArgs(schema, { model: 'a', evil: true })
  assert.equal(unknown.ok, false)
  assert.match(unknown.error, /不支持的参数 evil/)
})

test('脱敏：任何看起来像凭据的字段都被结构性屏蔽', () => {
  const masked = redact({
    provider: 'openai',
    apiKey: 'sk-real-secret',
    nested: { token: 'abc', headers: { Authorization: 'Bearer x' }, keep: 'ok' },
    list: [{ password: 'p' }],
  })
  assert.equal(masked.apiKey, '[已屏蔽]')
  assert.equal(masked.nested.token, '[已屏蔽]')
  assert.equal(masked.nested.headers.Authorization, '[已屏蔽]')
  assert.equal(masked.nested.keep, 'ok')
  assert.equal(masked.list[0].password, '[已屏蔽]')
  assert.equal(JSON.stringify(masked).includes('sk-real-secret'), false)
})

test('查询类工具：app.status 返回真实快照数值', async () => {
  const result = await invoke('app.status', {}, fakeCtx())
  assert.equal(result.ok, true)
  assert.match(result.summary, /就绪/)
  assert.match(result.summary, /共 2 个，其中可用 1 个/)
  assert.equal(result.data.models, 2)
  assert.equal(result.data.available, 1)
})

test('查询类工具：models.list 支持筛选与截断提示', async () => {
  const ctx = fakeCtx()
  const all = await invoke('models.list', { filter: 'all', limit: 20 }, ctx)
  assert.match(all.summary, /gpt-4o-mini/)
  assert.match(all.summary, /qwen-vl/)

  const available = await invoke('models.list', { filter: 'available', limit: 20 }, ctx)
  assert.match(available.summary, /gpt-4o-mini/)
  assert.equal(available.summary.includes('qwen-vl'), false)

  const truncated = await invoke('models.list', { filter: 'all', limit: 1 }, ctx)
  assert.match(truncated.summary, /已列出前 1 个/)
})

test('查询类工具：models.detail 对未知模型如实报错', async () => {
  const ctx = fakeCtx()
  const found = await invoke('models.detail', { model: 'gpt-4o-mini' }, ctx)
  assert.equal(found.ok, true)
  assert.match(found.summary, /最近检测结果/)
  const missing = await invoke('models.detail', { model: '不存在' }, ctx)
  assert.equal(missing.ok, false)
  assert.match(missing.error, /没有/)
})

test('查询类工具：logs.tail 支持关键词过滤，无匹配时如实说明', async () => {
  const ctx = fakeCtx()
  const all = await invoke('logs.tail', { limit: 10 }, ctx)
  assert.equal(all.ok, true)
  assert.equal(all.data.total, 3)

  const filtered = await invoke('logs.tail', { keyword: 'probe', limit: 10 }, ctx)
  assert.equal(filtered.data.matched, undefined)
  assert.match(filtered.summary, /model probe failed/)

  const none = await invoke('logs.tail', { keyword: '不存在的词', limit: 10 }, ctx)
  assert.equal(none.ok, true)
  assert.match(none.summary, /没有包含/)
  assert.equal(none.data.matched, 0)
})

test('导航类工具只切视图，不产生业务副作用', async () => {
  const ctx = fakeCtx()
  const result = await invoke('nav.open', { view: 'logs' }, ctx)
  assert.equal(result.ok, true)
  assert.deepEqual(ctx.calls, [{ name: 'navigate', value: 'logs' }])
  assert.deepEqual(NAVIGABLE_VIEWS.includes('logs'), true)
  // 视图枚举外的取值在校验阶段就被拦下，导航函数不会被调用。
  const rejected = await invoke('nav.open', { view: 'assistant' }, ctx)
  assert.equal(rejected.rejected, true)
  assert.equal(ctx.calls.length, 1)
})

test('操作类工具：核心未就绪时拒绝执行，且不会触达核心接口', async () => {
  const ctx = fakeCtx({ state: { phase: 'starting' } })
  const result = await invoke('core.refresh', {}, ctx)
  assert.equal(result.ok, false)
  assert.match(result.error, /暂时不能执行/)
  assert.equal(ctx.calls.length, 0, '前置检查未通过时不得调用核心')
})

test('操作类工具：导入结果按真实返回区分「写入 / 无需写入 / 取消」', async () => {
  const written = await invoke('core.import', {}, fakeCtx({
    coreAction: async () => ({ ok: true, result: { count: 3, changed: true, targets: { WorkBuddy: { status: 'ok', changed: true } } } }),
  }))
  assert.match(written.summary, /导入完成，共 3 个模型/)

  const noop = await invoke('core.import', {}, fakeCtx({
    coreAction: async () => ({ ok: true, result: { count: 3, changed: false } }),
  }))
  assert.match(noop.summary, /无需重复写入/)

  const canceled = await invoke('core.import', {}, fakeCtx({
    coreAction: async () => ({ ok: true, result: { canceled: true } }),
  }))
  assert.match(canceled.summary, /已取消/)
})

test('操作类工具：上游拒绝时如实回报错误，不伪装成功', async () => {
  const result = await invoke('models.reprobe', { model: 'gpt-4o-mini' }, fakeCtx({
    coreAction: async () => ({ ok: true, result: { started: false, message: '已有检测在进行中' } }),
  }))
  assert.equal(result.ok, false)
  assert.match(result.error, /已有检测在进行中/)
})

test('工具清单与确认文案可读且包含真实参数', () => {
  const catalog = toolCatalog()
  assert.match(catalog, /models\.list/)
  assert.match(catalog, /nav\.open/)
  const prompt = confirmPrompt(findTool('core.proxy'), { enabled: true })
  assert.equal(prompt.risk, 'medium')
  assert.match(prompt.detail, /enabled=true/)
})
