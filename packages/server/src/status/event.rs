//! The typed events every evidence source emits (decision 0182). A source never says "the status is X".
use termenbox_core::events::{AskKind, FailureCause};

/// Where an observation came from; ranks hook > transcript > screen > scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Transcript,
    Screen,
    Scan,
}

/// What a pane or a transcript shows, as opposed to what a hook said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    Working,
    Idle,
    Blocked,
}

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed(FailureCause),
    /// The human pressed Esc: idle, not failed (decision 0185).
    Interrupted,
}

/// How an ask closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskOutcome {
    Answered,
    Declined,
    /// Esc on the dialog: no hook fires, so a transcript or screen observation reports it.
    Escaped,
}

/// `SessionStart` source: `Compact` is the same session continuing, not a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    Startup,
    Resume,
    Clear,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    SessionStart(SessionSource),
    SessionEnd,
    TurnStart {
        title: Option<String>,
    },
    /// `bg` is the background work still alive, which `Stop` carries (fact 0073).
    TurnEnd {
        outcome: Outcome,
        bg: u32,
    },
    ToolStart {
        name: String,
    },
    ToolEnd {
        name: String,
    },
    AskOpen {
        key: String,
        kind: AskKind,
    },
    AskContent {
        key: String,
        text: String,
        options: Vec<String>,
    },
    AskClose {
        key: String,
        outcome: AskOutcome,
    },
    SubagentStart,
    SubagentEnd,
    Background {
        count: u32,
    },
    CompactStart,
    CompactEnd,
    Retry {
        active: bool,
    },
    /// A progress proof: a child process, a transcript append, a pane change outside the spinner.
    Progress,
    Observe {
        source: Source,
        seen: Seen,
    },
    ProcessGone,
    /// A human viewed the tab on a visible page of some device.
    Viewed,
    Dismiss,
}

/// One value per event shape, so a table can prove it covers all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    SessionStart,
    SessionEnd,
    TurnStart,
    TurnEnd,
    ToolStart,
    ToolEnd,
    AskOpen,
    AskContent,
    AskClose,
    SubagentStart,
    SubagentEnd,
    Background,
    CompactStart,
    CompactEnd,
    Retry,
    Progress,
    Observe,
    ProcessGone,
    Viewed,
    Dismiss,
}

impl Kind {
    pub const ALL: [Kind; 20] = [
        Kind::SessionStart,
        Kind::SessionEnd,
        Kind::TurnStart,
        Kind::TurnEnd,
        Kind::ToolStart,
        Kind::ToolEnd,
        Kind::AskOpen,
        Kind::AskContent,
        Kind::AskClose,
        Kind::SubagentStart,
        Kind::SubagentEnd,
        Kind::Background,
        Kind::CompactStart,
        Kind::CompactEnd,
        Kind::Retry,
        Kind::Progress,
        Kind::Observe,
        Kind::ProcessGone,
        Kind::Viewed,
        Kind::Dismiss,
    ];
}

impl Event {
    pub fn kind(&self) -> Kind {
        match self {
            Event::SessionStart(_) => Kind::SessionStart,
            Event::SessionEnd => Kind::SessionEnd,
            Event::TurnStart { .. } => Kind::TurnStart,
            Event::TurnEnd { .. } => Kind::TurnEnd,
            Event::ToolStart { .. } => Kind::ToolStart,
            Event::ToolEnd { .. } => Kind::ToolEnd,
            Event::AskOpen { .. } => Kind::AskOpen,
            Event::AskContent { .. } => Kind::AskContent,
            Event::AskClose { .. } => Kind::AskClose,
            Event::SubagentStart => Kind::SubagentStart,
            Event::SubagentEnd => Kind::SubagentEnd,
            Event::Background { .. } => Kind::Background,
            Event::CompactStart => Kind::CompactStart,
            Event::CompactEnd => Kind::CompactEnd,
            Event::Retry { .. } => Kind::Retry,
            Event::Progress => Kind::Progress,
            Event::Observe { .. } => Kind::Observe,
            Event::ProcessGone => Kind::ProcessGone,
            Event::Viewed => Kind::Viewed,
            Event::Dismiss => Kind::Dismiss,
        }
    }
}
