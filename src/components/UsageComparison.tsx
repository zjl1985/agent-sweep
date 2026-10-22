import type { ScanResult } from "../types";
import { strings, type Locale } from "../i18n";

const COLORS: Record<string, string> = {
  codex: "#3c5c50",
  claude: "#ff8263",
  opencode: "#9359a6",
  kimi: "#487e85",
  zcode: "#c59b4c",
};

export function UsageComparison({ scan, current, locale, disabled, onSelect, formatSize }: {
  scan: ScanResult;
  current: string;
  locale: Locale;
  disabled: boolean;
  onSelect: (id: string) => void;
  formatSize: (bytes: number) => string;
}) {
  const t = strings[locale];
  const total = scan.providers.reduce((sum, provider) => sum + provider.totalBytes, 0);
  const description = (name: string, bytes: number) => {
    const share = total > 0 ? bytes / total * 100 : 0;
    const percentage = share > 0 && share < 1 ? "<1" : String(Math.round(share));
    return `${name} · ${formatSize(bytes)} · ${percentage}%`;
  };

  return (
    <section class="agent-storage" aria-label={t.compareUsage}>
      <div class="agent-storage-heading">
        <span>{t.compareUsage}</span>
        <span><strong>{formatSize(total)}</strong><span class="agent-storage-caption">{t.agentsCombined(scan.providers.length)}</span></span>
      </div>
      <div class="agent-storage-bar" role="group" aria-label={t.compareUsage}>
        {scan.providers.filter((provider) => provider.totalBytes > 0).map((provider) => (
          <button
            key={provider.id}
            class="agent-storage-segment"
            style={{ width: `${provider.totalBytes / total * 100}%`, backgroundColor: COLORS[provider.id] ?? "#92949b" }}
            title={description(provider.name, provider.totalBytes)}
            aria-label={description(provider.name, provider.totalBytes)}
            aria-pressed={current === provider.id}
            disabled={disabled}
            onClick={() => onSelect(provider.id)}
          />
        ))}
      </div>
      <div class="agent-storage-legend">
        {scan.providers.map((provider) => (
          <button key={provider.id} class={`agent-legend-item ${current === provider.id ? "agent-legend-current" : ""}`}
            disabled={disabled} aria-pressed={current === provider.id}
            title={description(provider.name, provider.totalBytes)} onClick={() => onSelect(provider.id)}>
            <span class="agent-legend-dot" style={{ backgroundColor: COLORS[provider.id] ?? "#92949b" }} aria-hidden="true" />
            <span>{provider.name}</span>
            <span class="agent-legend-size">{formatSize(provider.totalBytes)}</span>
          </button>
        ))}
      </div>
    </section>
  );
}
