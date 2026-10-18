//! The hook receiver's side: turn an agent's hook payload into a
//! `HookRecord` and append it to agent-pinball's own spool.
//!
//! Payload fields (session id, cwd) per agent:
//! - Claude Code: `session_id`, `cwd` (<https://code.claude.com/docs/en/hooks>)
//! - Codex: `session_id`, `cwd` (<https://github.com/openai/codex/blob/main/codex-rs/hooks/src/lib.rs>)
//! - Cursor: `conversation_id`, `workspace_roots` (<https://cursor.com/docs/agent/hooks>)
//!
//! Everything else in the payload (prompts, `last_assistant_message`,
//! `custom_instructions`, transcript paths, `user_email`) is never read.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pinball_core::{AgentKind, HookEventKind, HookRecord};
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
#[must_use]
pub fn record(kind: AgentKind, event: HookEventKind, payload: &[u8], at_ms: u64) -> HookRecord {
    let value: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);
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
    HookRecord {
        kind,
        event,
        session_id,
        project,
        at_ms,
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
        let record = record(AgentKind::Claude, HookEventKind::Stop, payload, 5);
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
        let record = record(AgentKind::Cursor, HookEventKind::SessionStart, payload, 1);
        assert_eq!(record.session_id.as_deref(), Some("c-1"));
        assert_eq!(record.project.as_deref(), Some("api"));
        assert!(
            !serde_json::to_string(&record)
                .unwrap()
                .contains("example.com")
        );
    }

    // Adversarial: ids and directory names come from the agent's environment.
    #[test]
    fn hostile_fields_are_dropped_or_cleaned() {
        let long = "a".repeat(200);
        let payload = format!(
            r#"{{"session_id":"<script>alert(1)</script>","cwd":"/w/evil\\u202Egnp.exe\u0007{long}"}}"#
        );
        let record = record(AgentKind::Codex, HookEventKind::Stop, payload.as_bytes(), 1);
        assert_eq!(record.session_id, None);
        let project = record.project.unwrap();
        assert!(!project.contains('\u{202E}') && !project.contains('\u{7}'));
        assert_eq!(project.chars().count(), MAX_PROJECT_CHARS);
    }

    #[test]
    fn malformed_payload_still_records_the_event() {
        for payload in [&b""[..], b"\xff\xfe", b"{\"session_id\":", b"[]"] {
            let record = record(AgentKind::Claude, HookEventKind::SessionEnd, payload, 3);
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
            let record = record(AgentKind::Codex, HookEventKind::Stop, b"{}", at_ms);
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
        let record = record(AgentKind::Claude, HookEventKind::Stop, b"{}", 9);
        append(dir.path(), &record).unwrap();
        assert!(fs::metadata(&path).unwrap().len() < 200);
        assert!(path.with_extension("1.jsonl").exists());
        assert_eq!(last_event(dir.path(), AgentKind::Claude).unwrap().at_ms, 9);
    }
}
