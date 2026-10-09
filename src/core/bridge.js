// 前端业务内核：与 Tauri 壳的唯一边界。
// 契约与 Electron 版 window.buddy 一致（action/onState/onDismiss），
// 组件一律经由本模块调用，不直接触碰 window.__TAURI__。
import { activityText } from './activity.js'

const listeners = { state: [], dismiss: [] }
let started = false
// 最近一次完整状态：壳把 activity / modelResults 拆成 core-activity 单独发，
// 轻量快照与明细两路必须合并后再交给订阅者。
let lastState = {}

// 帧级合并发布：壳对同一帧变化连发 core-status + core-activity 两个事件，
// 若各自立刻 publish，根状态会被赋值两次、整树渲染两趟（探测期间成倍）。
// 改为把事件按到达顺序折算成状态操作入队，每帧最多 flush 一次，
// 渲染次数从「每事件一次」降为「每帧一次」。
// 对**订阅者**而言合并语义与逐个执行等价（status=替换、activity=合并、failed=替换，FIFO 保序、
// 后者覆盖前者）；但 `lastState` 要到帧末才更新，所以事件到达与 flush 之间的**同步读取点**
// 会读到旧一帧——目前是 `action()` 里的 `lastState.modelsFile` 与 `onState()` 的首次回放。
// 影响是极小概率多弹一次文件选择框 / 晚一帧的快照，下一帧自愈；新增同步读取点时必须想到这条。
const pendingOps = []
let flushScheduled = false

const scheduleFlush = typeof window.requestAnimationFrame === 'function'
  ? window.requestAnimationFrame.bind(window)
  : (cb) => setTimeout(cb, 16)

// rAF 在窗口不可见时会被暂停（本项在 WKWebView 里的真实停摆行为**未实测**）。
// 因此替换型 op 入队前先清空队列：它的 payload 整体替换状态，前面排队的 op 的产出必然被丢弃，
// 清空与逐条折叠严格等价，却让队列长度天然封顶（否则最小化期间每 ~0.5s 堆两个持有整份
// payload 的闭包，恢复显示时一帧内全部折叠）。
function enqueue(op, replaces = false) {
  if (replaces) pendingOps.length = 0
  pendingOps.push(op)
  if (flushScheduled) return
  flushScheduled = true
  scheduleFlush(() => {
    flushScheduled = false
    const ops = pendingOps.splice(0)
    let next = lastState
    for (const applyOp of ops) next = applyOp(next)
    publish(next)
  })
}

function publish(next) {
  lastState = next
  listeners.state.forEach(cb => cb(next))
}

function ensureBridge() {
  if (started) return
  started = true
  const { listen } = window.__TAURI__.event
  listen('core-status', event => enqueue(() => event.payload || {}, true)).catch(console.error)
  listen('core-activity', event => enqueue(prev => ({ ...prev, ...(event.payload || {}) }))).catch(console.error)
  listen('core-failed', event => {
    const payload = event.payload || {}
    enqueue(() => ({
      phase: 'error',
      message: payload.message || '核心服务启动失败',
      models: [], modelResults: {}, availableModels: [],
    }), true)
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
      // 核心的 /admin/import 在不传 modelsFile 时就是「同步到已解析配置」（与归档 JS 核心的
      // importModels() 语义一致）；只有定位不到配置时才需要用户手选文件。因此弹框是兜底，
      // 不是无条件的前置步骤——否则每次点「导入」都会被要求选目录。
      // 能走到这里按钮必然处于可用态（phase=ready），lastState 已由 core-status 填好。
      const resolved = typeof lastState.modelsFile === 'string' && lastState.modelsFile
      if (!resolved) {
        const selected = await window.__TAURI__.dialog.open({
          multiple: false,
          directory: false,
          filters: [{ name: 'WorkBuddy models.json', extensions: ['json'] }],
        })
        if (!selected) return { ok: true, result: { canceled: true } }
        value = { modelsFile: selected }
      }
    }
    const result = await invoke('core_action', { action: name, payload: value ?? null })
    return { ok: true, result }
  } catch (error) {
    return { ok: false, error: String(error) }
  }
}

// 以下两条是**只读**调用（新增视图用），与上面的 action/onState/onDismiss 契约互不影响：
// 面板不得借它们写文件，也不接受任何路径参数。

// 运行日志尾部：由壳读数据目录下的日志文件，截断上限在壳侧常量里（不在前端拼接路径）。
export async function readLog() {
  ensureBridge()
  try {
    const { invoke } = window.__TAURI__.core
    return { ok: true, result: await invoke('read_log') }
  } catch (error) {
    return { ok: false, error: String(error) }
  }
}

// 用系统浏览器打开外部链接（申请 Key 的官方页）。WebView 里 target=_blank 默认点不开，
// 必须经壳的 open_external 命令转发（壳侧有 https + 字符白名单校验）。只收 URL、不做任何 IO。
export async function openExternal(url) {
  ensureBridge()
  try {
    const { invoke } = window.__TAURI__.core
    await invoke('open_external', { url })
    return { ok: true, result: {} }
  } catch (error) {
    return { ok: false, error: String(error) }
  }
}

// 壳实际使用的数据目录（status.json / 运行日志都在这里）。
export async function dataDir() {
  ensureBridge()
  try {
    const { invoke } = window.__TAURI__.core
    return { ok: true, result: await invoke('data_dir_path') }
  } catch (error) {
    return { ok: false, error: String(error) }
  }
}

// ── 自动更新：updater / process 插件的封装 ─────────────────────────
// 两个插件的 JS 绑定由 withGlobalTauri 注入到 window.__TAURI__（见 tauri-plugin-*
// 的 api-iife.js），所以不需要任何 @tauri-apps/plugin-* npm 依赖；组件依旧不得直触全局对象。
// check() 返回的 Update 句柄只在模块内持有：交给组件就没法再调 downloadAndInstall，
// 且它带有一整组不可序列化的方法。

let pendingUpdate = null

function updateError(error) {
  return String(error?.message ?? error)
}

// 发现新版本时返回 { version, notes }；已是最新返回 result = null。
export async function checkUpdate() {
  ensureBridge()
  const updater = window.__TAURI__?.updater
  if (!updater?.check) return { ok: false, error: '此构建不含更新通道' }
  try {
    const update = await updater.check()
    pendingUpdate = update ?? null
    if (!update) return { ok: true, result: null }
    return {
      ok: true,
      result: { version: String(update.version ?? ''), notes: String(update.body ?? '') },
    }
  } catch (error) {
    pendingUpdate = null
    return { ok: false, error: updateError(error) }
  }
}

// onProgress({ received, total })：三个数都来自 updater 事件的真实字节数，
// 拿不到 contentLength 时 total 为 0，由调用方降级为「不显示百分比」。
export async function downloadUpdate(onProgress) {
  if (!pendingUpdate) return { ok: false, error: '没有待安装的更新，请先检查更新' }
  let received = 0
  let total = 0
  try {
    await pendingUpdate.downloadAndInstall(event => {
      const data = event?.data || {}
      if (event?.event === 'Started') {
        total = Number(data.contentLength) || 0
        received = 0
      } else if (event?.event === 'Progress') {
        received += Number(data.chunkLength) || 0
      }
      if (onProgress) onProgress({ received, total })
    })
    pendingUpdate = null
    return { ok: true, result: {} }
  } catch (error) {
    // 安装失败时句柄仍要留着，用户可以直接重试；只有明确成功才清空。
    return { ok: false, error: updateError(error) }
  }
}

// 装完重启才会用上新版本。relaunch 触发 ExitRequested(RESTART_EXIT_CODE)，
// 壳的 RunEvent::ExitRequested 分支里已有 graceful_stop + cleanup_before_exit，
// 所以这条路径同样会先停核心、收掉 OpenCode 子进程，不为更新另开一条退出链路。
export async function relaunchApp() {
  ensureBridge()
  const process = window.__TAURI__?.process
  if (!process?.relaunch) return { ok: false, error: '此构建不支持重启' }
  try {
    await process.relaunch()
    return { ok: true, result: {} }
  } catch (error) {
    return { ok: false, error: updateError(error) }
  }
}
