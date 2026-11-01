//! Matching a shell command on parsed arguments (guardrail `auto-answered-permissions-are-scoped-visible-and-audited`).
//!
//! A command is understood only when it is one simple command: words, single and double quotes, nothing that chains,
//! pipes, redirects, substitutes, expands or runs another command. Anything else is `None` and goes to the user.
//! Shared by owner rules (R85) and mission contracts (`contract.rs`).

#![allow(dead_code)] // not called until the permission path (R85 4.6) and the launch sheet (6.3) use it
/// The arguments of `cmd` when it is one simple command, else `None`.
pub fn parse_simple(cmd: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => cur.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '$' | '`' | '\\' | '!' => return None,
                        c => cur.push(c),
                    }
                }
            }
            c if c.is_whitespace() && c != '\n' && c != '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            // chaining, pipes, background, redirects, substitution, grouping, expansion, comments, newlines
            '&' | '|' | ';' | '<' | '>' | '`' | '$' | '(' | ')' | '{' | '}' | '\\' | '\n'
            | '\r' | '*' | '?' | '[' | ']' | '~' | '#' | '!' => return None,
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    let first = words.first()?;
    if matches!(
        first.as_str(),
        "eval"
            | "exec"
            | "source"
            | "."
            | "sudo"
            | "env"
            | "sh"
            | "bash"
            | "zsh"
            | "xargs"
            | "command"
            | "nohup"
            | "time"
    ) {
        return None;
    }
    // `VAR=value cmd` runs cmd with a changed environment; not a plain command.
    if first.contains('=') {
        return None;
    }
    Some(words)
}

/// Whether `cmd` is one simple command that `pattern` covers. A pattern is words that must equal the leading words
/// of the command; a final `*` word allows further plain arguments, otherwise the command must have exactly those.
pub fn matches(pattern: &str, cmd: &str) -> bool {
    let Some(args) = parse_simple(cmd) else {
        return false;
    };
    let Some(mut pat) = parse_pattern(pattern) else {
        return false;
    };
    let open = pat.last().is_some_and(|w| w == "*");
    if open {
        pat.pop();
    }
    if pat.is_empty() || args.len() < pat.len() || (!open && args.len() != pat.len()) {
        return false;
    }
    pat.iter().zip(&args).all(|(p, a)| p == a)
}

fn parse_pattern(pattern: &str) -> Option<Vec<String>> {
    let trimmed = pattern.trim_end();
    match trimmed.strip_suffix(" *") {
        Some(head) => parse_simple(head).map(|mut w| {
            w.push("*".into());
            w
        }),
        None => parse_simple(trimmed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_commands_parse_with_quotes() {
        assert_eq!(
            parse_simple("git commit -m 'a b'").unwrap(),
            ["git", "commit", "-m", "a b"]
        );
        assert_eq!(
            parse_simple("ls  \"dir name\"").unwrap(),
            ["ls", "dir name"]
        );
    }

    #[test]
    fn compound_and_indirect_forms_are_refused() {
        for c in [
            "git status && rm -rf x",
            "a || b",
            "a; b",
            "a | b",
            "a &",
            "a > f",
            "a < f",
            "echo $(id)",
            "echo `id`",
            "echo \"$HOME\"",
            "eval ls",
            "sh -c ls",
            "FOO=1 ls",
            "ls *",
            "ls\nrm x",
            "ls 'unclosed",
            "",
            "   ",
            "sudo ls",
        ] {
            assert!(parse_simple(c).is_none(), "{c:?} parsed");
        }
    }

    #[test]
    fn a_rule_never_covers_a_compound_command() {
        assert!(matches("git status", "git status"));
        assert!(!matches("git status", "git status && rm -rf x"));
        assert!(!matches("git status", "git status; id"));
    }

    #[test]
    fn exact_and_open_patterns() {
        assert!(!matches("git status", "git status -s"));
        assert!(matches("git status *", "git status -s"));
        assert!(matches("git status *", "git status"));
        assert!(!matches("git status *", "git push"));
        assert!(!matches("*", "ls"));
    }
}
