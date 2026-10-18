//! The hook receiver's side: turn an agent's hook payload into a
//! `HookRecord` and append it to agent-pinball's own spool.
//!
//! Payload fields (session id, cwd) per agent:
//! - Claude Code: `session_id`, `cwd` (<https://code.claude.com/docs/en/hooks>)
//! - Codex: `session_id`, `cwd` (<https://github.com/openai/codex/blob/main/codex-rs/hooks/src/lib.rs>)
//! - Cursor: `conversation_id`, `workspace_roots` (<https://cursor.com/docs/agent/hooks>)
//!
//! For `AwaitingInput` only, `hook_event_name` and Claude Code's
//! `notification_type` are matched against fixed names to pick a
//! `WaitingReason`; the payload's text is not copied.
//!
//! Everything else in the payload (prompts, `last_assistant_message`,
//! `custom_instructions`, `message`, `title`, `tool_input`, transcript paths,
//! `user_email`) is never read.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pinball_core::{AgentKind, HookEventKind, HookRecord, WaitingReason};
use serde_json::Value;

/// Most payload bytes read from a hook's stdin.
pub const MAX_PAYLOAD_BYTES: u64 = 64 * 1024;
/// How long the receiver waits for the payload before recording without it,
/// so a hook never holds up the agent.
pub const PAYLOAD_DEADLINE: Duration = Duration::from_secs(2);
/// A spool file over this size is rotated to `<kind>.1.jsonl`.
const MAX_SPOOL_BYTES: u64 = 1024 * 1024;
/// Bytes read from the end of a spool to find its newest record.
const TAIL_BYTES: u64 = 64 * 1024;
const MAX_SESSION_ID_CHARS: usize = 128;
const MAX_PROJECT_CHARS: usize = 80;

/// Reads at most `MAX_PAYLOAD_BYTES` from `input` within `deadline`. On
/// timeout it returns nothing; the reader thread is left to the process exit.
pub fn read_payload(input: impl Read + Send + 'static, deadline: Duration) -> Vec<u8> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        // A read error just means no payload; the event is still recorded.
        let _ = input.take(MAX_PAYLOAD_BYTES).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx.recv_timeout(deadline).unwrap_or_default()
}

/// Builds the record for one hook call. Malformed payloads still yield a
/// record, just without session or project.
///
/// `None` when the payload came from Cursor but the hook was installed for
/// another agent: Cursor also runs Claude Code's hooks from
/// `~/.claude/settings.json` (its payloads carry `cursor_version`), and its
/// own hook already records the event, so keeping it would log a Cursor
/// session twice, once as Claude. Also `None` for an `AwaitingInput` payload
/// that names no known reason to wait, such as Claude Code's `auth_success`
/// notification.
#[must_use]
pub fn record(
    kind: AgentKind,
    event: HookEventKind,
    payload: &[u8],
    at_ms: u64,
) -> Option<HookRecord> {
    let value: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);
    if kind != AgentKind::Cursor && value.get("cursor_version").is_some() {
        return None;
    }
    let reason = match event {
        HookEventKind::AwaitingInput => Some(waiting_reason(&value)?),
        _ => None,
    };
    let session_id = ["session_id", "conversation_id"]
        .into_iter()
        .find_map(|key| value.get(key)?.as_str())
        .filter(|id| is_session_id(id))
        .map(str::to_owned);
    let cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .or_else(|| value.get("workspace_roots")?.as_array()?.first()?.as_str());
    let project = cwd
        .and_then(|cwd| Path::new(cwd).file_name())
        .and_then(|name| name.to_str())
        .map(clean_label)
        .filter(|name| !name.is_empty());
    Some(HookRecord {
        kind,
        event,
        session_id,
        project,
        at_ms,
        reason,
    })
}

/// Why an `AwaitingInput` hook fired, from fixed event and notification
/// names. Claude Code: <https://code.claude.com/docs/en/hooks#notification>;
/// Codex: `permission-request.command.input.schema.json` in
/// <https://github.com/openai/codex/tree/main/codex-rs/hooks/schema/generated>.
fn waiting_reason(payload: &Value) -> Option<WaitingReason> {
    let field = |key| payload.get(key).and_then(Value::as_str);
    match field("hook_event_name")? {
        "PermissionRequest" => Some(WaitingReason::Approval),
        "Notification" => match field("notification_type")? {
            "permission_prompt" => Some(WaitingReason::Approval),
            "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input" => {
                Some(WaitingReason::Question)
            }
            "idle_prompt" => Some(WaitingReason::IdlePrompt),
            _ => None,
        },
        _ => None,
    }
}

/// Provider ids are UUIDs or similar; anything else is dropped, not cleaned.
fn is_session_id(id: &str) -> bool {
    (1..=MAX_SESSION_ID_CHARS).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// Strips control and bidi-override characters and truncates on a char
/// boundary, so a directory name cannot spoof or flood the panel.
fn clean_label(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .take(MAX_PROJECT_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

const fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn spool_path(state_dir: &Path, kind: AgentKind) -> PathBuf {
    state_dir.join("hooks").join(format!("{kind}.jsonl"))
}

/// Appends one record, rotating the spool once it passes its size cap.
pub fn append(state_dir: &Path, record: &HookRecord) -> io::Result<()> {
    let path = spool_path(state_dir, record.kind);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_SPOOL_BYTES) {
        fs::rename(&path, path.with_extension("1.jsonl"))?;
    }
    let mut line = serde_json::to_string(record).map_err(io::Error::other)?;
    line.push('\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&path)?.write_all(line.as_bytes())
}

/// Newest record in an agent's spool, reading only the file's tail.
#[must_use]
pub fn last_event(state_dir: &Path, kind: AgentKind) -> Option<HookRecord> {
    let mut file = File::open(spool_path(state_dir, kind)).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut tail).ok()?;
    String::from_utf8_lossy(&tail)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn claude_payload_keeps_ids_only() {
        let payload = br#"{"session_id":"9f1c-22","cwd":"/Users/x/dev/agent-pinball",
            "hook_event_name":"Stop","last_assistant_message":"sk-ant-FAKE0123456789",
            "transcript_path":"/Users/x/.claude/projects/a.jsonl"}"#;
        let record = record(AgentKind::Claude, HookEventKind::Stop, payload, 5).unwrap();
        assert_eq!(record.session_id.as_deref(), Some("9f1c-22"));
        assert_eq!(record.project.as_deref(), Some("agent-pinball"));
        let stored = serde_json::to_string(&record).unwrap();
        assert!(!stored.contains("FAKE"), "{stored}");
        assert!(!stored.contains(".claude"), "{stored}");
    }

    #[test]
    fn cursor_payload_uses_conversation_and_workspace() {
        let payload = br#"{"conversation_id":"c-1","workspace_roots":["/w/api"],
            "user_email":"someone@example.com"}"#;
        let record = record(AgentKind::Cursor, HookEventKind::SessionStart, payload, 1).unwrap();
        assert_eq!(record.session_id.as_deref(), Some("c-1"));
        assert_eq!(record.project.as_deref(), Some("api"));
        assert!(
            !serde_json::to_string(&record)
                .unwrap()
                .contains("example.com")
        );
    }

    /// Shape of a real Cursor 3.25 `sessionEnd` payload, anonymized, as
    /// Cursor sends it to both its own hooks and Claude Code's.
    const CURSOR_SESSION_END: &[u8] =
        br#"{"conversation_id":"6ea6d1a9-0000-4b0b-bfa2-d68c5d17053c",
        "generation_id":"","model":"unknown","reason":"window_close","duration_ms":0,
        "is_background_agent":false,"final_status":"unknown",
        "session_id":"6ea6d1a9-0000-4b0b-bfa2-d68c5d17053c","hook_event_name":"sessionEnd",
        "cursor_version":"3.25.9","workspace_roots":["/w/estudio"],
        "user_email":"someone@example.com","transcript_path":null}"#;

    // Cursor runs Claude Code's hooks too; its event must be recorded once,
    // as Cursor, never as a Claude session.
    #[test]
    fn cursor_payload_through_a_claude_hook_is_skipped() {
        assert_eq!(
            record(
                AgentKind::Claude,
                HookEventKind::SessionEnd,
                CURSOR_SESSION_END,
                1
            ),
            None
        );
        assert_eq!(
            record(
                AgentKind::Codex,
                HookEventKind::SessionEnd,
                CURSOR_SESSION_END,
                1
            ),
            None
        );
        let own = record(
            AgentKind::Cursor,
            HookEventKind::SessionEnd,
            CURSOR_SESSION_END,
            1,
        )
        .unwrap();
        assert_eq!(own.kind, AgentKind::Cursor);
        assert_eq!(own.project.as_deref(), Some("estudio"));
        // Control: Claude Code's own payload has no cursor_version.
        let claude = br#"{"session_id":"s-1","cwd":"/w/api","hook_event_name":"SessionEnd"}"#;
        assert!(record(AgentKind::Claude, HookEventKind::SessionEnd, claude, 1).is_some());
    }

    // Adversarial: ids and directory names come from the agent's environment.
    #[test]
    fn hostile_fields_are_dropped_or_cleaned() {
        let long = "a".repeat(200);
        let payload = format!(
            r#"{{"session_id":"<script>alert(1)</script>","cwd":"/w/evil\\u202Egnp.exe\u0007{long}"}}"#
        );
        let record = record(AgentKind::Codex, HookEventKind::Stop, payload.as_bytes(), 1).unwrap();
        assert_eq!(record.session_id, None);
        let project = record.project.unwrap();
        assert!(!project.contains('\u{202E}') && !project.contains('\u{7}'));
        assert_eq!(project.chars().count(), MAX_PROJECT_CHARS);
    }

    // Adversarial: a waiting payload's text carries secrets and markup; only
    // the reason may reach the spool.
    #[test]
    fn a_permission_prompt_keeps_only_its_reason() {
        let payload = br#"{"session_id":"s-1","cwd":"/w/api","hook_event_name":"Notification",
            "notification_type":"permission_prompt",
            "message":"Claude needs your permission to use sk-ant-FAKE0123 <script>alert(1)</script>",
            "title":"<script>t()</script>","tool_input":{"command":"echo sk-ant-FAKE0123"}}"#;
        let record = record(AgentKind::Claude, HookEventKind::AwaitingInput, payload, 4).unwrap();
        let stored = serde_json::to_string(&record).unwrap();
        assert_eq!(
            stored,
            r#"{"kind":"claude","event":"awaiting_input","sessionId":"s-1","project":"api","atMs":4,"reason":"approval"}"#
        );
    }

    #[rstest::rstest]
    #[case(
        r#"{"hook_event_name":"PermissionRequest","tool_input":{"command":"rm -rf /"}}"#,
        WaitingReason::Approval
    )]
    #[case(
        r#"{"hook_event_name":"Notification","notification_type":"permission_prompt"}"#,
        WaitingReason::Approval
    )]
    #[case(
        r#"{"hook_event_name":"Notification","notification_type":"elicitation_dialog"}"#,
        WaitingReason::Question
    )]
    #[case(
        r#"{"hook_event_name":"Notification","notification_type":"elicitation_url_dialog"}"#,
        WaitingReason::Question
    )]
    #[case(
        r#"{"hook_event_name":"Notification","notification_type":"agent_needs_input"}"#,
        WaitingReason::Question
    )]
    #[case(
        r#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#,
        WaitingReason::IdlePrompt
    )]
    fn waiting_payloads_map_to_a_reason(#[case] payload: &str, #[case] reason: WaitingReason) {
        for kind in [AgentKind::Claude, AgentKind::Codex] {
            let record = record(kind, HookEventKind::AwaitingInput, payload.as_bytes(), 1).unwrap();
            assert_eq!(record.reason, Some(reason), "{payload}");
        }
    }

    // Adversarial: anything but a known name records nothing, so no
    // attacker-chosen string becomes a reason.
    #[test]
    fn unknown_waiting_payloads_record_nothing() {
        let oversized = format!(
            r#"{{"hook_event_name":"Notification","notification_type":"permission_prompt{}"}}"#,
            "x".repeat(70_000)
        );
        for payload in [
            r#"{"hook_event_name":"Notification","notification_type":"auth_success"}"#,
            r#"{"hook_event_name":"Notification","notification_type":"Permission_Prompt"}"#,
            r#"{"hook_event_name":"Notification","notification_type":7}"#,
            r#"{"hook_event_name":"Notification","notification_type":["permission_prompt"]}"#,
            r#"{"hook_event_name":"Notification"}"#,
            r#"{"hook_event_name":"Stop","notification_type":"permission_prompt"}"#,
            r#"{"notification_type":"permission_prompt"}"#,
            "not json",
            "",
            &oversized,
        ] {
            assert_eq!(
                record(
                    AgentKind::Claude,
                    HookEventKind::AwaitingInput,
                    payload.as_bytes(),
                    1
                ),
                None,
                "{}",
                &payload[..payload.len().min(80)]
            );
        }
        // Control: other events never carry a reason, whatever the payload.
        let stop = br#"{"hook_event_name":"Notification","notification_type":"permission_prompt"}"#;
        let stop = record(AgentKind::Claude, HookEventKind::Stop, stop, 1).unwrap();
        assert_eq!(stop.reason, None);
    }

    // Cursor also runs Claude Code's Notification hook; that must not show a
    // Cursor session waiting as a Claude one.
    #[test]
    fn cursor_payload_through_a_claude_notification_is_skipped() {
        let payload = br#"{"session_id":"c-1","hook_event_name":"Notification",
            "notification_type":"permission_prompt","cursor_version":"3.25.9"}"#;
        assert_eq!(
            record(AgentKind::Claude, HookEventKind::AwaitingInput, payload, 1),
            None
        );
    }

    #[test]
    fn malformed_payload_still_records_the_event() {
        for payload in [&b""[..], b"\xff\xfe", b"{\"session_id\":", b"[]"] {
            let record = record(AgentKind::Claude, HookEventKind::SessionEnd, payload, 3).unwrap();
            assert_eq!(record.event, HookEventKind::SessionEnd);
            assert_eq!(record.session_id, None);
        }
    }

    // Adversarial: a hook payload must not make the receiver buffer without bound.
    #[test]
    fn payload_is_capped() {
        let huge = io::repeat(b'x').take(MAX_PAYLOAD_BYTES * 4);
        let read = read_payload(huge, PAYLOAD_DEADLINE);
        assert_eq!(read.len() as u64, MAX_PAYLOAD_BYTES);
    }

    #[test]
    fn a_silent_stdin_does_not_hold_the_hook() {
        struct Never;
        impl Read for Never {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                thread::sleep(Duration::from_secs(5));
                Ok(0)
            }
        }
        let started = std::time::Instant::now();
        assert!(read_payload(Never, Duration::from_millis(50)).is_empty());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn append_then_read_the_newest_record() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(last_event(dir.path(), AgentKind::Codex), None);
        for at_ms in [1, 2, 3] {
            let record = record(AgentKind::Codex, HookEventKind::Stop, b"{}", at_ms).unwrap();
            append(dir.path(), &record).unwrap();
        }
        assert_eq!(last_event(dir.path(), AgentKind::Codex).unwrap().at_ms, 3);
        assert_eq!(last_event(dir.path(), AgentKind::Claude), None);
    }

    #[test]
    fn spool_rotates_past_its_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = spool_path(dir.path(), AgentKind::Claude);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            vec![b'\n'; usize::try_from(MAX_SPOOL_BYTES).unwrap() + 1],
        )
        .unwrap();
        let record = record(AgentKind::Claude, HookEventKind::Stop, b"{}", 9).unwrap();
        append(dir.path(), &record).unwrap();
        assert!(fs::metadata(&path).unwrap().len() < 200);
        assert!(path.with_extension("1.jsonl").exists());
        assert_eq!(last_event(dir.path(), AgentKind::Claude).unwrap().at_ms, 9);
    }
}
