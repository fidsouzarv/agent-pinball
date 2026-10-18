//! Runs the built `agent-pinball hook` the way an agent does. This is an
//! integration target, not part of a `tests/suite`, because only integration
//! tests get `CARGO_BIN_EXE_*`, and stdout can only be checked on the real
//! process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

/// A `PermissionRequest` hook that prints anything answers the prompt for the
/// user (Claude Code and Codex read a decision from stdout), so the receiver
/// must stay silent and exit 0.
#[test]
fn a_permission_request_hook_prints_nothing_and_records_the_reason() {
    let payload = br#"{"session_id":"s-1","cwd":"/w/api","hook_event_name":"PermissionRequest",
        "tool_name":"Bash","tool_input":{"command":"echo sk-ant-FAKE0123"}}"#;
    for kind in ["claude", "codex"] {
        let state = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent-pinball"))
            .args(["hook", "--state-dir"])
            .arg(state.path())
            .args([kind, "awaiting_input"])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload).unwrap();
        let out = child.wait_with_output().unwrap();

        assert!(out.status.success(), "{kind}: {out:?}");
        assert!(out.stdout.is_empty(), "{kind} stdout: {:?}", out.stdout);
        let spool = fs::read_to_string(state.path().join(format!("hooks/{kind}.jsonl"))).unwrap();
        assert!(spool.contains(r#""reason":"approval""#), "{spool}");
        assert!(!spool.contains("FAKE"), "{spool}");
    }
}
