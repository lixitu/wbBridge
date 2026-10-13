// 索引的持久化层：IndexedDB 读写封装。
//
// 存什么：整份 BM25 索引（docs / df / postings / docById 等，见 bm25.js）作为**单个**
// 记录放进一个对象仓库。文档规模在百篇量级、词项在万级，单记录写入（几十 KB ~ 数百 KB）
// 远比逐词项建倒排表事务更快，也天然避免了「写到一半的索引」——一次事务要么全有要么全无。
//
// 纪律：
// • 任何一步失败都不抛异常，只返回 false / null。索引是**加速器**，不是功能主干：
//   拿不到持久化就退回内存索引重建（本轮会话仍可用），绝不让存储异常冒到界面上。
// • 打开数据库与读写都带超时。隐私模式 / 存储被策略禁用时，`indexedDB.open` 可能既不
//   成功也不失败地挂着，没有超时就会把整个索引初始化流程永久堵死。
// • 版本不一致（INDEX_VERSION 变化）时由调用方决定重建，本模块只负责如实回报版本。
const DB_NAME = 'wb-bridge-assistant'
const DB_VERSION = 1
const STORE_NAME = 'index'
const INDEX_KEY = 'bm25'
const META_KEY = 'meta'
const OPEN_TIMEOUT_MS = 4000
const IO_TIMEOUT_MS = 6000

let lastError = ''

function idb() {
  try {
    return globalThis.indexedDB || null
  } catch {
    return null
  }
}

export function indexedDbAvailable() {
  return !!idb()
}

export function storageError() {
  return lastError
}

function fail(message) {
  lastError = message
  return null
}

// 给可能永久挂起的 IDB 请求套一层超时。超时不是「失败」而是「放弃等待」，
// 调用方一律按不可用处理。
function withTimeout(request, ms, label) {
  return new Promise((resolve, reject) => {
    let settled = false
    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      reject(new Error(`${label}超时`))
    }, ms)
    request.onsuccess = () => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve(request.result)
    }
    request.onerror = () => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      reject(request.error || new Error(`${label}失败`))
    }
  })
}

let dbPromise = null

function openDatabase() {
  if (dbPromise) return dbPromise
  const factory = idb()
  if (!factory) return Promise.resolve(fail('当前环境不支持 IndexedDB'))
  dbPromise = new Promise((resolve) => {
    let request
    try {
      request = factory.open(DB_NAME, DB_VERSION)
    } catch (error) {
      resolve(fail(`打开索引库失败：${String(error)}`))
      return
    }
    const timer = setTimeout(() => resolve(fail('打开索引库超时')), OPEN_TIMEOUT_MS)
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(STORE_NAME)) db.createObjectStore(STORE_NAME)
    }
    request.onsuccess = () => {
      clearTimeout(timer)
      const db = request.result
      // 其它标签页要升级版本时，本连接必须让路，否则升级会一直等它关闭。
      db.onversionchange = () => {
        db.close()
        dbPromise = null
      }
      resolve(db)
    }
    request.onerror = () => {
      clearTimeout(timer)
      resolve(fail(`打开索引库失败：${request.error?.message || request.error || '未知原因'}`))
    }
    request.onblocked = () => {
      clearTimeout(timer)
      resolve(fail('索引库被其它窗口占用，升级被阻塞'))
    }
  })
  return dbPromise
}

function transact(mode, work) {
  return openDatabase().then((db) => {
    if (!db) return null
    return new Promise((resolve) => {
      let transaction
      try {
        transaction = db.transaction(STORE_NAME, mode)
      } catch (error) {
        resolve(fail(`索引事务创建失败：${String(error)}`))
        return
      }
      const store = transaction.objectStore(STORE_NAME)
      let result
      try {
        result = work(store)
      } catch (error) {
        resolve(fail(`索引操作失败：${String(error)}`))
        return
      }
      withTimeout(result, IO_TIMEOUT_MS, '索引读写')
        .then(value => resolve(value))
        .catch(error => resolve(fail(String(error?.message || error))))
      transaction.onabort = () => resolve(fail('索引事务被中止（可能是存储空间不足）'))
      transaction.onerror = () => resolve(fail(`索引事务错误：${transaction.error?.message || '未知原因'}`))
    })
  }).catch(error => fail(String(error)))
}

// 读取已持久化的索引。返回 null 表示「没有可用索引」——缺失、损坏、版本不符
// 或存储不可用，调用方一律按「需要重建」处理。
export async function readStoredIndex(expectedVersion) {
  const raw = await transact('readonly', store => store.get(INDEX_KEY))
  if (!raw || typeof raw !== 'object' || !Array.isArray(raw.docs) || !raw.postings) return null
  if (Number.isInteger(expectedVersion) && raw.version !== expectedVersion) return null
  return raw
}

// 写入索引。成功返回 true；失败返回 false 并保留 storageError() 里的原因。
export async function writeStoredIndex(index) {
  if (!index || typeof index !== 'object') return false
  const saved = await transact('readwrite', store => store.put(index, INDEX_KEY))
  if (saved === null) return false
  const metaSaved = await transact('readwrite', store => store.put({
    builtAt: index.builtAt || '',
    version: index.version || 0,
    docs: index.N || 0,
  }, META_KEY))
  // 索引本体已落盘即可用（meta 只承载「上次构建时间」这类展示信息），但按函数契约
  // 如实上报：meta 写失败也算失败，由调用方决定是否提示降级，不做静默「成功」。
  if (metaSaved === null) return false
  return true
}

// 清空索引（重建失败后的兜底，以及「索引损坏」时的恢复入口）。
export async function clearStoredIndex() {
  const done = await transact('readwrite', store => store.clear())
  return done !== null
}

// 读取索引元信息（不含内容），用于界面展示「上次构建时间」这类信息而不必反序列化整份索引。
export async function readStoredMeta() {
  const meta = await transact('readonly', store => store.get(META_KEY))
  return meta && typeof meta === 'object' ? meta : null
}
