// ops.js 的行为钉子：成功反馈文案、在飞互斥、完成冷却（节流）、拒绝节流窗口。
// 只测纯函数与常量，不依赖 DOM / Vue / Tauri；运行方式 `npm run test:ops`。
import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  ACTION_COOLDOWN_MS,
  COOLDOWN_ACTIONS,
  FEEDBACK_VISIBLE_MS,
  INFLIGHT_MESSAGE,
  INFLIGHT_THROTTLE_MS,
  cooldownMessage,
  rejectionOf,
  successMessage,
} from './ops.js'

// ── 常量与表 ────────────────────────────────────────────────────────

test('feedback visible window is finite and reasonable', () => {
  assert.ok(Number.isFinite(FEEDBACK_VISIBLE_MS) && FEEDBACK_VISIBLE_MS >= 2000 && FEEDBACK_VISIBLE_MS <= 10000)
})

test('cooldown window is finite and at least as long as the rejection throttle', () => {
  assert.ok(Number.isFinite(ACTION_COOLDOWN_MS) && ACTION_COOLDOWN_MS >= 1000)
  assert.ok(ACTION_COOLDOWN_MS >= INFLIGHT_THROTTLE_MS)
})

test('cooldown set covers exactly the terminal actions with labels', () => {
  assert.deepEqual([...COOLDOWN_ACTIONS].sort(), ['import', 'refresh', 'restart', 'system-proxy'])
  for (const action of COOLDOWN_ACTIONS) {
    assert.ok(!cooldownMessage(action).includes('undefined'))
    assert.ok(cooldownMessage(action).includes('刚完成'))
  }
  // probe 是提交型动作：不在冷却集合里，但提示表仍要有它的主语（在飞互斥文案共用标签）。
  assert.ok(!COOLDOWN_ACTIONS.includes('probe'))
  assert.ok(!cooldownMessage('probe').includes('undefined'))
})

// ── 在飞互斥（防抖）─────────────────────────────────────────────────

test('busy state rejects with the inflight message', () => {
  assert.equal(rejectionOf('refresh', { busy: true, lastFinished: {} }, 1000), INFLIGHT_MESSAGE)
})

test('rejection message stays stable so repeated clicks do not blink the banner', () => {
  // 同一动作在飞期间的拒绝文案必须逐字稳定（FeedbackBar 以 text 为 key，变化即重挂载）。
  assert.equal(rejectionOf('probe', { busy: true }, 1000), rejectionOf('probe', { busy: true }, 9_999_999))
})

// ── 完成冷却（节流）─────────────────────────────────────────────────

test('recently finished terminal action is rejected with a labeled cooldown message', () => {
  const state = { busy: false, lastFinished: { refresh: 9500 } }
  assert.equal(rejectionOf('refresh', state, 10_000), '读取免费模型刚完成，请稍候再试。')
  // 旧完成记录不拒绝。
  assert.equal(rejectionOf('refresh', state, 9500 + ACTION_COOLDOWN_MS), '')
})

test('different action is not blocked by another action’s cooldown', () => {
  const state = { busy: false, lastFinished: { refresh: 9900 } }
  assert.equal(rejectionOf('probe', state, 10_000), '')
})

test('clock rollback is treated as just-finished', () => {
  const state = { busy: false, lastFinished: { restart: 5000 } }
  assert.equal(rejectionOf('restart', state, 4000), cooldownMessage('restart'))
})

test('failed actions never enter cooldown because lastFinished is only written on success', () => {
  // 本测试钉的是契约：rejectionOf 只读 lastFinished；App.vue 只在成功分支写入。
  // 这里验证该字段确实不存在时全放行（失败路径不会写入）。
  assert.equal(rejectionOf('refresh', { busy: false, lastFinished: {} }, 10_000), '')
})

// ── 成功文案 ────────────────────────────────────────────────────────

test('success messages exist for terminal and submit actions', () => {
  assert.ok(successMessage('refresh', { count: 7 }).includes('7 个'))
  assert.ok(successMessage('refresh', {}).includes('免费模型'))
  assert.ok(successMessage('restart').includes('重启'))
  assert.equal(successMessage('system-proxy', { useSystemProxy: true }).includes('已开启'), true)
  assert.equal(successMessage('system-proxy', { useSystemProxy: false }).includes('已关闭'), true)
  assert.ok(successMessage('probe', { model: 'OC · demo' }).includes('OC · demo'))
  assert.ok(successMessage('probe', { started: true }).includes('检测已开始'))
  // started=false 的 probe 是核心拒收：文案为空，由 App.vue 的既有 refusal 分支负责。
  assert.equal(successMessage('probe', { started: false }), '')
})

test('import and provider key actions keep their view-specific feedback', () => {
  // import 逐目标汇报、平台卡片反馈都由视图自有机制承担，ops 不得代写。
  assert.equal(successMessage('import', { count: 3 }), '')
  assert.equal(successMessage('set-provider-key'), '')
})
