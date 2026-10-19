//! Mappers from what each tool reports to agent events (plan v6, P.3-P.5). Pure functions over the
//! tool's own data, so they are covered by recorded samples (`testdata/events/`). Text is not cleaned
//! here; the hub does that for every source.
use serde_json::Value;
use termenbox_core::events::{AgentState, Confidence, EventKind, Source, ToolPhase};

pub type Adapted = (Source, Confidence, Vec<EventKind>);

pub fn adapt(adapter: &str, data: &Value) -> Option<Adapted> {
    match adapter {
        "claude" => claude(data),
        "codex" => codex(data),
        _ => None,
    }
}

fn state(s: AgentState) -> EventKind {
    EventKind::State { state: s }
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// What a tool call is about, without the whole input: a command's first line, or a file path.
pub(crate) fn tool_summary(name: &str, input: &Value) -> String {
    let pick = |k: &str| text(input, k).lines().next().unwrap_or("").to_string();
    match name {
        "Bash" => pick("command"),
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => pick("file_path"),
        "Grep" | "Glob" => pick("pattern"),
        "WebFetch" => pick("url"),
        _ => String::new(),
    }
}

/// Tools that always wait for the human: their `PreToolUse` is the start of a wait, not of work.
const ASKING_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

/// Claude Code hook payloads (fact 0029), as turn and obligation edges for the reducer (`reducer.rs`): the
/// state is derived there, not here. Only the first line of the prompt becomes the task title; tool output is
/// never copied.
///
/// Measured on v2.1.302: `PermissionRequest` carries no `tool_use_id` and `Notification` follows it by up to six
/// seconds, so a wait is keyed by the tool name and a late notification never opens one on its own; pressing Esc
/// on a dialog or on a running turn fires no hook at all, which the screen reconciliation covers.
pub fn claude(d: &Value) -> Option<Adapted> {
    let tool = || text(d, "tool_name").to_string();
    let block = |key: String, open: bool| EventKind::Block { key, open };
    let kinds = match text(d, "hook_event_name") {
        // A compaction restarts the session mid-turn.
        "SessionStart" if text(d, "source") == "compact" => return None,
        "SessionStart" | "SessionEnd" => vec![turn_end(AgentState::Idle)],
        "UserPromptSubmit" => {
            let mut k = vec![EventKind::Turn {
                open: true,
                outcome: None,
            }];
            // The first line names the task; the rest of the prompt is never copied.
            let first = text(d, "prompt").lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                k.push(EventKind::Task {
                    title: first.to_string(),
                });
            }
            k
        }
        "PreToolUse" => {
            let name = tool();
            let summary = tool_summary(&name, d.get("tool_input").unwrap_or(&Value::Null));
            let asks = ASKING_TOOLS.contains(&name.as_str());
            let mut k = vec![EventKind::Tool {
                name: name.clone(),
                summary,
                phase: ToolPhase::Started,
            }];
            if asks {
                k.push(block(name, true));
            }
            k
        }
        "PostToolUse" | "PostToolUseFailure" => {
            let name = tool();
            let summary = tool_summary(&name, d.get("tool_input").unwrap_or(&Value::Null));
            vec![EventKind::Tool {
                name,
                summary,
                phase: ToolPhase::Finished,
            }]
        }
        "PermissionRequest" => vec![block(tool(), true)],
        // The tool is not run; the agent carries on with the refusal.
        "PermissionDenied" => vec![block(tool(), false)],
        "Elicitation" => vec![block(
            format!("elicit:{}", text(d, "mcp_server_name")),
            true,
        )],
        "ElicitationResult" => vec![block(
            format!("elicit:{}", text(d, "mcp_server_name")),
            false,
        )],
        "Notification" => match text(d, "notification_type") {
            // Only the words; the wait itself is opened by `PermissionRequest`. A notification alone opens one
            // for a setup whose hooks predate that event. Claude Code before it typed notifications sent none.
            "" | "permission_prompt" | "elicitation_dialog" => vec![
                block(crate::reducer::WEAK_KEY.into(), true),
                EventKind::Message {
                    summary: text(d, "message").to_string(),
                },
            ],
            // Claude has waited for the next prompt for a minute: a turn that ended without a `Stop`.
            "idle_prompt" => vec![turn_end(AgentState::Idle)],
            _ => return None,
        },
        "Stop" | "StopFailure" => {
            let ok = text(d, "hook_event_name") == "Stop";
            let outcome = if ok {
                AgentState::Done
            } else {
                AgentState::Error
            };
            let mut k = vec![turn_end(outcome)];
            let last = if ok {
                text(d, "last_assistant_message")
            } else {
                text(d, "error")
            };
            if !last.is_empty() {
                k.push(EventKind::Message {
                    summary: last.to_string(),
                });
            }
            k
        }
        _ => return None,
    };
    Some((Source::Hook, Confidence::High, kinds))
}

fn turn_end(outcome: AgentState) -> EventKind {
    EventKind::Turn {
        open: false,
        outcome: Some(outcome),
    }
}

/// Codex `notify` program argument. UNVERIFIED: Codex is not installed where this was written, so the
/// shape follows its documentation (fact 0029) and the sample in testdata is hand-written.
pub fn codex(d: &Value) -> Option<Adapted> {
    match text(d, "type") {
        "agent-turn-complete" => {
            let mut k = vec![state(AgentState::Done)];
            let last = text(d, "last-assistant-message");
            if !last.is_empty() {
                k.push(EventKind::Message {
                    summary: last.to_string(),
                });
            }
            Some((Source::Codex, Confidence::Medium, k))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------
// Process scan (decision 0061): which agent runs in a session and whether it is busy
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PaneScan {
    pub session: String,
    pub command: String,
    /// Seconds since the window last produced output.
    pub quiet_for: u64,
}

/// `tmux list-panes -a -F '#{session_name}|#{pane_current_command}|#{window_activity}'`
pub fn parse_panes(output: &str, now_s: u64) -> Vec<PaneScan> {
    output
        .lines()
        .filter_map(|l| {
            let mut f = l.split('|');
            let session = f.next()?.to_string();
            let command = f.next()?.to_string();
            let activity: u64 = f.next()?.trim().parse().ok()?;
            Some(PaneScan {
                session,
                command,
                quiet_for: now_s.saturating_sub(activity),
            })
        })
        .collect()
}

pub fn agent_name(command: &str) -> Option<&'static str> {
    let c = command.to_ascii_lowercase();
    AGENTS
        .into_iter()
        .find(|n| c == *n || c.starts_with(&format!("{n}-")))
}

/// Coding agents the scan recognises by the pane's foreground command.
pub const AGENTS: [&str; 11] = [
    "claude",
    "codex",
    "copilot",
    "opencode",
    "kilo",
    "gemini",
    "aider",
    "cursor-agent",
    "amp",
    "goose",
    "qwen",
];

/// Output in the last few seconds means busy; the floor, so always low confidence.
pub fn scan_state(p: &PaneScan) -> Option<AgentState> {
    agent_name(&p.command)?;
    Some(if p.quiet_for <= 3 {
        AgentState::Working
    } else {
        AgentState::Idle
    })
}

// ---------------------------------------------------------------------------------------------------
// Screen heuristics: "waiting for yes/no", best effort, always low confidence
// ---------------------------------------------------------------------------------------------------

/// The text that made the screen look like a question, if any. Looks only at the last lines.
pub fn detect_prompt(screen: &str) -> Option<String> {
    let lines: Vec<&str> = screen
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let tail = &lines[lines.len().saturating_sub(14)..];
    let has = |needle: &str| tail.iter().any(|l| l.to_ascii_lowercase().contains(needle));
    let yes_no_list = tail.iter().any(|l| {
        let l = l.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        l.starts_with("1. Yes") || l.starts_with("1) Yes")
    }) && tail.iter().any(|l| {
        let l = l.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        let mut c = l.chars();
        matches!(c.next(), Some('2'..='9'))
            && matches!(c.next(), Some('.' | ')'))
            && l[2..].trim_start().starts_with("No")
    });
    let inline = has("(y/n)") || has("[y/n]") || has("(yes/no)");
    if !(yes_no_list || inline) {
        return None;
    }
    // The question line: the last line that asks something.
    tail.iter()
        .rev()
        .find(|l| l.contains('?') || l.to_ascii_lowercase().contains("y/n"))
        .or(tail.last())
        .map(|l| l.to_string())
}

/// What a screen question offers and the keys that answer each choice.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PromptInfo {
    pub question: String,
    /// Label shown on the button and the exact keys sent for it.
    pub choices: Vec<(String, String)>,
}

/// Like `detect_prompt`, with the choices: a numbered list answers with its digit, an inline (y/n) with the
/// letter and Enter.
pub fn parse_prompt(screen: &str) -> Option<PromptInfo> {
    let question = detect_prompt(screen)?;
    let lines: Vec<&str> = screen
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let tail = &lines[lines.len().saturating_sub(14)..];
    let mut choices = Vec::new();
    for l in tail {
        let l = l.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        let mut c = l.chars();
        if let (Some(d @ '1'..='9'), Some('.' | ')')) = (c.next(), c.next()) {
            let label: String = l[2..].trim().chars().take(40).collect();
            if !label.is_empty() {
                choices.push((label, d.to_string()));
            }
        }
    }
    if choices.is_empty() {
        choices = vec![("Yes".into(), "y\r".into()), ("No".into(), "n\r".into())];
    }
    Some(PromptInfo { question, choices })
}

#[cfg(test)]
mod tests {
    use super::agent_name;

    #[test]
    fn well_known_agents_are_found() {
        for a in [
            "claude",
            "codex",
            "copilot",
            "opencode",
            "kilo",
            "gemini",
            "aider",
            "cursor-agent",
            "amp",
            "goose",
            "qwen",
        ] {
            assert_eq!(agent_name(a), Some(a));
            assert_eq!(agent_name(&format!("{}-1.2", a)), Some(a));
        }
        assert_eq!(agent_name("CLAUDE"), Some("claude"));
        assert_eq!(agent_name("bash"), None);
        assert_eq!(agent_name("ampersand"), None);
    }

    use super::*;
    use serde_json::json;

    fn sample(name: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/events")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}")))
            .unwrap()
    }

    fn open() -> EventKind {
        EventKind::Turn {
            open: true,
            outcome: None,
        }
    }

    fn end(outcome: AgentState) -> EventKind {
        EventKind::Turn {
            open: false,
            outcome: Some(outcome),
        }
    }

    fn block(key: &str, open: bool) -> EventKind {
        EventKind::Block {
            key: key.into(),
            open,
        }
    }

    fn tool(name: &str, summary: &str, phase: ToolPhase) -> EventKind {
        EventKind::Tool {
            name: name.into(),
            summary: summary.into(),
            phase,
        }
    }

    #[test]
    fn recorded_claude_hooks_map_to_turn_block_tool_and_message_events() {
        let k = |f: &str| claude(&sample(f)).unwrap().2;
        assert_eq!(
            k("claude-userpromptsubmit.json"),
            vec![
                open(),
                EventKind::Task {
                    title: "Run the bash command: echo hello. Then say done.".into()
                }
            ]
        );
        assert_eq!(
            k("claude-pretooluse.json"),
            vec![tool("Bash", "echo hello", ToolPhase::Started)]
        );
        assert_eq!(
            k("claude-posttooluse.json"),
            vec![tool("Bash", "echo hello", ToolPhase::Finished)]
        );
        assert_eq!(
            k("claude-stop.json"),
            vec![
                end(AgentState::Done),
                EventKind::Message {
                    summary: "The command printed `hello`. Done.".into()
                }
            ]
        );
        assert!(claude(&json!({"hook_event_name": "SomethingNew"})).is_none());
    }

    #[test]
    fn a_question_or_a_permission_is_a_wait_not_work() {
        let k = |f: &str| claude(&sample(f)).unwrap().2;
        // Recorded on v2.1.302: the request names the tool but carries no tool_use_id.
        assert_eq!(
            k("claude-permissionrequest.json"),
            vec![block("Bash", true)]
        );
        let ask = k("claude-pretooluse-askuserquestion.json");
        assert_eq!(ask.len(), 2);
        assert!(
            matches!(&ask[0], EventKind::Tool { name, phase: ToolPhase::Started, .. } if name == "AskUserQuestion")
        );
        assert_eq!(ask[1], block("AskUserQuestion", true));
        assert_eq!(
            k("claude-permissionrequest-askuserquestion.json"),
            vec![block("AskUserQuestion", true)]
        );
        // Answering is the tool finishing; the reducer resolves the wait by the tool's name.
        assert!(matches!(
            &k("claude-posttooluse-askuserquestion.json")[0],
            EventKind::Tool { name, phase: ToolPhase::Finished, .. } if name == "AskUserQuestion"
        ));
    }

    #[test]
    fn a_notification_is_only_a_weak_wait_and_an_idle_prompt_ends_the_turn() {
        let n = claude(&sample("claude-notification.json")).unwrap().2;
        assert_eq!(n[0], block(crate::reducer::WEAK_KEY, true));
        assert!(matches!(&n[1], EventKind::Message { summary } if summary.contains("permission")));
        let idle = json!({"hook_event_name": "Notification", "notification_type": "idle_prompt", "message": "Claude is waiting for your input"});
        assert_eq!(claude(&idle).unwrap().2, vec![end(AgentState::Idle)]);
        let other = json!({"hook_event_name": "Notification", "notification_type": "auth_success"});
        assert!(claude(&other).is_none());
        let untyped = json!({"hook_event_name": "Notification", "message": "needs a decision"});
        assert_eq!(
            claude(&untyped).unwrap().2[0],
            block(crate::reducer::WEAK_KEY, true)
        );
    }

    #[test]
    fn endings_and_denials_map_to_their_edges() {
        let ev = |name: &str| {
            claude(&json!({"hook_event_name": name, "tool_name": "Edit"}))
                .unwrap()
                .2
        };
        assert_eq!(ev("PermissionDenied"), vec![block("Edit", false)]);
        assert_eq!(ev("SessionEnd"), vec![end(AgentState::Idle)]);
        assert_eq!(ev("SessionStart"), vec![end(AgentState::Idle)]);
        assert!(claude(&json!({"hook_event_name": "SessionStart", "source": "compact"})).is_none());
        let fail = json!({"hook_event_name": "StopFailure", "error": "rate_limit"});
        assert_eq!(claude(&fail).unwrap().2[0], end(AgentState::Error));
        let el = json!({"hook_event_name": "Elicitation", "mcp_server_name": "github"});
        assert_eq!(claude(&el).unwrap().2, vec![block("elicit:github", true)]);
        let done = json!({"hook_event_name": "ElicitationResult", "mcp_server_name": "github"});
        assert_eq!(
            claude(&done).unwrap().2,
            vec![block("elicit:github", false)]
        );
    }

    #[test]
    fn only_the_first_prompt_line_and_no_tool_output_is_copied() {
        let out =
            serde_json::to_string(&claude(&sample("claude-posttooluse.json")).unwrap().2).unwrap();
        assert!(!out.contains("\"hello\""));
        let d = json!({"hook_event_name": "UserPromptSubmit", "prompt": "fix the login\nsecond line with detail"});
        let out = serde_json::to_string(&claude(&d).unwrap().2).unwrap();
        assert!(out.contains("fix the login") && !out.contains("second line"));
        let empty = json!({"hook_event_name": "UserPromptSubmit", "prompt": ""});
        assert_eq!(claude(&empty).unwrap().2, vec![open()]);
        // The question text and the answers of AskUserQuestion stay in the tool input.
        let ask = serde_json::to_string(
            &claude(&sample("claude-posttooluse-askuserquestion.json"))
                .unwrap()
                .2,
        )
        .unwrap();
        assert!(!ask.contains("colour") && !ask.contains("Red"));
    }

    #[test]
    fn codex_turn_complete_is_done_and_other_notifications_are_ignored() {
        let (src, _, kinds) = codex(&sample("codex-notify.json")).unwrap();
        assert_eq!(src, Source::Codex);
        assert_eq!(kinds[0], state(AgentState::Done));
        assert!(codex(&json!({"type": "something-else"})).is_none());
    }

    #[test]
    fn the_scan_finds_agents_and_tells_busy_from_quiet() {
        let panes = parse_panes(
            "a|claude|1000\nb|bash|1000\nc|codex|900\nbad line\nd|node|x\n",
            1002,
        );
        assert_eq!(panes.len(), 3);
        assert_eq!(scan_state(&panes[0]), Some(AgentState::Working));
        assert_eq!(scan_state(&panes[1]), None);
        assert_eq!(scan_state(&panes[2]), Some(AgentState::Idle));
    }

    fn fixture(name: &str) -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/screens")
            .join(name);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"))
    }

    #[test]
    fn the_numbered_choices_of_a_real_permission_screen_answer_with_their_digits() {
        let info = parse_prompt(&fixture("claude-permission.txt")).unwrap();
        let keys: Vec<_> = info.choices.iter().map(|(_, k)| k.as_str()).collect();
        assert_eq!(keys, ["1", "2", "3", "4"]);
        assert!(info.choices[0].0.starts_with("Yes"));
        assert!(info.choices[3].0.starts_with("No"));
        let inline = parse_prompt(&fixture("shell-yn.txt")).unwrap();
        assert_eq!(
            inline.choices,
            vec![("Yes".into(), "y\r".into()), ("No".into(), "n\r".into())]
        );
        assert!(parse_prompt(&fixture("claude-idle.txt")).is_none());
    }

    #[test]
    fn recorded_screens_are_recognised_as_questions_or_not() {
        assert!(detect_prompt(&fixture("claude-permission.txt"))
            .unwrap()
            .to_lowercase()
            .contains("proceed"));
        assert!(detect_prompt(&fixture("claude-idle.txt")).is_none());
        assert!(detect_prompt(&fixture("shell-yn.txt")).is_some());
        assert!(detect_prompt("hello\nworld").is_none());
        assert!(detect_prompt("").is_none());
    }
}
