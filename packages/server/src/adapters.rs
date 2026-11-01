//! Mappers from what each tool reports to agent events (plan v6, P.3-P.5). Pure functions over the
//! tool's own data, so they are covered by recorded samples (`testdata/events/`). Text is not cleaned
//! here; the hub does that for every source.
use serde_json::Value;
use termenbox_core::events::{
    AgentState, AskKind, Confidence, EventKind, FailureCause, Source, ToolPhase,
};

use crate::events::{sanitize_block, sanitize_text};
use crate::status::event::{AskOutcome, Event, Outcome, SessionSource};
use crate::status::machine::ANY_ASK;

pub type Adapted = (Source, Confidence, Vec<EventKind>);

pub fn adapt(adapter: &str, data: &Value) -> Option<Adapted> {
    match adapter {
        "claude" => claude(data),
        "codex" => codex(data),
        "opencode" => opencode(data),
        _ => None,
    }
}

/// The agent's own id for the conversation an event belongs to: Claude's and Codex's `workspace_id`, OpenCode's
/// `sessionID` (on the plugin's frame, or inside its data).
pub fn agent_session_id(adapter: &str, data: &Value) -> Option<String> {
    let id = match adapter {
        "claude" | "codex" => data.get("session_id").and_then(Value::as_str),
        "opencode" => data
            .get("sessionID")
            .or_else(|| data.get("data").and_then(|d| d.get("sessionID")))
            .and_then(Value::as_str),
        _ => None,
    }?;
    (!id.is_empty() && id.len() <= 128).then(|| id.to_string())
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
        // A compaction restarts the workspace mid-turn.
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

/// Untrusted agent text on a card (decision 0095): the question, an option, the plan.
#[allow(dead_code)] // read by the status machine from B.6
const ASK_TEXT_CHARS: usize = 600;
#[allow(dead_code)] // read by the status machine from B.6
const PLAN_CHARS: usize = 2_000;
#[allow(dead_code)] // read by the status machine from B.6
const PLAN_LINES: usize = 40;
#[allow(dead_code)] // read by the status machine from B.6
const ASK_OPTION_CHARS: usize = 80;
#[allow(dead_code)] // read by the status machine from B.6
const ASK_OPTIONS: usize = 8;

/// What a dialog asks, read from `tool_input`: `AskUserQuestion` carries the question and its options, `ExitPlanMode`
/// the plan, any other tool the tool and its summary (fact 0071: these were dropped before).
#[allow(dead_code)] // read by the status machine from B.6
fn ask_content(name: &str, input: &Value) -> Option<(String, Vec<String>)> {
    match name {
        "AskUserQuestion" => {
            let q = input.get("questions")?.as_array()?.first()?;
            let text = sanitize_text(text(q, "question"), ASK_TEXT_CHARS);
            let options = q
                .get("options")
                .and_then(Value::as_array)
                .map(|o| {
                    o.iter()
                        .map(|o| sanitize_text(text_of(o), ASK_OPTION_CHARS))
                        .filter(|o| !o.is_empty())
                        .take(ASK_OPTIONS)
                        .collect()
                })
                .unwrap_or_default();
            (!text.is_empty()).then_some((text, options))
        }
        "ExitPlanMode" => {
            let plan = sanitize_block(text(input, "plan"), PLAN_CHARS, PLAN_LINES);
            (!plan.is_empty()).then_some((plan, Vec::new()))
        }
        _ => {
            let summary = sanitize_text(&tool_summary(name, input), ASK_TEXT_CHARS);
            let name = sanitize_text(name, 40);
            (!name.is_empty()).then(|| {
                let text = if summary.is_empty() {
                    name
                } else {
                    format!("{name}: {summary}")
                };
                (text, Vec::new())
            })
        }
    }
}

/// An option is `{label, description}` in Claude's payload; a bare string is accepted too.
#[allow(dead_code)] // read by the status machine from B.6
fn text_of(o: &Value) -> &str {
    o.as_str().unwrap_or_else(|| text(o, "label"))
}

#[allow(dead_code)] // read by the status machine from B.6
fn ask_kind(tool: &str) -> AskKind {
    match tool {
        "AskUserQuestion" => AskKind::Question,
        "ExitPlanMode" => AskKind::Plan,
        _ => AskKind::Permission,
    }
}

/// `StopFailure.error` to the class of decision 0185. Anything unrecognised is `Unknown`, still a failure.
#[allow(dead_code)] // read by the status machine from B.6
fn failure_cause(error: &str) -> FailureCause {
    match error {
        "rate_limit" => FailureCause::RateLimit,
        "authentication_failed" | "auth" | "billing_error" => FailureCause::Auth,
        "server_error" | "invalid_request" | "max_output_tokens" => FailureCause::ApiError,
        e if e.contains("context") => FailureCause::Context,
        _ => FailureCause::Unknown,
    }
}

/// Claude Code hook payloads as typed events for the per-session machine (`status::machine`, decision 0182).
/// A hook states what happened, never the status. Every hook maps to something or is deliberately ignored:
/// `SubagentStop` with no `agent_type` fires from the main loop after each `Stop` and is noise (fact 0073), and
/// `idle_prompt` carries no information the `Stop` before it did not (fact 0073).
///
/// An ask is keyed by the tool name (`PermissionRequest` has no `tool_use_id`), an elicitation by its server.
/// `Notification` text has no key: it fills the one open ask that has none (`ANY_ASK`).
#[allow(dead_code)] // read by the status machine from B.6
pub fn claude_events(d: &Value) -> Vec<Event> {
    let tool = || text(d, "tool_name").to_string();
    let input = || d.get("tool_input").unwrap_or(&Value::Null);
    let open = |key: String, kind: AskKind, content: Option<(String, Vec<String>)>| {
        let mut v = vec![Event::AskOpen {
            key: key.clone(),
            kind,
        }];
        if let Some((text, options)) = content {
            v.push(Event::AskContent { key, text, options });
        }
        v
    };
    let close = |key: String, outcome| vec![Event::AskClose { key, outcome }];
    match text(d, "hook_event_name") {
        "SessionStart" => vec![Event::SessionStart(match text(d, "source") {
            "resume" => SessionSource::Resume,
            "clear" => SessionSource::Clear,
            "compact" => SessionSource::Compact,
            _ => SessionSource::Startup,
        })],
        "SessionEnd" => vec![Event::SessionEnd],
        "UserPromptSubmit" => {
            // The first line names the task; the rest of the prompt is never copied.
            let first = sanitize_text(text(d, "prompt").lines().next().unwrap_or(""), 120);
            vec![Event::TurnStart {
                title: (!first.is_empty()).then_some(first),
            }]
        }
        "PreToolUse" => {
            let name = tool();
            let mut v = vec![Event::ToolStart { name: name.clone() }];
            if matches!(name.as_str(), "AskUserQuestion" | "ExitPlanMode") {
                v.extend(open(
                    name.clone(),
                    ask_kind(&name),
                    ask_content(&name, input()),
                ));
            }
            v
        }
        "PostToolUse" | "PostToolUseFailure" => {
            let name = tool();
            let mut v = close(name.clone(), AskOutcome::Answered);
            v.push(Event::ToolEnd { name });
            v
        }
        "PermissionRequest" => {
            let name = tool();
            open(name.clone(), ask_kind(&name), ask_content(&name, input()))
        }
        // The tool is not run; the agent carries on with the refusal.
        "PermissionDenied" => close(tool(), AskOutcome::Declined),
        "Elicitation" => {
            let key = format!("elicit:{}", sanitize_text(text(d, "mcp_server_name"), 40));
            let msg = sanitize_text(text(d, "message"), ASK_TEXT_CHARS);
            open(
                key,
                AskKind::Elicitation,
                (!msg.is_empty()).then(|| (msg, Vec::new())),
            )
        }
        "ElicitationResult" => {
            let key = format!("elicit:{}", sanitize_text(text(d, "mcp_server_name"), 40));
            let outcome = match text(d, "action") {
                "accept" => AskOutcome::Answered,
                _ => AskOutcome::Declined,
            };
            close(key, outcome)
        }
        "Notification" => match text(d, "notification_type") {
            // Only the words: the ask itself is opened by `PermissionRequest` or `Elicitation`, and a notification
            // that comes first or never changes nothing (decision 0183).
            "" | "permission_prompt" | "elicitation_dialog" => {
                let msg = sanitize_text(text(d, "message"), ASK_TEXT_CHARS);
                if msg.is_empty() {
                    vec![]
                } else {
                    vec![Event::AskContent {
                        key: ANY_ASK.into(),
                        text: msg,
                        options: Vec::new(),
                    }]
                }
            }
            _ => vec![],
        },
        "Stop" => vec![Event::TurnEnd {
            outcome: Outcome::Done,
            // `background_tasks` and `session_crons` are what is still alive after the turn (fact 0073).
            bg: ["background_tasks", "session_crons"]
                .iter()
                .filter_map(|k| d.get(*k).and_then(Value::as_array))
                .map(|a| a.len() as u32)
                .sum(),
        }],
        "StopFailure" => vec![Event::TurnEnd {
            outcome: Outcome::Failed(failure_cause(text(d, "error"))),
            bg: 0,
        }],
        "SubagentStart" => vec![Event::SubagentStart],
        "SubagentStop" if !text(d, "agent_type").is_empty() => vec![Event::SubagentEnd],
        "PreCompact" => vec![Event::CompactStart],
        "PostCompact" => vec![Event::CompactEnd],
        _ => vec![],
    }
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

/// What an OpenCode tool call is about, without the whole input. OpenCode names its tools in lower case
/// (`bash`, `read`, `edit`, ...), unlike Claude Code's `Bash`/`Read`/`Edit`.
fn opencode_tool_summary(name: &str, input: &Value) -> String {
    let pick = |k: &str| {
        text(input, k)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let first = |keys: &[&str]| keys.iter().map(|k| pick(k)).find(|s| !s.is_empty());
    let lower = name.to_ascii_lowercase();
    first(match lower.as_str() {
        "bash" | "shell" => &["command"],
        "read" | "edit" | "write" | "patch" => &["filePath", "path"],
        "grep" | "glob" => &["pattern"],
        "webfetch" => &["url"],
        "websearch" => &["query"],
        _ => &[],
    })
    .unwrap_or_default()
}

/// OpenCode v2 plugin events (fact 0038: shapes recorded from OpenCode 2.0.31 on 2026-10-20). The plugin reports
/// `session.execution.*` (the precise turn edges), permissions, forms (OpenCode's questions), and tool calls.
/// `session.status` is not used because `busy` after a pending permission would clear the obligation. `session.idle`
/// is ignored too: it repeats the end that `session.execution.*` already carries, and a late idle would downgrade
/// Done to Idle and clear a block. The samples in testdata follow those captured events.
pub fn opencode(d: &Value) -> Option<Adapted> {
    let data = d.get("data").unwrap_or(&Value::Null);
    let block = |key: String, open: bool| EventKind::Block { key, open };
    let id = |prefix: &str| {
        let id = text(data, "id");
        if id.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}:{id}")
        }
    };
    let tool_name = || {
        let name = text(data, "toolName");
        if !name.is_empty() {
            name.to_string()
        } else {
            let name = text(data, "name");
            if name.is_empty() {
                "tool".to_string()
            } else {
                name.to_string()
            }
        }
    };
    let message = |text: &str| {
        (!text.trim().is_empty()).then(|| EventKind::Message {
            summary: text.to_string(),
        })
    };
    let kinds = match text(d, "type") {
        // The prompt was queued: its first line names the task. `execution.started` opens the turn.
        "session.inbox.enqueued" => {
            let item = data.get("item").unwrap_or(&Value::Null);
            if text(item, "type") != "user" {
                return None;
            }
            let prompt = text(item.get("payload").unwrap_or(&Value::Null), "text");
            let first = prompt.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                return None;
            }
            vec![EventKind::Task {
                title: first.to_string(),
            }]
        }
        "session.execution.started" => vec![EventKind::Turn {
            open: true,
            outcome: None,
        }],
        "session.execution.succeeded" => vec![turn_end(AgentState::Done)],
        "session.execution.failed" => {
            let mut k = vec![turn_end(AgentState::Error)];
            let error = data.get("error").unwrap_or(&Value::Null);
            k.extend(message(text(error, "message")));
            k
        }
        // Esc, a shutdown or a superseded prompt: the turn is over and nothing waits.
        "session.execution.interrupted" => vec![turn_end(AgentState::Idle)],
        "permission.asked" => {
            let mut k = vec![block(id("permission"), true)];
            let detail = text(data, "message");
            if detail.is_empty() {
                let action = text(data, "action");
                let resource = data
                    .get("resources")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .unwrap_or("");
                k.extend(message(&[action, resource].join(" ")));
            } else {
                k.extend(message(detail));
            }
            k
        }
        "permission.replied" => vec![block(
            format!("permission:{}", text(data, "requestID")),
            false,
        )],
        "form.created" => {
            let form = data.get("form").unwrap_or(&Value::Null);
            let form_id = text(form, "id");
            let key = if form_id.is_empty() {
                "form".to_string()
            } else {
                format!("form:{form_id}")
            };
            // A form's title can be the generic "Questions"; the first field carries the actual question.
            let detail = form
                .get("fields")
                .and_then(Value::as_array)
                .and_then(|fields| {
                    fields.iter().find_map(|f| {
                        let description = text(f, "description").trim();
                        if !description.is_empty() {
                            return Some(description.to_string());
                        }
                        let title = text(f, "title").trim();
                        (!title.is_empty()).then(|| title.to_string())
                    })
                })
                .unwrap_or_else(|| text(form, "title").to_string());
            let mut k = vec![block(key, true)];
            k.extend(message(&detail));
            k
        }
        "form.replied" | "form.cancelled" => vec![block(id("form"), false)],
        "session.tool.called" => {
            let name = tool_name();
            let summary = opencode_tool_summary(&name, data.get("input").unwrap_or(&Value::Null));
            vec![EventKind::Tool {
                name,
                summary,
                phase: ToolPhase::Started,
            }]
        }
        "session.tool.success" | "session.tool.failed" => vec![EventKind::Tool {
            name: tool_name(),
            summary: String::new(),
            phase: ToolPhase::Finished,
        }],
        _ => return None,
    };
    Some((Source::Hook, Confidence::High, kinds))
}

// ---------------------------------------------------------------------------------------------------
// Process scan (decision 0061): which agent runs in a workspace and whether it is busy
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PaneScan {
    pub workspace: String,
    pub command: String,
    /// Seconds since the window last produced output.
    pub quiet_for: u64,
    /// tmux's id of the pane (`%12`), empty when the listing did not give one.
    pub pane_id: String,
    /// The pane the workspace shows: the active pane of its active window.
    pub active: bool,
    /// The tab record this pane's window carries (`@termenbox_tab`), empty for an old window.
    pub tab: String,
}

/// `tmux list-panes -a -F '#{session_name}|#{pane_current_command}|#{window_activity}|#{pane_id}|#{window_active}|#{pane_active}|#{@termenbox_tab}'`
pub fn parse_panes(output: &str, now_s: u64) -> Vec<PaneScan> {
    output
        .lines()
        .filter_map(|l| {
            let mut f = l.split('|');
            let workspace = f.next()?.to_string();
            let command = f.next()?.to_string();
            let activity: u64 = f.next()?.trim().parse().ok()?;
            let pane_id = f.next().unwrap_or("").trim().to_string();
            let active =
                f.next().map(str::trim) == Some("1") && f.next().map(str::trim) == Some("1");
            let tab = f.next().unwrap_or("").trim().to_string();
            Some(PaneScan {
                workspace,
                command,
                quiet_for: now_s.saturating_sub(activity),
                pane_id,
                active,
                tab,
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

/// The command line that resumes one conversation of `agent` (decision 0152). This is the only place that knows the
/// flags; an agent without a resume command, or an id that is not a plain token, gives `None` and the tab stays a shell.
pub fn resume_command(agent: &str, session: &str) -> Option<String> {
    let plain = !session.is_empty()
        && session.len() <= 128
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !plain {
        return None;
    }
    match agent {
        "claude" => Some(format!("claude --resume {session}")),
        "codex" => Some(format!("codex resume {session}")),
        "opencode" => Some(format!("opencode --session {session}")),
        _ => None,
    }
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
    use super::{agent_name, resume_command};

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
    fn opencode_execution_events_open_and_close_the_turn_and_name_the_task() {
        let k = |f: &str| opencode(&sample(f)).unwrap().2;
        assert_eq!(
            k("opencode-inbox-enqueued.json"),
            vec![EventKind::Task {
                title: "Fix the failing test.".into()
            }]
        );
        assert_eq!(k("opencode-execution-started.json"), vec![open()]);
        assert_eq!(
            k("opencode-execution-succeeded.json"),
            vec![end(AgentState::Done)]
        );
        assert_eq!(
            k("opencode-execution-failed.json"),
            vec![
                end(AgentState::Error),
                EventKind::Message {
                    summary: "Provider returned 429: rate limited".into()
                }
            ]
        );
        assert_eq!(
            opencode(&json!({"type": "session.execution.interrupted", "data": {"reason": "user"}}))
                .unwrap()
                .2,
            vec![end(AgentState::Idle)]
        );
        // A late idle must not close (and clear) a turn that execution.succeeded already ended as Done.
        assert!(opencode(&json!({"type": "session.idle", "data": {"sessionID": "s"}})).is_none());
        assert!(opencode(
            &json!({"type": "session.inbox.enqueued", "data": {"item": {"type": "compaction"}}})
        )
        .is_none());
        assert!(
            opencode(&json!({"type": "session.status", "data": {"status": {"type": "busy"}}}))
                .is_none()
        );
    }

    #[test]
    fn opencode_permissions_forms_and_tools_map_to_blocks_and_tool_steps() {
        let k = |f: &str| opencode(&sample(f)).unwrap().2;
        assert_eq!(
            k("opencode-permission-asked.json"),
            vec![
                block("permission:per_01J0000000000000000000000", true),
                EventKind::Message {
                    summary: "bash cargo test -p termenbox-server".into()
                }
            ]
        );
        // A message, when OpenCode supplies one, is friendlier than the action and resource.
        assert_eq!(
            opencode(&json!({
                "type": "permission.asked",
                "data": {"id": "p1", "action": "bash", "resources": ["rm -rf /"], "message": "Allow this command?"}
            }))
            .unwrap()
            .2,
            vec![
                block("permission:p1", true),
                EventKind::Message {
                    summary: "Allow this command?".into()
                }
            ]
        );
        assert_eq!(
            k("opencode-permission-replied.json"),
            vec![block("permission:per_01J0000000000000000000000", false)]
        );
        assert_eq!(
            k("opencode-form-created.json"),
            vec![
                block("form:form_01J0000000000000000000000", true),
                EventKind::Message {
                    summary: "Pick a color".into()
                }
            ]
        );
        // A form without fields falls back to its title.
        assert_eq!(
            opencode(&json!({
                "type": "form.created",
                "data": {"form": {"id": "f1", "title": "Which color should the button be?", "fields": []}}
            }))
            .unwrap()
            .2,
            vec![
                block("form:f1", true),
                EventKind::Message {
                    summary: "Which color should the button be?".into()
                }
            ]
        );
        assert_eq!(
            k("opencode-form-replied.json"),
            vec![block("form:form_01J0000000000000000000000", false)]
        );
        assert_eq!(
            k("opencode-tool-called.json"),
            vec![tool(
                "bash",
                "cargo test -p termenbox-server",
                ToolPhase::Started
            )]
        );
        assert_eq!(
            k("opencode-tool-succeeded.json"),
            vec![tool("bash", "", ToolPhase::Finished)]
        );
        // Without the plugin's remembered name the adapter still reports a tool.
        assert_eq!(
            opencode(&json!({"type": "session.tool.called", "data": {"id": "c", "input": {"filePath": "src/lib.rs"}}}))
                .unwrap()
                .2,
            vec![tool("tool", "", ToolPhase::Started)]
        );
        assert!(opencode(&json!({"type": "something.else"})).is_none());
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

    #[test]
    fn a_listing_with_pane_ids_says_which_pane_the_workspace_shows() {
        let panes = parse_panes(
            "s|claude|1000|%3|0|1\ns|opencode|1000|%4|1|1\ns|opencode|1000|%5|1|0\n",
            1002,
        );
        assert_eq!(
            panes
                .iter()
                .map(|p| (p.pane_id.as_str(), p.active))
                .collect::<Vec<_>>(),
            [("%3", false), ("%4", true), ("%5", false)],
            "only the active pane of the active window is shown"
        );
    }

    #[test]
    fn the_agents_own_workspace_id_is_read_from_each_adapters_payload() {
        assert_eq!(
            agent_session_id("claude", &json!({"session_id": "c1"})).as_deref(),
            Some("c1")
        );
        assert_eq!(
            agent_session_id("codex", &json!({"session_id": "x9"})).as_deref(),
            Some("x9")
        );
        assert_eq!(
            agent_session_id("opencode", &json!({"sessionID": "ses_1", "data": {}})).as_deref(),
            Some("ses_1")
        );
        assert_eq!(
            agent_session_id("opencode", &json!({"data": {"sessionID": "ses_2"}})).as_deref(),
            Some("ses_2"),
            "an older plugin names it only inside the data, and not on every event"
        );
        assert_eq!(agent_session_id("opencode", &json!({"data": {}})), None);
        assert_eq!(agent_session_id("claude", &json!({"session_id": ""})), None);
        assert_eq!(agent_session_id("other", &json!({"session_id": "a"})), None);
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

    #[test]
    fn a_resume_command_exists_for_three_agents_and_only_for_a_plain_id() {
        assert_eq!(
            resume_command("claude", "6f1c-aa_01").as_deref(),
            Some("claude --resume 6f1c-aa_01")
        );
        assert_eq!(
            resume_command("codex", "019a").as_deref(),
            Some("codex resume 019a")
        );
        assert_eq!(
            resume_command("opencode", "ses_9").as_deref(),
            Some("opencode --session ses_9")
        );
        assert_eq!(resume_command("gemini", "x"), None);
        for bad in ["", "a b", "a;b", "$(x)", "-x y", &"a".repeat(129), "é"] {
            assert_eq!(resume_command("claude", bad), None, "{bad:?}");
        }
    }

    /// The payloads of one recorded A.2 run, in arrival order.
    fn run(name: &str) -> Vec<Value> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/events/a2")
            .join(format!("claude-{name}.jsonl"));
        std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{p:?}: {e}"))
            .lines()
            .map(|l| serde_json::from_str(l.split_once('\t').unwrap().1).unwrap())
            .collect()
    }

    fn events(name: &str) -> Vec<Event> {
        run(name).iter().flat_map(claude_events).collect()
    }

    fn ask(key: &str, kind: AskKind) -> Event {
        Event::AskOpen {
            key: key.into(),
            kind,
        }
    }

    fn close(key: &str, outcome: AskOutcome) -> Event {
        Event::AskClose {
            key: key.into(),
            outcome,
        }
    }

    #[test]
    fn an_ask_user_question_yields_its_question_and_options() {
        let ev = claude_events(&sample("claude-pretooluse-askuserquestion.json"));
        assert_eq!(
            ev,
            vec![
                Event::ToolStart {
                    name: "AskUserQuestion".into()
                },
                ask("AskUserQuestion", AskKind::Question),
                Event::AskContent {
                    key: "AskUserQuestion".into(),
                    text: "Which colour do you like?".into(),
                    options: vec!["Red".into(), "Blue".into()],
                },
            ]
        );
        assert_eq!(
            claude_events(&sample("claude-posttooluse-askuserquestion.json")),
            vec![
                close("AskUserQuestion", AskOutcome::Answered),
                Event::ToolEnd {
                    name: "AskUserQuestion".into()
                }
            ]
        );
    }

    #[test]
    fn a_plan_and_a_permission_carry_their_text_and_untrusted_text_is_cleaned() {
        let plan = json!({"hook_event_name": "PreToolUse", "tool_name": "ExitPlanMode",
            "tool_input": {"plan": "1. do it\x1b[31m\n2. check"}});
        let ev = claude_events(&plan);
        assert_eq!(ev[1], ask("ExitPlanMode", AskKind::Plan));
        let Event::AskContent { text, .. } = &ev[2] else {
            panic!("{ev:?}")
        };
        assert!(
            text.starts_with("1. do it") && text.contains("2. check") && !text.contains('\x1b')
        );

        let perm = claude_events(&sample("claude-permissionrequest.json"));
        assert_eq!(perm.len(), 2);
        assert!(matches!(
            &perm[0],
            Event::AskOpen {
                kind: AskKind::Permission,
                ..
            }
        ));
        let long = json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "x".repeat(5_000)}});
        let Event::AskContent { text, .. } = &claude_events(&long)[1] else {
            panic!()
        };
        assert!(text.chars().count() <= ASK_TEXT_CHARS);
    }

    #[test]
    fn the_plain_recorded_turn_maps_to_typed_events() {
        let start = claude_events(&sample("claude-userpromptsubmit.json"));
        assert!(matches!(&start[..], [Event::TurnStart { title: Some(_) }]));
        assert_eq!(
            claude_events(&sample("claude-stop.json")),
            vec![Event::TurnEnd {
                outcome: Outcome::Done,
                bg: 0
            }]
        );
        assert_eq!(
            claude_events(&sample("claude-pretooluse.json"))[0],
            Event::ToolStart {
                name: sample("claude-pretooluse.json")["tool_name"]
                    .as_str()
                    .unwrap()
                    .into()
            }
        );
    }

    #[test]
    fn a_stop_counts_background_work_and_scheduled_wakeups() {
        let stops = |name| -> Vec<u32> {
            events(name)
                .into_iter()
                .filter_map(|e| match e {
                    Event::TurnEnd { bg, .. } => Some(bg),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(stops("bg_bash"), [1, 0]);
        assert_eq!(stops("wakeup"), [1, 0]);
    }

    #[test]
    fn subagent_and_compaction_hooks_map_and_the_noise_after_each_stop_does_not() {
        let ev = events("subagent");
        assert_eq!(ev.iter().filter(|e| **e == Event::SubagentStart).count(), 1);
        assert_eq!(ev.iter().filter(|e| **e == Event::SubagentEnd).count(), 1);
        let ev = events("compact");
        assert!(ev.contains(&Event::CompactStart) && ev.contains(&Event::CompactEnd));
        assert!(ev.contains(&Event::SessionStart(SessionSource::Compact)));
        assert!(!ev.contains(&Event::SubagentEnd));
        assert!(events("resume").contains(&Event::SessionStart(SessionSource::Resume)));
    }

    #[test]
    fn an_elicitation_opens_with_its_message_and_closes_by_the_action() {
        let ev = events("elicitation");
        assert!(ev.contains(&ask("elicit:probe", AskKind::Elicitation)));
        assert!(ev.contains(&Event::AskContent {
            key: "elicit:probe".into(),
            text: "What is your name?".into(),
            options: vec![]
        }));
        assert!(ev.contains(&close("elicit:probe", AskOutcome::Declined)));
        // The keyless notification text is only content, never an ask of its own.
        assert!(!ev
            .iter()
            .any(|e| matches!(e, Event::AskOpen { key, .. } if key.is_empty())));
    }

    #[test]
    fn a_notification_only_fills_text_and_an_idle_prompt_is_ignored() {
        let n = claude_events(&sample("claude-notification.json"));
        assert!(
            matches!(&n[..], [Event::AskContent { key, .. }] if key == ANY_ASK) || n.is_empty()
        );
        assert!(events("idle_prompt")
            .iter()
            .all(|e| !matches!(e, Event::AskContent { .. })));
    }

    #[test]
    fn a_denied_permission_closes_as_declined() {
        let d = json!({"hook_event_name": "PermissionDenied", "tool_name": "Bash"});
        assert_eq!(claude_events(&d), vec![close("Bash", AskOutcome::Declined)]);
    }

    #[test]
    fn a_stop_failure_is_a_failed_turn_with_its_class() {
        let fail = |name: &str| {
            events(name).into_iter().find_map(|e| match e {
                Event::TurnEnd { outcome, .. } => Some(outcome),
                _ => None,
            })
        };
        assert_eq!(
            fail("err_429"),
            Some(Outcome::Failed(FailureCause::RateLimit))
        );
        assert_eq!(
            fail("err_500"),
            Some(Outcome::Failed(FailureCause::ApiError))
        );
        assert_eq!(failure_cause("authentication_failed"), FailureCause::Auth);
        assert_eq!(failure_cause("something new"), FailureCause::Unknown);
    }

    #[test]
    fn every_recorded_hook_and_every_unknown_payload_maps_without_panicking() {
        for name in [
            "bg_bash",
            "compact",
            "denied",
            "elicitation",
            "err_401",
            "idle_prompt",
            "parallel",
            "resume",
            "subagent",
            "wakeup",
        ] {
            assert!(!events(name).is_empty(), "{name}");
        }
        for junk in [
            json!(null),
            json!({}),
            json!({"hook_event_name": 5}),
            json!({"hook_event_name": "Nope"}),
            json!({"hook_event_name": "PreToolUse", "tool_input": 3}),
        ] {
            let _ = claude_events(&junk);
        }
    }
}
