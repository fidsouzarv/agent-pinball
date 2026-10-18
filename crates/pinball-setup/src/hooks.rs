//! Pure edits of each agent's hook config: inspect, install and remove
//! agent-pinball's own entries, leaving every other entry where it was.
//!
//! Formats:
//! - Claude Code `settings.json`: <https://code.claude.com/docs/en/hooks>
//! - Codex `hooks.json`: <https://github.com/openai/codex/blob/main/codex-rs/hooks/src/engine/discovery.rs>
//! - Cursor `hooks.json`: <https://cursor.com/docs/agent/hooks>

use std::path::Path;

use pinball_core::{AgentKind, HookEventKind, HookState};
use serde_json::{Map, Value, json};

/// How an agent nests its handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// `hooks.<Event>[] = { matcher?, hooks: [{ type, command }] }` (Claude, Codex).
    Grouped,
    /// `hooks.<event>[] = { command }`, plus a top-level `version` (Cursor).
    Flat,
}

/// Where and how one agent keeps its hooks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spec {
    /// Config file name inside the agent's root.
    pub file: &'static str,
    layout: Layout,
    /// The agent's names for `HookEventKind::ALL`, in the same order.
    events: [&'static str; 4],
}

impl Spec {
    pub(crate) const fn of(kind: AgentKind) -> Self {
        match kind {
            AgentKind::Claude => Self {
                file: "settings.json",
                layout: Layout::Grouped,
                events: ["SessionStart", "Stop", "SessionEnd", "PreCompact"],
            },
            AgentKind::Codex => Self {
                file: "hooks.json",
                layout: Layout::Grouped,
                events: ["SessionStart", "Stop", "SessionEnd", "PreCompact"],
            },
            AgentKind::Cursor => Self {
                file: "hooks.json",
                layout: Layout::Flat,
                events: ["sessionStart", "stop", "sessionEnd", "preCompact"],
            },
        }
    }

    fn event_names(self) -> impl Iterator<Item = (HookEventKind, &'static str)> {
        HookEventKind::ALL.into_iter().zip(self.events)
    }
}

/// A parsed config document. A missing or blank file is an empty object.
pub(crate) fn parse(text: Option<&str>) -> Result<Map<String, Value>, String> {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
        return Ok(Map::new());
    };
    match serde_json::from_str(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("the file is JSON but not an object".to_owned()),
        Err(err) => Err(format!("the file is not valid JSON ({err})")),
    }
}

/// Pretty JSON with a trailing newline, the way the agents write it.
pub(crate) fn render(doc: &Map<String, Value>) -> String {
    let mut text =
        serde_json::to_string_pretty(&Value::Object(doc.clone())).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

/// Whether a handler command is agent-pinball's `hook` subcommand, whatever
/// binary path or state directory it carries.
pub(crate) fn is_ours(command: &str) -> bool {
    let (program, rest) = match command.strip_prefix('\'') {
        Some(quoted) => match quoted.split_once('\'') {
            Some((program, rest)) => (program, rest),
            None => return false,
        },
        None => command.split_once(' ').unwrap_or((command, "")),
    };
    Path::new(program)
        .file_name()
        .is_some_and(|name| name == "agent-pinball")
        && rest.trim_start().starts_with("hook ")
}

/// Current state of agent-pinball's hooks in `doc`.
pub(crate) fn state(
    spec: Spec,
    doc: &Map<String, Value>,
    command: impl Fn(HookEventKind) -> String,
) -> HookState {
    let Some(hooks) = doc.get("hooks") else {
        return HookState::Missing;
    };
    let Some(hooks) = hooks.as_object() else {
        return HookState::Invalid {
            reason: "`hooks` is not an object".to_owned(),
        };
    };
    let mut any_ours = hooks
        .values()
        .any(|entries| !our_commands(spec.layout, entries).is_empty());
    let mut all_exact = true;
    for (event, name) in spec.event_names() {
        let ours = hooks
            .get(name)
            .map(|entries| our_commands(spec.layout, entries))
            .unwrap_or_default();
        any_ours |= !ours.is_empty();
        all_exact &= ours == [command(event)];
    }
    match (all_exact, any_ours) {
        (true, _) => HookState::Installed,
        (false, false) => HookState::Missing,
        (false, true) => HookState::Outdated,
    }
}

/// Adds or rewrites agent-pinball's hooks. An event already carrying exactly
/// the expected command is left in place: Codex keys its trust records by
/// position, so moving an entry would send the user's hooks back to review.
/// New entries are appended for the same reason.
pub(crate) fn install(
    spec: Spec,
    doc: &mut Map<String, Value>,
    command: impl Fn(HookEventKind) -> String,
) -> Result<(), String> {
    if spec.layout == Layout::Flat && !doc.contains_key("version") {
        // Cursor requires it; put it first in a new file.
        let mut fresh = Map::new();
        fresh.insert("version".to_owned(), json!(1));
        fresh.append(doc);
        *doc = fresh;
    }
    let hooks = doc
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("`hooks` is not an object")?;
    for (event, name) in spec.event_names() {
        let expected = command(event);
        let entries = hooks
            .entry(name)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("`hooks.{name}` is not a list"))?;
        if our_commands(spec.layout, &Value::Array(entries.clone())) == [expected.clone()] {
            continue;
        }
        remove_ours(spec.layout, entries);
        entries.push(entry(spec.layout, &expected));
    }
    Ok(())
}

/// Removes every agent-pinball handler, on any event. Lists and the `hooks`
/// object are dropped only when this removal is what emptied them.
pub(crate) fn remove(spec: Spec, doc: &mut Map<String, Value>) -> Result<(), String> {
    let Some(hooks) = doc.get_mut("hooks") else {
        return Ok(());
    };
    let hooks = hooks.as_object_mut().ok_or("`hooks` is not an object")?;
    let mut emptied = Vec::new();
    for (name, entries) in hooks.iter_mut() {
        let Some(list) = entries.as_array_mut() else {
            continue;
        };
        if remove_ours(spec.layout, list) && list.is_empty() {
            emptied.push(name.clone());
        }
    }
    let changed = !emptied.is_empty();
    for name in emptied {
        hooks.shift_remove(&name);
    }
    if changed && hooks.is_empty() {
        doc.shift_remove("hooks");
    }
    Ok(())
}

/// agent-pinball's entries that differ between two versions of a config,
/// as `+`/`-` lines naming the event and the command. Nothing else from the
/// documents is included.
pub(crate) fn own_changes(
    spec: Spec,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Vec<String> {
    let ours = |doc: &Map<String, Value>, name: &str| {
        doc.get("hooks")
            .and_then(|hooks| hooks.get(name))
            .map(|entries| our_commands(spec.layout, entries))
            .unwrap_or_default()
    };
    let mut names: Vec<&String> = Vec::new();
    for doc in [before, after] {
        for name in doc
            .get("hooks")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|h| h.keys())
        {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let mut lines = Vec::new();
    if spec.layout == Layout::Flat
        && !before.contains_key("version")
        && after.contains_key("version")
    {
        lines.push("+version: 1".to_owned());
    }
    for name in names {
        let (old, new) = (ours(before, name), ours(after, name));
        lines.extend(
            old.iter()
                .filter(|c| !new.contains(c))
                .map(|c| format!("-hooks.{name}: {c}")),
        );
        lines.extend(
            new.iter()
                .filter(|c| !old.contains(c))
                .map(|c| format!("+hooks.{name}: {c}")),
        );
    }
    lines
}

fn entry(layout: Layout, command: &str) -> Value {
    match layout {
        Layout::Grouped => json!({ "hooks": [{ "type": "command", "command": command }] }),
        Layout::Flat => json!({ "command": command }),
    }
}

/// Handlers of one event, whatever the layout.
fn handlers(layout: Layout, entries: &Value) -> Vec<&Value> {
    let Some(entries) = entries.as_array() else {
        return Vec::new();
    };
    match layout {
        Layout::Flat => entries.iter().collect(),
        Layout::Grouped => entries
            .iter()
            .filter_map(|group| group.get("hooks")?.as_array())
            .flatten()
            .collect(),
    }
}

fn command_of(handler: &Value) -> Option<&str> {
    handler.get("command")?.as_str()
}

fn our_commands(layout: Layout, entries: &Value) -> Vec<String> {
    handlers(layout, entries)
        .into_iter()
        .filter_map(command_of)
        .filter(|command| is_ours(command))
        .map(str::to_owned)
        .collect()
}

/// Drops agent-pinball handlers from one event list. Returns whether any was
/// removed. A group is dropped only when it held nothing but ours.
fn remove_ours(layout: Layout, entries: &mut Vec<Value>) -> bool {
    let before = entries.clone();
    let not_ours = |handler: &Value| !command_of(handler).is_some_and(is_ours);
    match layout {
        Layout::Flat => entries.retain(not_ours),
        Layout::Grouped => entries.retain_mut(|group| {
            let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let had = list.len();
            list.retain(not_ours);
            !(list.is_empty() && had > 0)
        }),
    }
    *entries != before
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn cmd(event: HookEventKind) -> String {
        format!("'/opt/bin/agent-pinball' hook --state-dir '/s' claude {event}")
    }

    fn installed(kind: AgentKind, text: Option<&str>) -> Map<String, Value> {
        let mut doc = parse(text).unwrap();
        install(Spec::of(kind), &mut doc, cmd).unwrap();
        doc
    }

    #[rstest]
    #[case("'/x/agent-pinball' hook claude stop", true)]
    #[case("/usr/local/bin/agent-pinball hook codex stop", true)]
    #[case("'/a b/agent-pinball' hook --state-dir '/s' cursor stop", true)]
    #[case("agent-pinball serve", false)]
    #[case("/bin/not-agent-pinball hook claude stop", false)]
    #[case("echo agent-pinball hook", false)]
    #[case("'unterminated", false)]
    fn recognises_only_its_own_command(#[case] command: &str, #[case] ours: bool) {
        assert_eq!(is_ours(command), ours, "{command}");
    }

    #[rstest]
    #[case(AgentKind::Claude)]
    #[case(AgentKind::Codex)]
    #[case(AgentKind::Cursor)]
    fn install_into_an_empty_file(#[case] kind: AgentKind) {
        let doc = installed(kind, None);
        assert_eq!(state(Spec::of(kind), &doc, cmd), HookState::Installed);
        assert_eq!(state(Spec::of(kind), &Map::new(), cmd), HookState::Missing);
    }

    #[test]
    fn cursor_gets_its_required_version_first() {
        let doc = installed(AgentKind::Cursor, Some("{}"));
        assert_eq!(doc.keys().next().map(String::as_str), Some("version"));
        assert_eq!(
            doc["hooks"]["stop"],
            json!([{ "command": cmd(HookEventKind::Stop) }])
        );
    }

    #[rstest]
    #[case(AgentKind::Claude)]
    #[case(AgentKind::Codex)]
    #[case(AgentKind::Cursor)]
    fn install_is_idempotent(#[case] kind: AgentKind) {
        let once = installed(kind, None);
        let mut twice = once.clone();
        install(Spec::of(kind), &mut twice, cmd).unwrap();
        assert_eq!(once, twice);
    }

    const FOREIGN: &str = r#"{
      "model": "opus",
      "hooks": {
        "Stop": [{ "matcher": "", "hooks": [{ "type": "command", "command": "notify-send done" }] }],
        "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "guard.sh" }] }]
      }
    }"#;

    #[test]
    fn foreign_entries_and_keys_survive_install_and_remove() {
        let original = parse(Some(FOREIGN)).unwrap();
        let doc = installed(AgentKind::Claude, Some(FOREIGN));
        assert_eq!(doc["model"], "opus");
        assert_eq!(doc["hooks"]["PreToolUse"], original["hooks"]["PreToolUse"]);
        // Ours is appended after the user's own Stop group, never before it.
        assert_eq!(doc["hooks"]["Stop"][0], original["hooks"]["Stop"][0]);
        assert_eq!(doc["hooks"]["Stop"].as_array().unwrap().len(), 2);

        let mut back = doc;
        remove(Spec::of(AgentKind::Claude), &mut back).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn a_moved_binary_is_outdated_and_rewritten_in_place_of_the_old_entry() {
        let old = |event: HookEventKind| format!("'/old/agent-pinball' hook claude {event}");
        let mut doc = parse(Some(FOREIGN)).unwrap();
        install(Spec::of(AgentKind::Claude), &mut doc, old).unwrap();
        assert_eq!(
            state(Spec::of(AgentKind::Claude), &doc, cmd),
            HookState::Outdated
        );

        install(Spec::of(AgentKind::Claude), &mut doc, cmd).unwrap();
        assert_eq!(
            state(Spec::of(AgentKind::Claude), &doc, cmd),
            HookState::Installed
        );
        let rendered = render(&doc);
        assert!(!rendered.contains("/old/"), "{rendered}");
        assert!(rendered.contains("notify-send done"));
    }

    #[test]
    fn a_partial_install_is_outdated() {
        let mut doc = installed(AgentKind::Codex, None);
        doc["hooks"]
            .as_object_mut()
            .unwrap()
            .shift_remove("PreCompact");
        assert_eq!(
            state(Spec::of(AgentKind::Codex), &doc, cmd),
            HookState::Outdated
        );
    }

    #[test]
    fn removing_from_a_shared_group_keeps_the_group() {
        let text = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"keep.sh"}},{{"type":"command","command":"{}"}}]}}]}}}}"#,
            cmd(HookEventKind::Stop).replace('\'', "")
        );
        let mut doc = parse(Some(&text)).unwrap();
        remove(Spec::of(AgentKind::Claude), &mut doc).unwrap();
        assert_eq!(
            doc["hooks"]["Stop"],
            json!([{ "hooks": [{ "type": "command", "command": "keep.sh" }] }])
        );
    }

    #[rstest]
    #[case("not json")]
    #[case("[1, 2]")]
    #[case("{\"hooks\": 3}")]
    #[case("{\"hooks\": {\"Stop\": {}}}")]
    fn malformed_configs_are_refused(#[case] text: &str) {
        let refused = parse(Some(text))
            .and_then(|mut doc| install(Spec::of(AgentKind::Claude), &mut doc, cmd));
        assert!(refused.is_err(), "{text}");
    }
}
