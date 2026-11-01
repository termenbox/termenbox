//! Mission contracts (decision 0172, plan step 6.2): a scope, limits and a safe default granted when work starts.
//!
//! The server enforces a contract as generated rule entries. A contract answers an ask only when the ask is inside
//! it; an ask outside passes to the next layer, and a crossed limit is a breach that becomes an incident (step 5.1).
//! The limits of `auto-answered-permissions-are-scoped-visible-and-audited` hold for every answer: commands match on
//! parsed arguments (`command_match`), a delete is never answered (it needs a verified checkpoint, step 8.1), a
//! fetch only to a named host, a path only inside the workspace, and a contract never widens an agent's sandbox.
#![allow(dead_code)] // not called until the permission path (R85 4.6) and the launch sheet (6.3) use it
use crate::command_match;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Edit,
    Execute,
    Delete,
    Fetch,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeDefault {
    Park,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub id: String,
    pub workspaces: Vec<String>,
    /// Globs relative to the workspace directory: `*` within one segment, `**` across segments.
    pub paths: Vec<String>,
    /// Command patterns (`command_match`); an `Execute` ask outside them passes down.
    pub commands: Vec<String>,
    pub tool_kinds: Vec<ToolKind>,
    pub hosts: Vec<String>,
    pub spend_cents: Option<u64>,
    pub wall_secs: Option<u64>,
    pub max_files: Option<u32>,
    pub done_criteria: Vec<String>,
    pub safe_default: SafeDefault,
}

/// One ask as the permission path sees it, reduced to what matching needs.
#[derive(Debug, Clone, Default)]
pub struct Ask {
    pub workspace: String,
    pub kind: Option<ToolKind>,
    pub command: Option<String>,
    pub path: Option<String>,
    pub host: Option<String>,
}

/// What the run has used so far.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub spend_cents: u64,
    pub wall_secs: u64,
    pub files_changed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Answer with the agent's allow option and mark the entry with `source` (`contract:<id>`).
    Allow {
        source: String,
    },
    /// Not covered: the next layer decides.
    Pass,
    Breach(Breach),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    Spend,
    WallTime,
    Files,
}

impl Breach {
    /// The cause key step 5.1 dedupes incidents by.
    pub fn cause(&self, contract: &str) -> String {
        let what = match self {
            Breach::Spend => "spend",
            Breach::WallTime => "wall-time",
            Breach::Files => "files",
        };
        format!("contract-breach:{contract}:{what}")
    }
}

/// A rule entry generated from a contract, for the rule engine of R85.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleEntry {
    pub source: String,
    pub workspace: String,
    pub kind: ToolKind,
    pub pattern: String,
}

/// The mode a contract asks of an agent. Only a mode the agent itself offers is ever applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Default,
    AcceptEdits,
}

impl Mode {
    fn id(self) -> &'static str {
        match self {
            Mode::Default => "default",
            Mode::AcceptEdits => "acceptEdits",
        }
    }
}

impl Contract {
    pub fn source(&self) -> String {
        format!("contract:{}", self.id)
    }

    pub fn check(&self, ask: &Ask, used: Usage) -> Verdict {
        if !self.workspaces.contains(&ask.workspace) {
            return Verdict::Pass;
        }
        if let Some(b) = self.breach(used) {
            return Verdict::Breach(b);
        }
        if self.covers(ask) {
            Verdict::Allow {
                source: self.source(),
            }
        } else {
            Verdict::Pass
        }
    }

    fn breach(&self, used: Usage) -> Option<Breach> {
        if self.spend_cents.is_some_and(|m| used.spend_cents > m) {
            Some(Breach::Spend)
        } else if self.wall_secs.is_some_and(|m| used.wall_secs > m) {
            Some(Breach::WallTime)
        } else if self.max_files.is_some_and(|m| used.files_changed > m) {
            Some(Breach::Files)
        } else {
            None
        }
    }

    fn covers(&self, ask: &Ask) -> bool {
        let Some(kind) = ask.kind else { return false };
        // The irreversible floor: a delete needs a verified checkpoint (step 8.1), so no contract answers it.
        if kind == ToolKind::Delete || !self.tool_kinds.contains(&kind) {
            return false;
        }
        match kind {
            ToolKind::Execute => ask
                .command
                .as_deref()
                .is_some_and(|c| self.commands.iter().any(|p| command_match::matches(p, c))),
            ToolKind::Fetch => ask
                .host
                .as_deref()
                .is_some_and(|h| self.hosts.iter().any(|x| x == h)),
            ToolKind::Read | ToolKind::Edit => ask.path.as_deref().is_some_and(|p| self.path_ok(p)),
            ToolKind::Other | ToolKind::Delete => false,
        }
    }

    fn path_ok(&self, path: &str) -> bool {
        // Absolute, home-relative and parent-escaping paths are outside the workspace.
        if path.starts_with('/')
            || path.starts_with('~')
            || path.split('/').any(|s| s == "..")
            || path.contains('\\')
        {
            return false;
        }
        let path = path.trim_start_matches("./");
        self.paths.iter().any(|g| glob(g, path))
    }

    /// The rule entries the engine receives: one per allowed command, host and path glob, per workspace.
    pub fn rules(&self) -> Vec<RuleEntry> {
        let mut out = Vec::new();
        for w in &self.workspaces {
            for k in &self.tool_kinds {
                let patterns: Vec<&String> = match k {
                    ToolKind::Execute => self.commands.iter().collect(),
                    ToolKind::Fetch => self.hosts.iter().collect(),
                    ToolKind::Read | ToolKind::Edit => self.paths.iter().collect(),
                    ToolKind::Delete | ToolKind::Other => Vec::new(),
                };
                out.extend(patterns.into_iter().map(|p| RuleEntry {
                    source: self.source(),
                    workspace: w.clone(),
                    kind: *k,
                    pattern: p.clone(),
                }));
            }
        }
        out
    }

    /// The agent mode to request, or `None`. Edits-only work may ask for `acceptEdits`; a mode the agent does not
    /// list in `offered` is never requested, so the sandbox stays the agent's own.
    pub fn mode(&self, offered: &[String]) -> Option<Mode> {
        let edits_only = !self.tool_kinds.is_empty()
            && self
                .tool_kinds
                .iter()
                .all(|k| matches!(k, ToolKind::Read | ToolKind::Edit));
        let want = if edits_only {
            Mode::AcceptEdits
        } else {
            Mode::Default
        };
        offered.iter().any(|m| m == want.id()).then_some(want)
    }
}

fn glob(pattern: &str, path: &str) -> bool {
    fn seg(p: &str, s: &str) -> bool {
        let (p, s): (Vec<char>, Vec<char>) = (p.chars().collect(), s.chars().collect());
        fn go(p: &[char], s: &[char]) -> bool {
            match p.split_first() {
                None => s.is_empty(),
                Some(('*', rest)) => (0..=s.len()).any(|i| go(rest, &s[i..])),
                Some((c, rest)) => s.first() == Some(c) && go(rest, &s[1..]),
            }
        }
        go(&p, &s)
    }
    fn walk(p: &[&str], s: &[&str]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((&"**", rest)) => (0..=s.len()).any(|i| walk(rest, &s[i..])),
            Some((head, rest)) => s
                .split_first()
                .is_some_and(|(x, tail)| seg(head, x) && walk(rest, tail)),
        }
    }
    let (p, s): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
    walk(&p, &s)
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    fn c() -> Contract {
        Contract {
            id: "c1".into(),
            workspaces: vec!["api".into()],
            paths: vec!["src/**".into(), "*.md".into()],
            commands: vec!["cargo test *".into(), "git status".into()],
            tool_kinds: vec![ToolKind::Execute, ToolKind::Edit, ToolKind::Fetch],
            hosts: vec!["crates.io".into()],
            spend_cents: Some(500),
            wall_secs: Some(3600),
            max_files: Some(20),
            done_criteria: vec!["cargo test".into()],
            safe_default: SafeDefault::Park,
        }
    }

    fn ask(kind: ToolKind) -> Ask {
        Ask {
            workspace: "api".into(),
            kind: Some(kind),
            ..Ask::default()
        }
    }

    fn allow() -> Verdict {
        Verdict::Allow {
            source: "contract:c1".into(),
        }
    }

    #[test]
    fn an_ask_inside_the_contract_is_answered_and_tagged() {
        let a = Ask {
            command: Some("cargo test -p x".into()),
            ..ask(ToolKind::Execute)
        };
        assert_eq!(c().check(&a, Usage::default()), allow());
        let e = Ask {
            path: Some("src/a/b.rs".into()),
            ..ask(ToolKind::Edit)
        };
        assert_eq!(c().check(&e, Usage::default()), allow());
    }

    #[test]
    fn compound_commands_never_match() {
        for cmd in [
            "git status && rm -rf x",
            "cargo test; id",
            "cargo test | sh",
            "cargo test $(id)",
        ] {
            let a = Ask {
                command: Some(cmd.into()),
                ..ask(ToolKind::Execute)
            };
            assert_eq!(c().check(&a, Usage::default()), Verdict::Pass, "{cmd}");
        }
    }

    #[test]
    fn the_irreversible_floor_holds() {
        let mut k = c();
        k.tool_kinds.push(ToolKind::Delete);
        let d = Ask {
            path: Some("src/a.rs".into()),
            ..ask(ToolKind::Delete)
        };
        assert_eq!(k.check(&d, Usage::default()), Verdict::Pass);
        let outside = ["/etc/passwd", "../x.rs", "~/x", "src/../../x"];
        for p in outside {
            let e = Ask {
                path: Some(p.into()),
                ..ask(ToolKind::Edit)
            };
            assert_eq!(k.check(&e, Usage::default()), Verdict::Pass, "{p}");
        }
        let f = Ask {
            host: Some("evil.example".into()),
            ..ask(ToolKind::Fetch)
        };
        assert_eq!(k.check(&f, Usage::default()), Verdict::Pass);
        let ok = Ask {
            host: Some("crates.io".into()),
            ..ask(ToolKind::Fetch)
        };
        assert_eq!(k.check(&ok, Usage::default()), allow());
    }

    #[test]
    fn another_workspace_is_never_answered() {
        let a = Ask {
            workspace: "web".into(),
            command: Some("git status".into()),
            ..ask(ToolKind::Execute)
        };
        assert_eq!(c().check(&a, Usage::default()), Verdict::Pass);
    }

    #[test]
    fn a_crossed_limit_is_a_breach_with_a_dedupable_cause() {
        let a = Ask {
            command: Some("git status".into()),
            ..ask(ToolKind::Execute)
        };
        let v = c().check(
            &a,
            Usage {
                spend_cents: 501,
                ..Usage::default()
            },
        );
        assert_eq!(v, Verdict::Breach(Breach::Spend));
        assert_eq!(Breach::Spend.cause("c1"), "contract-breach:c1:spend");
        assert_eq!(
            c().check(
                &a,
                Usage {
                    wall_secs: 3601,
                    ..Usage::default()
                }
            ),
            Verdict::Breach(Breach::WallTime)
        );
        assert_eq!(
            c().check(
                &a,
                Usage {
                    files_changed: 21,
                    ..Usage::default()
                }
            ),
            Verdict::Breach(Breach::Files)
        );
        assert_eq!(
            c().check(
                &a,
                Usage {
                    spend_cents: 500,
                    ..Usage::default()
                }
            ),
            allow()
        );
    }

    #[test]
    fn generated_rules_carry_the_source_and_never_a_delete() {
        let mut k = c();
        k.tool_kinds.push(ToolKind::Delete);
        let rules = k.rules();
        assert!(rules.iter().all(|r| r.source == "contract:c1"
            && r.workspace == "api"
            && r.kind != ToolKind::Delete));
        assert_eq!(
            rules.iter().filter(|r| r.kind == ToolKind::Execute).count(),
            2
        );
    }

    #[test]
    fn a_contract_never_widens_the_agents_sandbox() {
        let offered = vec!["default".to_string()];
        let mut k = c();
        k.tool_kinds = vec![ToolKind::Edit];
        // acceptEdits is not offered: nothing is requested beyond the agent's own modes.
        assert_eq!(k.mode(&offered), None);
        assert_eq!(
            k.mode(&["default".into(), "acceptEdits".into()]),
            Some(Mode::AcceptEdits)
        );
        // Work that runs commands never asks for a looser mode than the default.
        assert_eq!(
            c().mode(&["acceptEdits".into(), "bypassPermissions".into()]),
            None
        );
    }

    #[test]
    fn globs_stay_within_their_segments() {
        assert!(glob("*.md", "README.md"));
        assert!(!glob("*.md", "docs/a.md"));
        assert!(glob("src/**", "src/a/b.rs"));
        assert!(!glob("src/*", "src/a/b.rs"));
    }
}
