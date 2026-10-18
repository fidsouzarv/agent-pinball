//! Setup against a fake home in a temp dir: never the real one.

use std::fs;
use std::path::Path;

use pinball_core::{AgentKind, AgentReport, HookEventKind, HookState, SetupReport};
use tempfile::TempDir;

use crate::{Action, ApplyError, SetupEnv, agent_prompt, apply, plan, report, spool};

const CLAUDE_SETTINGS: &str = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
  }
}
"#;

/// A home with all three agents, Claude already holding a foreign hook.
fn home() -> (TempDir, SetupEnv) {
    let home = tempfile::tempdir().expect("temp home");
    let root = home.path();
    for dir in [".claude", ".codex", ".cursor", "bin"] {
        fs::create_dir_all(root.join(dir)).expect("mkdir");
    }
    fs::write(root.join(".claude/settings.json"), CLAUDE_SETTINGS).expect("seed");
    fs::write(root.join("bin/claude"), "").expect("fake binary");
    let env = SetupEnv {
        executable: root.join("bin/agent-pinball"),
        state_dir: root.join("state"),
        path_dirs: vec![root.join("bin")],
        claude_root: root.join(".claude"),
        codex_root: root.join(".codex"),
        cursor_root: root.join(".cursor"),
    };
    (home, env)
}

fn agent(report: &SetupReport, kind: AgentKind) -> &AgentReport {
    report
        .agents
        .iter()
        .find(|agent| agent.kind == kind)
        .expect("every agent is reported")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read")
}

#[test]
fn install_writes_all_three_with_a_backup_of_the_existing_file() {
    let (home, env) = home();
    let plans = plan(&env, Action::Install, &[]);
    assert_eq!(plans.len(), 3);
    let applied = apply(&plans, 42).expect("apply");

    let claude = home.path().join(".claude/settings.json");
    let backup_of = |path: &Path| {
        applied
            .iter()
            .find(|applied| applied.path == path)
            .expect("applied")
            .backup
            .clone()
    };
    let backup = backup_of(&claude).expect("backup of existing file");
    assert!(backup.ends_with("settings.json.agent-pinball-bak-42"));
    assert_eq!(read(&backup), CLAUDE_SETTINGS);
    assert!(read(&claude).contains("say done"));
    assert!(
        backup_of(&home.path().join(".codex/hooks.json")).is_none(),
        "codex had no file to back up"
    );

    for agent in report(&env).agents {
        assert_eq!(agent.hooks, HookState::Installed, "{:?}", agent.kind);
        assert_eq!(agent.pending_diff, None);
    }
    assert!(plan(&env, Action::Install, &[]).is_empty(), "idempotent");
}

#[test]
fn remove_restores_the_users_entries_and_drops_ours() {
    let (home, env) = home();
    apply(&plan(&env, Action::Install, &[]), 1).expect("install");
    apply(&plan(&env, Action::Remove, &[]), 2).expect("remove");
    let claude = read(&home.path().join(".claude/settings.json"));
    assert!(claude.contains("say done"));
    assert!(!claude.contains("agent-pinball"), "{claude}");
    for agent in report(&env).agents {
        assert_eq!(agent.hooks, HookState::Missing, "{:?}", agent.kind);
    }
}

#[test]
fn only_limits_the_plan() {
    let (_home, env) = home();
    let plans = plan(&env, Action::Install, &[AgentKind::Codex]);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].kind, AgentKind::Codex);
}

#[test]
fn an_agent_without_a_root_is_skipped_and_never_created() {
    let (home, env) = home();
    fs::remove_dir(home.path().join(".cursor")).expect("rmdir");
    let plans = plan(&env, Action::Install, &[]);
    assert!(plans.iter().all(|plan| plan.kind != AgentKind::Cursor));
    apply(&plans, 1).expect("apply");
    assert!(!home.path().join(".cursor").exists());
    assert!(!agent(&report(&env), AgentKind::Cursor).root_found);
}

// Adversarial: a broken config must be reported, never overwritten.
#[test]
fn invalid_config_is_left_untouched() {
    let (home, env) = home();
    let codex = home.path().join(".codex/hooks.json");
    fs::write(&codex, "{ not json").expect("seed");
    let plans = plan(&env, Action::Install, &[]);
    assert!(plans.iter().all(|plan| plan.kind != AgentKind::Codex));
    apply(&plans, 1).expect("apply");
    assert_eq!(read(&codex), "{ not json");
    assert!(matches!(
        agent(&report(&env), AgentKind::Codex).hooks,
        HookState::Invalid { .. }
    ));
}

// Adversarial: the user approved a diff against what they saw; a file edited
// in between must not be overwritten with changes they never reviewed.
#[test]
fn a_file_changed_after_the_plan_is_not_written() {
    let (home, env) = home();
    let plans = plan(&env, Action::Install, &[AgentKind::Claude]);
    let claude = home.path().join(".claude/settings.json");
    fs::write(&claude, "{\"edited\": true}\n").expect("concurrent edit");
    let err = apply(&plans, 1).expect_err("must refuse");
    assert!(matches!(err, ApplyError::ChangedSincePlan(_)));
    assert_eq!(read(&claude), "{\"edited\": true}\n");
}

#[cfg(unix)]
#[test]
fn permissions_survive_the_rewrite() {
    use std::os::unix::fs::PermissionsExt;
    let (home, env) = home();
    let claude = home.path().join(".claude/settings.json");
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o600)).expect("chmod");
    apply(&plan(&env, Action::Install, &[AgentKind::Claude]), 1).expect("apply");
    let mode = fs::metadata(&claude).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn hook_command_quotes_paths_with_spaces_and_quotes() {
    let (_home, mut env) = home();
    env.executable = "/Apps/My Tools/it's/agent-pinball".into();
    let command = env.hook_command(AgentKind::Claude, HookEventKind::Stop);
    assert!(command.starts_with(r"'/Apps/My Tools/it'\''s/agent-pinball' hook --state-dir '"));
    assert!(command.ends_with("' claude stop"));
}

#[test]
fn report_shows_binary_and_last_event() {
    let (home, env) = home();
    let record =
        spool::record(AgentKind::Claude, HookEventKind::Stop, b"{}", 77).expect("claude payload");
    spool::append(&env.state_dir, &record).expect("spool");
    let report = report(&env);
    let claude = agent(&report, AgentKind::Claude);
    assert_eq!(
        claude.binary,
        Some(home.path().join("bin/claude").display().to_string())
    );
    assert_eq!(claude.last_event.as_ref().map(|e| e.at_ms), Some(77));
    assert!(claude.pending_diff.as_deref().unwrap_or("").contains("+"));
    let codex = agent(&report, AgentKind::Codex);
    assert!(codex.note.as_deref().unwrap_or("").contains("/hooks"));
}

#[test]
fn agent_prompt_carries_diffs_commands_and_json() {
    let (_home, env) = home();
    let prompt = agent_prompt(&report(&env));
    assert!(prompt.contains("```diff"));
    assert!(prompt.contains("setup --yes"));
    assert!(prompt.contains("setup --remove --yes"));
    assert!(prompt.contains("\"agents\""));
}

// Adversarial: the report feeds the setup page and the brief pasted into
// another agent. A config whose formatting differs from ours would make a
// text diff show the whole file, secrets included; only agent-pinball's
// own entries may appear.
#[test]
fn report_never_carries_other_config_text() {
    let (home, env) = home();
    let secret = "sk-ant-FAKE0123456789abcdef";
    fs::write(
        home.path().join(".claude/settings.json"),
        format!("{{\"env\":{{\"ANTHROPIC_API_KEY\":\"{secret}\"}},\"hooks\":{{\"Stop\":[{{\"hooks\":[{{\"type\":\"command\",\"command\":\"notify --token {secret}\"}}]}}]}}}}"),
    )
    .expect("seed");

    let report = report(&env);
    let diff = agent(&report, AgentKind::Claude)
        .pending_diff
        .clone()
        .expect("install pending");
    assert!(!diff.contains("FAKE"), "{diff}");
    assert!(diff.contains("+hooks.Stop: '"), "{diff}");
    assert!(
        diff.lines().skip(2).all(|line| line.starts_with('+')),
        "{diff}"
    );
    assert!(!agent_prompt(&report).contains("FAKE"));

    // The full diff is for the user's own terminal only.
    let plans = plan(&env, Action::Install, &[AgentKind::Claude]);
    assert!(plans[0].diff().contains("FAKE"));
}
