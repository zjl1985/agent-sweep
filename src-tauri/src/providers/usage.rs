//! Read-only usage extraction. Unknown prices stay unknown, never zero-filled.
use crate::models::{ProviderScan, SessionUsage};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Tokens {
    input: u64,
    output: u64,
    read: u64,
    write: u64,
    write_1h: u64,
}
impl Tokens {
    fn subtract(self, old: Self) -> Self {
        Self {
            input: self.input.saturating_sub(old.input),
            output: self.output.saturating_sub(old.output),
            read: self.read.saturating_sub(old.read),
            write: self.write.saturating_sub(old.write),
            write_1h: self.write_1h.saturating_sub(old.write_1h),
        }
    }
    fn max(self, other: Self) -> Self {
        Self {
            input: self.input.max(other.input),
            output: self.output.max(other.output),
            read: self.read.max(other.read),
            write: self.write.max(other.write),
            write_1h: self.write_1h.max(other.write_1h),
        }
    }
    fn total(self) -> u64 {
        self.input + self.output + self.read + self.write
    }
}
#[derive(Clone, Default)]
struct Sample {
    model: String,
    tokens: Tokens,
}
#[derive(Clone, Default)]
struct Parsed {
    samples: BTreeMap<String, Sample>,
    incomplete: bool,
}
fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}
fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}
fn api_tokens(v: &Value, inclusive: bool) -> Tokens {
    let read = num(v, "cached_input_tokens").max(num(v, "cache_read_input_tokens"));
    let write = num(v, "cache_creation_input_tokens").max(num(v, "cache_write_input_tokens"));
    Tokens {
        input: if inclusive {
            num(v, "input_tokens").saturating_sub(read + write)
        } else {
            num(v, "input_tokens")
        },
        output: num(v, "output_tokens"),
        read,
        write,
        write_1h: num(&v["cache_creation"], "ephemeral_1h_input_tokens"),
    }
}

pub(super) fn enrich(provider: &str, root: &Path, scan: &mut ProviderScan) {
    if scan.sessions.is_empty() {
        return;
    }
    if matches!(provider, "opencode" | "zcode") {
        let db = root.join(if provider == "opencode" {
            "opencode.db"
        } else {
            "cli/db/db.sqlite"
        });
        match database_usage(&db, provider) {
            Ok(mut usage) => {
                for session in &mut scan.sessions {
                    if let Some(parsed) = usage.remove(&session.session_id) {
                        session.usage = finish(parsed);
                    }
                }
            }
            Err(e) => scan.issues.push(format!("Usage unavailable: {e}")),
        }
        return;
    }
    for session in &mut scan.sessions {
        let path = root.join(&session.session_key);
        let mut paths = vec![];
        match provider {
            "codex" => paths.push(path),
            "claude" => {
                paths.push(path.clone());
                let sub = path.with_extension("").join("subagents");
                collect_files(&sub, "jsonl", &mut paths);
            }
            "kimi" => collect_files(&path.join("agents"), "wire.jsonl", &mut paths),
            _ => continue,
        }
        let mut parsed = Parsed::default();
        for (i, path) in paths.iter().enumerate() {
            match cached_file(path, provider) {
                Ok(part) => {
                    parsed.incomplete |= part.incomplete;
                    for (k, v) in part.samples {
                        parsed.samples.insert(format!("{i}:{k}"), v);
                    }
                }
                Err(_) => parsed.incomplete = true,
            }
        }
        session.usage = finish(parsed);
    }
}
fn collect_files(root: &Path, suffix: &str, out: &mut Vec<PathBuf>) {
    for e in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(suffix) {
            out.push(e.into_path());
        }
    }
}
#[derive(Clone)]
struct Cached {
    len: u64,
    modified: SystemTime,
    parsed: Parsed,
}
static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
fn cached_file(path: &Path, provider: &str) -> Result<Parsed, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Not a regular file".into());
    }
    let modified = meta.modified().map_err(|e| e.to_string())?;
    let cache = CACHE.get_or_init(Default::default);
    if let Some(c) = cache.lock().unwrap().get(path) {
        if c.len == meta.len() && c.modified == modified {
            return Ok(c.parsed.clone());
        }
    }
    let parsed = parse_file(
        BufReader::new(File::open(path).map_err(|e| e.to_string())?),
        provider,
    )?;
    let mut cache = cache.lock().unwrap();
    if cache.len() > 4096 {
        cache.clear();
    }
    cache.insert(
        path.to_owned(),
        Cached {
            len: meta.len(),
            modified,
            parsed: parsed.clone(),
        },
    );
    Ok(parsed)
}
fn parse_file<R: BufRead>(mut reader: R, provider: &str) -> Result<Parsed, String> {
    let mut parsed = Parsed::default();
    let mut legacy = Parsed::default();
    let mut model = String::new();
    let mut high = Tokens::default();
    let mut line = Vec::new();
    let mut ordinal = 0;
    let mut native = false;
    loop {
        line.clear();
        let count = reader
            .by_ref()
            .take(2 * 1024 * 1024)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        ordinal += 1;
        if !line.ends_with(b"\n") && count == 2 * 1024 * 1024 {
            // discard without retaining large tool output
            loop {
                let available = reader.fill_buf().map_err(|e| e.to_string())?;
                if available.is_empty() {
                    break;
                }
                let stop = available.iter().position(|b| *b == b'\n');
                let n = stop.map_or(available.len(), |i| i + 1);
                reader.consume(n);
                if stop.is_some() {
                    break;
                }
            }
            parsed.incomplete = true;
            continue;
        }
        let v: Value = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => {
                if line.iter().any(|b| !b.is_ascii_whitespace()) {
                    parsed.incomplete = true;
                }
                continue;
            }
        };
        match provider {
            "claude" if v["type"] == "assistant" => {
                let m = &v["message"];
                let u = &m["usage"];
                if !u.is_object() {
                    continue;
                }
                let (input_key, output_key) = if provider == "kimi" {
                    ("inputOther", "output")
                } else {
                    ("input_tokens", "output_tokens")
                };
                if u[input_key].as_u64().is_none() || u[output_key].as_u64().is_none() {
                    parsed.incomplete = true;
                    continue;
                }
                let id = text(m, "id");

                let key = if id.is_empty() {
                    parsed.incomplete = true;
                    format!("line:{ordinal}")
                } else {
                    id
                };
                let tokens = api_tokens(u, false);
                // Claude emits local synthetic notices with explicit zero usage.
                if m["model"] == "<synthetic>" && tokens.total() == 0 {
                    continue;
                }
                let tokens = parsed
                    .samples
                    .get(&key)
                    .map_or(tokens, |old| old.tokens.max(tokens));
                parsed.samples.insert(
                    key,
                    Sample {
                        model: text(m, "model"),
                        tokens,
                    },
                );
            }
            "kimi" if v["type"] == "usage.record" => {
                let u = &v["usage"];
                if !u.is_object() {
                    continue;
                }
                // These are turn-scoped usage deltas, not context window measurements.
                let key = format!(
                    "{}:{}:{}",
                    text(&v, "agentId"),
                    v["time"],
                    text(&v, "model")
                );
                let key = if v["time"].is_null() {
                    format!("line:{ordinal}")
                } else {
                    key
                };
                parsed.samples.insert(
                    key,
                    Sample {
                        model: text(&v, "model"),
                        tokens: Tokens {
                            input: num(u, "inputOther"),
                            output: num(u, "output"),
                            read: num(u, "inputCacheRead"),
                            write: num(u, "inputCacheCreation"),
                            write_1h: 0,
                        },
                    },
                );
            }
            "codex" => {
                let p = &v["payload"];
                if v["type"] == "turn_context" {
                    model = text(p, "model");
                }
                let record = if v["type"] == "token_usage_record" {
                    p
                } else if v["type"] == "compacted" {
                    &p["latest_token_usage_record"]
                } else {
                    &Value::Null
                };
                if record["usage"].is_object() {
                    native = true;
                    if record["usage"]["input_tokens"].as_u64().is_none()
                        || record["usage"]["output_tokens"].as_u64().is_none()
                    {
                        parsed.incomplete = true;
                        continue;
                    }
                    let id = text(record, "response_id");
                    let key = if id.is_empty() {
                        parsed.incomplete = true;
                        format!("line:{ordinal}")
                    } else {
                        id
                    };
                    let m = text(record, "model");
                    let tokens = api_tokens(&record["usage"], true);
                    if let Some(old) = parsed.samples.get(&key) {
                        if old.tokens != tokens {
                            parsed.incomplete = true;
                        }
                    } else {
                        parsed.samples.insert(
                            key,
                            Sample {
                                model: if m.is_empty() { model.clone() } else { m },
                                tokens,
                            },
                        );
                    }
                }
                if v["type"] == "event_msg"
                    && p["type"] == "token_count"
                    && p["info"]["total_token_usage"].is_object()
                {
                    let total = api_tokens(&p["info"]["total_token_usage"], true);
                    // A decreasing counter is a reset; don't turn it into an enormous delta.
                    let delta = if total.total() < high.total() {
                        legacy.incomplete = true;
                        total
                    } else {
                        total.subtract(high)
                    };
                    high = total;
                    let last = &p["info"]["last_token_usage"];
                    if delta.total() > 0 && (!last.is_object() || api_tokens(last, true) != delta) {
                        legacy.incomplete = true;
                    }
                    if delta.total() > 0 {
                        legacy.samples.insert(
                            format!("line:{ordinal}"),
                            Sample {
                                model: model.clone(),
                                tokens: delta,
                            },
                        );
                    }
                }
            }
            _ => (),
        }
    }
    if provider == "codex" {
        if !native {
            legacy.incomplete |= parsed.incomplete;
            return Ok(legacy);
        }
        // New native response records and legacy checkpoints describe the same calls.
        // Never sum both. Older history missing native records is explicitly partial.
        let native_sum: u64 = parsed.samples.values().map(|s| s.tokens.total()).sum();
        let legacy_sum: u64 = legacy.samples.values().map(|s| s.tokens.total()).sum();
        if legacy_sum > native_sum {
            parsed.incomplete = true;
        }
    }
    Ok(parsed)
}
fn database_usage(path: &Path, provider: &str) -> Result<HashMap<String, Parsed>, String> {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT id, session_id, data FROM message")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut result: HashMap<String, Parsed> = HashMap::new();
    for row in rows {
        let (id, sid, data) = row.map_err(|e| e.to_string())?;
        let v: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => {
                result.entry(sid).or_default().incomplete = true;
                continue;
            }
        };
        if v["role"] != "assistant" || !v["tokens"].is_object() {
            continue;
        }
        let t = &v["tokens"];
        if t["input"].as_u64().is_none() || t["output"].as_u64().is_none() {
            result.entry(sid).or_default().incomplete = true;
            continue;
        }
        let read = num(&t["cache"], "read");
        let write = num(&t["cache"], "write");
        let mut model = text(&v, "modelID");
        if model.is_empty() {
            model = text(&v, "modelId");
        }
        if model.is_empty() {
            model = text(&v["model"], "modelID");
        }
        let provider_id = text(&v, "providerID");
        let qualified = format!("{provider_id}/{model}").to_lowercase();
        if prices()["prices"].get(&qualified).is_some() {
            model = qualified;
        }
        let tokens = Tokens {
            input: if provider == "zcode" {
                num(t, "input").saturating_sub(read + write)
            } else {
                num(t, "input")
            },
            output: num(t, "output")
                + if provider == "opencode" {
                    num(t, "reasoning")
                } else {
                    0
                },
            read,
            write,
            write_1h: 0,
        };
        result
            .entry(sid)
            .or_default()
            .samples
            .insert(id, Sample { model, tokens });
    }
    Ok(result)
}
fn prices() -> &'static Value {
    static PRICES: OnceLock<Value> = OnceLock::new();
    PRICES.get_or_init(|| {
        serde_json::from_str(include_str!("../../data/prices.json")).expect("bundled price catalog")
    })
}
fn cost(sample: &Sample) -> Option<f64> {
    let id = sample.model.to_lowercase();
    let catalog = &prices()["prices"];
    let normalized = if id.contains("claude-") {
        id.replace('.', "-")
    } else {
        id.clone()
    };
    let base = catalog.get(&id).or_else(|| catalog.get(&normalized))?;
    let t = sample.tokens;
    let prompt = t.input + t.read + t.write;
    let mut rate = base;
    if let Some(tiers) = base["tiers"].as_array() {
        for tier in tiers {
            if let Some(threshold) = tier.pointer("/tier/size").and_then(Value::as_u64) {
                if prompt > threshold {
                    rate = tier;
                }
            }
        }
    }
    let rate_for = |key: &str| {
        rate.get(key)
            .or_else(|| base.get(key))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    let charge = |n: u64, key: &str| {
        if n == 0 {
            Some(0.0)
        } else {
            Some(n as f64 * rate_for(key)?)
        }
    };
    let one_hour = if t.write_1h == 0 {
        0.0
    } else {
        t.write_1h as f64 * rate_for("input")? * 2.0
    };
    Some(
        (charge(t.input, "input")?
            + charge(t.output, "output")?
            + charge(t.read, "cache_read")?
            + charge(t.write.saturating_sub(t.write_1h), "cache_write")?
            + one_hour)
            / 1_000_000.0,
    )
}
fn finish(parsed: Parsed) -> Option<SessionUsage> {
    if parsed.samples.is_empty() {
        return None;
    }
    let mut u = SessionUsage {
        estimated_cost_usd: Some(0.0),
        incomplete: parsed.incomplete,
        price_date: prices()["updated"].as_str().unwrap_or("").into(),
        ..Default::default()
    };
    for s in parsed.samples.values() {
        if !s.model.is_empty() && !u.models.contains(&s.model) {
            u.models.push(s.model.clone());
        }
        u.input_tokens += s.tokens.input;
        u.output_tokens += s.tokens.output;
        u.cache_read_tokens += s.tokens.read;
        u.cache_write_tokens += s.tokens.write;
        u.requests += 1;
        u.estimated_cost_usd = u.estimated_cost_usd.zip(cost(s)).map(|(a, b)| a + b);
    }
    u.models.sort();
    if u.incomplete {
        u.estimated_cost_usd = None;
    }
    Some(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(lines: &[Value], provider: &str) -> SessionUsage {
        let text = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        finish(parse_file(text.as_bytes(), provider).unwrap()).unwrap()
    }
    #[test]
    fn native_codex_records_win_over_duplicate_legacy_and_compaction() {
        let record = serde_json::json!({"response_id":"r1","usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":20}});
        let u = parse(
            &[
                serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6-sol"}}),
                serde_json::json!({"type":"token_usage_record","payload":record}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":20}}}}),
                serde_json::json!({"type":"compacted","payload":{"latest_token_usage_record":record}}),
            ],
            "codex",
        );
        assert_eq!(u.requests, 1);
        assert_eq!(u.input_tokens, 400);
        assert_eq!(u.cache_read_tokens, 600);
        assert_eq!(u.output_tokens, 20);
        assert!((u.estimated_cost_usd.unwrap() - 0.00112).abs() < 1e-9);
    }
    #[test]
    fn legacy_counters_do_not_double_count_replays() {
        let event = |i| serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":i,"output_tokens":10}}}});
        let u = parse(&[event(100), event(100), event(150)], "codex");
        assert_eq!(u.input_tokens, 150);
        assert_eq!(u.output_tokens, 10);
        assert_eq!(u.estimated_cost_usd, None);
    }
    #[test]
    fn claude_streaming_snapshots_count_once_and_cache_separately() {
        let line = |output| serde_json::json!({"type":"assistant","requestId":"req","message":{"id":"msg","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":output,"cache_read_input_tokens":200,"cache_creation_input_tokens":50}}});
        let u = parse(&[line(1), line(20), line(1)], "claude");
        assert_eq!(u.requests, 1);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 20);
        assert_eq!(u.cache_write_tokens, 50);
        assert!((u.estimated_cost_usd.unwrap() - 0.0008475).abs() < 1e-9);
    }
    #[test]
    fn zero_usage_synthetic_notices_do_not_block_real_cost() {
        let u = parse(
            &[
                serde_json::json!({"type":"assistant","message":{"id":"synthetic","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}),
                serde_json::json!({"type":"assistant","message":{"id":"real","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":20}}}),
            ],
            "claude",
        );
        assert_eq!(u.requests, 1);
        assert_eq!(u.models, vec!["claude-sonnet-4-6"]);
        assert!((u.estimated_cost_usd.unwrap() - 0.0006).abs() < 1e-9);
    }

    #[test]
    fn kimi_ignores_context_measurements() {
        let u = parse(
            &[
                serde_json::json!({"type":"token_counting.measured","tokens":100000}),
                serde_json::json!({"type":"usage.record","agentId":"main","time":1,"model":"unknown/private","usage":{"inputOther":3,"output":4,"inputCacheRead":5,"inputCacheCreation":6}}),
            ],
            "kimi",
        );
        assert_eq!(
            (
                u.input_tokens,
                u.output_tokens,
                u.cache_read_tokens,
                u.cache_write_tokens
            ),
            (3, 4, 5, 6)
        );
        assert_eq!(u.estimated_cost_usd, None);
    }
    #[test]
    fn pricing_uses_request_context_tier_and_unknown_is_not_zero() {
        let mut sample = Sample {
            model: "gpt-6-sol".into(),
            tokens: Tokens {
                input: 272000,
                output: 100,
                ..Default::default()
            },
        };
        assert!((cost(&sample).unwrap() - 0.545).abs() < 1e-9);
        sample.tokens.input += 1;
        assert!((cost(&sample).unwrap() - 1.089504).abs() < 1e-9);
        sample.model = "unknown-gpt-6-sol".into();
        assert_eq!(cost(&sample), None);
    }
    #[test]
    fn malformed_history_suppresses_complete_cost() {
        let text="{\"type\":\"assistant\",\"message\":{\"id\":\"m\",\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n{broken";
        let u = finish(parse_file(text.as_bytes(), "claude").unwrap()).unwrap();
        assert!(u.incomplete);
        assert_eq!(u.estimated_cost_usd, None);
    }
    #[test]
    fn database_adapters_normalize_different_cache_and_reasoning_conventions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE message(id TEXT, session_id TEXT, data TEXT)")
            .unwrap();
        let data = serde_json::json!({"role":"assistant","modelID":"gpt-6-sol","tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":60,"write":0}}});
        db.execute(
            "INSERT INTO message VALUES ('m','s',?1)",
            [data.to_string()],
        )
        .unwrap();
        let oc = finish(
            database_usage(&path, "opencode")
                .unwrap()
                .remove("s")
                .unwrap(),
        )
        .unwrap();
        let zc = finish(database_usage(&path, "zcode").unwrap().remove("s").unwrap()).unwrap();
        assert_eq!((oc.input_tokens, oc.output_tokens), (100, 25));
        assert_eq!((zc.input_tokens, zc.output_tokens), (40, 20));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM message", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn missing_database_is_never_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing");
        assert!(database_usage(&path, "opencode").is_err());
        assert!(!path.exists());
    }
    #[test]
    fn changed_file_invalidates_usage_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        std::fs::write(&path, "{}").unwrap();
        assert!(cached_file(&path, "kimi").unwrap().samples.is_empty());
        std::fs::write(
            &path,
            r#"{"type":"usage.record","model":"unknown","usage":{"inputOther":5,"output":2}}"#,
        )
        .unwrap();
        assert_eq!(
            finish(cached_file(&path, "kimi").unwrap())
                .unwrap()
                .input_tokens,
            5
        );
    }
    #[test]
    #[ignore = "Read-only local-data validation; writes only an aggregate scan to a specified temporary file"]
    fn real_usage_scan() {
        let result = super::super::ProviderRegistry::built_in().scan();
        for p in &result.providers {
            let sessions: Vec<_> = result
                .sessions
                .iter()
                .filter(|s| s.provider_id == p.id)
                .collect();
            eprintln!(
                "{}: {} sessions, {} usage, {} priced",
                p.id,
                sessions.len(),
                sessions.iter().filter(|s| s.usage.is_some()).count(),
                sessions
                    .iter()
                    .filter(|s| s
                        .usage
                        .as_ref()
                        .is_some_and(|u| u.estimated_cost_usd.is_some()))
                    .count()
            );
        }
        if let Ok(path) = std::env::var("AGENT_SWEEP_USAGE_PREVIEW") {
            std::fs::write(path, serde_json::to_vec(&result).unwrap()).unwrap();
        }
    }
}
