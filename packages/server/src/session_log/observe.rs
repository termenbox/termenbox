//! The transcript as an evidence source for the status machine (decision 0182). A tail reads the lines appended to a
//! Claude session file since its last read and maps them to typed `Event`s: tool calls and results, a pending ask with
//! its question and options, the Esc paths, an error stop. It never says a status.
//!
//! The transcript is a reconciler, not the lead: a turn that starts or ends is an `Observe` at transcript rank, which
//! the machine weighs against the hook (fact 0072: the file trails the hook by up to 400 ms, and holds a pending ask
//! and an Esc within 500 ms). Only the ask events carry content, and that content is cleaned and capped by the
//! adapter helpers, as for a hook.
//!
//! An unreadable line is skipped and counted, never shown; a file that shrank, or whose head is not a version this
//! adapter knows, is an error for the caller to degrade on (`LogError`), logged without content.
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

use serde_json::Value;
use termenbox_core::events::AskKind;

use super::claude::{next_line, AtRead};
use super::LogError;
use crate::adapters::{ask_content, ask_kind, failure_cause};
use crate::status::event::{AskOutcome, Event, Outcome, Seen, Source};

/// A human prompt starts with this when the user stopped the turn (measured on 2.1.308).
const INTERRUPTED: &str = "[Request interrupted by user";
/// The result text of a tool the user declined, or stopped with Esc.
const REJECTED: &str = "The user doesn't want to proceed";

/// Where a read of one session file stands: how far it got, and the open tool calls it has seen.
#[derive(Debug, Default)]
pub struct Tail {
    offset: u64,
    /// `tool_use` id to tool name, until its result arrives.
    open: HashMap<String, String>,
    /// Tools whose result was a refusal: an Esc record next makes them `Escaped`, any other record `Declined`.
    refused: Vec<String>,
}

impl Tail {
    /// A tail that replays the whole file (fixtures, a session found already running).
    pub fn from_start() -> Self {
        Tail::default()
    }

    /// A tail that sees only what is appended after `len`.
    pub fn from(len: u64) -> Self {
        Tail {
            offset: len,
            ..Tail::default()
        }
    }

    /// The events of every complete line appended since the last call. A last line still being written is left for
    /// the next call. A file shorter than the offset was replaced: `StaleCursor`, and the caller starts a new tail.
    pub fn read(&mut self, file: &File) -> Result<Vec<Event>, LogError> {
        let len = file
            .metadata()
            .map_err(|_| LogError::Disabled("unreadable"))?
            .len();
        if len < self.offset {
            return Err(LogError::StaleCursor);
        }
        let mut r = BufReader::new(AtRead {
            file,
            pos: self.offset,
        });
        let (mut buf, mut out, mut bad) = (Vec::new(), Vec::new(), 0u32);
        while let Some(took) =
            next_line(&mut r, &mut buf).map_err(|_| LogError::Disabled("unreadable"))?
        {
            if buf.last() != Some(&b'\n') {
                break;
            }
            self.offset += took;
            // An over-long line comes back empty: stepped over, nothing to map.
            match serde_json::from_slice::<Value>(&buf) {
                Ok(v) => self.line(&v, &mut out),
                Err(_) if buf.is_empty() => {}
                Err(_) => bad += 1,
            }
        }
        if bad > 0 {
            tracing::warn!(lines = bad, "transcript lines not understood, skipped");
        }
        Ok(out)
    }

    fn line(&mut self, v: &Value, out: &mut Vec<Event>) {
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        let content = v.pointer("/message/content");
        let blocks: &[Value] = content.and_then(Value::as_array).map_or(&[], Vec::as_slice);
        match ty {
            "user" => {
                let esc = blocks.iter().any(|b| text(b).starts_with(INTERRUPTED));
                if esc {
                    self.settle(AskOutcome::Escaped, out);
                    out.push(seen(Seen::Idle));
                    return;
                }
                self.settle(AskOutcome::Declined, out);
                match content {
                    Some(Value::String(_)) => out.push(seen(Seen::Working)),
                    _ => {
                        for b in blocks {
                            if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                                self.result(b, out);
                            }
                        }
                    }
                }
            }
            "assistant" => {
                self.settle(AskOutcome::Declined, out);
                if v.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true) {
                    let cause = failure_cause(v.get("error").and_then(Value::as_str).unwrap_or(""));
                    out.push(Event::TurnEnd {
                        outcome: Outcome::Failed(cause),
                        bg: 0,
                    });
                    return;
                }
                for b in blocks {
                    if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                        self.call(b, out);
                    }
                }
                let stop = v.pointer("/message/stop_reason").and_then(Value::as_str);
                out.push(seen(if stop == Some("end_turn") {
                    Seen::Idle
                } else {
                    Seen::Working
                }));
            }
            _ => {}
        }
    }

    fn call(&mut self, b: &Value, out: &mut Vec<Event>) {
        let (Some(id), Some(name)) = (
            b.get("id").and_then(Value::as_str),
            b.get("name").and_then(Value::as_str),
        ) else {
            return;
        };
        self.open.insert(id.to_string(), name.to_string());
        out.push(Event::ToolStart {
            name: name.to_string(),
        });
        if matches!(name, "AskUserQuestion" | "ExitPlanMode") {
            let kind: AskKind = ask_kind(name);
            out.push(Event::AskOpen {
                key: name.to_string(),
                kind,
            });
            let input = b.get("input").unwrap_or(&Value::Null);
            if let Some((text, options)) = ask_content(name, input) {
                out.push(Event::AskContent {
                    key: name.to_string(),
                    text,
                    options,
                });
            }
        }
    }

    fn result(&mut self, b: &Value, out: &mut Vec<Event>) {
        let Some(name) = b
            .get("tool_use_id")
            .and_then(Value::as_str)
            .and_then(|id| self.open.remove(id))
        else {
            return;
        };
        let refused = match b.get("content") {
            Some(Value::String(s)) => s.starts_with(REJECTED),
            Some(Value::Array(a)) => a.iter().any(|c| text(c).starts_with(REJECTED)),
            _ => false,
        };
        if refused {
            self.refused.push(name.clone());
        } else if matches!(name.as_str(), "AskUserQuestion" | "ExitPlanMode") {
            out.push(Event::AskClose {
                key: name.clone(),
                outcome: AskOutcome::Answered,
            });
        }
        out.push(Event::ToolEnd { name });
    }

    /// Close the asks of refused tools: the record that follows says whether it was Esc or a plain "no".
    fn settle(&mut self, outcome: AskOutcome, out: &mut Vec<Event>) {
        for key in self.refused.drain(..) {
            out.push(Event::AskClose { key, outcome });
        }
    }
}

fn seen(s: Seen) -> Event {
    Event::Observe {
        source: Source::Transcript,
        seen: s,
    }
}

fn text(b: &Value) -> &str {
    b.get("text").and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lag(name: &str) -> Vec<Event> {
        let path = format!(
            "{}/testdata/transcripts/lag/claude-{name}.jsonl",
            env!("CARGO_MANIFEST_DIR")
        );
        Tail::from_start().read(&File::open(path).unwrap()).unwrap()
    }

    fn ask(key: &str, kind: AskKind) -> Event {
        Event::AskOpen {
            key: key.into(),
            kind,
        }
    }

    #[test]
    fn a_question_is_pending_with_its_text_then_answered() {
        let ev = lag("question");
        assert!(ev.contains(&ask("AskUserQuestion", AskKind::Question)));
        assert!(ev
            .iter()
            .any(|e| matches!(e, Event::AskContent { text, options, .. }
            if text == "Do you prefer red or blue?" && options == &["Red", "Blue"])));
        assert!(ev.contains(&Event::AskClose {
            key: "AskUserQuestion".into(),
            outcome: AskOutcome::Answered
        }));
        assert_eq!(ev.last(), Some(&seen(Seen::Idle)));
    }

    #[test]
    fn esc_on_a_question_or_a_permission_closes_it_escaped() {
        for (name, key) in [
            ("question_esc", "AskUserQuestion"),
            ("permission_esc", "Bash"),
            ("esc_turn", "Bash"),
        ] {
            let ev = lag(name);
            assert!(
                ev.contains(&Event::AskClose {
                    key: key.into(),
                    outcome: AskOutcome::Escaped
                }),
                "{name}"
            );
            assert_eq!(ev.last(), Some(&seen(Seen::Idle)), "{name}");
        }
    }

    #[test]
    fn a_tool_with_and_without_a_result() {
        let ev = lag("tool");
        let at = |e: &Event| ev.iter().position(|x| x == e).unwrap();
        assert!(
            at(&Event::ToolStart {
                name: "Bash".into()
            }) < at(&Event::ToolEnd {
                name: "Bash".into()
            })
        );
        assert_eq!(ev.last(), Some(&seen(Seen::Idle)));
        // A call with no result yet (the file is cut after it) leaves the tool open.
        let mut t = Tail::from_start();
        let cut = std::fs::read_to_string(format!(
            "{}/testdata/transcripts/lag/claude-silent.jsonl",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let upto: String = cut
            .lines()
            .take_while(|l| !l.contains("\"tool_result\""))
            .map(|l| format!("{l}\n"))
            .collect();
        let f = tmp(&upto);
        let ev = t.read(&f).unwrap();
        assert!(ev.contains(&Event::ToolStart {
            name: "Bash".into()
        }));
        assert!(!ev.iter().any(|e| matches!(e, Event::ToolEnd { .. })));
    }

    #[test]
    fn a_plain_refusal_is_declined_not_escaped() {
        let f = tmp(concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"x"}}],"stop_reason":"tool_use"}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"The user doesn't want to proceed with this tool use."}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}}"#,
            "\n",
        ));
        let ev = Tail::from_start().read(&f).unwrap();
        assert!(ev.contains(&Event::AskClose {
            key: "Bash".into(),
            outcome: AskOutcome::Declined
        }));
        assert!(!ev.iter().any(|e| matches!(
            e,
            Event::AskClose {
                outcome: AskOutcome::Escaped,
                ..
            }
        )));
    }

    #[test]
    fn an_error_stop_is_a_failed_turn_end() {
        let f = tmp(concat!(
            r#"{"type":"assistant","isApiErrorMessage":true,"error":"rate_limit","message":{"content":[{"type":"text","text":"limit"}]}}"#,
            "\n",
        ));
        let ev = Tail::from_start().read(&f).unwrap();
        assert_eq!(
            ev,
            vec![Event::TurnEnd {
                outcome: Outcome::Failed(termenbox_core::events::FailureCause::RateLimit),
                bg: 0
            }]
        );
    }

    #[test]
    fn a_malformed_transcript_gives_nothing_and_no_panic() {
        let f = tmp(
            "not json\n{\"type\":\n\u{0}\u{0}\n[1,2]\n{\"type\":\"assistant\",\"message\":7}\n",
        );
        let ev = Tail::from_start().read(&f).unwrap();
        // The one parsable assistant record has no content: it is only a working observation.
        assert!(ev.iter().all(|e| matches!(e, Event::Observe { .. })));
    }

    #[test]
    fn a_half_written_line_waits_and_a_shrunk_file_is_stale() {
        let line = r#"{"type":"user","message":{"content":"hi"}}"#;
        let f = tmp(&format!("{line}\n{}", &line[..20]));
        let mut t = Tail::from_start();
        assert_eq!(t.read(&f).unwrap(), vec![seen(Seen::Working)]);
        assert_eq!(t.read(&f).unwrap(), vec![]);
        let g = tmp("");
        assert_eq!(t.read(&g), Err(LogError::StaleCursor));
    }

    #[test]
    fn nothing_from_the_text_of_a_tool_reaches_an_event() {
        let ev = lag("tool");
        assert!(
            !format!("{ev:?}").contains("lagprobe"),
            "tool input and output stay in the file"
        );
    }

    fn tmp(body: &str) -> File {
        use std::io::Write;
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("tb-observe-{}-{n}", std::process::id()));
        let mut f = File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        let f = File::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        f
    }
}
