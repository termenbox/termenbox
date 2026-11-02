//! Outcome spec (decision 0179, plan step 10.1): how the server judges a finished task without a human.
//!
//! A spec holds verifiers the server runs on the node after a task: a command that must pass, a bound on the diff,
//! and a check by a reviewer model that is not the author's. The result is one `Verdict` for the telemetry (step 1.4).
//! No output of a verifier is kept: only pass, fail or none (`a-cheaper-tier-needs-a-verifier-for-the-outcome`).
#![allow(dead_code)] // not called until the task path records a verdict (10.2) and the router reads it (10.3)
use crate::attention_meter::{AttentionMeter, Verdict};
use anyhow::Result;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verifier {
    /// Run through `sh -c` in the workspace; exit 0 passes. A timeout fails.
    Command { cmd: String, timeout_secs: u64 },
    /// The diff against HEAD stays within these bounds.
    DiffBound { max_files: u32, max_lines: u32 },
    /// A model other than the author's judges the change, through the deputy port (step 7.1).
    Reviewer,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutcomeSpec {
    pub verifiers: Vec<Verifier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStat {
    pub files: u32,
    pub lines: u32,
}

/// What a verifier needs from the node; faked in tests.
pub trait Env {
    /// True when the command exits 0 within the time; a command that cannot start or times out is false.
    fn run(&self, cmd: &str, dir: &Path, timeout: Duration) -> bool;
    fn diff_stat(&self, dir: &Path) -> Result<DiffStat>;
}

/// A reviewer behind the deputy port. It reports its model so the spec can refuse the author's own.
pub trait Reviewer {
    fn model(&self) -> &str;
    fn review(&self, dir: &Path) -> Result<bool>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    /// One verdict per verifier, in spec order.
    pub each: Vec<Verdict>,
}

/// Runs every verifier. Any fail fails the task. A verifier that could not judge (no reviewer, or the author's own
/// model) counts as `none`, and a task with no pass and no fail is `none`: a gap is never reported as a pass.
pub fn run(
    spec: &OutcomeSpec,
    author_model: &str,
    dir: &Path,
    env: &dyn Env,
    reviewer: Option<&dyn Reviewer>,
) -> Outcome {
    let each: Vec<Verdict> = spec
        .verifiers
        .iter()
        .map(|v| match v {
            Verifier::Command { cmd, timeout_secs } => {
                flag(env.run(cmd, dir, Duration::from_secs(*timeout_secs)))
            }
            Verifier::DiffBound {
                max_files,
                max_lines,
            } => match env.diff_stat(dir) {
                Ok(s) => flag(s.files <= *max_files && s.lines <= *max_lines),
                Err(_) => Verdict::None,
            },
            Verifier::Reviewer => match reviewer {
                Some(r) if r.model() != author_model => match r.review(dir) {
                    Ok(ok) => flag(ok),
                    Err(_) => Verdict::None,
                },
                _ => Verdict::None,
            },
        })
        .collect();
    let verdict = if each.contains(&Verdict::Fail) {
        Verdict::Fail
    } else if !each.is_empty() && each.iter().all(|v| *v == Verdict::Pass) {
        Verdict::Pass
    } else {
        Verdict::None
    };
    Outcome { verdict, each }
}

/// Records the task's verdict in the day's telemetry (step 1.4). Only the verdict is stored.
pub fn record(meter: &AttentionMeter, outcome: &Outcome, now: u64) {
    meter.verified(outcome.verdict, now);
}

fn flag(ok: bool) -> Verdict {
    if ok {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

/// The node's own runner: `sh -c` and `git diff --numstat`. Output is discarded.
pub struct NodeEnv;

impl Env for NodeEnv {
    fn run(&self, cmd: &str, dir: &Path, timeout: Duration) -> bool {
        let Ok(mut child) = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(st)) => return st.success(),
                Ok(None) if start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }

    fn diff_stat(&self, dir: &Path) -> Result<DiffStat> {
        let out = Command::new("git")
            .args(["diff", "--numstat", "HEAD"])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        anyhow::ensure!(out.status.success(), "git diff failed");
        let (mut files, mut lines) = (0u32, 0u32);
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            let mut p = l.split('\t');
            let n = |s: Option<&str>| s.and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
            lines = lines
                .saturating_add(n(p.next()))
                .saturating_add(n(p.next()));
            files += 1;
        }
        Ok(DiffStat { files, lines })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        ok: bool,
        stat: Option<DiffStat>,
    }
    impl Env for Fake {
        fn run(&self, _: &str, _: &Path, _: Duration) -> bool {
            self.ok
        }
        fn diff_stat(&self, _: &Path) -> Result<DiffStat> {
            self.stat.ok_or_else(|| anyhow::anyhow!("no repo"))
        }
    }
    struct Rev(&'static str, bool);
    impl Reviewer for Rev {
        fn model(&self) -> &str {
            self.0
        }
        fn review(&self, _: &Path) -> Result<bool> {
            Ok(self.1)
        }
    }
    fn cmd() -> Verifier {
        Verifier::Command {
            cmd: "cargo test".into(),
            timeout_secs: 5,
        }
    }
    fn spec(v: Vec<Verifier>) -> OutcomeSpec {
        OutcomeSpec { verifiers: v }
    }
    fn dir() -> &'static Path {
        Path::new(".")
    }

    #[test]
    fn dispatcher_spec_a_failing_command_fails_the_task() {
        let f = Fake {
            ok: false,
            stat: None,
        };
        let o = run(&spec(vec![cmd()]), "a", dir(), &f, None);
        assert_eq!(o.verdict, Verdict::Fail);
        let f = Fake {
            ok: true,
            stat: None,
        };
        assert_eq!(
            run(&spec(vec![cmd()]), "a", dir(), &f, None).verdict,
            Verdict::Pass
        );
    }

    #[test]
    fn dispatcher_spec_a_missing_verifier_is_none_never_a_pass() {
        let f = Fake {
            ok: true,
            stat: None,
        };
        let o = run(&spec(vec![]), "a", dir(), &f, None);
        assert_eq!(o.verdict, Verdict::None);
        assert!(o.each.is_empty());
        // A reviewer that is declared but absent leaves the task unjudged even when the command passed.
        let o = run(&spec(vec![cmd(), Verifier::Reviewer]), "a", dir(), &f, None);
        assert_eq!(o.verdict, Verdict::None);
        // ...but a fail elsewhere still fails it.
        let f = Fake {
            ok: false,
            stat: None,
        };
        let o = run(&spec(vec![cmd(), Verifier::Reviewer]), "a", dir(), &f, None);
        assert_eq!(o.verdict, Verdict::Fail);
    }

    #[test]
    fn dispatcher_spec_a_reviewer_never_uses_the_authors_model() {
        let f = Fake {
            ok: true,
            stat: None,
        };
        let same = Rev("m1", true);
        let o = run(
            &spec(vec![Verifier::Reviewer]),
            "m1",
            dir(),
            &f,
            Some(&same),
        );
        assert_eq!(o.verdict, Verdict::None);
        let other = Rev("m2", true);
        let o = run(
            &spec(vec![Verifier::Reviewer]),
            "m1",
            dir(),
            &f,
            Some(&other),
        );
        assert_eq!(o.verdict, Verdict::Pass);
        let no = Rev("m2", false);
        let o = run(&spec(vec![Verifier::Reviewer]), "m1", dir(), &f, Some(&no));
        assert_eq!(o.verdict, Verdict::Fail);
    }

    #[test]
    fn dispatcher_spec_a_diff_bound_checks_files_and_lines() {
        let b = Verifier::DiffBound {
            max_files: 3,
            max_lines: 50,
        };
        let ok = Fake {
            ok: true,
            stat: Some(DiffStat {
                files: 3,
                lines: 50,
            }),
        };
        assert_eq!(
            run(&spec(vec![b.clone()]), "a", dir(), &ok, None).verdict,
            Verdict::Pass
        );
        let big = Fake {
            ok: true,
            stat: Some(DiffStat { files: 4, lines: 1 }),
        };
        assert_eq!(
            run(&spec(vec![b.clone()]), "a", dir(), &big, None).verdict,
            Verdict::Fail
        );
        let none = Fake {
            ok: true,
            stat: None,
        };
        assert_eq!(
            run(&spec(vec![b]), "a", dir(), &none, None).verdict,
            Verdict::None
        );
    }

    #[test]
    fn dispatcher_spec_the_node_runner_reads_exit_status_and_times_out() {
        let d = std::env::temp_dir();
        assert!(NodeEnv.run("true", &d, Duration::from_secs(5)));
        assert!(!NodeEnv.run("false", &d, Duration::from_secs(5)));
        assert!(!NodeEnv.run("sleep 5", &d, Duration::from_millis(100)));
    }
}
