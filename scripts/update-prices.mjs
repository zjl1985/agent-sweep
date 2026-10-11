// Explicit, developer-run refresh. The desktop app never sends session data online.
import { writeFile } from "node:fs/promises";
const source = "https://models.dev/api.json";
const response = await fetch(source);
if (!response.ok) throw new Error(`Price download failed: ${response.status}`);
const catalog = await response.json();
const prices = {};
const makers = ["openai", "anthropic", "moonshotai", "deepseek", "google", "xai", "zai", "zhipuai", "minimax", "mistral", "alibaba"];
for (const provider of [...makers, "opencode"]) {
  if (!catalog[provider]) continue;
  for (const model of Object.values(catalog[provider].models ?? {})) {
    const cost = model.cost;
    if (!cost || !Number.isFinite(cost.input) || !Number.isFinite(cost.output) || cost.input < 0 || cost.output < 0) continue;
    prices[`${provider}/${model.id}`.toLowerCase()] = cost;
    // Bare names use the model maker's price, never a reseller or subscription rate.
    if (provider !== "opencode") prices[model.id.toLowerCase()] ??= cost;
  }
}
if (Object.keys(prices).length < 100) throw new Error("Unexpectedly small catalog; keeping existing snapshot");
await writeFile(new URL("../src-tauri/data/prices.json", import.meta.url), JSON.stringify({
  source, updated: new Date().toISOString().slice(0, 10), currency: "USD", unit: "per million tokens", prices,
}, null, 2) + "\n");
console.log(`Updated ${Object.keys(prices).length} price entries`);
