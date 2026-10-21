//! The opt-in hook surface of each agent (plan v6, P.3, decisions 0079 and 0106). A client asks about one agent;
//! this module sends the request to the agent's own installer. Claude Code reads JSON hooks from a settings file,
//! OpenCode loads a plugin from its config directory: the mechanics differ, the consent and journal rules do not.
use crate::claude_hooks;
use crate::opencode_hooks;

/// The answer to an `agent_hooks` request (U10, decisions 0079 and 0106).
#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    /// `connected`, `not_connected`, `unsupported` (no verified hook surface) or `unavailable`.
    pub state: &'static str,
    pub preview: Option<String>,
    pub error: Option<String>,
}

impl Reply {
    pub fn unsupported() -> Self {
        Self {
            state: "unsupported",
            preview: None,
            error: None,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            state: "unavailable",
            preview: None,
            error: None,
        }
    }
}

/// Where both installers keep their files for one server.
#[derive(Clone, Debug)]
pub struct Targets {
    pub claude: claude_hooks::Target,
    pub opencode: opencode_hooks::Target,
}

/// The agents whose event surface Termenbox can connect: Claude Code's hooks and OpenCode's plugin.
pub fn supported(agent: &str) -> bool {
    claude_hooks::supported(agent) || opencode_hooks::supported(agent)
}

/// Handles one `status`, `preview`, `apply` or `remove` for `agent`. File contents never leave the installer
/// except as the preview the owner asked for; callers log the event only.
pub fn respond(targets: Option<&Targets>, agent: &str, action: &str) -> Reply {
    if !supported(agent) {
        return Reply::unsupported();
    }
    match agent {
        "claude" => claude_hooks::respond(targets.map(|t| &t.claude), agent, action),
        "opencode" => opencode_hooks::respond(targets.map(|t| &t.opencode), agent, action),
        _ => Reply::unsupported(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(d: &std::path::Path) -> Targets {
        Targets {
            claude: claude_hooks::Target {
                settings: d.join("settings.json"),
                auto: false,
                binary: "/bin/termenbox-server".into(),
                state_dir: d.join("state"),
            },
            opencode: opencode_hooks::Target {
                plugin: d.join("opencode/plugins/termenbox.js"),
                auto: false,
                binary: "/bin/termenbox-server".into(),
                state_dir: d.join("state"),
            },
        }
    }

    #[test]
    fn each_agent_uses_its_own_installer_and_unknown_agents_are_unsupported() {
        let d = std::env::temp_dir().join(format!("tm-ah-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let t = targets(&d);
        assert_eq!(respond(Some(&t), "claude", "apply").state, "connected");
        assert!(t.claude.settings.exists());
        assert!(!t.opencode.plugin.exists());
        assert_eq!(respond(Some(&t), "opencode", "apply").state, "connected");
        assert!(t.opencode.plugin.exists());
        assert_eq!(respond(Some(&t), "codex", "apply"), Reply::unsupported());
        assert_eq!(respond(None, "claude", "status"), Reply::unavailable());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn both_agents_are_advertised() {
        assert!(supported("claude"));
        assert!(supported("opencode"));
        assert!(!supported("codex"));
    }
}
