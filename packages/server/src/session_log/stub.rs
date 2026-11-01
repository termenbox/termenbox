//! A stub adapter over a tiny JSONL format, to prove the contract suite passes a correct adapter and fails a wrong one.
//!
//! Line 1 is `{"stub":1}`. Each later line is `{"turn","kind","text"}` with `kind` one of `user`, `message`, `tool`
//! (`text` is the tool's name, `output` its output) or anything else (unmappable). `"abandoned":true` marks a record
//! on a branch that was rewound. A line without a newline at the end of the file is torn and dropped.
use std::path::PathBuf;

use serde_json::Value;

use super::*;

pub struct Stub {
    pub name: &'static str,
    pub root: PathBuf,
    /// The bug the suite must catch: put a tool's output into its entry.
    pub copy_tool_output: bool,
}

impl Stub {
    pub fn path(&self, s: &SessionRef) -> PathBuf {
        self.root.join(format!("{}.jsonl", s.id))
    }

    fn read(&self, s: &SessionRef) -> Result<(Fingerprint, Vec<Entry>), LogError> {
        let path = self.path(s);
        let fp = Fingerprint::of(&path).map_err(|_| LogError::Missing)?;
        let raw = std::fs::read(&path).map_err(|_| LogError::Missing)?;
        let text = String::from_utf8_lossy(&raw);
        let mut lines: Vec<&str> = text.split('\n').collect();
        lines.pop(); // after the last newline: empty, or a torn line
        let mut it = lines.into_iter();
        let head: Value = serde_json::from_str(it.next().unwrap_or(""))
            .map_err(|_| LogError::Disabled("unreadable header"))?;
        match head.get("stub").and_then(Value::as_u64) {
            Some(1) => {}
            Some(_) => return Err(LogError::Disabled("unknown version")),
            None => return Err(LogError::Disabled("unreadable header")),
        }
        let mut out = Vec::new();
        for (n, line) in it.enumerate() {
            let turn = format!("t{n}");
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                out.push(Entry::notice(turn, "unreadable record"));
                continue;
            };
            if v.get("abandoned").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let turn = v
                .get("turn")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(turn);
            let text = v.get("text").and_then(Value::as_str).unwrap_or("");
            out.push(match v.get("kind").and_then(Value::as_str) {
                Some("user") => Entry::new(turn, Kind::User, text),
                Some("message") => Entry::new(turn, Kind::Message, text),
                Some("tool") if self.copy_tool_output => Entry::new(
                    turn,
                    Kind::Tool,
                    &format!(
                        "{text} {}",
                        v.get("output").and_then(Value::as_str).unwrap_or("")
                    ),
                ),
                Some("tool") => Entry::new(turn, Kind::Tool, text),
                _ => Entry::notice(turn, "a record this version cannot show"),
            });
        }
        Ok((fp, out))
    }
}

impl SessionLog for Stub {
    fn agent(&self) -> &str {
        self.name
    }

    fn search(&self, s: &SessionRef, query: &str, max_hits: usize) -> Result<Vec<Hit>, LogError> {
        let (_, entries) = self.read(s)?;
        Ok(entries
            .iter()
            .rev()
            .filter(|e| e.kind != Kind::Tool)
            .filter_map(|e| {
                let (snippet, highlight) = snippet(&e.text, query)?;
                Some(Hit {
                    turn: e.turn.clone(),
                    kind: e.kind,
                    snippet,
                    highlight,
                    at: None,
                })
            })
            .take(max_hits)
            .collect())
    }

    fn window(
        &self,
        s: &SessionRef,
        anchor: &str,
        before: usize,
        after: usize,
    ) -> Result<Window, LogError> {
        let (fingerprint, entries) = self.read(s)?;
        let at = entries
            .iter()
            .position(|e| e.turn == anchor)
            .ok_or(LogError::NoAnchor)?;
        let from = at.saturating_sub(before);
        let to = (at + after + 1).min(entries.len());
        Ok(Window {
            entries: entries[from..to].to_vec(),
            anchor: anchor.to_string(),
            fingerprint,
        })
    }

    fn page(
        &self,
        s: &SessionRef,
        cursor: Option<&Cursor>,
        direction: Direction,
        count: usize,
    ) -> Result<Page, LogError> {
        let (fingerprint, entries) = self.read(s)?;
        if let Some(c) = cursor {
            if !c.fingerprint.still_holds(&fingerprint) {
                return Err(LogError::StaleCursor);
            }
        }
        let pos = cursor
            .map_or(
                if direction == Direction::Older {
                    entries.len()
                } else {
                    0
                },
                |c| c.pos as usize,
            )
            .min(entries.len());
        let (slice, next) = match direction {
            Direction::Older => {
                let from = pos.saturating_sub(count);
                (&entries[from..pos], (from > 0).then_some(from))
            }
            Direction::Newer => {
                let to = (pos + count).min(entries.len());
                (&entries[pos..to], (to < entries.len()).then_some(to))
            }
        };
        Ok(Page {
            entries: slice.to_vec(),
            next: next.map(|p| Cursor {
                fingerprint,
                pos: p as u64,
            }),
        })
    }
}
