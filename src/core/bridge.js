// 前端业务内核：与 Tauri 壳的唯一边界。
// 契约与 Electron 版 window.buddy 一致（action/onState/onDismiss），
// 组件一律经由本模块调用，不直接触碰 window.__TAURI__。
import { activityText } from './activity.js'

const listeners = { state: [], dismiss: [] }
let started = false

function ensureBridge() {
  if (started) return
  started = true
  const { listen } = window.__TAURI__.event
  listen('core-status', event => listeners.state.forEach(cb => cb(event.payload))).catch(console.error)
  listen('core-failed', event => {
    const payload = event.payload || {}
    listeners.state.forEach(cb => cb({
      phase: 'error',
      message: payload.message || '核心服务启动失败',
      models: [], modelResults: {}, availableModels: [],
    }))
  }).catch(console.error)
  listen('tauri://blur', () => listeners.dismiss.forEach(cb => cb())).catch(console.error)
}

export { activityText }

export function onState(cb) {
  ensureBridge()
  listeners.state.push(cb)
  // 订阅时回放最近一次状态，避免组件挂载晚于首条事件时白屏等待。
  const { getCurrent } = window.__TAURI__
  getCurrent?.window?.emit?.('wb-bridge/replay-request').catch?.(() => {})
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
