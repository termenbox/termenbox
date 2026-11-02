//! The fetched model price table (decision 0195, plan step 1.4b).
//!
//! `--fetch-prices` (or, if the owner enables it, a daily timer) reads one public list of model prices, turns it into
//! the meter's `Price` shape (US dollars per million tokens) and replaces `prices_fetched.json` beside the meter file.
//! The owner's `prices.json` is never touched and wins per model. The request is one plain `GET` of a fixed URL: no
//! key, no body, no query, and nothing about agents, models in use or usage. A failed, malformed or oversized fetch
//! keeps the old file.
//!
//! Two formats are read, told apart by the content: LiteLLM's `model_prices_and_context_window.json` (an object keyed
//! by model id, dollars per token) and OpenRouter's `/api/v1/models` (`{"data": [...]}`, dollars per token as strings).
//! Fact 0076 records what each looks like.
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::attention_meter::{Price, FETCHED_FILE};

/// The default source: LiteLLM's list (MIT).
pub const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
/// Longest body read and longest wait for the whole fetch.
const MAX_BODY: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
/// A list that prices fewer models than this is taken for a wrong answer and dropped.
const MIN_MODELS: usize = 10;
/// A refreshed table is at most this old before the timer fetches again.
const REFRESH_AFTER: Duration = Duration::from_secs(24 * 3600);
/// Dollars per million tokens above this is a unit mistake in the source.
const MAX_PRICE: f64 = 100_000.0;
/// Prefixes of cloud-reseller and regional copies in LiteLLM keys (`us.anthropic.claude-…`): they add a surcharge, so only
/// the provider's own bare id is taken.
const COPY_PREFIXES: [&str; 18] = [
    "anthropic.",
    "us.",
    "eu.",
    "apac.",
    "au.",
    "jp.",
    "global.",
    "us-gov.",
    "meta.",
    "amazon.",
    "mistral.",
    "cohere.",
    "ai21.",
    "deepseek.",
    "qwen.",
    "openai.",
    "writer.",
    "moonshot.",
];

/// A model id the way both a table and a lookup write it: lower case; a provider prefix (`anthropic/`), a `:variant` or
/// `@version` suffix, a Bedrock `-v1` and a trailing date or `-latest` dropped; dots written as dashes.
/// `claude-sonnet-5.5`, `anthropic/claude-sonnet-5.5` and `claude-sonnet-5-5-20260101` are all `claude-sonnet-5-5`.
pub fn normalise(id: &str) -> String {
    let id = id.trim().to_ascii_lowercase();
    let id = id.rsplit('/').next().unwrap_or("");
    let id = id.split([':', '@']).next().unwrap_or("");
    let mut id = id.to_string();
    if let Some(r) = COPY_PREFIXES.iter().find_map(|p| id.strip_prefix(p)) {
        id = r.to_string();
        if let Some(r) = COPY_PREFIXES.iter().find_map(|p| id.strip_prefix(p)) {
            id = r.to_string(); // `us.anthropic.claude-…`
        }
    }
    loop {
        let before = id.len();
        if let Some(r) = id.strip_suffix("-latest") {
            id = r.to_string();
        }
        if let Some((r, v)) = id.rsplit_once("-v") {
            if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) {
                id = r.to_string();
            }
        }
        if let Some((r, d)) = id.rsplit_once('-') {
            if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) {
                id = r.to_string();
            }
        }
        // 2025-04-16
        let parts: Vec<&str> = id.rsplitn(4, '-').collect();
        if parts.len() == 4
            && parts[0].len() == 2
            && parts[1].len() == 2
            && parts[2].len() == 4
            && parts
                .iter()
                .take(3)
                .all(|p| p.bytes().all(|b| b.is_ascii_digit()))
        {
            id = parts[3].to_string();
        }
        if id.len() == before {
            break;
        }
    }
    id.replace('.', "-")
}

/// The prices in a list, by normalised model id. The format is told from the content.
pub fn parse(raw: &[u8]) -> Result<BTreeMap<String, Price>> {
    let v: Value = serde_json::from_slice(raw).context("the price list is not JSON")?;
    let table = if v.get("data").is_some_and(Value::is_array) {
        parse_openrouter(&v)
    } else if let Some(o) = v.as_object() {
        parse_litellm(o)
    } else {
        bail!("the price list is neither an object nor an OpenRouter answer");
    };
    if table.len() < MIN_MODELS {
        bail!(
            "the price list prices only {} models; keeping the old table",
            table.len()
        );
    }
    Ok(table)
}

fn per_million(per_token: f64) -> Option<f64> {
    let m = per_token * 1e6;
    (m.is_finite() && (0.0..=MAX_PRICE).contains(&m)).then_some((m * 1e6).round() / 1e6)
}

fn parse_litellm(o: &serde_json::Map<String, Value>) -> BTreeMap<String, Price> {
    // Shorter keys first, so the bare provider id beats a longer copy that normalises to the same id.
    let mut keys: Vec<&String> = o.keys().collect();
    keys.sort_by_key(|k| (k.len(), k.as_str()));
    let mut out = BTreeMap::new();
    for k in keys {
        if k == "sample_spec" || k.contains('/') || COPY_PREFIXES.iter().any(|p| k.starts_with(p)) {
            continue;
        }
        let e = &o[k];
        if e.get("mode")
            .and_then(Value::as_str)
            .is_some_and(|m| !matches!(m, "chat" | "responses" | "completion"))
        {
            continue;
        }
        let cost = |f: &str| e.get(f).and_then(Value::as_f64);
        let (Some(input), Some(output)) = (
            cost("input_cost_per_token").and_then(per_million),
            cost("output_cost_per_token").and_then(per_million),
        ) else {
            continue;
        };
        let price = Price {
            input,
            output,
            cache_read: cost("cache_read_input_token_cost")
                .and_then(per_million)
                .unwrap_or(0.0),
            cache_write: cost("cache_creation_input_token_cost")
                .and_then(per_million)
                .unwrap_or(0.0),
        };
        out.entry(normalise(k)).or_insert(price);
    }
    out
}

fn parse_openrouter(v: &Value) -> BTreeMap<String, Price> {
    let mut rows: Vec<(&str, &Value)> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| Some((m.get("id")?.as_str()?, m.get("pricing")?)))
        // `:batch`, `:free`, `:thinking` and the like are variants with their own price.
        .filter(|(id, _)| !id.contains(':'))
        .collect();
    rows.sort_by_key(|(id, _)| (id.len(), *id));
    let mut out = BTreeMap::new();
    for (id, p) in rows {
        let cost = |f: &str| {
            let v = p.get(f)?;
            v.as_f64()
                .or_else(|| v.as_str()?.parse().ok())
                .and_then(per_million)
        };
        // A router entry prices at -1 per token, which `per_million` refuses.
        let (Some(input), Some(output)) = (cost("prompt"), cost("completion")) else {
            continue;
        };
        let price = Price {
            input,
            output,
            cache_read: cost("input_cache_read").unwrap_or(0.0),
            cache_write: cost("input_cache_write").unwrap_or(0.0),
        };
        out.entry(normalise(id)).or_insert(price);
    }
    out
}

/// `https`, or `http` to this machine (tests, or the owner's own mirror).
fn allowed(url: &str) -> Result<()> {
    let u = reqwest::Url::parse(url).context("the price URL is not a URL")?;
    let local = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    match u.scheme() {
        "https" => Ok(()),
        "http" if local => Ok(()),
        _ => bail!("the price URL must be https (or http to this machine)"),
    }
}

/// One `GET` with no key, body or query beyond what the URL itself says. At most `MAX_BODY` bytes are kept.
async fn fetch(url: &str) -> Result<Vec<u8>> {
    allowed(url)?;
    let http = reqwest::Client::builder()
        .user_agent(concat!("termenbox-server/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()?;
    let mut resp = http
        .get(url)
        .send()
        .await
        .context("the price list did not answer")?;
    if !resp.status().is_success() {
        bail!("the price list answered {}", resp.status());
    }
    if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        bail!("the price list is larger than {} MiB", MAX_BODY >> 20);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.context("the price list was cut off")? {
        if body.len() + chunk.len() > MAX_BODY {
            bail!("the price list is larger than {} MiB", MAX_BODY >> 20);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Fetches `url`, and when the answer parses writes `prices_fetched.json` in `dir`. Returns the table written. On any
/// failure the old file is left as it was.
pub async fn refresh(dir: &Path, url: &str) -> Result<BTreeMap<String, Price>> {
    let table = parse(&fetch(url).await?)?;
    let tmp = dir.join(format!(".{FETCHED_FILE}.tmp"));
    let write = || -> Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&serde_json::to_vec(&table)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, dir.join(FETCHED_FILE))?;
        Ok(())
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(table)
}

/// The fetched table is missing or a day old.
pub fn due(dir: &Path) -> bool {
    std::fs::metadata(dir.join(FETCHED_FILE))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age >= REFRESH_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn litellm(n: usize) -> Value {
        let mut o = serde_json::Map::new();
        o.insert(
            "sample_spec".into(),
            json!({"input_cost_per_token": 0.0, "output_cost_per_token": 0.0}),
        );
        for i in 0..n {
            o.insert(
                format!("model-{i}"),
                json!({"mode": "chat", "input_cost_per_token": 1e-6, "output_cost_per_token": 4e-6}),
            );
        }
        Value::Object(o)
    }

    #[test]
    fn price_table_normalises_the_spelling_of_each_agent() {
        for (raw, want) in [
            ("claude-sonnet-5-5", "claude-sonnet-5-5"),
            ("claude-sonnet-5.5", "claude-sonnet-5-5"),
            ("anthropic/claude-sonnet-5.5", "claude-sonnet-5-5"),
            ("Anthropic/Claude-Sonnet-5.5:batch", "claude-sonnet-5-5"),
            (
                "anthropic.claude-sonnet-4-5-20250929-v1:0",
                "claude-sonnet-4-5",
            ),
            ("us.anthropic.claude-sonnet-5-5", "claude-sonnet-5-5"),
            ("claude-haiku-4-5-20251001", "claude-haiku-4-5"),
            ("claude-sonnet-4-5@20250929", "claude-sonnet-4-5"),
            ("claude-3-5-sonnet-latest", "claude-3-5-sonnet"),
            ("gpt-5.1-codex", "gpt-5-1-codex"),
            ("openai/gpt-4.1", "gpt-4-1"),
            ("o3-2025-04-16", "o3"),
            ("gemini-2.5-pro", "gemini-2-5-pro"),
            ("", ""),
        ] {
            assert_eq!(normalise(raw), want, "{raw}");
        }
    }

    #[test]
    fn price_table_takes_the_bare_litellm_id_and_converts_to_dollars_per_million() {
        let mut v = litellm(10);
        let o = v.as_object_mut().unwrap();
        let e = |i: f64, c: f64| json!({"mode": "chat", "input_cost_per_token": i, "output_cost_per_token": 1e-5, "cache_read_input_token_cost": c, "cache_creation_input_token_cost": 2.5e-6});
        o.insert("claude-sonnet-5-5".into(), e(2e-6, 1e-7));
        o.insert("us.anthropic.claude-sonnet-5-5".into(), e(2.2e-6, 1.1e-7));
        o.insert("vertex_ai/claude-sonnet-5-5".into(), e(9e-6, 1e-7));
        o.insert("whisper-1".into(), json!({"mode": "audio_transcription", "input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6}));
        o.insert("no-output".into(), json!({"input_cost_per_token": 1e-6}));
        let t = parse(&serde_json::to_vec(&v).unwrap()).unwrap();
        let p = t["claude-sonnet-5-5"];
        assert_eq!(
            (p.input, p.output, p.cache_read, p.cache_write),
            (2.0, 10.0, 0.1, 2.5)
        );
        assert!(
            !t.contains_key("whisper-1")
                && !t.contains_key("no-output")
                && !t.contains_key("sample_spec")
        );
        assert!(!t
            .keys()
            .any(|k| k.starts_with("us-") || k.contains("vertex")));
    }

    #[test]
    fn price_table_reads_the_openrouter_answer() {
        let mut data = vec![
            json!({"id": "anthropic/claude-sonnet-5.5", "pricing": {"prompt": "0.000002", "completion": "0.00001", "input_cache_read": "0.0000001", "input_cache_write": "0.0000025"}}),
            json!({"id": "anthropic/claude-sonnet-5.5:batch", "pricing": {"prompt": "0.000001", "completion": "0.000005"}}),
            json!({"id": "openrouter/auto", "pricing": {"prompt": "-1", "completion": "-1"}}),
        ];
        for i in 0..10 {
            data.push(json!({"id": format!("acme/m-{i}"), "pricing": {"prompt": "0.000001", "completion": "0.000002"}}));
        }
        let t = parse(&serde_json::to_vec(&json!({ "data": data })).unwrap()).unwrap();
        let p = t["claude-sonnet-5-5"];
        assert_eq!(
            (p.input, p.output, p.cache_read, p.cache_write),
            (2.0, 10.0, 0.1, 2.5)
        );
        assert!(!t.contains_key("auto"));
        assert_eq!(t.len(), 11);
    }

    #[test]
    fn price_table_refuses_a_list_that_is_too_small_or_not_a_list() {
        assert!(parse(&serde_json::to_vec(&litellm(3)).unwrap()).is_err());
        assert!(parse(b"<html>").is_err());
        assert!(parse(b"[1,2]").is_err());
    }

    #[test]
    fn price_table_fetches_only_over_https_or_to_this_machine() {
        assert!(allowed("https://example.com/p.json").is_ok());
        assert!(allowed("http://127.0.0.1:9/p.json").is_ok());
        assert!(allowed("http://example.com/p.json").is_err());
        assert!(allowed("file:///etc/passwd").is_err());
    }

    /// Serves one answer and hands back the request it got.
    async fn serve(
        status: &'static str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prices.json", l.local_addr().unwrap());
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
            }
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes()).await;
            let _ = s.write_all(&body).await;
            let _ = s.shutdown().await;
            String::from_utf8_lossy(&req).into_owned()
        });
        (url, h)
    }

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tb-price-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn price_table_refresh_writes_the_file_and_the_request_carries_nothing_about_the_owner() {
        let d = tmp();
        let (url, req) = serve("200 OK", serde_json::to_vec(&litellm(12)).unwrap()).await;
        let t = refresh(&d, &url).await.unwrap();
        assert_eq!(t.len(), 12);
        let stored: BTreeMap<String, Price> =
            serde_json::from_slice(&std::fs::read(d.join(FETCHED_FILE)).unwrap()).unwrap();
        assert_eq!(stored, t);
        assert!(!d.join(format!(".{FETCHED_FILE}.tmp")).exists());
        let req = req.await.unwrap();
        let mut lines = req.lines();
        assert_eq!(
            lines.next(),
            Some("GET /prices.json HTTP/1.1"),
            "no query, no body"
        );
        let names: Vec<String> = lines
            .filter_map(|l| l.split(':').next())
            .map(str::to_ascii_lowercase)
            .collect();
        for n in names.iter().filter(|n| !n.is_empty()) {
            assert!(
                ["host", "user-agent", "accept"].contains(&n.as_str()),
                "unexpected header {n}"
            );
        }
        assert!(req
            .to_ascii_lowercase()
            .contains("user-agent: termenbox-server/"));
        assert!(
            !req.contains("\r\n\r\nx") && req.ends_with("\r\n\r\n"),
            "no body"
        );
    }

    #[tokio::test]
    async fn price_table_a_failed_or_oversized_fetch_keeps_the_old_file() {
        let d = tmp();
        std::fs::write(d.join(FETCHED_FILE), b"{\"old\":{\"input\":1.0}}").unwrap();
        for (status, body) in [
            (
                "500 Internal Server Error",
                serde_json::to_vec(&litellm(12)).unwrap(),
            ),
            ("200 OK", b"not json".to_vec()),
            ("200 OK", serde_json::to_vec(&litellm(3)).unwrap()),
            ("200 OK", vec![b' '; MAX_BODY + 1]),
        ] {
            let (url, _req) = serve(status, body).await;
            assert!(refresh(&d, &url).await.is_err(), "{status}");
            assert_eq!(
                std::fs::read(d.join(FETCHED_FILE)).unwrap(),
                b"{\"old\":{\"input\":1.0}}"
            );
        }
        assert!(refresh(&d, "http://example.com/p.json").await.is_err());
        assert!(refresh(&d, "http://127.0.0.1:1/p.json").await.is_err());
    }

    #[test]
    fn price_table_is_due_when_missing_or_a_day_old() {
        let d = tmp();
        assert!(due(&d));
        std::fs::write(d.join(FETCHED_FILE), b"{}").unwrap();
        assert!(!due(&d));
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(d.join(FETCHED_FILE))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(due(&d));
    }
}
