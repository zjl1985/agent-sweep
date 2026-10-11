import type { SessionEntry, SessionUsage as Usage } from "../types";
import type { Locale } from "../i18n";

const labels = {
  zh: { totalHint: "输入 + 输出；缓存读写单独列出", noEstimate: "未估价", noPrice: "缺少单价", noUsage: "无用量记录", recorded: "已记录", input: "输入", output: "输出", cache: "缓存读取", write: "缓存写入", missing: "没有可用的用量记录", partial: "记录不完整", unknown: "缺少对应模型或缓存单价，无法估算完整费用", estimate: "按公开 API 单价估算（USD），不是实际账单；不含订阅、折扣及自定义渠道价格", source: "价目表：models.dev", coverage: "会话有用量记录", priced: "会话可估价", request: "用量记录", subtotal: "估算费用小计" },
  en: { totalHint: "Input + output; cache reads and writes are listed separately", noEstimate: "Not estimated", noPrice: "No price", noUsage: "No usage data", recorded: "Recorded", input: "Input", output: "Output", cache: "Cache read", write: "Cache write", missing: "No usage records available", partial: "Partial records", unknown: "Missing model or cache prices; full cost unavailable", estimate: "Estimated at public API rates (USD), not a bill; excludes subscriptions, discounts and custom provider rates", source: "Prices: models.dev", coverage: "sessions with usage", priced: "sessions priced", request: "usage records", subtotal: "Estimated cost subtotal" },
};
export function formatTokens(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}
function money(value: number): string {
  return value === 0 ? "$0" : value < 0.001 ? "<$0.001" : `$${value.toLocaleString("en", { minimumFractionDigits: 2, maximumFractionDigits: 3 })}`;
}
function estimatedMoney(value: number): string {
  return `${value > 0 && value < 0.001 ? "" : "≈"}${money(value)}`;
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
  const cost = usage?.estimatedCostUsd;
  const reason = !usage ? t.noUsage : usage.incomplete ? t.partial : t.noPrice;
  return <>
    <div class="row-usage" title={usage ? details(usage, locale) : t.missing}>
      {usage ? <>
        <div class="usage-total" title={t.totalHint}><strong>{formatTokens(usage.inputTokens + usage.outputTokens)}</strong><span class="token-unit">tokens</span>{usage.incomplete && <span class="usage-partial">{t.recorded}</span>}</div>
        <div class="usage-breakdown">
          {[metrics.slice(0, 2), metrics.slice(2).filter(([,value]) => value > 0)].filter(group => group.length > 0).map((group, index) => <div class="usage-metrics-line" key={index}>{group.map(([label,value]) => <span class="usage-metric" key={label}><span>{label}</span><span class="usage-number">{formatTokens(value)}</span></span>)}</div>)}
        </div>
      </> : <span class="usage-unavailable">{t.noUsage}</span>}
    </div>
    <div class={`row-cost ${cost != null ? "cost-known" : ""}`} title={cost != null ? `${t.estimate}\n${t.source} · ${usage!.priceDate}` : !usage ? t.missing : usage.incomplete ? t.partial : t.unknown}>
      {cost != null ? <span>{estimatedMoney(cost)}</span> : <><span class="cost-unavailable">{t.noEstimate}</span><span class="cost-reason">{reason}</span></>}
    </div>
  </>;
}
export function UsageSummary({ sessions, locale }: { sessions: SessionEntry[]; locale: Locale }) {
  const t = labels[locale];
  const known = sessions.flatMap(s => s.usage ? [s.usage] : []);
  const priced = known.filter(u => u.estimatedCostUsd != null);
  if (!known.length) return null;
  const tokens = known.reduce((n,u) => n + u.inputTokens + u.outputTokens, 0);
  const cost = priced.reduce((n,u) => n + (u.estimatedCostUsd ?? 0), 0);
  return <div class="usage-summary">
    <span>{known.length}/{sessions.length} {t.coverage}</span>
    <strong>{formatTokens(tokens)} tokens{known.some(u => u.incomplete) ? ` · ${t.partial}` : ""}</strong>
    {priced.length > 0 && <span title={t.estimate}>{t.subtotal} <strong class="cost-known">{estimatedMoney(cost)}</strong> <span>({priced.length}/{sessions.length} {t.priced})</span></span>}
  </div>;
}
