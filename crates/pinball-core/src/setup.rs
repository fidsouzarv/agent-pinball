//! What `agent-pinball setup` found, shared by the terminal, the setup page
//! and the report handed to another agent.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{AgentKind, WaitingReason};

/// An agent event agent-pinball hooks into. Prompt-carrying events
/// (`UserPromptSubmit`, `beforeSubmitPrompt`) are left out on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventKind {
    /// A session started or resumed.
    SessionStart,
    /// The agent finished a turn and handed control back.
    Stop,
    /// The session ended.
    SessionEnd,
    /// The context is about to be compacted.
    PreCompact,
    /// The agent stopped to wait for the user: a permission prompt, a
    /// question, or an idle prompt.
    AwaitingInput,
}

impl HookEventKind {
    /// Every event the `hook` command accepts. Which ones an agent gets, and
    /// under what name, is decided per agent by `setup`.
    pub const ALL: [Self; 5] = [
        Self::SessionStart,
        Self::Stop,
        Self::SessionEnd,
        Self::PreCompact,
        Self::AwaitingInput,
    ];

    /// Stable identifier used on the `hook` command line and in the spool.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::Stop => "stop",
            Self::SessionEnd => "session_end",
            Self::PreCompact => "pre_compact",
            Self::AwaitingInput => "awaiting_input",
        }
    }
}

impl fmt::Display for HookEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned when a string names no hooked event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHookEventKindError(String);

impl fmt::Display for ParseHookEventKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown hook event: {:?}", self.0)
    }
}

impl std::error::Error for ParseHookEventKindError {}

impl FromStr for HookEventKind {
    type Err = ParseHookEventKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| ParseHookEventKindError(s.to_owned()))
    }
}

/// Whether an agent's config carries agent-pinball's hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HookState {
    /// None of agent-pinball's hooks are present.
    Missing,
    /// Every hook is present with the expected command.
    Installed,
    /// Some hooks are missing or point at another binary or state directory.
    Outdated,
    /// The config file cannot be safely edited; setup leaves it alone.
    Invalid {
        /// Why, for the user.
        reason: String,
    },
}

/// One event received through a hook, as stored in agent-pinball's spool.
/// Holds identifiers only: never prompt, response or tool text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRecord {
    /// Agent that ran the hook.
    pub kind: AgentKind,
    /// Event that fired.
    pub event: HookEventKind,
    /// Provider session id, when the payload carried a well-formed one.
    pub session_id: Option<String>,
    /// Last component of the session's working directory.
    pub project: Option<String>,
    /// When the hook ran, epoch milliseconds.
    pub at_ms: u64,
    /// Why the agent is waiting; set only on `AwaitingInput`. Absent from
    /// spool lines written before the field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<WaitingReason>,
}

/// Setup state of one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReport {
    /// Which agent.
    pub kind: AgentKind,
    /// Executable found on `PATH`, if any.
    pub binary: Option<String>,
    /// The agent's data directory.
    pub root: String,
    /// Whether that directory exists. Setup never creates it.
    pub root_found: bool,
    /// The file the hooks live in.
    pub config_path: String,
    /// Current hook state.
    pub hooks: HookState,
    /// agent-pinball's own entries setup would add or remove, as diff
    /// lines; `None` when nothing would change. Never other config text.
    pub pending_diff: Option<String>,
    /// Newest event received from this agent's hooks.
    pub last_event: Option<HookRecord>,
    /// Something the user must do in the agent itself after setup.
    pub note: Option<String>,
}

/// Everything `setup` knows, in agent display order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    /// The binary the hooks call.
    pub executable: String,
    /// Where hook events are spooled.
    pub state_dir: String,
    /// One entry per agent.
    pub agents: Vec<AgentReport>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn event_kinds_round_trip() {
        for kind in HookEventKind::ALL {
            assert_eq!(kind.as_str().parse::<HookEventKind>().unwrap(), kind);
        }
        assert!("user_prompt_submit".parse::<HookEventKind>().is_err());
    }

    #[test]
    fn records_serialize_as_camel_case() {
        let record = HookRecord {
            kind: AgentKind::Claude,
            event: HookEventKind::PreCompact,
            session_id: Some("abc".to_owned()),
            project: None,
            at_ms: 7,
            reason: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"claude","event":"pre_compact","sessionId":"abc","project":null,"atMs":7}"#
        );
        assert_eq!(serde_json::from_str::<HookRecord>(&json).unwrap(), record);
    }

    #[test]
    fn a_waiting_record_carries_its_reason_and_old_lines_still_parse() {
        let record = HookRecord {
            kind: AgentKind::Codex,
            event: HookEventKind::AwaitingInput,
            session_id: None,
            project: None,
            at_ms: 3,
            reason: Some(WaitingReason::Approval),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.ends_with(r#""atMs":3,"reason":"approval"}"#), "{json}");
        assert_eq!(serde_json::from_str::<HookRecord>(&json).unwrap(), record);

        let old = r#"{"kind":"claude","event":"stop","sessionId":null,"project":null,"atMs":1}"#;
        assert_eq!(
            serde_json::from_str::<HookRecord>(old).unwrap().reason,
            None
        );
    }

    #[test]
    fn invalid_state_carries_its_reason() {
        let json = serde_json::to_string(&HookState::Invalid {
            reason: "not JSON".to_owned(),
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"invalid","reason":"not JSON"}"#);
    }
}
