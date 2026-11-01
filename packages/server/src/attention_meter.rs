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
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use termenbox_core::protocol::AttentionDay;

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

fn device_class(d: Option<&str>) -> &'static str {
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
            inner: Mutex::default(),
        }
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

    /// Writes the buckets if they changed. Called on an interval, not per event.
    pub fn flush(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let body = {
            let mut g = self.inner.lock().unwrap();
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
        m.flush().unwrap();
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
        m.flush().unwrap();
        assert_eq!(std::fs::read(d.join(FILE)).unwrap(), b"{not json");
    }
}
