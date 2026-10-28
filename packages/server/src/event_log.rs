//! The per-session event log (decision 0139, step 3.4): every `session/update` of an agent conversation, scrubbed,
//! stamped with a sequence number and the server's epoch, in a ring bounded by bytes.
//!
//! A client that reconnects says where it was (`after` = the last `seq` it applied, and the `epoch`). If the log still
//! holds everything after that, it gets exactly that tail: gapless. Otherwise (the ring wrapped, the server
//! restarted, or the client holds nothing) it gets a *compacted replay*: chunks merged, tool calls collapsed to one
//! `tool_call`, only the latest plan, mode, commands, usage and title, still standard `session/update` frames, flagged
//! with the `reset` extension so the client drops what it held. Applying the replay gives the same state as applying
//! the full stream.
//!
//! The agent's text is untrusted. A frame is scrubbed like the workspace view is (`events::sanitize_block`: control
//! characters, secrets and addresses out, lengths capped) and raw tool input and output are never copied (decision
//! 0095). The extensions ride in `_meta` under `termenbox.dev/...` and never beside the standard fields (fact 0057).
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Map, Value};
use termenbox_core::ext::{ExtMeta, Extension, Negotiated, Registry, Reset, SeqStamp};

use crate::events::sanitize_block;

/// Default ring size per session: about four times what the longest recorded conversation in `testdata/acp/`
/// scrubs to (the `event_log_budget_...` test prints and checks the figure), so a whole turn replays gapless.
pub const DEFAULT_BUDGET_BYTES: usize = 1024 * 1024;
/// A frame above this is dropped: the direct transport carries 64 KiB a message and a replay frame rides in one.
const MAX_FRAME_BYTES: usize = 40 * 1024;
const MAX_TEXT_CHARS: usize = 8_000;
const MAX_TEXT_LINES: usize = 400;
const MAX_ARRAY: usize = 100;
/// Keys whose value is raw tool data: never copied (decision 0095).
const RAW_KEYS: [&str; 2] = ["rawInput", "rawOutput"];

/// Bytes held by every log in the process, for the health sample.
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

pub fn total_bytes() -> u64 {
    TOTAL_BYTES.load(Ordering::Relaxed)
}

/// What a client gets for "resume from seq N".
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub epoch: u64,
    /// The log's last `seq` now: the client applies frames up to it and asks `after = last` next time.
    pub last: u64,
    /// A compacted rebuild: the client drops what it held first.
    pub reset: bool,
    /// `session/update` notification params, `{"sessionId", "update"}`, in order.
    pub frames: Vec<Value>,
}

/// One frame of a compacted stream, with its size so that the budget is kept without serialising it again.
#[derive(Clone)]
struct Item {
    /// The `seq` of the last original frame it covers.
    seq: u64,
    update: Value,
    size: usize,
}

impl Item {
    fn new(seq: u64, update: Value) -> Self {
        let size = update.to_string().len() + FRAME_OVERHEAD;
        Self { seq, update, size }
    }
}

/// What `{"sessionId":..,"update":..}` and the stamp add around an update.
const FRAME_OVERHEAD: usize = 120;

struct Entry {
    seq: u64,
    frame: Value,
    bytes: usize,
}

pub struct EventLog {
    session: String,
    epoch: u64,
    next_seq: u64,
    budget: usize,
    ring: VecDeque<Entry>,
    /// What fell out of the ring, compacted (never raw, never gaplessly replayable): it lets a reset replay still
    /// build the whole conversation once the ring wrapped, for as long as it fits the budget.
    base: Vec<Item>,
    base_bytes: usize,
    /// Bytes of the ring and the base together.
    bytes: usize,
    /// Frames that did not fit `MAX_FRAME_BYTES` or did not decode, for the metrics and the tests.
    dropped: u64,
}

impl EventLog {
    pub fn new(session: &str, epoch: u64, budget: usize) -> Self {
        Self {
            session: session.to_string(),
            epoch,
            next_seq: 1,
            budget,
            ring: VecDeque::new(),
            base: Vec::new(),
            base_bytes: 0,
            bytes: 0,
            dropped: 0,
        }
    }

    /// The ACP `sessionId` the frames are for (known once the agent has answered `session/new`). Frames already
    /// logged keep the id they were made with: set it before the first update.
    pub fn set_session(&mut self, id: &str) {
        self.session = id.to_string();
    }

    #[allow(dead_code)] // read by the tests; the client of step 5.1 and the metrics use them
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The `seq` of the newest frame; 0 before the first.
    pub fn last_seq(&self) -> u64 {
        self.next_seq - 1
    }

    #[allow(dead_code)] // read by the tests; the client of step 5.1 and the metrics use them
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[allow(dead_code)] // read by the tests; the client of step 5.1 and the metrics use them
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Scrubs `update`, stamps it and appends it, dropping the oldest frames while the ring is over its budget.
    /// Returns its `seq`, or `None` for a frame that is not an object or is too large to carry.
    pub fn push(&mut self, update: &Value) -> Option<u64> {
        let Some(mut u) = scrub(update).filter(|u| u.is_object()) else {
            self.dropped += 1;
            return None;
        };
        let seq = self.next_seq;
        stamp(
            &mut u,
            SeqStamp {
                seq,
                epoch: self.epoch,
            },
        );
        let frame = json!({"sessionId": self.session, "update": u});
        let bytes = frame.to_string().len();
        if bytes > MAX_FRAME_BYTES {
            self.dropped += 1;
            return None;
        }
        self.next_seq += 1;
        self.ring.push_back(Entry { seq, frame, bytes });
        self.grow(bytes);
        self.fit();
        Some(seq)
    }

    /// Brings the log under its budget: the oldest raw frames move into the compacted base, and when that alone is
    /// too big its oldest items go. The newest frame is always kept, even if it alone is over a tiny budget.
    fn fit(&mut self) {
        while self.bytes > self.budget {
            if self.ring.len() > 1 {
                let Some(old) = self.ring.pop_front() else {
                    break;
                };
                self.shrink(old.bytes);
                let before = self.base_bytes;
                if let Some(u) = old.frame.get("update") {
                    absorb(&mut self.base, old.seq, u);
                }
                self.base_bytes = self.base.iter().map(|i| i.size).sum();
                // The base is part of the budget, but moving a frame into it can also shrink the whole.
                if self.base_bytes >= before {
                    self.grow(self.base_bytes - before);
                } else {
                    self.shrink(before - self.base_bytes);
                }
            } else if !self.base.is_empty() {
                let gone = self.base.remove(0);
                self.base_bytes -= gone.size;
                self.shrink(gone.size);
            } else {
                break;
            }
        }
    }

    fn grow(&mut self, n: usize) {
        self.bytes += n;
        TOTAL_BYTES.fetch_add(n as u64, Ordering::Relaxed);
    }

    fn shrink(&mut self, n: usize) {
        self.bytes -= n;
        TOTAL_BYTES.fetch_sub(n as u64, Ordering::Relaxed);
    }

    /// Resume from `after` (the last `seq` the client applied) in `epoch`.
    pub fn resume(&self, after: Option<u64>, epoch: Option<u64>) -> Replay {
        let last = self.last_seq();
        let first = self.ring.front().map(|e| e.seq);
        let gapless = match (after, epoch, first) {
            (Some(a), Some(e), Some(f)) => e == self.epoch && a <= last && a + 1 >= f,
            // Nothing logged yet: a client at 0 in this epoch has everything.
            (Some(a), Some(e), None) => e == self.epoch && a == last,
            _ => false,
        };
        if gapless {
            let a = after.unwrap_or(0);
            let frames = self
                .ring
                .iter()
                .filter(|e| e.seq > a)
                .map(|e| e.frame.clone())
                .collect();
            return Replay {
                epoch: self.epoch,
                last,
                reset: false,
                frames,
            };
        }
        let mut all = self.base.clone();
        for e in &self.ring {
            if let Some(u) = e.frame.get("update") {
                absorb(&mut all, e.seq, u);
            }
        }
        let frames = finish(all, &self.session, self.epoch, last);
        Replay {
            epoch: self.epoch,
            last,
            reset: true,
            frames,
        }
    }
}

impl Drop for EventLog {
    fn drop(&mut self) {
        TOTAL_BYTES.fetch_sub(self.bytes as u64, Ordering::Relaxed);
    }
}

/// Puts `stamp` into the update's `_meta`.
fn stamp<E: Extension>(update: &mut Value, ext: E) {
    let negotiated = both_ways();
    let Some(o) = update.as_object_mut() else {
        return;
    };
    let meta = o.entry("_meta").or_insert_with(|| json!({}));
    if let Some(m) = meta.as_object() {
        let mut em = ExtMeta(m.clone());
        em.put(&ext, &negotiated);
        *meta = Value::Object(em.0);
    }
}

/// The registry as both sides of the log know it: the log is internal, so what it stamps is always "negotiated"; a
/// connection strips what its peer did not negotiate before sending (`ExtMeta::strip_unnegotiated`).
fn both_ways() -> Negotiated {
    let us = Registry::new().with::<SeqStamp>().with::<Reset>();
    let mut caps = Map::new();
    us.advertise(&mut caps);
    us.negotiate(Some(&caps))
}

/// The stamp of a logged frame.
#[allow(dead_code)] // read by the tests; the client of step 5.1 and the metrics use them
pub fn stamp_of(frame: &Value) -> Option<SeqStamp> {
    let meta = frame.pointer("/update/_meta")?.as_object()?.clone();
    ExtMeta(meta).get::<SeqStamp>(&both_ways()).ok().flatten()
}

/// The frames of a replay for a peer that did not negotiate our extensions: plain ACP.
#[allow(dead_code)] // read by the tests; the client of step 5.1 and the metrics use them
pub fn plain(frames: &[Value]) -> Vec<Value> {
    frames
        .iter()
        .map(|f| {
            let mut f = f.clone();
            if let Some(Value::Object(m)) = f.pointer_mut("/update/_meta") {
                let mut em = ExtMeta(std::mem::take(m));
                em.strip_all();
                *m = em.0;
            }
            f
        })
        .collect()
}

/// Cleans untrusted JSON: strings scrubbed and capped, raw tool data and over-long arrays cut. `None` for a value
/// that cannot be kept at all (not an object at the top).
fn scrub(v: &Value) -> Option<Value> {
    fn walk(v: &Value, key: Option<&str>) -> Value {
        match v {
            Value::String(s) => {
                // Ids and kinds are short plain strings; text and diffs are cleaned block-wise.
                Value::String(sanitize_block(s, MAX_TEXT_CHARS, MAX_TEXT_LINES))
            }
            Value::Array(a) => {
                Value::Array(a.iter().take(MAX_ARRAY).map(|x| walk(x, key)).collect())
            }
            Value::Object(o) => Value::Object(
                o.iter()
                    .filter(|(k, _)| !RAW_KEYS.contains(&k.as_str()))
                    .map(|(k, x)| (k.clone(), walk(x, Some(k))))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    v.is_object().then(|| walk(v, None))
}

fn kind_of(update: &Value) -> &str {
    update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn text_of(update: &Value) -> Option<&str> {
    let c = update.get("content")?;
    (c.get("type")?.as_str()? == "text")
        .then(|| c.get("text")?.as_str())
        .flatten()
}

fn is_chunk(kind: &str) -> bool {
    matches!(
        kind,
        "user_message_chunk"
            | "agent_message_chunk"
            | "agent_thought_chunk"
            | "compaction_summary_chunk"
    )
}

/// The kinds of which only the newest matters.
fn is_latest_wins(kind: &str) -> bool {
    matches!(
        kind,
        "plan"
            | "available_commands_update"
            | "current_mode_update"
            | "config_option_update"
            | "session_info_update"
            | "usage_update"
    )
}

/// Adds one frame to a compacted stream, merging it into what is there when it can be (see the module doc): a run of
/// text chunks of one kind becomes one chunk, a tool call and its updates one `tool_call` at the place of the first,
/// and only the newest plan, mode, commands, config, title and usage stay.
fn absorb(out: &mut Vec<Item>, seq: u64, u: &Value) {
    let kind = kind_of(u).to_string();
    if is_chunk(&kind) {
        if let (Some(prev), Some(add)) = (out.last_mut(), text_of(u)) {
            if kind_of(&prev.update) == kind
                && prev.update.get("messageId") == u.get("messageId")
                && prev.update.get("compactionId") == u.get("compactionId")
            {
                if let Some(t) = prev.update.pointer_mut("/content/text") {
                    if let Some(old) = t.as_str() {
                        *t = Value::String(format!("{old}{add}"));
                        prev.seq = seq;
                        prev.size += add.len();
                        return;
                    }
                }
            }
        }
    }
    if kind == "tool_call" || kind == "tool_call_update" {
        let id = u.get("toolCallId");
        if let Some(at) = out.iter().position(|p| {
            matches!(kind_of(&p.update), "tool_call" | "tool_call_update")
                && p.update.get("toolCallId") == id
        }) {
            merge_tool(&mut out[at].update, u);
            out[at].seq = seq;
            out[at].size = out[at].update.to_string().len() + FRAME_OVERHEAD;
            return;
        }
    }
    if is_latest_wins(&kind) {
        out.retain(|p| kind_of(&p.update) != kind);
    }
    out.push(Item::new(seq, u.clone()));
}

/// The frames of a replay: each carries the `seq` of the last original it covers, the first also says `reset`.
fn finish(all: Vec<Item>, session: &str, epoch: u64, last: u64) -> Vec<Value> {
    all.into_iter()
        .enumerate()
        .map(
            |(
                i,
                Item {
                    seq, update: mut u, ..
                },
            )| {
                stamp(&mut u, SeqStamp { seq, epoch });
                if i == 0 {
                    stamp(&mut u, Reset { last });
                }
                json!({"sessionId": session, "update": u})
            },
        )
        .collect()
}

/// `update` (a `tool_call` or `tool_call_update`) laid over `into`: sent fields replace, the kind stays `tool_call`
/// when `into` is one.
fn merge_tool(into: &mut Value, update: &Value) {
    let (Some(a), Some(b)) = (into.as_object_mut(), update.as_object()) else {
        return;
    };
    for (k, v) in b {
        if k == "sessionUpdate" || k == "_meta" || v.is_null() {
            continue;
        }
        a.insert(k.clone(), v.clone());
    }
}

#[cfg(test)]
mod tests;
