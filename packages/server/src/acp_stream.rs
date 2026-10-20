//! The T0 session stream (decision 0111): what the ACP session view shows, built from the same `session/update`
//! payloads the event mapper reads, but keeping the structure a screen needs (whole Markdown replies, tool calls
//! with kind and status, diffs, the plan, permission requests and how they ended) instead of one-line summaries.
//!
//! Only what the protocol carries is here. Raw tool input and tool output are never copied (decision 0095); a diff
//! is the one piece of tool content that is, because it is what a permission asks the owner to approve. Every text is
//! untrusted agent text: control characters and escape sequences are removed, secrets and addresses are replaced
//! (`events::sanitize_block`) and every length is capped, so a hostile agent cannot make this grow without bound.
//! Memory per session is bounded by `MAX_ENTRIES` and `MAX_BYTES`; the oldest entries are dropped first.
use std::collections::{HashMap, VecDeque};

use serde_json::Value;
use termenbox_core::events::{DecisionOption, PlanItem};
use termenbox_core::protocol::{
    AcpBody, AcpCommand, AcpDiff, AcpEntry, AcpInfo, AcpLocation, AcpMode, AcpUsage, AcpView,
};

use crate::events::{sanitize_block, sanitize_text};

pub const MAX_ENTRIES: usize = 400;
pub const MAX_BYTES: usize = 512 * 1024;
/// The direct transport carries at most 64 KiB in one message and closes the connection on more (rtc.rs), so a view is
/// cut into messages of at most this many bytes of JSON, and no single entry is allowed more than `MAX_ENTRY_BYTES`.
pub const MAX_MESSAGE_BYTES: usize = 48 * 1024;
pub const MAX_ENTRY_BYTES: usize = 24 * 1024;
/// One reply, thought or prompt.
const MESSAGE_CHARS: usize = 16_000;
const MESSAGE_LINES: usize = 600;
/// What is kept raw while a reply grows; a longer reply stops growing (the entry says so).
const RAW_BYTES: usize = 48 * 1024;
const TITLE_CHARS: usize = 200;
const PATH_CHARS: usize = 300;
const DIFF_CHARS: usize = 6_000;
const DIFF_LINES: usize = 200;
const DIFFS_PER_TOOL: usize = 3;
const LOCATIONS: usize = 5;
const PLAN_ITEMS: usize = 20;
const PLAN_CHARS: usize = 8_000;
const MAX_COMMANDS: usize = 120;
const MAX_MODES: usize = 12;

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn line(text: &str, max: usize) -> String {
    sanitize_text(text.lines().next().unwrap_or(""), max)
}

/// The cut length of a string by characters.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Chunk {
    Message,
    Thought,
}

/// The reply or thought that is still growing: its raw text is kept so a secret split over two chunks is still
/// found when the whole is scrubbed again.
struct Open {
    kind: Chunk,
    id: u64,
    message_id: Option<String>,
    raw: String,
    /// Chunks arrived that the entry does not show yet. The text is cleaned when it is read or closed, not per chunk,
    /// so a reply of thousands of chunks costs one pass per update to the client and not one per chunk.
    dirty: bool,
}

struct Slot {
    entry: AcpEntry,
    weight: usize,
}

/// How the mode is changed on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModeVia {
    /// `session/set_mode` (the agent listed `modes`).
    Modes,
    /// `session/set_config_option` with this config id (the agent listed a `mode` config option only).
    Config(String),
    None,
}

pub struct Stream {
    epoch: u64,
    rev: u64,
    next_id: u64,
    slots: VecDeque<Slot>,
    bytes: usize,
    open: Option<Open>,
    tools: HashMap<String, u64>,
    perms: HashMap<String, u64>,
    plan: Option<u64>,
    info: AcpInfo,
    info_rev: u64,
    via: ModeVia,
}

fn weight(body: &AcpBody) -> usize {
    let diffs = |d: &[AcpDiff]| {
        d.iter()
            .map(|x| x.path.len() + x.new_text.len() + x.old_text.as_ref().map_or(0, String::len))
            .sum::<usize>()
    };
    64 + match body {
        AcpBody::User { text }
        | AcpBody::Message { text }
        | AcpBody::Thought { text }
        | AcpBody::Notice { text, .. } => text.len(),
        AcpBody::Tool {
            title,
            diffs: d,
            locations,
            ..
        } => title.len() + diffs(d) + locations.iter().map(|l| l.path.len()).sum::<usize>(),
        AcpBody::Plan { items } => items.iter().map(|i| i.text.len() + 20).sum(),
        AcpBody::Permission {
            title,
            diffs: d,
            plan,
            options,
            ..
        } => {
            title.len()
                + diffs(d)
                + plan.as_ref().map_or(0, String::len)
                + options
                    .iter()
                    .map(|o| o.name.len() + o.id.len())
                    .sum::<usize>()
        }
        AcpBody::Unknown => 0,
    }
}

fn diffs_of(content: &Value) -> Vec<AcpDiff> {
    content
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| s(c, "type") == "diff")
                .take(DIFFS_PER_TOOL)
                .map(|c| {
                    let new_text = s(c, "newText");
                    let old_text = c.get("oldText").and_then(Value::as_str);
                    let cutting = |t: &str| sanitize_block(t, DIFF_CHARS, DIFF_LINES);
                    let (n, o) = (cutting(new_text), old_text.map(cutting));
                    let truncated = new_text.chars().count() > DIFF_CHARS
                        || new_text.lines().count() > DIFF_LINES
                        || old_text.is_some_and(|t| {
                            t.chars().count() > DIFF_CHARS || t.lines().count() > DIFF_LINES
                        });
                    AcpDiff {
                        path: sanitize_text(s(c, "path"), PATH_CHARS),
                        old_text: o,
                        new_text: n,
                        truncated,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn locations_of(v: &Value) -> Vec<AcpLocation> {
    v.as_array()
        .map(|a| {
            a.iter()
                .take(LOCATIONS)
                .filter(|l| !s(l, "path").is_empty())
                .map(|l| AcpLocation {
                    path: sanitize_text(s(l, "path"), PATH_CHARS),
                    line: l
                        .get("line")
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn plan_items(entries: &Value, text_key: &str) -> Vec<PlanItem> {
    entries
        .as_array()
        .map(|a| {
            a.iter()
                .take(PLAN_ITEMS)
                .map(|e| PlanItem {
                    text: sanitize_text(s(e, text_key), 200),
                    status: sanitize_text(s(e, "status"), 20),
                })
                .filter(|i| !i.text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn mode_options(list: &Value, id_key: &str) -> Vec<AcpMode> {
    list.as_array()
        .map(|a| {
            a.iter()
                .take(MAX_MODES)
                .filter(|m| !s(m, id_key).is_empty())
                .map(|m| AcpMode {
                    id: s(m, id_key).to_string(),
                    name: sanitize_text(
                        if s(m, "name").is_empty() {
                            s(m, id_key)
                        } else {
                            s(m, "name")
                        },
                        40,
                    ),
                    description: Some(sanitize_text(s(m, "description"), 160))
                        .filter(|d| !d.is_empty()),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn texts_mut(body: &mut AcpBody) -> Vec<&mut String> {
    fn diffs(d: &mut [AcpDiff]) -> Vec<&mut String> {
        d.iter_mut()
            .flat_map(|x| {
                x.old_text
                    .iter_mut()
                    .chain(std::iter::once(&mut x.new_text))
            })
            .collect()
    }
    match body {
        AcpBody::User { text }
        | AcpBody::Message { text }
        | AcpBody::Thought { text }
        | AcpBody::Notice { text, .. } => vec![text],
        AcpBody::Tool { diffs: d, .. } => diffs(d),
        AcpBody::Permission { diffs: d, plan, .. } => {
            let mut v = diffs(d);
            v.extend(plan.iter_mut());
            v
        }
        AcpBody::Plan { .. } | AcpBody::Unknown => Vec::new(),
    }
}

/// Makes one entry fit `MAX_ENTRY_BYTES` of JSON by halving its long texts (a character count says little about bytes:
/// emoji take four, and JSON escapes more). A reply that cannot be made to fit is replaced by a notice that says so.
fn fit(e: &mut AcpEntry) {
    for _ in 0..16 {
        if serde_json::to_vec(&e).map_or(0, |v| v.len()) <= MAX_ENTRY_BYTES {
            return;
        }
        for t in texts_mut(&mut e.body) {
            if t.chars().count() > 200 {
                *t = cut(t, t.chars().count() / 2);
            }
        }
        if let AcpBody::Tool { diffs, .. } | AcpBody::Permission { diffs, .. } = &mut e.body {
            diffs.iter_mut().for_each(|d| d.truncated = true);
        }
    }
    e.body = AcpBody::Notice {
        level: "info".into(),
        text: "This entry is too large to show.".into(),
    };
}

/// Cuts a view into messages of at most `MAX_MESSAGE_BYTES`. Entries go in order of their revision, so that after any
/// message the client holds every entry up to that message's `rev` and a dropped connection resumes correctly; only the
/// first part of a reset resets; the facts follow in a message of their own.
pub fn split(view: AcpView) -> Vec<AcpView> {
    let AcpView {
        session,
        epoch,
        rev,
        reset,
        first,
        mut entries,
        info,
    } = view;
    entries.sort_by_key(|e| e.rev);
    let part = |reset: bool, rev: u64, entries: Vec<AcpEntry>, info: Option<AcpInfo>| AcpView {
        session: session.clone(),
        epoch,
        rev,
        reset,
        first,
        entries,
        info,
    };
    let mut out: Vec<AcpView> = Vec::new();
    let mut current: Vec<AcpEntry> = Vec::new();
    let mut size = 0;
    for mut e in entries {
        fit(&mut e);
        let n = serde_json::to_vec(&e).map_or(0, |v| v.len()) + 1;
        if size + n > MAX_MESSAGE_BYTES && !current.is_empty() {
            let rev_here = current.last().map_or(rev, |l| l.rev);
            out.push(part(
                reset && out.is_empty(),
                rev_here,
                std::mem::take(&mut current),
                None,
            ));
            size = 0;
        }
        size += n;
        current.push(e);
    }
    // The last part of the entries carries the stream's revision.
    out.push(part(reset && out.is_empty(), rev, current, None));
    if let Some(mut i) = info {
        // The command list is the one long thing in the facts: shorten it until the message fits.
        while serde_json::to_vec(&i).map_or(0, |v| v.len()) > MAX_MESSAGE_BYTES - 1024
            && !i.commands.is_empty()
        {
            i.commands.truncate(i.commands.len() / 2);
        }
        out.push(part(false, rev, Vec::new(), Some(i)));
    }
    out
}

impl Stream {
    pub fn new(agent: &str, dir: &str, now_ms: u64) -> Self {
        Self {
            epoch: now_ms,
            rev: 0,
            next_id: 1,
            slots: VecDeque::new(),
            bytes: 0,
            open: None,
            tools: HashMap::new(),
            perms: HashMap::new(),
            plan: None,
            info: AcpInfo {
                agent: sanitize_text(agent, 40),
                dir: cut(dir, 512),
                status: "running".into(),
                ..AcpInfo::default()
            },
            info_rev: 1,
            via: ModeVia::None,
        }
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn mode_via(&self) -> &ModeVia {
        &self.via
    }

    pub fn mode_ids(&self) -> Vec<String> {
        self.info.modes.iter().map(|m| m.id.clone()).collect()
    }

    fn touch(&mut self) -> u64 {
        self.rev += 1;
        self.rev
    }

    fn info_changed(&mut self) {
        self.info_rev += 1;
        self.touch();
    }

    fn first_id(&self) -> u64 {
        self.slots.front().map_or(self.next_id, |s| s.entry.id)
    }

    fn slot_mut(&mut self, id: u64) -> Option<&mut Slot> {
        let first = self.slots.front()?.entry.id;
        let idx = usize::try_from(id.checked_sub(first)?).ok()?;
        self.slots.get_mut(idx).filter(|s| s.entry.id == id)
    }

    fn push(&mut self, body: AcpBody) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let rev = self.touch();
        let w = weight(&body);
        self.bytes += w;
        self.slots.push_back(Slot {
            entry: AcpEntry { id, rev, body },
            weight: w,
        });
        while self.slots.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            match self.slots.pop_front() {
                Some(old) => self.bytes -= old.weight,
                None => break,
            }
        }
        id
    }

    /// Replaces the body of entry `id` (a no-op when it has been dropped) and stamps the new revision.
    fn set(&mut self, id: u64, body: AcpBody) {
        let rev = self.touch();
        let mut delta = 0isize;
        if let Some(slot) = self.slot_mut(id) {
            let w = weight(&body);
            delta = w as isize - slot.weight as isize;
            slot.weight = w;
            slot.entry = AcpEntry { id, rev, body };
        }
        self.bytes = (self.bytes as isize + delta).max(0) as usize;
    }

    fn body(&self, id: u64) -> Option<&AcpBody> {
        let first = self.slots.front()?.entry.id;
        let idx = usize::try_from(id.checked_sub(first)?).ok()?;
        self.slots
            .get(idx)
            .filter(|s| s.entry.id == id)
            .map(|s| &s.entry.body)
    }

    /// Cleans the growing reply into its entry (see `Open::dirty`).
    fn flush_open(&mut self) {
        let Some(open) = self.open.as_mut().filter(|o| o.dirty) else {
            return;
        };
        open.dirty = false;
        let (id, kind, clean) = (
            open.id,
            open.kind,
            sanitize_block(&open.raw, MESSAGE_CHARS, MESSAGE_LINES),
        );
        let body = match kind {
            Chunk::Message => AcpBody::Message { text: clean },
            Chunk::Thought => AcpBody::Thought { text: clean },
        };
        self.set(id, body);
    }

    /// Ends the growing reply or thought: what it holds is final.
    fn close_open(&mut self) {
        self.flush_open();
        self.open = None;
    }

    /// The owner sent a prompt.
    pub fn user(&mut self, text: &str) {
        self.close_open();
        self.push(AcpBody::User {
            text: sanitize_block(text, MESSAGE_CHARS, MESSAGE_LINES),
        });
        self.info.working = true;
        self.info_changed();
    }

    /// A line the view says itself: `level` is `info` or `error`.
    pub fn notice(&mut self, level: &str, text: &str) {
        self.close_open();
        self.push(AcpBody::Notice {
            level: level.into(),
            text: sanitize_text(text, 300),
        });
    }

    /// The turn ended. `stop_reason` is the `session/prompt` response's, `error` a failed request's message.
    pub fn turn_end(&mut self, stop_reason: Option<&str>, error: Option<&str>) {
        self.close_open();
        // Whatever the turn left open is dropped by the agent with it.
        self.cancel_open_permissions();
        if let Some(e) = error {
            self.push(AcpBody::Notice {
                level: "error".into(),
                text: sanitize_text(e, 300),
            });
        } else {
            match stop_reason {
                Some("end_turn") | None => {}
                Some("cancelled") => {
                    self.push(AcpBody::Notice {
                        level: "info".into(),
                        text: "Interrupted".into(),
                    });
                }
                Some(other) => {
                    self.push(AcpBody::Notice {
                        level: "info".into(),
                        text: format!("Stopped: {}", sanitize_text(other, 40)),
                    });
                }
            }
        }
        self.info.working = false;
        self.info_changed();
    }

    /// The agent process is gone (`ended`) or died (`failed`, with its exit status).
    pub fn status(&mut self, status: &str, text: Option<&str>) {
        self.close_open();
        self.cancel_open_permissions();
        self.info.status = status.into();
        self.info.status_text = text.map(|t| sanitize_text(t, 120));
        self.info.working = false;
        let notice = match status {
            "failed" => Some(("error", "The agent stopped unexpectedly.")),
            "ended" => Some(("info", "The session ended.")),
            _ => None,
        };
        if let Some((level, text)) = notice {
            self.notice(level, text);
        }
        self.info_changed();
    }

    /// What `session/new`, `session/resume` or `session/load` returned: the modes and the current one.
    pub fn session_state(&mut self, result: &Value) {
        if let Some(modes) = result.get("modes") {
            let list = mode_options(&modes["availableModes"], "id");
            if !list.is_empty() {
                self.info.modes = list;
                self.via = ModeVia::Modes;
            }
            if let Some(cur) = modes.get("currentModeId").and_then(Value::as_str) {
                self.info.mode = Some(cut(cur, 80));
            }
        }
        if self.via == ModeVia::None {
            self.config_modes(&result["configOptions"]);
        }
        self.info_changed();
    }

    /// The agent's `mode` config option, for agents that list modes only there (opencode, kilo, gemini).
    fn config_modes(&mut self, options: &Value) {
        let Some(opt) = options.as_array().and_then(|a| {
            a.iter()
                .find(|o| s(o, "category") == "mode" && o.get("options").is_some())
        }) else {
            return;
        };
        let list = mode_options(&opt["options"], "value");
        if list.is_empty() {
            return;
        }
        self.info.modes = list;
        if self.via == ModeVia::None {
            self.via = ModeVia::Config(cut(s(opt, "id"), 80));
        }
        if let Some(cur) = opt.get("currentValue").and_then(Value::as_str) {
            self.info.mode = Some(cut(cur, 80));
        }
    }

    pub fn set_mode(&mut self, mode: &str) {
        self.info.mode = Some(cut(mode, 80));
        self.info_changed();
    }

    /// A `session/update` payload.
    pub fn update(&mut self, u: &Value) {
        match s(u, "sessionUpdate") {
            "agent_message_chunk" => self.chunk(Chunk::Message, u),
            "agent_thought_chunk" => self.chunk(Chunk::Thought, u),
            "tool_call" | "tool_call_update" => {
                self.close_open();
                self.tool(u);
            }
            "plan" => {
                self.close_open();
                let items = plan_items(&u["entries"], "content");
                if !items.is_empty() {
                    self.set_plan(items);
                }
            }
            "usage_update" => {
                if let (Some(used), Some(size)) = (
                    u.get("used").and_then(Value::as_u64),
                    u.get("size").and_then(Value::as_u64),
                ) {
                    let cost = u.get("cost").filter(|c| c.is_object()).map(|c| {
                        sanitize_text(
                            &format!(
                                "{} {}",
                                c.get("amount").map(|a| a.to_string()).unwrap_or_default(),
                                s(c, "currency")
                            ),
                            40,
                        )
                    });
                    self.info.usage = Some(AcpUsage { used, size, cost });
                    self.info_changed();
                }
            }
            "session_info_update" => {
                if let Some(t) = u.get("title").and_then(Value::as_str) {
                    self.info.title = Some(sanitize_text(t, TITLE_CHARS)).filter(|t| !t.is_empty());
                    self.info_changed();
                }
            }
            "current_mode_update" => {
                self.info.mode = Some(cut(s(u, "currentModeId"), 80)).filter(|m| !m.is_empty());
                self.info_changed();
            }
            "config_option_update" => {
                self.config_modes(&u["configOptions"]);
                self.info_changed();
            }
            "available_commands_update" => {
                self.info.commands = u["availableCommands"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .take(MAX_COMMANDS)
                            .filter(|c| !s(c, "name").is_empty())
                            .map(|c| AcpCommand {
                                name: sanitize_text(s(c, "name"), 64),
                                description: Some(sanitize_text(s(c, "description"), 160))
                                    .filter(|d| !d.is_empty()),
                                hint: c
                                    .get("input")
                                    .map(|i| sanitize_text(s(i, "hint"), 80))
                                    .filter(|h| !h.is_empty()),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.info_changed();
            }
            _ => {}
        }
    }

    fn chunk(&mut self, kind: Chunk, u: &Value) {
        let content = u.get("content").unwrap_or(&Value::Null);
        if s(content, "type") != "text" {
            return;
        }
        let text = s(content, "text");
        let message_id = u
            .get("messageId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let continues = self.open.as_ref().is_some_and(|o| {
            o.kind == kind
                && (o.message_id.is_none() || message_id.is_none() || o.message_id == message_id)
        });
        if !continues {
            self.close_open();
            if text.trim().is_empty() {
                return;
            }
            let body = match kind {
                Chunk::Message => AcpBody::Message {
                    text: String::new(),
                },
                Chunk::Thought => AcpBody::Thought {
                    text: String::new(),
                },
            };
            let id = self.push(body);
            self.open = Some(Open {
                kind,
                id,
                message_id,
                raw: String::new(),
                dirty: false,
            });
        }
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let room = RAW_BYTES.saturating_sub(open.raw.len());
        if room == 0 {
            return;
        }
        let mut take = text.len().min(room);
        while !text.is_char_boundary(take) {
            take -= 1;
        }
        open.raw.push_str(&text[..take]);
        open.dirty = true;
        self.touch();
    }

    fn set_plan(&mut self, items: Vec<PlanItem>) {
        let body = AcpBody::Plan { items };
        match self.plan.filter(|id| self.body(*id).is_some()) {
            Some(id) => self.set(id, body),
            None => self.plan = Some(self.push(body)),
        }
    }

    fn tool(&mut self, u: &Value) {
        let tool_id = cut(s(u, "toolCallId"), 120);
        // kilo keeps its todo list in a `todowrite` call's input: the one place a plan is measured (fact 0040).
        if let Some(todos) = u.get("rawInput").and_then(|r| r.get("todos")) {
            let items = plan_items(todos, "content");
            if !items.is_empty() {
                self.set_plan(items);
            }
        }
        let existing = self
            .tools
            .get(&tool_id)
            .copied()
            .filter(|id| self.body(*id).is_some());
        let diffs = diffs_of(&u["content"]);
        let locations = locations_of(&u["locations"]);
        match existing {
            None => {
                let id = self.push(AcpBody::Tool {
                    tool_id: tool_id.clone(),
                    title: line(s(u, "title"), TITLE_CHARS),
                    tool_kind: sanitize_text(
                        if s(u, "kind").is_empty() {
                            "other"
                        } else {
                            s(u, "kind")
                        },
                        20,
                    ),
                    status: sanitize_text(
                        if s(u, "status").is_empty() {
                            "pending"
                        } else {
                            s(u, "status")
                        },
                        20,
                    ),
                    diffs,
                    locations,
                });
                if self.tools.len() >= 256 {
                    self.tools.clear();
                }
                self.tools.insert(tool_id, id);
            }
            Some(id) => {
                let Some(AcpBody::Tool {
                    title,
                    tool_kind,
                    status,
                    diffs: d,
                    locations: l,
                    tool_id: tid,
                }) = self.body(id).cloned()
                else {
                    return;
                };
                let body = AcpBody::Tool {
                    tool_id: tid,
                    title: if s(u, "title").is_empty() {
                        title
                    } else {
                        line(s(u, "title"), TITLE_CHARS)
                    },
                    tool_kind: if s(u, "kind").is_empty() {
                        tool_kind
                    } else {
                        sanitize_text(s(u, "kind"), 20)
                    },
                    status: if s(u, "status").is_empty() {
                        status
                    } else {
                        sanitize_text(s(u, "status"), 20)
                    },
                    diffs: if diffs.is_empty() { d } else { diffs },
                    locations: if locations.is_empty() { l } else { locations },
                };
                if self.body(id) != Some(&body) {
                    self.set(id, body);
                }
            }
        }
    }

    /// A `session/request_permission`. `options` are the ones that may be shown (one-time kinds only).
    pub fn permission(&mut self, prompt_id: &str, params: &Value, options: Vec<DecisionOption>) {
        self.close_open();
        let tool = params.get("toolCall").unwrap_or(&Value::Null);
        let plan = tool
            .get("rawInput")
            .and_then(|r| r.get("plan"))
            .and_then(Value::as_str)
            .or_else(|| {
                (s(tool, "kind") == "switch_mode")
                    .then(|| tool["content"][0]["content"]["text"].as_str())
                    .flatten()
            })
            .map(|t| sanitize_block(t, PLAN_CHARS, 200))
            .filter(|t| !t.is_empty());
        let id = self.push(AcpBody::Permission {
            prompt_id: sanitize_text(prompt_id, 40),
            tool_id: cut(s(tool, "toolCallId"), 120),
            title: line(s(tool, "title"), TITLE_CHARS),
            tool_kind: sanitize_text(
                if s(tool, "kind").is_empty() {
                    "other"
                } else {
                    s(tool, "kind")
                },
                20,
            ),
            options: options
                .into_iter()
                .map(|o| DecisionOption {
                    id: o.id,
                    kind: o.kind,
                    name: sanitize_text(&o.name, 40),
                })
                .collect(),
            diffs: diffs_of(&tool["content"]),
            plan,
            state: "open".into(),
            chosen: None,
        });
        self.perms.insert(prompt_id.to_string(), id);
    }

    /// The owner chose `option_id`.
    pub fn permission_answered(&mut self, prompt_id: &str, option_id: &str) {
        self.resolve(prompt_id, "answered", Some(option_id.to_string()));
    }

    fn resolve(&mut self, prompt_id: &str, state: &str, chosen: Option<String>) {
        let Some(id) = self.perms.remove(prompt_id) else {
            return;
        };
        let Some(AcpBody::Permission {
            prompt_id: p,
            tool_id,
            title,
            tool_kind,
            options,
            diffs,
            plan,
            ..
        }) = self.body(id).cloned()
        else {
            return;
        };
        self.set(
            id,
            AcpBody::Permission {
                prompt_id: p,
                tool_id,
                title,
                tool_kind,
                options,
                diffs,
                plan,
                state: state.into(),
                chosen,
            },
        );
    }

    /// Every request still open was answered `cancelled` (interrupt, or the turn ended).
    pub fn cancel_open_permissions(&mut self) {
        let ids: Vec<String> = self.perms.keys().cloned().collect();
        for p in ids {
            self.resolve(&p, "cancelled", None);
        }
    }

    /// What changed after revision `after`. A client that holds another epoch, nothing, or a revision from the
    /// future (the stream was rebuilt) gets everything. `info_seen` is the info revision it already holds.
    pub fn view(
        &mut self,
        session: &str,
        after: Option<u64>,
        epoch: Option<u64>,
        info_seen: u64,
    ) -> (AcpView, u64) {
        self.flush_open();
        let reset = epoch != Some(self.epoch) || !matches!(after, Some(a) if a <= self.rev);
        let since = if reset { 0 } else { after.unwrap_or(0) };
        let entries = self
            .slots
            .iter()
            .filter(|s| s.entry.rev > since)
            .map(|s| s.entry.clone())
            .collect();
        let send_info = reset || self.info_rev > info_seen;
        (
            AcpView {
                session: session.to_string(),
                epoch: self.epoch,
                rev: self.rev,
                reset,
                first: self.first_id(),
                entries,
                info: send_info.then(|| self.info.clone()),
            },
            self.info_rev,
        )
    }
}

#[cfg(test)]
mod tests;
