//! Domain types shared by every agent-pinball crate.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

mod setup;
mod waiting;

pub use setup::{
    AgentReport, HookEventKind, HookRecord, HookState, ParseHookEventKindError, SetupReport,
};
pub use waiting::WaitingReason;

/// A coding agent the panel monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    /// OpenAI Codex CLI.
    Codex,
    /// Cursor.
    Cursor,
    /// Claude Code.
    Claude,
}

impl AgentKind {
    /// Every supported agent, in display order.
    pub const ALL: [Self; 3] = [Self::Codex, Self::Cursor, Self::Claude];

    /// Stable lowercase identifier used in config, URLs and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Claude => "claude",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned when a string names no supported agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseAgentKindError(String);

impl fmt::Display for ParseAgentKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown agent kind: {:?}", self.0)
    }
}

impl std::error::Error for ParseAgentKindError {}

impl FromStr for AgentKind {
    type Err = ParseAgentKindError;

    /// Parses once at the boundary; trims and ignores case so config files
    /// written by hand do not fail on `Claude` vs `claude`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == normalized)
            .ok_or_else(|| ParseAgentKindError(s.to_owned()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("codex", AgentKind::Codex)]
    #[case("cursor", AgentKind::Cursor)]
    #[case("claude", AgentKind::Claude)]
    #[case("  Claude ", AgentKind::Claude)]
    #[case("CODEX", AgentKind::Codex)]
    fn parses_known_kinds(#[case] input: &str, #[case] expected: AgentKind) {
        assert_eq!(input.parse::<AgentKind>().unwrap(), expected);
    }

    #[rstest]
    #[case("")]
    #[case("copilot")]
    #[case("claude-code")]
    fn rejects_unknown_kinds(#[case] input: &str) {
        assert_eq!(
            input.parse::<AgentKind>(),
            Err(ParseAgentKindError(input.to_owned()))
        );
    }

    #[test]
    fn display_round_trips_through_parse() {
        for kind in AgentKind::ALL {
            assert_eq!(kind.to_string().parse::<AgentKind>().unwrap(), kind);
        }
    }
}
