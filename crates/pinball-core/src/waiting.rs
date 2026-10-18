//! Why a session is waiting for the user. Stored as this enum only, never the
//! agent's prompt or message text.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What the agent is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitingReason {
    /// A tool call needs the user's permission.
    Approval,
    /// The agent asked the user a question or opened a form.
    Question,
    /// The agent has been idle at its prompt for a while.
    IdlePrompt,
}

impl WaitingReason {
    /// Stable identifier, the same as the serialized form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::Question => "question",
            Self::IdlePrompt => "idle_prompt",
        }
    }
}

impl fmt::Display for WaitingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_its_identifier() {
        for reason in [
            WaitingReason::Approval,
            WaitingReason::Question,
            WaitingReason::IdlePrompt,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, format!("\"{reason}\""));
            assert_eq!(
                serde_json::from_str::<WaitingReason>(&json).unwrap(),
                reason
            );
        }
    }
}
