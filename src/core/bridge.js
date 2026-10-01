// 前端业务内核：与 Tauri 壳的唯一边界。
// 契约与 Electron 版 window.buddy 一致（action/onState/onDismiss），
// 组件一律经由本模块调用，不直接触碰 window.__TAURI__。
import { activityText } from './activity.js'

const listeners = { state: [], dismiss: [] }
let started = false
// 最近一次完整状态：壳把 activity / modelResults 拆成 core-activity 单独发，
// 轻量快照与明细两路必须合并后再交给订阅者。
let lastState = {}

function publish(next) {
  lastState = next
  listeners.state.forEach(cb => cb(next))
}

function ensureBridge() {
  if (started) return
  started = true
  const { listen } = window.__TAURI__.event
  listen('core-status', event => publish(event.payload || {})).catch(console.error)
  listen('core-activity', event => publish({ ...lastState, ...(event.payload || {}) })).catch(console.error)
  listen('core-failed', event => {
    const payload = event.payload || {}
    publish({
      phase: 'error',
      message: payload.message || '核心服务启动失败',
      models: [], modelResults: {}, availableModels: [],
    })
  }).catch(console.error)
  listen('tauri://blur', () => listeners.dismiss.forEach(cb => cb())).catch(console.error)
}

export { activityText }

export function onState(cb) {
  ensureBridge()
  listeners.state.push(cb)
  // 立即回放缓存，避免组件挂载晚于首条事件时白屏等待；缓存为空（还没收到任何事件）时
  // 由壳的状态轮询补齐。
  cb(lastState)
}

export function onDismiss(cb) {
  ensureBridge()
  listeners.dismiss.push(cb)
}

export async function action(name, value) {
  ensureBridge()
  try {
    const { invoke } = window.__TAURI__.core
    if (name === 'restart') {
      await invoke('restart_core')
      return { ok: true, result: {} }
    }
    if (name === 'import' && !value) {
      const selected = await window.__TAURI__.dialog.open({
        multiple: false,
        directory: false,
        filters: [{ name: 'WorkBuddy models.json', extensions: ['json'] }],
      })
      if (!selected) return { ok: true, result: { canceled: true } }
      value = { modelsFile: selected }
    }
    const result = await invoke('core_action', { action: name, payload: value ?? null })
    return { ok: true, result }
  } catch (error) {
    return { ok: false, error: String(error) }
  }
}
