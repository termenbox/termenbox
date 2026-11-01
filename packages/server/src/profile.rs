//! The session-view profile engine (decision 0103): turns a screen into a cleaned tail, a task line, a state and,
//! when the agent is asking, a parsed prompt whose options carry the only keys a tap may send.
//!
//! Three tiers: T1 (structured, from hooks) lives in `events.rs`; here are T2 (a profile file for one agent) and T3
//! (the generic rules every agent gets). Adding an agent is a TOML file under `packages/server/profiles/`, listed
//! in `BUILTIN` below, plus recorded screens and tests (procedure `add-an-agent-profile`).
//!
//! # Profile schema
//!
//! ```toml
//! agent = "claude"              # the name `adapters::agent_name` gives the foreground command
//!
//! [prompt]
//! question = ["do you want to"] # lines containing any of these (case-insensitive) are the question; the last one wins
//! numbered = true               # the options are the "1. Yes" style lines under it; the key for an option is its digit
//! numbered_labels = ["yes", "no"] # optional: the list counts only if options start with each of these words
//! yes_no   = ["(y/n)"]          # or: an inline marker; the options are Yes (key `y`+Enter) and No (`n`+Enter)
//!
//! [markers]                     # case-insensitive substrings of the last lines of the screen
//! working = ["esc to interrupt"]
//! idle    = ["? for shortcuts"]
//! blocked = ["esc to cancel"]   # a dialog that waits for the human is on screen (any of these on the last lines)
//! input_rule = "─"              # optional: the character of the rule that closes the input box. What is under the
//!                               # last such line is the footer and the user's own status line, whose text is theirs
//!                               # ("120k tokens · $1", "Opus (thinking)"): `working` is never read there
//!
//! [task]
//! prefix = "> "                 # a line starting with this holds the current task (the latest one wins)
//! ```
//!
//! Keys are never written in a profile. The engine derives them from what the screen showed: a digit for a numbered
//! option, `y`/`n` plus Enter for an inline question. A profile that tried to name keys is rejected (unknown field),
//! and `Prompt::keys` can only return one of those derived strings.
//!
//! Everything read from the screen is untrusted: control sequences are removed, secrets and addresses scrubbed (as
//! for hook events), lines and options capped. Parsed text is never logged.
use std::sync::OnceLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::events::sanitize_text;

pub const TAIL_LINES: usize = 30;
pub const TAIL_CHARS: usize = 200;
pub const MAX_OPTIONS: usize = 9;
const OPTION_CHARS: usize = 40;
const QUESTION_CHARS: usize = 200;
/// Where on the screen a question or marker is looked for: its last non-empty lines.
const SCAN_LINES: usize = 14;
/// How many of the last non-empty lines count as the footer an idle agent draws under its input box.
const FOOTER_LINES: usize = 3;
const MAX_PATTERNS: usize = 32;
const MAX_PATTERN_CHARS: usize = 80;

/// How much the view knows about the agent (decision 0103). `T1` is the hook-based transcript timeline (U09); the screen profile still reads prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    T1,
    T2,
    T3,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::T1 => "t1",
            Tier::T2 => "t2",
            Tier::T3 => "t3",
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
struct PromptRules {
    #[serde(default)]
    question: Vec<String>,
    #[serde(default)]
    numbered: bool,
    /// A list only counts when, for each word here, some option starts with it (case-insensitive): T3 uses
    /// `["yes", "no"]` so an ordinary numbered list in the output is not taken for a question.
    #[serde(default)]
    numbered_labels: Vec<String>,
    #[serde(default)]
    yes_no: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
struct Markers {
    #[serde(default)]
    working: Vec<String>,
    #[serde(default)]
    idle: Vec<String>,
    /// The footer of a dialog that waits for the human: a permission, a question, a plan to approve.
    #[serde(default)]
    blocked: Vec<String>,
    /// See the schema at the top: where the agent's own activity line ends and the user's status line begins.
    #[serde(default)]
    input_rule: Option<char>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
struct TaskRules {
    prefix: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub agent: String,
    #[serde(default)]
    prompt: PromptRules,
    #[serde(default)]
    markers: Markers,
    #[serde(default)]
    task: TaskRules,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileError {
    Syntax(String),
    Invalid(&'static str),
}

impl Profile {
    pub fn parse(text: &str) -> Result<Profile, ProfileError> {
        let p: Profile =
            toml::from_str(text).map_err(|e| ProfileError::Syntax(e.message().to_string()))?;
        let name_ok = !p.agent.is_empty()
            && p.agent
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !name_ok {
            return Err(ProfileError::Invalid("agent must be a lowercase name"));
        }
        let lists = [
            &p.prompt.question,
            &p.prompt.yes_no,
            &p.prompt.numbered_labels,
            &p.markers.working,
            &p.markers.idle,
            &p.markers.blocked,
        ];
        for l in lists {
            if l.len() > MAX_PATTERNS
                || l.iter()
                    .any(|s| s.is_empty() || s.chars().count() > MAX_PATTERN_CHARS)
            {
                return Err(ProfileError::Invalid(
                    "a pattern list is too long or has an empty or oversized pattern",
                ));
            }
        }
        if !p.prompt.question.is_empty() && !p.prompt.numbered && p.prompt.yes_no.is_empty() {
            return Err(ProfileError::Invalid(
                "prompt.question needs prompt.numbered or prompt.yes_no",
            ));
        }
        Ok(p)
    }
}

/// One option as parsed: the label shown, and the keys a tap sends. The keys never leave the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptOption {
    pub label: String,
    keys: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub question: String,
    /// `choice` or `yes_no`.
    pub kind: &'static str,
    pub options: Vec<PromptOption>,
}

impl Prompt {
    /// Names this prompt as drawn: the same screen gives the same id, a different question or option list does not.
    pub fn id(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.question.as_bytes());
        for o in &self.options {
            h.update([0]);
            h.update(o.label.as_bytes());
        }
        format!(
            "v-{}",
            h.finalize()[..6]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }

    /// The keys for option `index`, or `None` when there is no such option. The only way to get keys out.
    pub fn keys(&self, index: usize) -> Option<&str> {
        self.options.get(index).map(|o| o.keys.as_str())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScreenView {
    pub tail: Vec<String>,
    pub task: Option<String>,
    pub working: bool,
    pub idle: bool,
    /// A dialog that waits for the human is on screen (the profile's `blocked` markers, or a parsed prompt).
    pub blocked: bool,
    pub prompt: Option<Prompt>,
}

/// Removes escape sequences (CSI, OSC and two-byte), then control characters, and scrubs secrets.
fn clean_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    // Direction overrides and zero-width characters can reorder or hide text in a label.
    let out: String = out
        .chars()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect();
    sanitize_text(&out, TAIL_CHARS)
}

fn lower_contains(line: &str, needles: &[String]) -> bool {
    let l = line.to_lowercase();
    needles.iter().any(|n| l.contains(&n.to_lowercase()))
}

/// `1. Yes` / `❯ 2) No`: the digit and the label, after any bullet or cursor.
fn numbered_option(line: &str) -> Option<(char, String)> {
    let l = line.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    let mut c = l.chars();
    let d = c.next().filter(|d| ('1'..='9').contains(d))?;
    if !matches!(c.next(), Some('.' | ')')) {
        return None;
    }
    let label: String = l[2..].trim().chars().take(OPTION_CHARS).collect();
    (!label.is_empty()).then_some((d, label))
}

impl Profile {
    /// Whether this profile can tell a working screen from an idle one, which the reconciliation and the scan rely on.
    pub fn has_state_markers(&self) -> bool {
        !self.markers.working.is_empty() && !self.markers.idle.is_empty()
    }

    /// Reads a screen as `capture-pane -p` gives it.
    pub fn read(&self, screen: &str) -> ScreenView {
        let lines: Vec<String> = screen.lines().map(clean_line).collect();
        let mut tail: Vec<String> = lines.clone();
        while tail.last().is_some_and(|l| l.is_empty()) {
            tail.pop();
        }
        let non_empty: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
        let scan = &non_empty[non_empty.len().saturating_sub(SCAN_LINES)..];
        let skip = tail.len().saturating_sub(TAIL_LINES);
        let tail = tail.split_off(skip);

        let has = |needles: &[String]| scan.iter().any(|l| lower_contains(l, needles));
        let task = self.task.prefix.as_ref().and_then(|p| {
            non_empty
                .iter()
                .rev()
                .find_map(|l| l.strip_prefix(p.trim()).map(str::trim))
                .filter(|t| !t.is_empty())
                .map(|t| t.chars().take(crate::events::TASK_CHARS).collect())
        });
        // The spinner is drawn above the input box. Under it sits a status line the user may have configured, and
        // its words (token counts, a model's "(thinking)") must not read as the agent working.
        let above_box = match self.markers.input_rule {
            Some(rule) => {
                let is_rule = |l: &&String| {
                    let t = l.trim();
                    t.chars().count() >= 10 && t.chars().all(|c| c == rule)
                };
                non_empty
                    .iter()
                    .rposition(is_rule)
                    .map(|cut| &non_empty[..cut])
            }
            None => None,
        };
        let working = match above_box {
            Some(lines) => lines[lines.len().saturating_sub(SCAN_LINES)..]
                .iter()
                .any(|l| lower_contains(l, &self.markers.working)),
            None => has(&self.markers.working),
        };
        // The idle footer (the last lines) is where an agent at its input prompt shows its hints. A question higher up
        // is then the agent's own transcript (a `(y/n)` it printed, a numbered list in a reply), not a dialog that
        // waits: a dialog replaces the footer. Only the dialog markers still say blocked then.
        let footer = &non_empty[non_empty.len().saturating_sub(FOOTER_LINES)..];
        let idle_footer = !working && footer.iter().any(|l| lower_contains(l, &self.markers.idle));
        let prompt = self.prompt_in(scan).filter(|_| !idle_footer);
        ScreenView {
            working,
            blocked: has(&self.markers.blocked) || prompt.is_some(),
            // A screen is not idle while it is working: some footers (opencode's) are on both.
            idle: !working && has(&self.markers.idle),
            prompt,
            tail,
            task,
        }
    }

    fn prompt_in(&self, scan: &[&String]) -> Option<Prompt> {
        let r = &self.prompt;
        let question_at = scan.iter().rposition(|l| lower_contains(l, &r.question));
        if r.numbered {
            // The list under the question line (or, with no question patterns, the last list on the screen).
            let from = question_at.map(|i| i + 1).unwrap_or(0);
            let mut options: Vec<PromptOption> = Vec::new();
            for l in &scan[from..] {
                if let Some((d, label)) = numbered_option(l) {
                    // A list restarts at 1; keep the last one.
                    if d == '1' {
                        options.clear();
                    }
                    if options.len() < MAX_OPTIONS {
                        options.push(PromptOption {
                            label,
                            keys: d.to_string(),
                        });
                    }
                }
            }
            let labelled = r.numbered_labels.iter().all(|w| {
                options
                    .iter()
                    .any(|o| o.label.to_lowercase().starts_with(&w.to_lowercase()))
            });
            if options.len() >= 2 && labelled && (question_at.is_some() || r.question.is_empty()) {
                let question = question_at
                    .map(|i| scan[i].as_str())
                    .unwrap_or("Choose an option");
                return Some(Prompt {
                    question: question.chars().take(QUESTION_CHARS).collect(),
                    kind: "choice",
                    options,
                });
            }
        }
        if !r.yes_no.is_empty() {
            if let Some(i) = scan.iter().rposition(|l| lower_contains(l, &r.yes_no)) {
                return Some(Prompt {
                    question: scan[i].chars().take(QUESTION_CHARS).collect(),
                    kind: "yes_no",
                    options: vec![
                        PromptOption {
                            label: "Yes".into(),
                            keys: "y\r".into(),
                        },
                        PromptOption {
                            label: "No".into(),
                            keys: "n\r".into(),
                        },
                    ],
                });
            }
        }
        None
    }
}

/// The T3 rules every agent gets: a numbered list with a "Yes" and a "No", or an inline (y/n).
const GENERIC: &str = r#"
agent = "generic"
[prompt]
question = ["?"]
numbered = true
numbered_labels = ["yes", "no"]
yes_no = ["(y/n)", "[y/n]", "(yes/no)"]
"#;

const BUILTIN: &[&str] = &[
    include_str!("../profiles/claude.toml"),
    include_str!("../profiles/opencode.toml"),
    include_str!("../profiles/kilo.toml"),
];

pub struct Profiles {
    generic: Profile,
    by_agent: Vec<Profile>,
}

impl Profiles {
    pub fn builtin() -> &'static Profiles {
        static P: OnceLock<Profiles> = OnceLock::new();
        P.get_or_init(|| Profiles {
            generic: Profile::parse(GENERIC).expect("the generic profile is valid"),
            by_agent: BUILTIN
                .iter()
                .map(|t| Profile::parse(t).expect("a built-in profile is valid"))
                .collect(),
        })
    }

    /// The profile for an agent (T2) or the generic one (T3).
    pub fn for_agent(&self, agent: Option<&str>) -> (&Profile, Tier) {
        match agent.and_then(|a| self.by_agent.iter().find(|p| p.agent == a)) {
            Some(p) => (p, Tier::T2),
            None => (&self.generic, Tier::T3),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(name: &str) -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/screens")
            .join(format!("{name}.txt"));
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"))
    }

    fn profiles() -> &'static Profiles {
        Profiles::builtin()
    }

    #[test]
    fn a_recorded_claude_permission_screen_is_parsed_by_its_profile() {
        let (p, tier) = profiles().for_agent(Some("claude"));
        assert_eq!(tier, Tier::T2);
        let v = p.read(&screen("claude-permission"));
        let prompt = v.prompt.expect("the permission prompt");
        assert_eq!(prompt.question, "Do you want to proceed?");
        assert_eq!(prompt.kind, "choice");
        assert_eq!(prompt.options.len(), 4);
        assert_eq!(prompt.options[0].label, "Yes");
        assert_eq!(prompt.options[3].label, "No");
        assert_eq!(
            (0..4).map(|i| prompt.keys(i).unwrap()).collect::<Vec<_>>(),
            ["1", "2", "3", "4"]
        );
        assert_eq!(prompt.keys(4), None);
        assert!(v.tail.len() <= TAIL_LINES);
    }

    fn view(agent: &str, name: &str) -> ScreenView {
        let (p, tier) = profiles().for_agent(Some(agent));
        assert_eq!(tier, Tier::T2, "{agent}");
        p.read(&screen(name))
    }

    #[test]
    fn claude_states_over_the_recorded_screens() {
        // (screen, working, idle)
        for (name, working, idle) in [
            ("claude-idle", false, true),
            ("claude-working", true, false),
            ("claude-working-tool", true, false),
            ("claude-finished", false, true),
            ("claude-permission", false, false),
        ] {
            let v = view("claude", name);
            assert_eq!((v.working, v.idle), (working, idle), "{name}");
            assert_eq!(v.prompt.is_some(), name == "claude-permission", "{name}");
        }
    }

    #[test]
    fn claude_free_text_questions_are_left_to_the_terminal() {
        // A digit on "Type something." only moves the cursor, so the profile offers no tap for these screens.
        for name in ["claude-question", "claude-question-freetext"] {
            let v = view("claude", name);
            assert_eq!(v.prompt, None, "{name}");
            assert!(
                v.tail
                    .iter()
                    .any(|l| l.contains("Which colour do you like?")),
                "{name}"
            );
        }
    }

    #[test]
    fn claude_trust_screen_and_unrelated_output_match_nothing() {
        assert_eq!(view("claude", "claude-trust").prompt, None);
        let v = profiles()
            .for_agent(Some("claude"))
            .0
            .read("Which one?\n1. a\n2. b\n");
        assert_eq!(v.prompt, None);
    }

    #[test]
    fn opencode_states_over_the_recorded_screens() {
        // `opencode-permission` was recorded while the agent was working (its footer shows "esc interrupt"); no
        // permission prompt has been captured.
        for (name, working, idle) in [
            ("opencode-idle", false, true),
            ("opencode-permission", true, false),
            ("opencode-after", false, true),
            ("opencode-working", false, true),
        ] {
            let v = view("opencode", name);
            assert_eq!((v.working, v.idle), (working, idle), "{name}");
            assert_eq!(v.prompt, None, "{name}");
        }
        assert!(!view("opencode", "kilo-error").working);
        assert_eq!(view("opencode", "claude-permission").prompt, None);
    }

    #[test]
    fn kilo_reads_the_recorded_screens_without_inventing_a_state() {
        for name in ["kilo-working", "kilo-error", "kilo-idle"] {
            let v = view("kilo", name);
            assert_eq!((v.working, v.prompt), (false, None), "{name}");
        }
        let err = view("kilo", "kilo-error");
        assert!(err.idle);
        assert!(err.tail.iter().any(|l| l.contains("Nemotron 3 Ultra")));
        assert!(
            view("kilo", "kilo-idle").tail.is_empty(),
            "the blank capture has no tail"
        );
    }

    #[test]
    fn agents_without_a_profile_stay_t3() {
        for a in [
            "codex",
            "copilot",
            "gemini",
            "aider",
            "cursor-agent",
            "amp",
            "goose",
            "qwen",
        ] {
            assert_eq!(profiles().for_agent(Some(a)).1, Tier::T3, "{a}");
        }
        for a in ["claude", "opencode", "kilo"] {
            assert_eq!(profiles().for_agent(Some(a)).1, Tier::T2, "{a}");
        }
    }

    #[test]
    fn an_agent_without_a_profile_gets_the_generic_rules() {
        let (g, tier) = profiles().for_agent(Some("codex"));
        assert_eq!((tier, g.agent.as_str() == "generic"), (Tier::T3, true));
        assert_eq!(profiles().for_agent(None).1, Tier::T3);
        let v = g.read(&screen("shell-yn"));
        let prompt = v.prompt.expect("an inline (y/n)");
        assert_eq!(prompt.kind, "yes_no");
        assert_eq!(prompt.question, "Deploy to production? (y/n)");
        assert_eq!((prompt.keys(0), prompt.keys(1)), (Some("y\r"), Some("n\r")));
        let v = g.read(&screen("claude-permission"));
        assert_eq!(
            v.prompt.unwrap().options.len(),
            4,
            "a Yes/No list is a prompt for any agent"
        );
    }

    #[test]
    fn a_screen_where_nothing_should_match_yields_no_prompt() {
        let plain = "Which of these?\n1. apples\n2. pears\n$ ls\nfile\n";
        for (agent, text) in [
            (Some("claude"), screen("claude-idle")),
            (Some("claude"), plain.to_string()),
            (None, plain.to_string()),
            (None, screen("claude-idle")),
            (None, String::new()),
            (None, "no questions here\n".to_string()),
        ] {
            let (p, _) = profiles().for_agent(agent);
            assert_eq!(p.read(&text).prompt, None, "{agent:?}: {text}");
        }
        // every recorded screen that is not a question stays quiet under the generic rules
        for name in [
            "claude-idle",
            "kilo-idle",
            "kilo-working",
            "opencode-idle",
            "opencode-working",
            "qwen-working",
            "aider-working",
        ] {
            assert_eq!(
                profiles().for_agent(None).0.read(&screen(name)).prompt,
                None,
                "{name}"
            );
        }
    }

    #[test]
    fn every_key_comes_from_a_listed_option_on_every_recorded_screen() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/screens");
        let mut seen = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            // `lag/` holds the runs of `scripts/status-lag.mjs`, not profile fixtures.
            if path.is_dir() {
                continue;
            }
            let text = std::fs::read_to_string(path).unwrap();
            for agent in [None, Some("claude")] {
                let (p, _) = profiles().for_agent(agent);
                let Some(prompt) = p.read(&text).prompt else {
                    continue;
                };
                seen += 1;
                assert!(prompt.options.len() >= 2 && prompt.options.len() <= MAX_OPTIONS);
                for (i, o) in prompt.options.iter().enumerate() {
                    let k = prompt.keys(i).unwrap();
                    let numbered = k.len() == 1 && k.chars().all(|c| ('1'..='9').contains(&c));
                    assert!(numbered || k == "y\r" || k == "n\r", "{k:?} for {o:?}");
                }
                assert_eq!(prompt.keys(prompt.options.len()), None);
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn a_profile_cannot_name_keys_or_use_unknown_settings() {
        for bad in [
            "agent = 'x'\n[prompt]\nkeys = ['rm -rf ~\\r']\n",
            "agent = 'x'\n[keys]\nyes = 'y'\n",
            "agent = 'x'\nsend = 'whoami'\n",
            "agent = 'X Y'\n",
            "agent = ''\n",
            "agent = 'x'\n[prompt]\nquestion = ['q']\n",
            "agent = 'x'\n[markers]\nworking = ['']\n",
            "not toml at all [",
        ] {
            assert!(Profile::parse(bad).is_err(), "{bad}");
        }
        assert!(Profile::parse("agent = 'x'\n").is_ok());
    }

    #[test]
    fn untrusted_text_is_stripped_scrubbed_and_capped() {
        let evil = "\u{1b}[31mDeploy\u{1b}[0m \u{1b}]0;title\u{7}now ghp_abcdefghijklmnopqrstuvwxyz0123456789 \u{202e}evil\u{200b}\n";
        let mut text = String::new();
        for i in 0..60 {
            text.push_str(&format!("line {i} {}\n", "x".repeat(300)));
        }
        text.push_str(evil);
        text.push_str("Run it? (y/n)\n");
        let (g, _) = profiles().for_agent(None);
        let v = g.read(&text);
        assert!(v.tail.len() <= TAIL_LINES);
        assert!(v.tail.iter().all(|l| l.chars().count() <= TAIL_CHARS));
        let joined = v.tail.join("\n");
        for forbidden in ["\u{1b}", "ghp_", "\u{202e}", "\u{200b}", "\u{7}", "title"] {
            assert!(!joined.contains(forbidden), "{forbidden:?} in {joined}");
        }
        assert!(joined.contains("Deploy now [token] evil"), "{joined}");
        assert_eq!(v.prompt.unwrap().question, "Run it? (y/n)");
    }

    #[test]
    fn markers_and_the_task_line_come_from_the_profile() {
        let p = Profile::parse(
            "agent = 'x'\n[markers]\nworking = ['Esc to interrupt']\nidle = ['ready']\n[task]\nprefix = '> '\n",
        )
        .unwrap();
        let v = p.read("> fix the build\n> now add tests\nthinking... esc to interrupt\n");
        assert_eq!(v.task.as_deref(), Some("now add tests"));
        assert!((v.working, v.idle) == (true, false));
        let v = p.read("Ready for input\n");
        assert!((v.working, v.idle, v.task) == (false, true, None));
    }

    #[test]
    fn the_prompt_id_changes_with_the_question_and_the_options() {
        let (g, _) = profiles().for_agent(None);
        let a = g.read("Proceed? (y/n)").prompt.unwrap();
        assert_eq!(a.id(), g.read("Proceed? (y/n)").prompt.unwrap().id());
        assert_ne!(a.id(), g.read("Delete it? (y/n)").prompt.unwrap().id());
    }

    #[test]
    fn claude_dialogs_are_blocked_screens_and_working_or_idle_ones_are_not() {
        for name in [
            "claude-permission",
            "claude-question",
            "claude-question-freetext",
        ] {
            let v = view("claude", name);
            assert!(v.blocked && !v.working, "{name}");
        }
        for name in [
            "claude-idle",
            "claude-finished",
            "claude-working",
            "claude-working-tool",
        ] {
            assert!(!view("claude", name).blocked, "{name}");
        }
        let (p, _) = Profiles::builtin().for_agent(Some("claude"));
        assert!(p.has_state_markers());
    }

    #[test]
    fn a_redraw_is_not_inference_a_working_screen_is() {
        // The scan used to call a pane working when it had just printed anything, such as a redraw on attach.
        assert!(!view("claude", "claude-idle").working && view("claude", "claude-idle").idle);
        assert!(view("claude", "claude-working").working);
    }

    /// An idle agent's last lines are its own transcript: a question it printed earlier is not a dialog that waits,
    /// and an agent that is sitting at its input must not raise "needs you" for it when a client attaches.
    #[test]
    fn a_question_in_the_transcript_above_an_idle_footer_is_not_a_dialog() {
        let (p, _) = profiles().for_agent(Some("claude"));
        let idle = screen("claude-idle");
        let with_question = idle.replacen(
            "\n\n\n",
            "\n  Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n\n\n",
            1,
        );
        assert_ne!(with_question, idle);
        let v = p.read(&with_question);
        assert!(v.idle && !v.blocked && v.prompt.is_none());
        // The same question without the footer (a dialog replaces it) is still one.
        let dialog = p.read("  Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n");
        assert!(dialog.blocked && dialog.prompt.is_some());
    }
}
