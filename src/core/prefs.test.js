// src/core/prefs.js 的单测：node --test，无 DOM、无框架、不联网。
// 覆盖收口门槛要求的三类行为：默认值合并、损坏数据降级、键白名单，外加存储不可用时的静默降级。
import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  DEFAULTS,
  SILENT_CHECK_MIN_INTERVAL_MS,
  VIEW_IDS,
  isAllowedKey,
  readPref,
  sanitizeUpdate,
  sanitizeView,
  saveAutoCheck,
  saveCheckedAt,
  saveView,
  shouldSilentCheck,
  writePref,
} from './prefs.js'

// 假 localStorage：可注入「读时抛错」「写时抛错」两种失效形态（隐私模式 / 配额耗尽）。
function fakeStorage(initial = {}) {
  const data = { ...initial }
  return {
    data,
    getItem(key) { return Object.prototype.hasOwnProperty.call(data, key) ? data[key] : null },
    setItem(key, value) { data[key] = String(value) },
  }
}

function withStorage(store, fn) {
  const had = 'localStorage' in globalThis
  const previous = had ? globalThis.localStorage : undefined
  if (store) globalThis.localStorage = store
  else delete globalThis.localStorage
  try {
    return fn()
  } finally {
    if (had) globalThis.localStorage = previous
    else delete globalThis.localStorage
  }
}

test('白名单外的键一律拒写，读取也只回落默认值', () => {
  assert.equal(isAllowedKey('view'), true)
  assert.equal(isAllowedKey('update'), true)
  assert.equal(isAllowedKey('apiKey'), false)
  assert.equal(isAllowedKey('modelsFile'), false)

  const store = fakeStorage()
  withStorage(store, () => {
    assert.equal(writePref('apiKey', 'secret'), false)
    assert.equal(writePref('modelsFile', '/some/path'), false)
    assert.deepEqual(Object.keys(store.data), [], '拒写的键不得留下任何存储痕迹')
    // 未登记的键读不到任何数据，只能拿到默认值（默认值表里没有就是 undefined，不编造）。
    assert.equal(readPref('apiKey'), undefined)
    assert.equal(readPref('modelsFile'), undefined)
  })
})

test('凭据类字段即使在值里也进不了存储', () => {
  withStorage(fakeStorage(), () => {
    // update 只投影 autoCheck / lastCheckedAt，多余字段（包括看似凭据的键）必须被丢掉。
    assert.equal(
      writePref('update', { autoCheck: true, apiKey: 'leak', token: 'leak', modelsFile: '/x' }),
      true,
    )
    const stored = JSON.parse(globalThis.localStorage.getItem('wb.update'))
    assert.deepEqual(Object.keys(stored).sort(), ['autoCheck', 'lastCheckedAt'])
    assert.equal(JSON.stringify(stored).includes('leak'), false)
    assert.equal(JSON.stringify(stored).includes('/x'), false)
  })
})

test('view：合法值往返，非法值回落默认', () => {
  assert.equal(sanitizeView('logs'), 'logs')
  for (const id of VIEW_IDS) assert.equal(sanitizeView(id), id)
  assert.equal(sanitizeView('nope'), DEFAULTS.view)
  assert.equal(sanitizeView(''), DEFAULTS.view)
  assert.equal(sanitizeView(null), DEFAULTS.view)
  assert.equal(sanitizeView(42), DEFAULTS.view)
  withStorage(fakeStorage(), () => {
    assert.equal(saveView('usage'), true)
    assert.equal(globalThis.localStorage.getItem('wb.view'), '"usage"')
    assert.equal(readPref('view'), 'usage')
    // 存了个已被删除的视图名（例如未来改名）→ 回落默认，而不是渲染空白页。
    globalThis.localStorage.setItem('wb.view', '"settings-v0"')
    assert.equal(readPref('view'), DEFAULTS.view)
  })
})

test('损坏 JSON 与类型不符一律降级，不抛异常', () => {
  const store = fakeStorage({ 'wb.view': '{not json', 'wb.update': 'null' })
  withStorage(store, () => {
    assert.equal(readPref('view'), DEFAULTS.view)
    assert.deepEqual(readPref('update'), DEFAULTS.update)
  })
  withStorage(fakeStorage({ 'wb.update': '[1,2,3]' }), () => {
    assert.deepEqual(readPref('update'), DEFAULTS.update)
  })
  withStorage(fakeStorage({ 'wb.update': '{"autoCheck":"yes","lastCheckedAt":"abc"}' }), () => {
    assert.deepEqual(readPref('update'), { autoCheck: false, lastCheckedAt: 0 })
  })
})

test('update：默认值合并与逐字段写回', () => {
  assert.deepEqual(sanitizeUpdate(undefined), DEFAULTS.update)
  assert.deepEqual(sanitizeUpdate({}), DEFAULTS.update)
  assert.deepEqual(sanitizeUpdate({ autoCheck: false }), { autoCheck: false, lastCheckedAt: 0 })
  assert.deepEqual(sanitizeUpdate({ lastCheckedAt: 1234 }), { autoCheck: true, lastCheckedAt: 1234 })
  // 负数与 NaN 都不是合法时间戳。
  assert.equal(sanitizeUpdate({ lastCheckedAt: -5 }).lastCheckedAt, 0)
  assert.equal(sanitizeUpdate({ lastCheckedAt: Number.NaN }).lastCheckedAt, 0)
  withStorage(fakeStorage(), () => {
    assert.equal(saveAutoCheck(false), true)
    assert.deepEqual(readPref('update'), { autoCheck: false, lastCheckedAt: 0 })
    // 只改一个字段不能把另一个抹掉（读-改-写必须经过同一份清洗）。
    assert.equal(saveCheckedAt(1_700_000_000_000), true)
    assert.deepEqual(readPref('update'), { autoCheck: false, lastCheckedAt: 1_700_000_000_000 })
  })
})

test('存储不可用时静默降级：读写都不抛、返回默认值 / false', () => {
  withStorage(null, () => {
    assert.equal(readPref('view'), DEFAULTS.view)
    assert.deepEqual(readPref('update'), DEFAULTS.update)
    assert.equal(writePref('view', 'logs'), false)
    assert.equal(saveView('logs'), false)
    assert.equal(saveAutoCheck(false), false)
    assert.equal(saveCheckedAt(1), false)
  })

  // getItem / setItem 抛错（Safari 隐私模式一类）同样不能把异常传出去。
  withStorage({ getItem() { throw new Error('blocked') }, setItem() { throw new Error('quota') } }, () => {
    assert.equal(readPref('view'), DEFAULTS.view)
    assert.equal(writePref('view', 'logs'), false)
  })

  // localStorage 存在但不是对象（被改写过的极端情况）。
  withStorage({ getItem: 'nope' }, () => {
    assert.equal(readPref('view'), DEFAULTS.view)
    assert.equal(writePref('view', 'logs'), false)
  })
})

test('超大值不进存储：清洗后只剩已知字段，超出上限一律拒写', () => {
  withStorage(fakeStorage(), () => {
    // 多余字段（哪怕塞 1MB 文本）在清洗阶段就被丢掉，落盘的仍是两个字段。
    assert.equal(writePref('update', { autoCheck: true, blob: 'x'.repeat(1024 * 1024) }), true)
    assert.ok(globalThis.localStorage.getItem('wb.update').length < 2048)
    // view 只可能是字符串；超长字符串不是合法视图名，回落默认值后即远低于上限。
    assert.equal(writePref('view', 'y'.repeat(1024 * 1024)), true)
    assert.equal(readPref('view'), DEFAULTS.view)
  })
})

test('静默检查节流：开关关闭、间隔内、间隔后', () => {
  const now = 1_700_000_000_000
  assert.equal(shouldSilentCheck({ autoCheck: false, lastCheckedAt: 0 }, now), false)
  assert.equal(shouldSilentCheck({ autoCheck: true, lastCheckedAt: now - 1000 }, now), false)
  assert.equal(
    shouldSilentCheck({ autoCheck: true, lastCheckedAt: now - SILENT_CHECK_MIN_INTERVAL_MS }, now),
    true,
  )
  assert.equal(shouldSilentCheck({ autoCheck: true, lastCheckedAt: 0 }, now), true)
  // 时间戳在未来（改过系统时钟）不应永远拦住检查。
  assert.equal(shouldSilentCheck({ autoCheck: true, lastCheckedAt: now + 10 * 60 * 1000 }, now), false)
})
