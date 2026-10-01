<script setup>
// 用量与额度：只读展示核心累计的真实请求统计（status.json 的 usage 字段）。
// 数据全部来自壳推送的轻量快照，本视图不发任何请求、不查上游额度。
import { computed } from 'vue'

const props = defineProps({
  usage: { type: Object, default: null },
})

const total = computed(() => props.usage?.total || { requests: 0, ok: 0, failed: 0 })

// 逐模型行：按请求数降序（并列按模型 id 字典序），保证重复渲染顺序稳定。
const rows = computed(() =>
  Object.entries(props.usage?.models || {})
    .map(([id, stat]) => ({ id, ...stat }))
    .sort((a, b) => (b.requests || 0) - (a.requests || 0) || a.id.localeCompare(b.id)),
)

function percent(ok, requests) {
  if (!requests) return '—'
  return `${((ok / requests) * 100).toFixed(1)}%`
}

function ms(value) {
  return Number.isFinite(value) ? `${value} ms` : '—'
}
</script>

<template>
  <section class="view" aria-label="用量与额度">
    <header>
      <div>
        <h2>用量与额度</h2>
        <p class="subtitle">核心累计的真实客户端请求统计。</p>
      </div>
    </header>

    <div class="metrics">
      <div><b>{{ total.requests }}</b><span>真实请求</span></div>
      <div><b class="ok">{{ total.ok }}</b><span>成功</span></div>
      <div><b :class="{ bad: total.failed > 0 }">{{ total.failed }}</b><span>失败</span></div>
      <div><b :class="{ idle: !total.requests }">{{ percent(total.ok, total.requests) }}</b><span>成功率</span></div>
    </div>

    <p class="meta">
      统计起始：{{ usage?.since ? new Date(usage.since).toLocaleString() : '未知' }}（计数随核心重启延续）
    </p>

    <table v-if="rows.length" class="usage">
      <thead>
        <tr>
          <th scope="col">模型</th>
          <th scope="col" class="num">请求</th>
          <th scope="col" class="num">成功</th>
          <th scope="col" class="num">失败</th>
          <th scope="col" class="num">最近用时</th>
          <th scope="col" class="num">平均用时</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.id">
          <td class="mono">{{ row.id }}</td>
          <td class="num">{{ row.requests ?? 0 }}</td>
          <td class="num">{{ row.ok ?? 0 }}</td>
          <td class="num">{{ row.failed ?? 0 }}</td>
          <td class="num">{{ ms(row.lastMs) }}</td>
          <td class="num">{{ ms(row.avgMs) }}</td>
        </tr>
      </tbody>
    </table>
    <p v-else class="notice">还没有真实请求记录：WorkBuddy 通过本服务发起对话后才会出现统计。</p>

    <p class="note">
      口径：只统计 WorkBuddy 发出的真实请求（source = request），启动与手动「检测全部」的探测请求不计入；
      客户端取消的请求不计为成功。免费额度与余额由上游 OpenCode 决定，本面板不查询、不估算。
    </p>
  </section>
</template>

<style scoped>
header { display: flex; justify-content: space-between; gap: var(--sp-3); align-items: center; }
.subtitle { margin: 0; color: var(--muted-strong); font-size: var(--fs-sm); }
/* 指标条：一格一数，格间用极淡竖线分组 */
.metrics {
  display: flex;
  align-items: stretch;
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  overflow: hidden;
}
.metrics div { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: var(--sp-1); padding: 10px var(--sp-4); }
.metrics div + div { border-left: 1px solid var(--line); }
.metrics b { font-size: 24px; font-weight: 600; font-variant-numeric: tabular-nums; line-height: 1.15; }
.metrics b.ok { color: var(--green); }
.metrics b.bad { color: var(--orange); }
.metrics b.idle { color: var(--muted); }
.metrics span { font-size: var(--fs-xs); color: var(--muted-strong); }
.meta { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); }
.usage {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-sm);
  background: var(--row);
  border: 1px solid var(--line);
  border-radius: var(--radius-m);
  overflow: hidden;
}
.usage th, .usage td { padding: var(--sp-2) var(--sp-3); text-align: left; border-bottom: 1px solid var(--line); }
/* 表头吸顶：行数多时滚动仍能对上列名 */
.usage th {
  position: sticky;
  top: 0;
  z-index: 1;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--muted-strong);
  background: var(--row);
}
.usage tbody tr { transition: background var(--dur-1) var(--ease-standard); }
.usage tbody tr:hover { background: color-mix(in srgb, var(--row) 88%, var(--green) 12%); }
.usage tr:last-child td { border-bottom: none; }
.num { text-align: right; font-variant-numeric: tabular-nums; }
.mono { font-family: ui-monospace, monospace; color: var(--text); overflow-wrap: anywhere; }
/* 无数据：虚线框居中，与列表空态同一套语言 */
.notice {
  margin: 0;
  padding: 14px;
  border: 1px dashed var(--line);
  border-radius: var(--radius-m);
  font-size: var(--fs-sm);
  color: var(--muted-strong);
  text-align: center;
}
.note { margin: 0; font-size: var(--fs-xs); color: var(--muted-strong); line-height: var(--lh-loose); }
</style>
