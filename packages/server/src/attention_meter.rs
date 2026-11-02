//! The attention meter (decision 0169, plan step 1.1): how many human seconds an agent-hour costs.
//!
//! For every obligation (an Attention item) it records when it was raised, when it was resolved, which layer
//! resolved it (`human` or `rule`; `contract` and `deputy` come later), the answering device class, and for a human
//! the time the card was in the foreground and the time to answer. It also integrates the time tabs spend working.
//! Numbers only: no prompt, no agent text and no workspace or device name ever reaches the store
//! (`audit-logging-no-secrets`, decision 0105 for the pattern).
//!
//! The store is one small JSON file of daily buckets in the state directory, the last `KEEP_DAYS` days, written to a
//! temporary file and renamed, so a crash leaves the old file or the new one. A restart loses only the obligations
//! and working spans in flight.
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use termenbox_core::protocol::{AttentionDay, ModelUse};

use crate::history::Tokens;

/// Days of buckets kept.
pub const KEEP_DAYS: usize = 30;
pub const DAY_MS: u64 = 86_400_000;
/// Open obligations and working spans tracked at once; past this a new one is not counted.
const MAX_OPEN: usize = 400;
/// Resolved obligations remembered, so a late `card_seen` is credited to the right layer.
const MAX_RESOLVED: usize = 200;
/// One `card_seen` is credited at most this long: a card left open on a table is not attention.
const MAX_DWELL_MS: u64 = 10 * 60_000;
/// Ids are `a<number>`; anything longer or odd is not one of ours.
const MAX_ID: usize = 32;
const FILE: &str = "attention_meter.json";
const FILE_VERSION: u32 = 1;
/// The owner's price table (plan step 1.4): `{"<model id>": {"input": 3.0, "output": 15.0, "cache_read": 0.3, "cache_write": 3.75}}`,
/// US dollars per million tokens. Termenbox starts from `DEFAULT_PRICES`, the fetched table (`FETCHED_FILE`, step 1.4b)
/// replaces them per model and the owner's entries replace both; a model with no entry in any costs nothing in the
/// meter and its turns are counted as unpriced.
const PRICES_FILE: &str = "prices.json";
/// The table `--fetch-prices` writes (decision 0195), never the owner's file.
pub const FETCHED_FILE: &str = "prices_fetched.json";
/// Anthropic list prices, standard tier, 5-minute cache writes (fact 0041, accessed 2026-10-04, expires 2027-01-04):
/// (model id, input, output, cache read, cache write). Estimates only: the owner's `prices.json` wins.
const DEFAULT_PRICES: [(&str, f64, f64, f64, f64); 6] = [
    ("claude-fable-5-1", 10.0, 50.0, 0.25, 12.5),
    ("claude-opus-5-5", 4.0, 20.0, 0.2, 5.0),
    ("claude-opus-5", 5.0, 25.0, 0.5, 6.25),
    ("claude-sonnet-5-5", 2.0, 10.0, 0.2, 2.5),
    ("claude-sonnet-5", 2.0, 10.0, 0.2, 2.5),
    ("claude-haiku-4-5", 1.0, 5.0, 0.1, 1.25),
];

/// A table with its ids written the way a lookup writes them (`price_table::normalise`).
fn normalised(t: HashMap<String, Price>) -> HashMap<String, Price> {
    t.into_iter()
        .map(|(k, p)| (crate::price_table::normalise(&k), p))
        .collect()
}

/// The three layers, later ones winning per model: built-in, fetched, the owner's. A missing or unreadable file adds nothing.
fn load_prices(dir: &Path) -> HashMap<String, Price> {
    let mut prices = default_prices();
    for file in [FETCHED_FILE, PRICES_FILE] {
        if let Some(t) = std::fs::read(dir.join(file))
            .ok()
            .and_then(|raw| serde_json::from_slice::<HashMap<String, Price>>(&raw).ok())
        {
            prices.extend(normalised(t));
        }
    }
    prices
}

fn default_prices() -> HashMap<String, Price> {
    DEFAULT_PRICES
        .iter()
        .map(|&(id, input, output, cache_read, cache_write)| {
            let p = Price {
                input,
                output,
                cache_read,
                cache_write,
            };
            (id.to_string(), p)
        })
        .collect()
}

/// Models kept per day, and the longest model id; a model past the cap is counted as `other`.
const MAX_MODELS: usize = 16;
const MAX_MODEL_LEN: usize = 64;

/// What a verifier said about a finished task. `None` is "no verifier ran" and is reported as such, never as a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    None,
}

impl Verdict {
    fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::None => "none",
        }
    }
}

/// Dollars per million tokens.
#[derive(Serialize, Deserialize, Clone, Copy, Default, Debug, PartialEq)]
pub struct Price {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

impl Price {
    /// Millionths of a dollar for these counts: dollars per million tokens times tokens is exactly micro-dollars.
    fn micros(&self, t: &Tokens) -> u64 {
        let n = |c: Option<u64>, p: f64| c.unwrap_or(0) as f64 * p.max(0.0);
        (n(t.input, self.input)
            + n(t.output, self.output)
            + n(t.cache_read, self.cache_read)
            + n(t.cache_write, self.cache_write))
        .round() as u64
    }
}

/// A model id as a bucket key: ASCII letters, digits and `-_.:/`, cut short; anything else makes it `unknown`.
fn model_key(m: Option<&str>) -> String {
    let m = m.unwrap_or("").trim();
    let ok = !m.is_empty()
        && m.len() <= MAX_MODEL_LEN
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c));
    if ok {
        m.to_string()
    } else {
        "unknown".to_string()
    }
}

/// Who resolved an obligation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Human,
    Rule,
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Human => "human",
            Layer::Rule => "rule",
        }
    }
}

/// The device classes kept; anything else is `other`. Never a device name.
const DEVICES: [&str; 3] = ["phone", "desktop", "other"];

pub fn device_class(d: Option<&str>) -> &'static str {
    DEVICES
        .iter()
        .find(|k| Some(**k) == d)
        .copied()
        .unwrap_or("other")
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    days: Vec<AttentionDay>,
}

struct Open {
    raised_at: u64,
}

#[derive(Default)]
struct Inner {
    days: BTreeMap<u32, AttentionDay>,
    open: HashMap<String, Open>,
    resolved: VecDeque<(String, Layer)>,
    /// When each working tab started its current span.
    spans: HashMap<(String, String), u64>,
    dirty: bool,
}

pub struct AttentionMeter {
    path: Option<PathBuf>,
    prices: RwLock<HashMap<String, Price>>,
    inner: Mutex<Inner>,
}

fn day_of(ms: u64) -> u32 {
    (ms / DAY_MS) as u32
}

/// Keeps only keys the meter writes, so a hand-edited or newer file cannot carry free text into memory.
fn scrub(d: &mut AttentionDay) {
    d.by_layer
        .retain(|k, _| matches!(k.as_str(), "human" | "rule"));
    d.by_device.retain(|k, _| DEVICES.contains(&k.as_str()));
    d.verified
        .retain(|k, _| matches!(k.as_str(), "pass" | "fail" | "none"));
    let mut by_model = BTreeMap::new();
    for (k, v) in std::mem::take(&mut d.by_model).into_iter().take(MAX_MODELS) {
        let k = model_key(Some(&k));
        let e: &mut ModelUse = by_model.entry(k).or_default();
        e.turns = e.turns.saturating_add(v.turns);
        e.input = e.input.saturating_add(v.input);
        e.output = e.output.saturating_add(v.output);
        e.cache_read = e.cache_read.saturating_add(v.cache_read);
        e.cache_write = e.cache_write.saturating_add(v.cache_write);
        e.cost_micros = e.cost_micros.saturating_add(v.cost_micros);
        e.unpriced = e.unpriced.saturating_add(v.unpriced);
    }
    d.by_model = by_model;
}

impl Inner {
    fn bucket(&mut self, now: u64) -> &mut AttentionDay {
        let day = day_of(now);
        if !self.days.contains_key(&day) {
            self.days.insert(
                day,
                AttentionDay {
                    day,
                    ..Default::default()
                },
            );
            while self.days.len() > KEEP_DAYS {
                self.days.pop_first();
            }
        }
        // `pop_first` may have removed an older day only, and `day` is the newest unless the clock went back.
        self.days.entry(day).or_insert_with(|| AttentionDay {
            day,
            ..Default::default()
        })
    }

    /// Adds `[from, to)` of working time, split at UTC midnights.
    fn add_working(&mut self, from: u64, to: u64) {
        let mut at = from;
        while at < to {
            let end = ((at / DAY_MS) + 1) * DAY_MS;
            let end = end.min(to);
            self.bucket(at).agent_ms += end - at;
            at = end;
        }
        self.dirty = true;
    }
}

impl AttentionMeter {
    /// A meter that keeps nothing on disk (tests, or a state directory that cannot be used).
    pub fn in_memory() -> Self {
        Self {
            path: None,
            prices: RwLock::default(),
            inner: Mutex::default(),
        }
    }

    /// The same, with a price table (tests).
    #[cfg(test)]
    fn with_prices(mut self, json: &str) -> Self {
        let owner: HashMap<String, Price> = serde_json::from_str(json).unwrap();
        self.prices = RwLock::new(normalised(owner));
        self
    }

    /// The entry for a model id, or for the longest listed id it extends (`claude-haiku-4-5-20251001` is `claude-haiku-4-5`).
    fn price_of(&self, key: &str) -> Option<Price> {
        let key = crate::price_table::normalise(key);
        let prices = self.prices.read().unwrap();
        prices.get(&key).copied().or_else(|| {
            prices
                .iter()
                .filter(|(id, _)| {
                    key.strip_prefix(id.as_str())
                        .is_some_and(|r| r.starts_with('-'))
                })
                .max_by_key(|(id, _)| id.len())
                .map(|(_, p)| *p)
        })
    }

    /// The models in the stored days, split by whether a price is known for them (`--fetch-prices` prints it).
    pub fn price_report(&self) -> (Vec<String>, Vec<String>) {
        let mut models: Vec<String> = {
            let g = self.inner.lock().unwrap();
            let mut all: Vec<String> = g
                .days
                .values()
                .flat_map(|d| d.by_model.keys().cloned())
                .collect();
            all.sort();
            all.dedup();
            all
        };
        models.retain(|m| m != "unknown" && m != "other");
        models.into_iter().partition(|m| self.price_of(m).is_some())
    }

    /// Reads the price layers again (after `--fetch-prices` ran in the server). A meter without a store keeps its table.
    pub fn reload_prices(&self) {
        if let Some(dir) = self.path.as_deref().and_then(Path::parent) {
            *self.prices.write().unwrap() = load_prices(dir);
        }
    }

    /// A turn used `t` on `model` (None when the agent names none). Counts only; the estimated cost uses the owner's
    /// price table and is zero for a model it does not list, and is left out when `estimate` is false (the agent
    /// reports its own cost, `agent_cost`).
    pub fn turn_used(&self, model: Option<&str>, t: &Tokens, estimate: bool, now: u64) {
        if t.is_empty() {
            return;
        }
        let key = model_key(model);
        let price = if estimate { self.price_of(&key) } else { None };
        let cost = price.map_or(0, |p| p.micros(t));
        let unpriced = estimate && price.is_none();
        let mut g = self.inner.lock().unwrap();
        let b = g.bucket(now);
        let key = if b.by_model.contains_key(&key) || b.by_model.len() < MAX_MODELS {
            key
        } else {
            "other".to_string()
        };
        let m = b.by_model.entry(key).or_default();
        let add = |a: u64, n: Option<u64>| a.saturating_add(n.unwrap_or(0));
        m.turns = m.turns.saturating_add(1);
        m.input = add(m.input, t.input);
        m.output = add(m.output, t.output);
        m.cache_read = add(m.cache_read, t.cache_read);
        m.cache_write = add(m.cache_write, t.cache_write);
        m.cost_micros = m.cost_micros.saturating_add(cost);
        m.unpriced = m.unpriced.saturating_add(u32::from(unpriced));
        b.cost_micros = b.cost_micros.saturating_add(cost);
        g.dirty = true;
    }

    /// An agent reported `micros` more US dollars spent on `model` (ACP `usage_update`). Counted as the day's cost and
    /// the model's, with no turn: the turn's tokens arrive apart.
    pub fn agent_cost(&self, model: Option<&str>, micros: u64, now: u64) {
        if micros == 0 {
            return;
        }
        let key = model_key(model);
        let mut g = self.inner.lock().unwrap();
        let b = g.bucket(now);
        let key = if b.by_model.contains_key(&key) || b.by_model.len() < MAX_MODELS {
            key
        } else {
            "other".to_string()
        };
        let m = b.by_model.entry(key).or_default();
        m.cost_micros = m.cost_micros.saturating_add(micros);
        b.cost_micros = b.cost_micros.saturating_add(micros);
        g.dirty = true;
    }

    /// A finished task was judged by a verifier (or by none).
    pub fn verified(&self, v: Verdict, now: u64) {
        let mut g = self.inner.lock().unwrap();
        let n = g
            .bucket(now)
            .verified
            .entry(v.as_str().to_string())
            .or_default();
        *n = n.saturating_add(1);
        g.dirty = true;
    }

    /// Opens the store in `dir`. A missing or unreadable file starts empty; the meter costs a number, never the server.
    pub fn open(dir: &Path, now: u64) -> Result<Self> {
        let path = dir.join(FILE);
        let mut inner = Inner::default();
        if let Ok(raw) = std::fs::read(&path) {
            if let Ok(stored) = serde_json::from_slice::<Stored>(&raw) {
                for mut d in stored.days {
                    scrub(&mut d);
                    inner.days.insert(d.day, d);
                }
            }
        }
        let oldest = day_of(now).saturating_sub(KEEP_DAYS as u32 - 1);
        inner.days.retain(|d, _| *d >= oldest);
        Ok(Self {
            path: Some(path),
            prices: RwLock::new(load_prices(dir)),
            inner: Mutex::new(inner),
        })
    }

    /// An obligation was raised.
    pub fn raised(&self, id: &str, now: u64) {
        if id.len() > MAX_ID {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        if g.open.contains_key(id) || g.open.len() >= MAX_OPEN {
            return;
        }
        g.open.insert(id.to_string(), Open { raised_at: now });
        g.bucket(now).raised += 1;
        g.dirty = true;
    }

    /// An obligation went away without an answer (a newer one replaced it, its workspace is gone): not counted.
    pub fn forget(&self, id: &str) {
        self.inner.lock().unwrap().open.remove(id);
    }

    /// An obligation was resolved by `layer`. `device` is a class (`phone`, `desktop`), anything else is `other`.
    pub fn resolved(&self, id: &str, layer: Layer, device: Option<&str>, now: u64) {
        let mut g = self.inner.lock().unwrap();
        let Some(open) = g.open.remove(id) else {
            return;
        };
        let latency = now.saturating_sub(open.raised_at);
        let d = g.bucket(now);
        *d.by_layer.entry(layer.as_str().to_string()).or_default() += 1;
        if layer == Layer::Human {
            *d.by_device
                .entry(device_class(device).to_string())
                .or_default() += 1;
            d.latency_ms += latency;
        }
        g.resolved.push_back((id.to_string(), layer));
        while g.resolved.len() > MAX_RESOLVED {
            g.resolved.pop_front();
        }
        g.dirty = true;
    }

    /// The card of `id` was in the foreground for `ms`. Credited to the human only when the obligation is still open
    /// or a human resolved it; a rule-resolved obligation adds nothing. Returns whether it was credited.
    pub fn card_seen(&self, id: &str, ms: u64, now: u64) -> bool {
        let mut g = self.inner.lock().unwrap();
        let known = g.open.contains_key(id)
            || g.resolved
                .iter()
                .any(|(r, layer)| r == id && *layer == Layer::Human);
        if !known {
            return false;
        }
        g.bucket(now).human_ms += ms.min(MAX_DWELL_MS);
        g.dirty = true;
        true
    }

    /// A tab started or stopped working.
    pub fn tab_working(&self, workspace: &str, tab: &str, working: bool, now: u64) {
        let key = (workspace.to_string(), tab.to_string());
        let mut g = self.inner.lock().unwrap();
        if working {
            if !g.spans.contains_key(&key) && g.spans.len() < MAX_OPEN {
                g.spans.insert(key, now);
            }
        } else if let Some(from) = g.spans.remove(&key) {
            g.add_working(from, now);
        }
    }

    /// Whether any tab is working now, so its time is still growing.
    pub fn working(&self) -> bool {
        !self.inner.lock().unwrap().spans.is_empty()
    }

    /// Closes the working spans of workspaces for which `gone` is true (a closed tab, a removed workspace).
    pub fn close_where(&self, now: u64, gone: impl Fn(&str, &str) -> bool) {
        let mut g = self.inner.lock().unwrap();
        let ended: Vec<_> = g
            .spans
            .keys()
            .filter(|(w, t)| gone(w, t))
            .cloned()
            .collect();
        for key in ended {
            if let Some(from) = g.spans.remove(&key) {
                g.add_working(from, now);
            }
        }
    }

    /// The buckets, oldest first, with the spans still open counted up to `now` (nothing is changed).
    pub fn stats(&self, now: u64) -> Vec<AttentionDay> {
        let g = self.inner.lock().unwrap();
        let mut view = Inner {
            days: g.days.clone(),
            ..Default::default()
        };
        for from in g.spans.values() {
            view.add_working(*from, now);
        }
        view.days.into_values().collect()
    }

    /// Writes the buckets if they changed. Called on an interval, not per event. The open working spans are banked
    /// up to `now` first and restarted there, so a restart or a crash loses at most one interval of a running turn.
    /// (A tab found working at startup is counted from its next event, as it has no span until then.)
    pub fn flush(&self, now: u64) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let body = {
            let mut g = self.inner.lock().unwrap();
            let spans: Vec<_> = g.spans.iter().map(|(k, from)| (k.clone(), *from)).collect();
            for (key, from) in spans {
                if now > from {
                    g.add_working(from, now);
                    g.spans.insert(key, now);
                }
            }
            if !g.dirty {
                return Ok(());
            }
            g.dirty = false;
            serde_json::to_vec(&Stored {
                version: FILE_VERSION,
                days: g.days.values().cloned().collect(),
            })?
        };
        let tmp = path.with_extension("json.tmp");
        let write = || -> Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
                .context("open the meter file")?;
            f.write_all(&body)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        };
        write().inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
            self.inner.lock().unwrap().dirty = true;
        })
    }
}

/// The figures the meter line shows for the last `window` days ending at `today`. The client computes the same from
/// the `AttentionStats` days (step 1.2); this is the reference the tests hold the arithmetic to.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Totals {
    pub human_seconds: f64,
    pub agent_hours: f64,
    pub raised: u32,
    /// Share of resolved obligations that no human resolved, 0.0 to 1.0; `None` when nothing was resolved.
    pub without_human: Option<f64>,
}

#[cfg(test)]
impl Totals {
    /// Human seconds per agent-hour; `None` with no agent time.
    pub fn leverage(&self) -> Option<f64> {
        (self.agent_hours > 0.0).then(|| self.human_seconds / self.agent_hours)
    }
}

#[cfg(test)]
pub fn totals(days: &[AttentionDay], today: u32, window: u32) -> Totals {
    let from = today.saturating_sub(window.saturating_sub(1));
    let (mut human_ms, mut agent_ms, mut raised, mut by_human, mut resolved) =
        (0u64, 0u64, 0u32, 0u32, 0u32);
    for d in days.iter().filter(|d| d.day >= from && d.day <= today) {
        human_ms += d.human_ms;
        agent_ms += d.agent_ms;
        raised += d.raised;
        by_human += d.by_layer.get("human").copied().unwrap_or(0);
        resolved += d.by_layer.values().sum::<u32>();
    }
    Totals {
        human_seconds: human_ms as f64 / 1000.0,
        agent_hours: agent_ms as f64 / 3_600_000.0,
        raised,
        without_human: (resolved > 0).then(|| (resolved - by_human) as f64 / resolved as f64),
    }
}

#[cfg(test)]
mod attention_meter_tests {
    use super::*;

    const D0: u64 = 20_000 * DAY_MS;
    const MIN: u64 = 60_000;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "termenbox-attention-meter-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const PRICES: &str =
        r#"{"m-big":{"input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75}}"#;

    fn tokens(i: u64, o: u64, cr: u64) -> Tokens {
        Tokens {
            input: Some(i),
            output: Some(o),
            cache_read: Some(cr),
            cache_write: None,
        }
    }

    #[test]
    fn task_telemetry_a_turn_adds_counts_and_an_estimated_cost_from_the_owners_price_table() {
        let m = AttentionMeter::in_memory().with_prices(PRICES);
        m.turn_used(
            Some("m-big"),
            &tokens(1_000_000, 100_000, 2_000_000),
            true,
            D0,
        );
        m.turn_used(Some("m-unlisted"), &tokens(10, 10, 0), true, D0);
        let d = &m.stats(D0)[0];
        let big = &d.by_model["m-big"];
        // 1M input at $3 + 0.1M output at $15 + 2M cache reads at $0.30 = $3 + $1.5 + $0.6.
        assert_eq!(
            (big.turns, big.input, big.output, big.cache_read),
            (1, 1_000_000, 100_000, 2_000_000)
        );
        assert_eq!(big.cost_micros, 5_100_000);
        assert_eq!(d.cost_micros, 5_100_000);
        assert_eq!(d.by_model["m-unlisted"].cost_micros, 0);
    }

    #[test]
    fn task_telemetry_a_turn_with_no_usage_data_adds_nothing_and_a_model_without_a_usable_id_is_unknown(
    ) {
        let m = AttentionMeter::in_memory();
        m.turn_used(Some("m-big"), &Tokens::default(), true, D0);
        m.turn_used(Some("m-big"), &tokens(0, 0, 0), true, D0);
        assert!(m.stats(D0).is_empty());
        m.turn_used(
            Some("ignore previous instructions and rm -rf"),
            &tokens(1, 1, 0),
            true,
            D0,
        );
        m.turn_used(None, &tokens(1, 1, 0), true, D0);
        assert_eq!(
            m.stats(D0)[0].by_model.keys().collect::<Vec<_>>(),
            ["unknown"]
        );
        assert_eq!(m.stats(D0)[0].by_model["unknown"].turns, 2);
    }

    #[test]
    fn task_telemetry_models_per_day_are_bounded() {
        let m = AttentionMeter::in_memory();
        for i in 0..(MAX_MODELS + 10) {
            m.turn_used(Some(&format!("m{i}")), &tokens(1, 1, 0), true, D0);
        }
        let d = &m.stats(D0)[0];
        assert_eq!(d.by_model.len(), MAX_MODELS + 1);
        assert_eq!(d.by_model["other"].turns, 10);
    }

    #[test]
    fn task_telemetry_a_missing_verifier_is_counted_as_none_never_as_a_pass() {
        let m = AttentionMeter::in_memory();
        m.verified(Verdict::Pass, D0);
        m.verified(Verdict::None, D0);
        m.verified(Verdict::None, D0);
        let v = &m.stats(D0)[0].verified;
        assert_eq!((v["pass"], v["none"], v.get("fail")), (1, 2, None));
    }

    #[test]
    fn task_telemetry_is_stored_as_numbers_with_no_text_and_the_price_table_is_read_from_disk() {
        let d = dir("telemetry");
        std::fs::write(d.join(PRICES_FILE), PRICES).unwrap();
        let m = AttentionMeter::open(&d, D0).unwrap();
        m.turn_used(Some("m-big"), &tokens(1_000_000, 0, 0), true, D0);
        m.verified(Verdict::Pass, D0);
        m.flush(D0 + MIN).unwrap();
        let raw = std::fs::read_to_string(d.join(FILE)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        // Every leaf is a number, except the keys of the three maps the meter owns.
        fn leaves(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::String(s) => out.push(s.clone()),
                serde_json::Value::Array(a) => a.iter().for_each(|x| leaves(x, out)),
                serde_json::Value::Object(o) => o.values().for_each(|x| leaves(x, out)),
                _ => {}
            }
        }
        let mut strings = Vec::new();
        leaves(&v, &mut strings);
        assert!(strings.is_empty(), "no string value is stored: {strings:?}");
        let again = AttentionMeter::open(&d, D0).unwrap();
        assert_eq!(again.stats(D0)[0].cost_micros, 3_000_000);
        assert_eq!(again.stats(D0)[0].verified["pass"], 1);
    }

    #[test]
    fn task_telemetry_default_prices_apply_to_dated_ids_and_the_owners_file_overrides_them() {
        let d = dir("defaults");
        let m = AttentionMeter::open(&d, D0).unwrap();
        m.turn_used(
            Some("claude-haiku-4-5-20251001"),
            &tokens(1_000_000, 0, 0),
            true,
            D0,
        );
        m.turn_used(
            Some("claude-sonnet-5-5"),
            &tokens(1_000_000, 0, 0),
            true,
            D0,
        );
        let by = &m.stats(D0)[0].by_model;
        assert_eq!(by["claude-haiku-4-5-20251001"].cost_micros, 1_000_000);
        assert_eq!(by["claude-sonnet-5-5"].cost_micros, 2_000_000);
        std::fs::write(
            d.join(PRICES_FILE),
            r#"{"claude-sonnet-5-5":{"input":7.0}}"#,
        )
        .unwrap();
        let m = AttentionMeter::open(&d, D0).unwrap();
        m.turn_used(
            Some("claude-sonnet-5-5"),
            &tokens(1_000_000, 0, 0),
            true,
            D0,
        );
        m.turn_used(Some("claude-opus-5-5"), &tokens(1_000_000, 0, 0), true, D0);
        let by = &m.stats(D0)[0].by_model;
        assert_eq!(by["claude-sonnet-5-5"].cost_micros, 7_000_000);
        assert_eq!(by["claude-opus-5-5"].cost_micros, 4_000_000);
    }

    #[test]
    fn task_telemetry_fetched_prices_sit_between_the_built_in_list_and_the_owners_file() {
        let d = dir("fetched");
        std::fs::write(
            d.join(FETCHED_FILE),
            r#"{"claude-sonnet-5.5":{"input":3.0},"gpt-5.1-codex":{"input":1.25,"output":10.0},"acme-1":{"input":9.0}}"#,
        )
        .unwrap();
        std::fs::write(d.join(PRICES_FILE), r#"{"acme-1":{"input":0.5}}"#).unwrap();
        let m = AttentionMeter::open(&d, D0).unwrap();
        for model in [
            "claude-sonnet-5-5",
            "gpt-5.1-codex",
            "acme-1",
            "claude-opus-5-5",
        ] {
            m.turn_used(Some(model), &tokens(1_000_000, 0, 0), true, D0);
        }
        let by = &m.stats(D0)[0].by_model;
        assert_eq!(
            by["claude-sonnet-5-5"].cost_micros, 3_000_000,
            "fetched beats built-in, dots or dashes"
        );
        assert_eq!(
            by["gpt-5.1-codex"].cost_micros, 1_250_000,
            "a model only the fetch knows"
        );
        assert_eq!(
            by["acme-1"].cost_micros, 500_000,
            "the owner's file beats the fetch"
        );
        assert_eq!(
            by["claude-opus-5-5"].cost_micros, 4_000_000,
            "built-in stays where nothing replaces it"
        );
        // a refresh while running is picked up without a restart
        std::fs::write(d.join(FETCHED_FILE), r#"{"gpt-5.1-codex":{"input":2.0}}"#).unwrap();
        m.reload_prices();
        m.turn_used(Some("gpt-5.1-codex"), &tokens(1_000_000, 0, 0), true, D0);
        assert_eq!(
            m.stats(D0)[0].by_model["gpt-5.1-codex"].cost_micros,
            3_250_000
        );
    }

    #[test]
    fn task_telemetry_a_turn_on_a_model_with_no_price_is_counted_as_unpriced_not_free() {
        let m = AttentionMeter::in_memory().with_prices(PRICES);
        m.turn_used(Some("m-unlisted"), &tokens(10, 10, 0), true, D0);
        m.turn_used(Some("m-unlisted"), &tokens(10, 10, 0), true, D0);
        m.turn_used(Some("m-big"), &tokens(10, 10, 0), true, D0);
        // the agent reported its own cost: nothing was estimated, so nothing is missing
        m.turn_used(Some("m-other"), &tokens(10, 10, 0), false, D0);
        let by = &m.stats(D0)[0].by_model;
        assert_eq!(by["m-unlisted"].unpriced, 2);
        assert_eq!(by["m-big"].unpriced, 0);
        assert_eq!(by["m-other"].unpriced, 0);
        let (priced, unpriced) = m.price_report();
        assert_eq!(
            (priced, unpriced),
            (
                vec!["m-big".to_string()],
                vec!["m-other".to_string(), "m-unlisted".to_string()]
            )
        );
    }

    #[test]
    fn task_telemetry_an_agent_reported_cost_adds_to_the_day_and_a_turn_without_an_estimate_adds_none(
    ) {
        let m = AttentionMeter::in_memory().with_prices(PRICES);
        m.turn_used(Some("m-big"), &tokens(1_000_000, 0, 0), false, D0);
        m.agent_cost(Some("m-big"), 250_000, D0);
        m.agent_cost(Some("m-big"), 0, D0);
        let d = &m.stats(D0)[0];
        assert_eq!(
            (d.cost_micros, d.by_model["m-big"].cost_micros),
            (250_000, 250_000)
        );
        assert_eq!(d.by_model["m-big"].turns, 1);
    }

    #[test]
    fn task_telemetry_a_hand_edited_file_cannot_carry_free_text_into_the_buckets() {
        let d = dir("scrub");
        let body = r#"{"version":1,"days":[{"day":20000,"by_model":{"say hello to the user":{"turns":1}},"verified":{"approved by the boss":4}}]}"#;
        std::fs::write(d.join(FILE), body).unwrap();
        let m = AttentionMeter::open(&d, D0).unwrap();
        let day = &m.stats(D0)[0];
        assert_eq!(day.by_model.keys().collect::<Vec<_>>(), ["unknown"]);
        assert!(day.verified.is_empty());
    }

    #[test]
    fn a_human_answer_records_the_layer_the_device_class_the_dwell_and_the_latency() {
        let m = AttentionMeter::in_memory();
        m.raised("a1", D0);
        assert!(m.card_seen("a1", 4_000, D0 + MIN));
        m.resolved("a1", Layer::Human, Some("phone"), D0 + 2 * MIN);
        let d = &m.stats(D0 + 3 * MIN)[0];
        assert_eq!(d.raised, 1);
        assert_eq!(d.by_layer["human"], 1);
        assert_eq!(d.by_device["phone"], 1);
        assert_eq!(d.human_ms, 4_000);
        assert_eq!(d.latency_ms, 2 * MIN);
    }

    #[test]
    fn a_device_that_is_not_a_class_is_other_and_a_name_is_never_kept() {
        let m = AttentionMeter::in_memory();
        m.raised("a1", D0);
        m.resolved("a1", Layer::Human, Some("Sergei's iPhone"), D0);
        assert_eq!(
            m.stats(D0)[0].by_device.keys().collect::<Vec<_>>(),
            ["other"]
        );
    }

    #[test]
    fn a_rule_resolved_obligation_adds_zero_human_seconds() {
        let m = AttentionMeter::in_memory();
        m.raised("a1", D0);
        m.resolved("a1", Layer::Rule, None, D0 + 1_000);
        assert!(
            !m.card_seen("a1", 9_000, D0 + 2_000),
            "late dwell is not credited"
        );
        let d = &m.stats(D0 + 3_000)[0];
        assert_eq!((d.human_ms, d.latency_ms), (0, 0));
        assert_eq!(d.by_layer["rule"], 1);
        assert!(d.by_device.is_empty());
        let t = totals(&m.stats(D0 + 3_000), 20_000, 7);
        assert_eq!((t.human_seconds, t.without_human), (0.0, Some(1.0)));
    }

    #[test]
    fn dwell_for_an_unknown_obligation_or_a_long_time_is_bounded() {
        let m = AttentionMeter::in_memory();
        assert!(!m.card_seen("nope", 1_000, D0));
        m.raised("a1", D0);
        m.card_seen("a1", u64::from(u32::MAX), D0);
        assert_eq!(m.stats(D0)[0].human_ms, MAX_DWELL_MS);
    }

    #[test]
    fn a_forgotten_obligation_is_raised_but_never_resolved() {
        let m = AttentionMeter::in_memory();
        m.raised("a1", D0);
        m.forget("a1");
        m.resolved("a1", Layer::Human, None, D0 + 1);
        let d = &m.stats(D0 + 1)[0];
        assert_eq!((d.raised, d.by_layer.len()), (1, 0));
    }

    #[test]
    fn working_time_is_summed_over_tabs_and_split_at_midnight() {
        let m = AttentionMeter::in_memory();
        let late = D0 + DAY_MS - 10 * MIN;
        m.tab_working("w", "t1", true, late);
        m.tab_working("w", "t2", true, late);
        m.tab_working("w", "t1", false, late + 30 * MIN);
        // t2 is still working: counted up to now without closing the span.
        let days = m.stats(late + 30 * MIN);
        assert_eq!(days.len(), 2);
        assert_eq!((days[0].agent_ms, days[1].agent_ms), (20 * MIN, 40 * MIN));
        m.close_where(late + 60 * MIN, |w, _| w == "w");
        let days = m.stats(late + 90 * MIN);
        assert_eq!(days[0].agent_ms + days[1].agent_ms, 90 * MIN);
    }

    #[test]
    fn a_span_open_across_flushes_is_banked_once_and_survives_a_restart() {
        let d = dir("bank");
        let m = AttentionMeter::open(&d, D0).unwrap();
        m.tab_working("w", "t", true, D0);
        m.flush(D0 + MIN).unwrap();
        m.flush(D0 + 2 * MIN).unwrap();
        assert_eq!(m.stats(D0 + 2 * MIN)[0].agent_ms, 2 * MIN);
        // The server dies a minute after the last flush: that minute of the running turn is the most that is lost.
        let again = AttentionMeter::open(&d, D0 + 3 * MIN).unwrap();
        assert_eq!(again.stats(D0 + 3 * MIN)[0].agent_ms, 2 * MIN);
        // The tab stops after a flush: only the time since it is added.
        m.flush(D0 + 3 * MIN).unwrap();
        m.tab_working("w", "t", false, D0 + 3 * MIN + 20_000);
        assert_eq!(m.stats(D0 + 4 * MIN)[0].agent_ms, 3 * MIN + 20_000);
    }

    #[test]
    fn buckets_roll_over_by_day_and_stay_bounded() {
        let m = AttentionMeter::in_memory();
        for i in 0..(KEEP_DAYS as u64 + 10) {
            m.raised(&format!("a{i}"), D0 + i * DAY_MS);
        }
        let days = m.stats(D0 + 40 * DAY_MS);
        assert_eq!(days.len(), KEEP_DAYS);
        assert_eq!(days[0].day, 20_010);
        assert_eq!(days.last().unwrap().day, 20_039);
    }

    #[test]
    fn open_obligations_are_bounded() {
        let m = AttentionMeter::in_memory();
        for i in 0..(MAX_OPEN + 50) {
            m.raised(&format!("a{i}"), D0);
        }
        assert_eq!(m.stats(D0)[0].raised as usize, MAX_OPEN);
        m.raised(&"x".repeat(MAX_ID + 1), D0);
        assert_eq!(m.stats(D0)[0].raised as usize, MAX_OPEN);
    }

    #[test]
    fn the_seven_day_figure_is_human_seconds_per_agent_hour() {
        let m = AttentionMeter::in_memory();
        m.tab_working("w", "t", true, D0);
        m.raised("a1", D0);
        m.card_seen("a1", 30_000, D0);
        m.resolved("a1", Layer::Human, None, D0 + MIN);
        m.raised("a2", D0);
        m.resolved("a2", Layer::Rule, None, D0);
        m.tab_working("w", "t", false, D0 + 3_600_000);
        let t = totals(&m.stats(D0 + 3_600_000), 20_000, 7);
        assert_eq!((t.human_seconds, t.agent_hours, t.raised), (30.0, 1.0, 2));
        assert_eq!((t.leverage(), t.without_human), (Some(30.0), Some(0.5)));
        let none = totals(&[], 20_000, 7);
        assert_eq!((none.leverage(), none.without_human), (None, None));
    }

    #[test]
    fn the_file_survives_a_restart_and_holds_no_text() {
        let d = dir("file");
        let m = AttentionMeter::open(&d, D0).unwrap();
        m.raised("a1", D0);
        m.resolved("a1", Layer::Human, Some("desktop"), D0 + 5_000);
        m.tab_working("secret-workspace-name", "secret-tab", true, D0);
        m.tab_working("secret-workspace-name", "secret-tab", false, D0 + MIN);
        m.flush(D0 + MIN).unwrap();
        let raw = std::fs::read_to_string(d.join(FILE)).unwrap();
        for text in ["secret-workspace-name", "secret-tab", "a1"] {
            assert!(!raw.contains(text), "{text} reached the file");
        }
        // Every string in the file is a field name or one of the layer and device classes.
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let mut strings = Vec::new();
        fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::String(s) => out.push(s.clone()),
                serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        walk(&v, &mut strings);
        assert!(strings.is_empty(), "free text in the file: {strings:?}");
        let again = AttentionMeter::open(&d, D0 + MIN).unwrap();
        let day = &again.stats(D0 + MIN)[0];
        assert_eq!((day.raised, day.agent_ms, day.latency_ms), (1, MIN, 5_000));
        assert!(!d.join("attention_meter.json.tmp").exists());
    }

    #[test]
    fn a_file_with_free_text_or_old_days_is_scrubbed_on_open() {
        let d = dir("scrub");
        let old = 20_000 - KEEP_DAYS as u32 - 5;
        let body = format!(
            r#"{{"version":1,"days":[
              {{"day":{old},"raised":9}},
              {{"day":20000,"raised":1,"by_layer":{{"human":1,"the prompt said x":4}},"by_device":{{"Sergei":1,"phone":2}}}}]}}"#
        );
        std::fs::write(d.join(FILE), body).unwrap();
        let m = AttentionMeter::open(&d, D0).unwrap();
        let days = m.stats(D0);
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].by_layer.keys().collect::<Vec<_>>(), ["human"]);
        assert_eq!(days[0].by_device.keys().collect::<Vec<_>>(), ["phone"]);
    }

    #[test]
    fn a_corrupt_file_starts_empty_and_a_clean_meter_writes_nothing() {
        let d = dir("corrupt");
        std::fs::write(d.join(FILE), b"{not json").unwrap();
        let m = AttentionMeter::open(&d, D0).unwrap();
        assert!(m.stats(D0).is_empty());
        m.flush(D0 + MIN).unwrap();
        assert_eq!(std::fs::read(d.join(FILE)).unwrap(), b"{not json");
    }
}
