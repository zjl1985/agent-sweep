import type { SessionEntry, SessionUsage as Usage } from "../types";
import type { Locale } from "../i18n";

const labels = {
  zh: { totalHint: "输入 + 输出；缓存读写单独列出", noUsage: "无用量记录", input: "输入", output: "输出", cache: "缓存读取", write: "缓存写入", missing: "没有可用的用量记录", partial: "记录不完整", coverage: "会话有用量记录", request: "用量记录" },
  en: { totalHint: "Input + output; cache reads and writes are listed separately", noUsage: "No usage data", input: "Input", output: "Output", cache: "Cache read", write: "Cache write", missing: "No usage records available", partial: "Partial records", coverage: "sessions with usage", request: "usage records" },
};
export function formatTokens(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}
function details(usage: Usage, locale: Locale): string {
  const t = labels[locale];
  return `${t.input} ${usage.inputTokens.toLocaleString()} · ${t.output} ${usage.outputTokens.toLocaleString()}\n${t.cache} ${usage.cacheReadTokens.toLocaleString()} · ${t.write} ${usage.cacheWriteTokens.toLocaleString()}\n${usage.requests} ${t.request}${usage.incomplete ? ` · ${t.partial}` : ""}`;
}
export function SessionUsage({ usage, locale }: { usage?: Usage | null; locale: Locale }) {
  const t = labels[locale];
  const metrics = usage ? [
    [t.input, usage.inputTokens], [t.output, usage.outputTokens],
    [t.cache, usage.cacheReadTokens], [t.write, usage.cacheWriteTokens],
  ] as const : [];
  return <>
    <div class="row-usage" title={usage ? details(usage, locale) : t.missing}>
      {usage ? <>
        <div class="usage-total" title={t.totalHint}><strong>{formatTokens(usage.inputTokens + usage.outputTokens)}</strong><span class="token-unit">tokens</span>{usage.incomplete && <span class="usage-partial">{t.partial}</span>}</div>
        <div class="usage-breakdown">
          {[metrics.slice(0, 2), metrics.slice(2).filter(([,value]) => value > 0)].filter(group => group.length > 0).map((group, index) => <div class="usage-metrics-line" key={index}>{group.map(([label,value]) => <span class="usage-metric" key={label}><span>{label}</span><span class="usage-number">{formatTokens(value)}</span></span>)}</div>)}
        </div>
      </> : <span class="usage-unavailable">{t.noUsage}</span>}
    </div>
  </>;
}
export function UsageSummary({ sessions, locale }: { sessions: SessionEntry[]; locale: Locale }) {
  const t = labels[locale];
  const known = sessions.flatMap(s => s.usage ? [s.usage] : []);
  if (!known.length) return null;
  const tokens = known.reduce((n,u) => n + u.inputTokens + u.outputTokens, 0);
  return <div class="usage-summary">
    <span>{known.length}/{sessions.length} {t.coverage}</span>
    <strong>{formatTokens(tokens)} tokens{known.some(u => u.incomplete) ? ` · ${t.partial}` : ""}</strong>
  </div>;
}
