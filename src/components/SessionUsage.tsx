import type { SessionEntry, SessionUsage as Usage } from "../types";
import type { Locale } from "../i18n";

const labels = {
  zh: { input: "输入", output: "输出", cache: "缓存读取", write: "缓存写入", missing: "没有可用的用量记录", partial: "记录不完整", unknown: "缺少对应模型或缓存单价，无法估算完整费用", estimate: "按公开 API 单价估算（USD），不是实际账单；不含订阅、折扣及自定义渠道价格", source: "价目表：models.dev", coverage: "会话有用量记录", priced: "会话可估价", request: "用量记录", subtotal: "估算费用小计" },
  en: { input: "Input", output: "Output", cache: "Cache read", write: "Cache write", missing: "No usage records available", partial: "Incomplete records", unknown: "Missing model or cache prices; full cost unavailable", estimate: "Estimated at public API rates (USD), not a bill; excludes subscriptions, discounts and custom provider rates", source: "Prices: models.dev", coverage: "sessions with usage", priced: "sessions priced", request: "usage records", subtotal: "Estimated cost subtotal" },
};
export function formatTokens(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}
function money(value: number): string {
  return value === 0 ? "$0" : value < 0.001 ? "<$0.001" : `$${value.toLocaleString("en", { minimumFractionDigits: 2, maximumFractionDigits: 3 })}`;
}
function details(usage: Usage, locale: Locale): string {
  const t = labels[locale];
  return `${t.input} ${usage.inputTokens.toLocaleString()} · ${t.output} ${usage.outputTokens.toLocaleString()}\n${t.cache} ${usage.cacheReadTokens.toLocaleString()} · ${t.write} ${usage.cacheWriteTokens.toLocaleString()}\n${usage.requests} ${t.request}${usage.incomplete ? ` · ${t.partial}` : ""}`;
}
export function SessionUsage({ usage, locale }: { usage?: Usage | null; locale: Locale }) {
  const t = labels[locale];
  return <>
    <div class="row-usage" title={usage ? details(usage, locale) : t.missing}>
      <strong>{usage ? `${usage.incomplete ? "~" : ""}${formatTokens(usage.inputTokens + usage.outputTokens)}` : "—"}<span class="token-unit">{usage ? " tokens" : ""}</span></strong>
      {usage && <span class="usage-breakdown">{t.input} {formatTokens(usage.inputTokens)} · {t.output} {formatTokens(usage.outputTokens)}{usage.cacheReadTokens > 0 ? ` · ${t.cache} ${formatTokens(usage.cacheReadTokens)}` : ""}</span>}
    </div>
    <div class={`row-cost ${usage?.estimatedCostUsd != null ? "cost-known" : ""}`} title={usage?.estimatedCostUsd != null ? `${t.estimate}\n${t.source} · ${usage.priceDate}` : !usage ? t.missing : usage.incomplete ? t.partial : t.unknown}>
      {usage?.estimatedCostUsd != null ? `≈${money(usage.estimatedCostUsd)}` : "—"}
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
    <strong>{known.some(u => u.incomplete) ? "~" : ""}{formatTokens(tokens)} tokens</strong>
    {priced.length > 0 && <span title={t.estimate}>{t.subtotal} <strong class="cost-known">≈{money(cost)}</strong> <span>({priced.length}/{sessions.length} {t.priced})</span></span>}
  </div>;
}
