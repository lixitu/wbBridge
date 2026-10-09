// 面板操作守卫的唯一实现：成功反馈文案 + 在飞互斥（防抖）+ 完成冷却（节流）。
//
// 为什么集中在这里（AGENTS.md「交互反馈分层」的落点）：
// • 每一个触发后台动作的入口都必须让用户知道「点了之后发生了什么」——进行中给 spinner，
//   结束给成功/失败文案，绝不允许「点了没反应」；成功文案自动消隐，失败文案常驻可关闭。
// • 同一动作在飞时重复触发一律拒绝并提示原因（防抖）；终态动作刚成功完成的短窗口内
//   再触发也要拒绝（节流冷却）——refresh / system-proxy 这类动作会重启隔离子进程并重跑
//   探测，连点只会把刚拿到的结果又冲掉。失败不进入冷却：马上重试是正当操作。
// • 拒绝提示本身按 INFLIGHT_THROTTLE_MS 节流，长按 / 连点不会把反馈条变成闪烁的报错灯。
// 纯函数 + 常量，不依赖 DOM / Vue / Tauri，`node --test`（npm run test:ops）直测。

// 成功文案的显示时长，之后自动清除。给足阅读时间，又不会在下一轮操作前挡视线。
export const FEEDBACK_VISIBLE_MS = 5000
// 动作成功完成后的冷却窗口：期间重复触发同一动作一律提示「刚完成」。依据：refresh 一轮
// = 重启子进程 + 重新发现 + 自动探测（几十秒），没有「秒内重刷」的正当场景。
export const ACTION_COOLDOWN_MS = 5000
// 拒绝类提示的节流间隔：同一次在飞 / 冷却期间的连点只提示一次。
export const INFLIGHT_THROTTLE_MS = 800

// 在飞中重复触发的统一提示。
export const INFLIGHT_MESSAGE = '操作正在进行中，请稍候。'

// 进入冷却的「终态动作」集合：动作本身在响应返回时就已整体完成（含其触发的后台收尾）。
// probe 不在内——它只负责把探测任务提交给核心，真正结束由状态推送（probe.running /
// singleProbes）表达，提交后马上再次提交会被在飞互斥与核心侧闸门拦住，不该叫「刚完成」。
export const COOLDOWN_ACTIONS = ['refresh', 'restart', 'system-proxy', 'import']

// 冷却提示里的动作名词（让「刚完成」有主语，用户才知道是哪一步被挡住了）。
const COOLDOWN_LABELS = {
  refresh: '读取免费模型',
  restart: '重启核心',
  'system-proxy': '切换系统代理',
  import: '导入配置',
  probe: '模型检测',
}

export function cooldownMessage(action) {
  return `${COOLDOWN_LABELS[action] || '此操作'}刚完成，请稍候再试。`
}

// 是否应当拒绝本次触发：返回拒绝文案（空串 = 放行）。
// state 形状：{ busy: boolean, lastFinished: { [action]: 完成时间戳 } }，由调用方从
// busyAction / 冷却表构造；now 与 cooldownMs 注入是为了可测（时钟回拨场景一并覆盖）。
export function rejectionOf(action, { busy = false, lastFinished = {} } = {}, now = Date.now(), cooldownMs = ACTION_COOLDOWN_MS) {
  if (busy === true) return INFLIGHT_MESSAGE
  const finishedAt = Number(lastFinished?.[action]) || 0
  if (finishedAt > 0) {
    const elapsed = now - finishedAt
    // elapsed < 0（时钟回拨）视同刚完成：宁可多挡一次，也不放行一次可能叠跑的重启。
    if (elapsed < 0 || elapsed < cooldownMs) return cooldownMessage(action)
  }
  return ''
}

// 成功反馈文案表：只覆盖「响应返回即整体完成」或「提交成功需要落点」的动作。
// 返回空串 = 该动作的成功反馈由视图自有的机制承担（如 import 的逐目标汇报、
// set-provider-key 的卡片内反馈），这里不得代写一份语义不符的。
export function successMessage(action, result = {}) {
  if (action === 'refresh') {
    const count = result?.count
    return Number.isFinite(count)
      ? `已重新读取免费模型，共 ${count} 个，检测随后自动开始。`
      : '免费模型已重新读取，检测随后自动开始。'
  }
  if (action === 'restart') return '核心服务已重启，模型服务恢复中。'
  if (action === 'system-proxy') {
    if (result?.useSystemProxy === true) return '系统代理已开启，模型服务已按新代理重新接入。'
    if (result?.useSystemProxy === false) return '系统代理已关闭，模型服务已改回直连。'
    return '系统代理设置已应用，模型服务已重新接入。'
  }
  if (action === 'probe') {
    if (result?.model) return `已提交重新检测（${result.model}），结果稍后自动更新。`
    if (result?.started !== false) return '模型检测已开始，完成后列表自动刷新。'
    return ''
  }
  return ''
}
