// 自动更新的唯一状态机：App.vue 启动后静默检查一次，「关于与更新」视图只渲染与触发。
// 与壳/插件的所有交互都经 bridge.js，本模块不碰 window.__TAURI__。
// 自动检查开关与上次检查时间是界面偏好，读写统一走 prefs.js（白名单 + 静默降级）。
import { checkUpdate, downloadUpdate, relaunchApp } from './bridge.js'
import { loadUpdatePref, saveAutoCheck, saveCheckedAt, shouldSilentCheck } from './prefs.js'

// 冷启动延迟：避免与核心启动（下载运行时、探测模型）抢带宽与 CPU。
const SILENT_CHECK_DELAY_MS = 5000

// idle：还没结果（含静默检查失败——不打扰用户）；其余一一对应更新流程。
const current = {
  status: 'idle',
  version: '',
  notes: '',
  received: 0,
  total: 0,
  error: '',
  checkedAt: 0,
  // 上次检查时间戳由 prefs 恢复：面板重开时「上次检查：…」不该显示成空。
  autoCheck: loadUpdatePref().autoCheck,
}

const listeners = []

function snapshot() {
  return { ...current }
}

function publish(next) {
  Object.assign(current, next)
  listeners.forEach(cb => cb(snapshot()))
}

export function subscribe(cb) {
  listeners.push(cb)
  cb(snapshot())
  return () => {
    const index = listeners.indexOf(cb)
    if (index > -1) listeners.splice(index, 1)
  }
}

// silent：冷启动的后台检查。只有真的发现新版本才改状态，
// 无更新与检查失败都保持安静（视图不该为「一切正常」冒出噪音）。
export async function check({ silent = false } = {}) {
  if (current.status === 'checking' || current.status === 'downloading') return snapshot()
  publish({ status: 'checking', error: '' })
  const response = await checkUpdate()
  if (!response.ok) {
    publish({ status: silent ? 'idle' : 'error', error: silent ? '' : response.error })
    return snapshot()
  }
  // 只有真的联系过端点才记时间戳：失败也记会把下一次重试一起节流掉。
  const checkedAt = Date.now()
  saveCheckedAt(checkedAt)
  if (!response.result) {
    publish({ status: 'uptodate', version: '', notes: '', checkedAt })
    return snapshot()
  }
  publish({
    status: 'available',
    version: response.result.version,
    notes: response.result.notes,
    checkedAt,
  })
  return snapshot()
}

export async function install() {
  if (current.status !== 'available') return snapshot()
  publish({ status: 'downloading', received: 0, total: 0 })
  const response = await downloadUpdate(({ received, total }) => {
    publish({ received, total })
  })
  if (!response.ok) {
    publish({ status: 'error', error: response.error, received: 0, total: 0 })
    return snapshot()
  }
  publish({ status: 'ready', received: 0, total: 0 })
  return snapshot()
}

export function restart() {
  return relaunchApp()
}

// FeedbackBar 的关闭按钮：只清掉错误提示，不动已发现的版本（否则关掉一条报错会把更新一起丢掉）。
export function dismiss() {
  if (current.status !== 'error') return
  publish({ status: current.version ? 'available' : 'idle', error: '' })
}

// 「启动后自动检查更新」开关：写偏好 + 立刻反映到状态机，视图不必自己存。
// 关掉只影响冷启动的静默检查，手动「检查更新」始终可用。
export function setAutoCheck(enabled) {
  const next = enabled === true
  saveAutoCheck(next)
  publish({ autoCheck: next })
  return snapshot()
}

let started = false

export function startSilentCheck() {
  if (started) return
  started = true
  setTimeout(() => {
    // 节流依据存在 prefs 里（跨重启有效）：刚检查过或用户关了自动检查就不打扰。
    if (!shouldSilentCheck()) return
    check({ silent: true })
  }, SILENT_CHECK_DELAY_MS)
}
