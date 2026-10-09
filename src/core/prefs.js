// 面板偏好的唯一读写边界：组件一律经本模块读写 localStorage，不直接触碰全局对象。
//
// 纪律（为什么限制这么死）：
// • 这里只放**界面偏好**。权威配置（api-key、系统代理状态、模型名单、models.json 路径）
//   由核心 / 壳的 settings.json 与 status.json 掌握，前端再存一份就会出现两处真相，
//   重启后谁覆盖谁无法确定。
// • 键名走**白名单**：白名单外的键一律拒写，读取时也只看白名单内的键。
//   这样即便未来有人误把配置对象整个塞进来，凭据也进不了 WebView 存储。
// • 任何一步失败都**静默降级**为「本次会话不记忆」：隐私模式、配额耗尽、WebView 禁用存储
//   都会让 localStorage 抛异常，偏好不是功能主干，绝不能把异常冒到面板上。
// • 纯函数（sanitize* / merge）与存储访问分离，前者可在没有 DOM 的环境里用 `node --test` 直接测。

const PREFIX = 'wb.'

// 侧栏导航的视图 id（与 SideBar.vue 的 groups 一一对应）。恢复偏好时用它做存在性校验，
// 视图被改名或删除后，旧偏好会自然回落默认值而不是渲染一个不存在的视图。
export const VIEW_IDS = ['models', 'providers', 'logs', 'usage', 'workbuddy', 'about']

export const DEFAULTS = {
  view: 'models',
  update: { autoCheck: true, lastCheckedAt: 0 },
}

// 上次静默检查距今超过这个间隔才再查一次；间隔内启动直接跳过，避免每次冷启动都打一次端点。
export const SILENT_CHECK_MIN_INTERVAL_MS = 12 * 60 * 60 * 1000

function asString(value, fallback) {
  return typeof value === 'string' && value ? value : fallback
}

function asFiniteNumber(value, fallback) {
  const n = Number(value)
  return Number.isFinite(n) && n >= 0 ? n : fallback
}

// 视图名必须仍在入口表内；否则回落默认值。
export function sanitizeView(raw) {
  const value = asString(raw, DEFAULTS.view)
  return VIEW_IDS.includes(value) ? value : DEFAULTS.view
}

// 只投影已知字段：未知字段丢弃，类型不符回落默认，绝不原样回存。
export function sanitizeUpdate(raw) {
  const source = raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {}
  return {
    autoCheck: source.autoCheck === undefined ? DEFAULTS.update.autoCheck : source.autoCheck === true,
    lastCheckedAt: asFiniteNumber(source.lastCheckedAt, DEFAULTS.update.lastCheckedAt),
  }
}

const SANITIZERS = {
  view: sanitizeView,
  update: sanitizeUpdate,
}

// 偏好 JSON 的上限：界面偏好本就是一两个字段，超限说明被塞了别的东西（例如整份配置）。
const MAX_JSON_BYTES = 2048

export function isAllowedKey(key) {
  return Object.prototype.hasOwnProperty.call(SANITIZERS, key)
}

// localStorage 可能在禁用 Web Storage 的环境里直接抛错，所以每次访问都即时取、即时包。
function storage() {
  try {
    const store = globalThis.localStorage
    return store && typeof store.getItem === 'function' && typeof store.setItem === 'function' ? store : null
  } catch {
    return null
  }
}

// 读取并清洗一个偏好项；缺失、坏 JSON、类型不符一律回落默认值。
export function readPref(key) {
  if (!isAllowedKey(key)) return DEFAULTS[key]
  const store = storage()
  if (!store) return DEFAULTS[key]
  let raw
  try {
    raw = store.getItem(PREFIX + key)
  } catch {
    return DEFAULTS[key]
  }
  if (typeof raw !== 'string' || !raw) return DEFAULTS[key]
  let parsed
  try {
    parsed = JSON.parse(raw)
  } catch {
    // 损坏的偏好（老版本写入、手工改过存储）不该影响渲染，直接当没存过。
    return DEFAULTS[key]
  }
  return SANITIZERS[key](parsed)
}

// 写入清洗后的值。白名单外、序列化失败、写盘失败都返回 false 且不抛出。
export function writePref(key, value) {
  if (!isAllowedKey(key)) return false
  const store = storage()
  if (!store) return false
  let encoded
  try {
    encoded = JSON.stringify(SANITIZERS[key](value))
  } catch {
    return false
  }
  if (encoded.length > MAX_JSON_BYTES) return false
  try {
    store.setItem(PREFIX + key, encoded)
  } catch {
    return false
  }
  return true
}

export function loadView() {
  return readPref('view')
}

export function saveView(id) {
  return writePref('view', id)
}

export function loadUpdatePref() {
  return readPref('update')
}

export function saveAutoCheck(enabled) {
  return writePref('update', { ...loadUpdatePref(), autoCheck: enabled === true })
}

export function saveCheckedAt(timestamp) {
  return writePref('update', { ...loadUpdatePref(), lastCheckedAt: asFiniteNumber(timestamp, DEFAULTS.update.lastCheckedAt) })
}

// 静默检查的节流判据：关掉自动检查、或距上次检查不足间隔，都不发请求。
export function shouldSilentCheck(pref = loadUpdatePref(), now = Date.now(), minInterval = SILENT_CHECK_MIN_INTERVAL_MS) {
  if (pref.autoCheck !== true) return false
  const last = asFiniteNumber(pref.lastCheckedAt, 0)
  return now - last >= minInterval
}
