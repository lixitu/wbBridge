// 检索层的文本归一化与分词。
//
// 为什么自己分词：项目依赖只有 vue 与 @tauri-apps/*，没有任何 NLP / 分词库，
// 而中文没有词边界——直接按空格切会把整句当成一个词，tf / df / 文档长度全部失真，
// BM25 退化成「整句字符串相等才算命中」。
//
// 这里采用 **单字（unigram）+ 二元组（bigram）** 方案，也就是 Lucene CJKAnalyzer 一路的
// 通用做法：不依赖词典即可覆盖任意未登录词，查询侧与文档侧用**同一套**切分规则，
// 因此「相邻字对命中」等价于「短语局部命中」，再由 BM25 的 IDF 把高频单字（的 / 是 / 个）
// 的权重自然压低。单字保留是为了短查询（「额度」「日志」）与单字查询仍能召回，
// 对本文档规模（百篇量级、每篇千字内）带来的索引膨胀可以忽略。
//
// 本模块是纯函数、无 DOM、无 IO，可直接被 `node --test` 覆盖（src/core/text.test.js）。

// 全角 ASCII（！～）与全角空格 → 半角：让「ＡＰＩ」与「api」落到同一个词项。
function toHalfWidth(input) {
  return input
    .replace(/[\uff01-\uff5e]/g, ch => String.fromCharCode(ch.charCodeAt(0) - 0xfee0))
    .replace(/\u3000/g, ' ')
}

// 归一化：全角转半角 → 英文小写 → 空白折叠。检索与索引两侧都必须先过这一步，
// 否则「检测」「检测 」会被当成两个不同词项。
export function normalizeText(input) {
  if (typeof input !== 'string' || !input) return ''
  return toHalfWidth(input).toLowerCase().replace(/\s+/g, ' ').trim()
}

// 单个汉字/假名停用字：只在**单字**位置过滤（bigram 带上下文，不过滤）。
// 不做完整停用词表——中文虚词的单字形态有限，而 bigram 已能提供上下文约束。
const STOP_CHARS = new Set([
  '的', '了', '是', '在', '和', '与', '及', '或', '我', '你', '他', '她', '它', '们',
  '这', '那', '有', '没', '吗', '呢', '吧', '啊', '把', '被', '就', '都', '也', '很',
  '个', '为', '对', '从', '到', '会', '能', '要', '不', '之', '其', '并', '而', '以',
])

function isCjk(ch) {
  const code = ch.codePointAt(0)
  return (code >= 0x3400 && code <= 0x4dbf)
    || (code >= 0x4e00 && code <= 0x9fff)
    || (code >= 0xf900 && code <= 0xfaff)
    || (code >= 0x3040 && code <= 0x30ff)
    || (code >= 0xac00 && code <= 0xd7af)
}

// 归一化之后的 ASCII 词内字符：字母、数字与常见的标识符分隔符。
// 带 `-` `.` `+` `#` 是为了让 model id（gpt-4o / c++ / c#）与命令名（core-refresh）
// 始终保持为**一个**词项；它们是否再拆成子词由 flushLatin 决定。
function isLatin(ch) {
  return (ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9') || ch === '_' || ch === '-' || ch === '+' || ch === '#' || ch === '.'
}

const LATIN_TRIM = /^[._+#-]+|[._+#-]+$/g
const LATIN_SPLIT = /[._+#-]+/

// 分词：中文段 → 单字 + 相邻二元组；ASCII 段 → 整体词项 + 按分隔符拆出的子词。
// 标点、空白一律作为边界丢弃。
export function tokenize(input) {
  const text = normalizeText(input)
  const tokens = []
  if (!text) return tokens

  let latin = ''
  let cjk = ''

  const flushLatin = () => {
    if (!latin) return
    const word = latin.replace(LATIN_TRIM, '')
    latin = ''
    if (!word) return
    tokens.push(word)
    // 复合标识符（core-refresh / v1.models）再拆一层，让自然语言里的
    // 「core refresh」也能命中；单段词不拆，避免产生与整体重复的词项。
    const parts = word.split(LATIN_SPLIT).filter(part => part.length > 0 && part !== word)
    if (parts.length > 1) tokens.push(...parts)
  }

  const flushCjk = () => {
    if (!cjk) return
    const run = cjk
    cjk = ''
    if (run.length === 1) {
      if (!STOP_CHARS.has(run)) tokens.push(run)
      return
    }
    for (let i = 0; i < run.length; i += 1) {
      const ch = run[i]
      if (!STOP_CHARS.has(ch)) tokens.push(ch)
      if (i + 1 < run.length) tokens.push(run.slice(i, i + 2))
    }
  }

  for (const ch of text) {
    if (isCjk(ch)) {
      flushLatin()
      cjk += ch
    } else if (isLatin(ch)) {
      flushCjk()
      latin += ch
    } else {
      flushLatin()
      flushCjk()
    }
  }
  flushLatin()
  flushCjk()
  return tokens
}

// 文档指纹：用于索引增量更新时判断「内容是否真的变了」。
// FNV-1a 32 位足以区分文档改写，且不需要引入哈希依赖。
export function fingerprint(input) {
  const text = normalizeText(input)
  let hash = 0x811c9dc5
  for (let i = 0; i < text.length; i += 1) {
    hash ^= text.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return hash.toString(16).padStart(8, '0')
}

// 摘要：检索命中的文档要给模型看正文，但不能把整篇灌进上下文。
export function excerpt(input, limit = 240) {
  const text = typeof input === 'string' ? input.replace(/\s+/g, ' ').trim() : ''
  if (text.length <= limit) return text
  return `${text.slice(0, limit - 1)}…`
}
