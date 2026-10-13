// AI 助手的工具注册表：把应用**真实存在**的能力声明成可被模型调用的工具。
//
// 铁律（为什么这张表必须与实现严格对应）：
// 1. 每个工具的 run 都必须落到应用已有的调用路径上（bridge.action 的动作、壳的只读命令），
//    不存在「工具名有了、后面没有真实实现」的占位项。做不到的能力不注册，而不是先注册再补。
// 2. 参数一律先经 validateArgs 校验（类型 / 必填 / 枚举 / 长度），再交给业务函数；
//    模型传来的东西是**不可信输入**，校验失败一律拒绝执行，不做任何隐式转换或猜测。
// 3. 操作类工具带 risk 与 needConfirm：中高风险操作必须由用户在界面上确认后才执行，
//    模型无法代替用户确认（确认入口只在视图层，见 AssistantView.vue）。
// 4. 秘书级凭据不注册：`设置平台 Key` 这类需要密钥参数的工具有意缺席——密钥不允许
//    经过模型（模型输出可能被诱导、也可能自己编造），该动作只能在「平台」视图手动完成。
//    `清除平台 Key` 因为不可逆，注册但强制确认。
//
// 本模块只做「定义 + 校验 + 调用转发」，不含任何状态：便于 node --test 直接覆盖。

// 视图 id（与 src/core/prefs.js 的 VIEW_IDS、SideBar.vue 的分组一一对应）。
// AI 助手自身不在可导航目标里——「打开我自己」没有意义。
export const NAVIGABLE_VIEWS = ['models', 'providers', 'logs', 'usage', 'workbuddy', 'about']

const PHASE_TEXT = {
  starting: '正在启动核心',
  ready: '就绪',
  stopping: '正在停止',
  stopped: '已停止',
  error: '启动失败',
}

function ok(summary, data) {
  return { ok: true, summary, data: data === undefined ? null : data }
}

function bad(error) {
  return { ok: false, error }
}

const SENSITIVE_KEY = /(api[-_]?key|apikey|token|secret|password|authorization|bearer|credential)/i

// 深度脱敏：交给模型的数据里不允许出现任何看起来像凭据的字段。
// 这是**结构性**防线（按字段名屏蔽），不依赖「哪些接口本来就不返回密钥」这种假设。
export function redact(value, depth = 0) {
  if (depth > 6) return '[层级过深，已省略]'
  if (Array.isArray(value)) return value.slice(0, 50).map(item => redact(item, depth + 1))
  if (value && typeof value === 'object') {
    const output = {}
    for (const [key, item] of Object.entries(value)) {
      output[key] = SENSITIVE_KEY.test(key) ? '[已屏蔽]' : redact(item, depth + 1)
    }
    return output
  }
  if (typeof value === 'string' && value.length > 800) return `${value.slice(0, 800)}…`
  return value
}

// 文本级脱敏：日志是自由文本，redact() 的「按字段名屏蔽」在这里无从下手。
// 只覆盖最常见的几种凭据书写形式（Authorization 头、Bearer token、key=value 形式），
// 命中即整体替换为 [已屏蔽]——宁可多遮蔽，也不让密钥经工具结果回灌给模型。
const TEXT_SECRET_PATTERNS = [
  /(Bearer\s+)[A-Za-z0-9._~+/=-]{4,}/gi,
  /((?:api[-_]?key|apikey|token|secret|password|passwd|pwd)\s*[:=]\s*)["']?[^\s"',;]+/gi,
  /(authorization\s*[:=]\s*)(?!Bearer)[^\s,;]+/gi,
]

export function scrubSecrets(text) {
  let output = String(text)
  for (const pattern of TEXT_SECRET_PATTERNS) output = output.replace(pattern, '$1[已屏蔽]')
  return output
}

// ── 参数校验（纯函数，可单测） ────────────────────────────────────────────
//
// 规则：未知参数一律报错（而不是忽略）。模型传了表里没有的参数，说明它对工具的理解
// 有偏差，静默忽略会让它以为「参数生效了」；报错能让它纠正后重试。
export function validateArgs(params, args) {
  const schema = params && typeof params === 'object' ? params : {}
  const input = args && typeof args === 'object' && !Array.isArray(args) ? args : {}
  const value = {}
  for (const key of Object.keys(input)) {
    if (!Object.prototype.hasOwnProperty.call(schema, key)) {
      return { ok: false, error: `不支持的参数 ${key}（可用参数：${Object.keys(schema).join('、') || '无'}）` }
    }
  }
  for (const [name, spec] of Object.entries(schema)) {
    const raw = input[name]
    if (raw === undefined || raw === null || raw === '') {
      if (spec.required) return { ok: false, error: `缺少必填参数 ${name}${spec.desc ? `（${spec.desc}）` : ''}` }
      if (spec.default !== undefined) value[name] = spec.default
      continue
    }
    if (spec.type === 'string') {
      if (typeof raw !== 'string') return { ok: false, error: `参数 ${name} 必须是字符串` }
      const max = Number.isInteger(spec.max) ? spec.max : 200
      if (raw.length > max) return { ok: false, error: `参数 ${name} 过长（上限 ${max} 字符）` }
      if (Array.isArray(spec.enum) && spec.enum.length && !spec.enum.includes(raw)) {
        return { ok: false, error: `参数 ${name} 取值必须是：${spec.enum.join(' / ')}` }
      }
      value[name] = raw
    } else if (spec.type === 'number') {
      const num = typeof raw === 'number' ? raw : Number.NaN
      if (!Number.isFinite(num)) return { ok: false, error: `参数 ${name} 必须是数字` }
      if (Number.isInteger(spec.min) && num < spec.min) return { ok: false, error: `参数 ${name} 不能小于 ${spec.min}` }
      if (Number.isInteger(spec.max) && num > spec.max) return { ok: false, error: `参数 ${name} 不能大于 ${spec.max}` }
      value[name] = Math.trunc(num)
    } else if (spec.type === 'boolean') {
      if (typeof raw !== 'boolean') return { ok: false, error: `参数 ${name} 必须是布尔值（true / false）` }
      value[name] = raw
    } else {
      return { ok: false, error: `参数 ${name} 的类型定义无效` }
    }
  }
  return { ok: true, value }
}

// ── 状态读取的共用小工具 ─────────────────────────────────────────────────
function stateOf(ctx) {
  const state = ctx?.state
  return state && typeof state === 'object' ? state : {}
}

// 阶段文案：认识的阶段给中文说明，不认识的原样回显——宁可显示原始值，
// 也不要为了「好看」把未知状态翻译成一个可能错误的结论。
export function describePhase(state) {
  const phase = state && typeof state.phase === 'string' ? state.phase : ''
  if (!phase) return '未知（尚未收到核心状态）'
  return PHASE_TEXT[phase] || phase
}

const phaseText = describePhase

// 操作类工具的统一前置条件：核心必须处于就绪状态。
// 界面上按钮同样是这么禁用的（App.vue 的 :disabled 判定），保持同一口径。
function requireReady(ctx) {
  const state = stateOf(ctx)
  if (state.phase !== 'ready' && state.phase !== 'error') {
    return `核心当前${phaseText(state)}，暂时不能执行该操作`
  }
  return ''
}

function modelList(ctx) {
  const state = stateOf(ctx)
  const models = Array.isArray(state.models) ? state.models : []
  const available = new Set(Array.isArray(state.availableModels) ? state.availableModels : [])
  return { models, available }
}

function findModel(ctx, id) {
  const { models, available } = modelList(ctx)
  const model = models.find(item => item && item.id === id)
  if (!model) return { error: `当前模型名单里没有 ${id}` }
  return { model, available: available.has(id) }
}

function describeModel(item, available) {
  const flags = []
  if (available) flags.push('可用')
  else flags.push('不可用')
  if (item.images) flags.push('支持图片')
  if (item.reasoning) flags.push('支持推理')
  if (item.context) flags.push(`上下文 ${item.context}`)
  return `${item.id}（${flags.join('，')}）`
}

// 执行类工具的统一包装：把 coreAction 的 { ok, error } 契约翻成人话。
async function runCoreAction(ctx, name, value, successText) {
  const response = await ctx.coreAction(name, value)
  if (!response || response.ok !== true) return bad(response?.error || `${name} 执行失败`)
  return ok(successText, redact(response.result))
}

// ── 工具定义 ─────────────────────────────────────────────────────────────
export const TOOLS = [
  {
    id: 'app.status',
    title: '查询运行状态',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '查看核心是否就绪、当前在与上游做什么、版本与代理状态',
    describe: '读取核心的运行阶段、当前活动、核心版本、系统代理开关状态与模型数量，用于回答「现在运行正常吗」这类问题。',
    resultHint: '返回当前阶段的文字描述与关键计数',
    params: {},
    async run(ctx) {
      const state = stateOf(ctx)
      const models = Array.isArray(state.models) ? state.models : []
      const available = Array.isArray(state.availableModels) ? state.availableModels : []
      const lines = [
        `运行阶段：${phaseText(state)}`,
        state.message ? `提示信息：${state.message}` : '',
        `核心版本：${state.version || '未知'}`,
        `系统代理：${state.useSystemProxy === true ? '已开启' : '未开启'}`,
        `模型：共 ${models.length} 个，其中可用 ${available.length} 个`,
        state.probe?.running ? `正在批量检测：还剩 ${state.probe.pending?.length ?? '?'} 个未完成` : '',
        state.sync?.time ? `最近一次导入：${state.sync.time}，写入 ${state.sync.count ?? 0} 个模型` : '',
        ctx.activityText?.(state.activity) ? `上游活动：${ctx.activityText(state.activity)}` : '',
      ].filter(Boolean)
      return ok(lines.join('\n'), redact({
        phase: state.phase || '',
        message: state.message || '',
        version: state.version || '',
        useSystemProxy: state.useSystemProxy === true,
        models: models.length,
        available: available.length,
        probeRunning: state.probe?.running === true,
      }))
    },
  },
  {
    id: 'app.usage',
    title: '查询请求用量',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '查看本机发出的请求计数（总请求数与按模型拆分）',
    describe: '读取用量统计。口径：只有真实客户端请求计入，模型探测不计入。上游剩余额度无法查询，不要据此推断剩余额度。',
    resultHint: '返回总请求数与按模型分布的请求数',
    params: {},
    async run(ctx) {
      const usage = stateOf(ctx).usage
      if (!usage || typeof usage !== 'object') return ok('还没有用量数据（核心尚未统计到请求）。', null)
      const total = Number(usage.total) || 0
      const models = usage.models && typeof usage.models === 'object' ? usage.models : {}
      const entries = Object.entries(models).sort((a, b) => (Number(b[1]) || 0) - (Number(a[1]) || 0))
      const detail = entries.slice(0, 20).map(([id, count]) => `${id}：${count} 次`).join('\n')
      return ok(
        `累计请求 ${total} 次${entries.length ? `，按模型分布：\n${detail}` : ''}\n（这是本机请求计数，不是剩余额度。）`,
        redact({ total, models }),
      )
    },
  },
  {
    id: 'app.dataDir',
    title: '查询数据目录',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '查看核心状态文件与日志所在的数据目录路径',
    describe: '返回应用数据目录的绝对路径（status.json、运行日志、平台配置都在这下面）。只读取路径，不做任何文件写入。',
    resultHint: '返回数据目录绝对路径',
    params: {},
    async run(ctx) {
      const response = await ctx.dataDir()
      if (!response?.ok) return bad(response?.error || '无法读取数据目录')
      const path = typeof response.result === 'string' ? response.result : JSON.stringify(response.result)
      return ok(`数据目录：${path}`, { path })
    },
  },
  {
    id: 'models.list',
    title: '列出模型',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '按可用性列出模型（含是否支持图片/推理、上下文长度）',
    describe: '列出本次窗口发现的模型。filter 可选 all（全部，默认）/ available（仅当前可用的）/ unavailable（仅不可用的）。',
    resultHint: '返回模型 id 列表与可用性，必要时提示数量截断',
    params: {
      filter: { type: 'string', required: false, enum: ['all', 'available', 'unavailable'], default: 'all', desc: '筛选可用性' },
      limit: { type: 'number', required: false, min: 1, max: 100, default: 20, desc: '返回条数上限' },
    },
    async run(ctx, args) {
      const { models, available } = modelList(ctx)
      if (!models.length) return ok('当前还没有模型名单，可以先执行「读取免费模型」。', { count: 0 })
      const filtered = models.filter((item) => {
        if (!item || typeof item.id !== 'string') return false
        const isAvailable = available.has(item.id)
        if (args.filter === 'available') return isAvailable
        if (args.filter === 'unavailable') return !isAvailable
        return true
      })
      if (!filtered.length) return ok('没有符合该筛选条件的模型。', { count: 0 })
      const shown = filtered.slice(0, args.limit)
      const text = shown.map(item => describeModel(item, available.has(item.id))).join('\n')
      const tail = filtered.length > shown.length ? `\n（共 ${filtered.length} 个，已列出前 ${shown.length} 个）` : ''
      return ok(`${text}${tail}`, redact({
        total: filtered.length,
        available: available.size,
        models: shown.map(item => ({ id: item.id, available: available.has(item.id), images: !!item.images, reasoning: !!item.reasoning, context: item.context ?? null })),
      }))
    },
  },
  {
    id: 'models.detail',
    title: '查看模型详情',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '查看单个模型的检测结果与能力信息',
    describe: '返回指定模型的原始检测结果（最近一次探测的用时、状态、错误信息）与能力字段。model 必须是当前名单里的 id。',
    resultHint: '返回该模型的能力字段与最近一次检测结果',
    params: {
      model: { type: 'string', required: true, max: 200, desc: '模型 id（来自 models.list）' },
    },
    async run(ctx, args) {
      const found = findModel(ctx, args.model)
      if (found.error) return bad(found.error)
      const results = stateOf(ctx).modelResults || {}
      const result = results[args.model] || null
      const model = found.model
      const lines = [
        describeModel(model, found.available),
        result ? `最近检测结果：${JSON.stringify(redact(result))}` : '还没有检测结果',
      ]
      return ok(lines.join('\n'), redact({ model: { id: model.id, images: !!model.images, reasoning: !!model.reasoning, context: model.context ?? null }, result }))
    },
  },
  {
    id: 'logs.tail',
    title: '读取运行日志',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '读取日志尾部，可按关键词过滤',
    describe: '读取核心日志文件的尾部内容（只读）。keyword 可过滤包含该关键词的行；limit 控制返回行数上限（默认 40，最多 200）。',
    resultHint: '返回日志文本（可能被行数与长度截断）',
    params: {
      keyword: { type: 'string', required: false, max: 80, desc: '按关键词过滤行' },
      limit: { type: 'number', required: false, min: 1, max: 200, default: 40, desc: '返回行数上限' },
    },
    async run(ctx, args) {
      const response = await ctx.readLog()
      if (!response?.ok) return bad(response?.error || '无法读取运行日志')
      const raw = typeof response.result === 'string' ? response.result : ''
      if (!raw.trim()) return ok('日志文件是空的。', { lines: 0 })
      let lines = raw.split(/\r?\n/).filter(line => line.length > 0)
      const total = lines.length
      if (args.keyword) {
        const needle = args.keyword.toLowerCase()
        lines = lines.filter(line => line.toLowerCase().includes(needle))
        if (!lines.length) return ok(`日志里没有包含「${args.keyword}」的行（共读取 ${total} 行）。`, { matched: 0, total })
      }
      const shown = lines.slice(-args.limit)
      // 日志可能含历史写入的凭据（Authorization 头、key=value），交给模型前一律脱敏。
      const clipped = shown
        .map(line => (line.length > 400 ? `${line.slice(0, 400)}…` : line))
        .map(scrubSecrets)
      return ok(
        `日志尾部（共读取 ${total} 行，返回 ${clipped.length} 行${args.keyword ? `，过滤词「${args.keyword}」` : ''}）：\n${clipped.join('\n')}`,
        { total, returned: clipped.length, keyword: args.keyword || '' },
      )
    },
  },
  {
    id: 'providers.list',
    title: '查看平台 Key 配置状态',
    kind: 'query',
    risk: 'low',
    needConfirm: false,
    summary: '查看各平台是否已配置 API Key（不返回 Key 内容）',
    describe: '读取各模型平台的 Key 配置状态。返回值里任何密钥字段都会被结构性屏蔽，AI 助手看不到也拿不到密钥内容。',
    resultHint: '返回各平台的配置状态（不含密钥）',
    params: {},
    async run(ctx) {
      const response = await ctx.coreAction('provider-status')
      if (!response?.ok) return bad(response?.error || '无法读取平台状态')
      const payload = redact(response.result)
      return ok(`平台 Key 配置状态（已做脱敏）：\n${JSON.stringify(payload, null, 2)}`, payload)
    },
  },
  {
    id: 'nav.open',
    title: '切换到指定视图',
    kind: 'navigate',
    risk: 'low',
    needConfirm: false,
    summary: '打开某个页面（模型与服务 / 平台 / 运行日志 / 用量与额度 / 插件集成 / 关于与更新）',
    describe: '切换面板当前显示的视图，等同于点击侧栏对应条目。只是导航，不改动任何数据。',
    resultHint: '返回已切换到的视图名称',
    params: {
      view: { type: 'string', required: true, enum: NAVIGABLE_VIEWS, desc: '目标视图 id' },
    },
    async run(ctx, args) {
      const labels = {
        models: '模型与服务', providers: '平台', logs: '运行日志',
        usage: '用量与额度', workbuddy: '插件集成', about: '关于与更新',
      }
      ctx.navigate(args.view)
      return ok(`已切换到「${labels[args.view] || args.view}」。`, { view: args.view })
    },
  },
  {
    id: 'models.reprobe',
    title: '重新检测单个模型',
    kind: 'action',
    risk: 'medium',
    needConfirm: true,
    summary: '对指定模型发起一次探测（会消耗少量免费额度，数十秒）',
    describe: '只对指定模型重跑一次探测，不重跑全量检测。会向上游发送一次真实请求并消耗少量免费额度，耗时通常数十秒。核心忙碌或该模型正在检测中时会被拒绝。',
    resultHint: '返回是否已开始检测',
    params: {
      model: { type: 'string', required: true, max: 200, desc: '模型 id（来自 models.list）' },
    },
    async run(ctx, args) {
      const ready = requireReady(ctx)
      if (ready) return bad(ready)
      const found = findModel(ctx, args.model)
      if (found.error) return bad(found.error)
      const pending = stateOf(ctx).singleProbes
      if (Array.isArray(pending) && pending.includes(args.model)) return bad(`${args.model} 正在检测中，等它结束再试`)
      const response = await ctx.coreAction('probe', { model: args.model })
      if (!response?.ok) return bad(response?.error || '发起检测失败')
      const result = response.result || {}
      const refusal = result.error?.message || (result.started === false ? result.message : '')
      if (refusal) return bad(refusal)
      return ok(`已向 ${args.model} 发起重新检测，结果会在模型详情里更新。`, redact(result))
    },
  },
  {
    id: 'core.refresh',
    title: '读取免费模型',
    kind: 'action',
    risk: 'medium',
    needConfirm: true,
    summary: '重新拉取模型名单并逐个检测（耗时较长，会消耗少量额度）',
    describe: '等同于点击「读取免费模型」：从上游重新获取名单，并对每个模型发起一次简短请求完成检测。耗时从数十秒到数分钟，期间面板会显示检测进度。',
    resultHint: '返回刷新是否已提交',
    params: {},
    async run(ctx) {
      const ready = requireReady(ctx)
      if (ready) return bad(ready)
      return runCoreAction(ctx, 'refresh', undefined, '已开始读取免费模型，完成后模型列表会更新（期间可在「模型与服务」查看进度）。')
    },
  },
  {
    id: 'core.import',
    title: '导入模型到插件',
    kind: 'action',
    risk: 'medium',
    needConfirm: true,
    summary: '把当前可用模型写入 WorkBuddy / CodeBuddy 配置',
    describe: '等同于点击「导入 WorkBuddy / CodeBuddy」：把当前可用模型写入所有检测到的插件配置。若核心定位不到配置文件，会弹出文件选择框需要手动选择。',
    resultHint: '返回写入的目标与模型数量',
    params: {},
    async run(ctx) {
      const ready = requireReady(ctx)
      if (ready) return bad(ready)
      const response = await ctx.coreAction('import')
      if (!response?.ok) return bad(response?.error || '导入失败')
      const result = response.result || {}
      if (result.canceled) return ok('已取消导入，配置未更改。', { canceled: true })
      if (result.changed === false) return ok(`配置已是最新，共 ${result.count ?? 0} 个模型，无需重复写入。`, redact(result))
      const targets = result.targets || {}
      const written = Object.entries(targets)
        .filter(([, item]) => item && item.status === 'ok' && item.changed)
        .map(([name]) => name)
      return ok(
        `导入完成，共 ${result.count ?? 0} 个模型${written.length ? `，写入：${written.join('、')}` : ''}。`,
        redact(result),
      )
    },
  },
  {
    id: 'core.proxy',
    title: '设置系统代理开关',
    kind: 'action',
    risk: 'medium',
    needConfirm: true,
    summary: '打开或关闭核心使用系统代理',
    describe: '等同于侧栏的「使用系统代理」开关：控制核心访问上游时是否沿用系统代理。立即生效，会影响核心与上游的连接方式。',
    resultHint: '返回代理开关是否已应用',
    params: {
      enabled: { type: 'boolean', required: true, desc: 'true 打开，false 关闭' },
    },
    async run(ctx, args) {
      const ready = requireReady(ctx)
      if (ready) return bad(ready)
      const on = args.enabled === true
      return runCoreAction(ctx, 'system-proxy', { enabled: on }, on ? '已打开系统代理。' : '已关闭系统代理。')
    },
  },
  {
    id: 'core.restart',
    title: '重启核心服务',
    kind: 'action',
    risk: 'medium',
    needConfirm: true,
    summary: '重启核心进程（会中断正在进行的检测与请求）',
    describe: '等同于点「重启核心服务」：停止并重新拉起核心进程。正在进行的模型检测与请求会被中断，重启期间面板不可操作。',
    resultHint: '返回重启是否已提交',
    params: {},
    async run(ctx) {
      return runCoreAction(ctx, 'restart', undefined, '已提交重启，核心恢复后模型列表会重新加载。')
    },
  },
  {
    id: 'providers.clearKey',
    title: '清除平台 API Key',
    kind: 'action',
    risk: 'high',
    needConfirm: true,
    summary: '删除某个平台已配置的 Key（不可恢复，需重新申请填写）',
    describe: '清除指定平台已保存的 API Key。此操作不可恢复：清除后该平台的模型会失效，直到你重新填入 Key。只能清除，不能设置——设置 Key 必须在「平台」视图手动完成。',
    resultHint: '返回清除结果',
    params: {
      provider: { type: 'string', required: true, max: 80, desc: '平台名称（来自 providers.list）' },
    },
    async run(ctx, args) {
      const response = await ctx.coreAction('clear-provider-key', { provider: args.provider })
      if (!response?.ok) return bad(response?.error || `清除 ${args.provider} 的 Key 失败`)
      return ok(`已清除 ${args.provider} 的 Key。该平台需要重新配置才能使用。`, redact(response.result))
    },
  },
]

const TOOL_INDEX = new Map(TOOLS.map(tool => [tool.id, tool]))

export function findTool(id) {
  return typeof id === 'string' ? TOOL_INDEX.get(id) || null : null
}

export function toolIds() {
  return TOOLS.map(tool => tool.id)
}

// 给模型看的工具清单：只保留「选工具」所需的字段（id / 用途 / 参数名与取值），
// 不放 risk / 实现细节——那些是执行期的规则，不需要占用上下文。
export function toolCatalog(tools = TOOLS) {
  return tools.map((tool) => {
    const params = Object.entries(tool.params || {}).map(([name, spec]) => {
      const values = Array.isArray(spec.enum) && spec.enum.length ? `，取值 ${spec.enum.join('/')}` : ''
      return `${name}${spec.required ? '' : '?'}${values}`
    }).join(', ')
    return `${tool.id}(${params}) — ${tool.summary}`
  }).join('\n')
}

// 确认文案：必须把「要做什么、影响是什么」说清楚，而不是只报工具名。
export function confirmPrompt(tool, args) {
  const params = Object.entries(args || {}).map(([name, value]) => `${name}=${JSON.stringify(value)}`).join('，')
  return {
    title: tool.title,
    detail: `${tool.describe}${params ? `\n\n参数：${params}` : ''}`,
    risk: tool.risk,
  }
}
