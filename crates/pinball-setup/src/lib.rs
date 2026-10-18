//! `agent-pinball setup`: finds Codex, Cursor and Claude, and adds or removes
//! agent-pinball's hook entries in their configs.
//!
//! This is the one crate allowed to write into agent files (ADR 0006). Every
//! write goes through [`apply`], after the caller showed the [`Plan`] diff
//! and got the user's confirmation.

mod agent_prompt;
mod apply;
mod hooks;
pub mod spool;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use pinball_core::{AgentKind, AgentReport, HookEventKind, HookState, SetupReport};

pub use agent_prompt::agent_prompt;
pub use apply::{Applied, ApplyError, apply};

/// Largest agent config file setup will read. Real ones are a few KB.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Where everything lives, resolved once by the CLI from its config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupEnv {
    /// The `agent-pinball` binary the hooks will call (absolute).
    pub executable: PathBuf,
    /// agent-pinball's state directory, where hook events are spooled.
    pub state_dir: PathBuf,
    /// Directories searched for agent binaries, in `PATH` order.
    pub path_dirs: Vec<PathBuf>,
    /// Claude Code's root (`~/.claude`).
    pub claude_root: PathBuf,
    /// Codex's root (`~/.codex`).
    pub codex_root: PathBuf,
    /// Cursor's dot directory (`~/.cursor`), where its hooks live.
    pub cursor_root: PathBuf,
}

impl SetupEnv {
    fn root(&self, kind: AgentKind) -> &Path {
        match kind {
            AgentKind::Claude => &self.claude_root,
            AgentKind::Codex => &self.codex_root,
            AgentKind::Cursor => &self.cursor_root,
        }
    }

    fn config_path(&self, kind: AgentKind) -> PathBuf {
        self.root(kind).join(hooks::Spec::of(kind).file)
    }

    /// The shell command an agent runs for `event`. The state directory is
    /// spelled out because agents do not pass agent-pinball's environment on.
    #[must_use]
    pub fn hook_command(&self, kind: AgentKind, event: HookEventKind) -> String {
        format!(
            "{} hook --state-dir {} {kind} {event}",
            shell_quote(&self.executable),
            shell_quote(&self.state_dir)
        )
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// What setup is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Add or rewrite agent-pinball's hooks.
    Install,
    /// Take them out again.
    Remove,
}

/// One file setup would rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Agent the file belongs to.
    pub kind: AgentKind,
    /// The config file.
    pub path: PathBuf,
    /// Its current text, `None` when it does not exist yet.
    pub before: Option<String>,
    /// The text it would have.
    pub after: String,
}

impl Plan {
    /// Unified diff of the whole file, for the user's own terminal. It can
    /// show any line of the agent's config, secrets included, so it never
    /// goes into the report: use [`Plan::own_changes`] there.
    #[must_use]
    pub fn diff(&self) -> String {
        let label = self.path.display().to_string();
        similar::TextDiff::from_lines(self.before.as_deref().unwrap_or(""), &self.after)
            .unified_diff()
            .header(&label, &label)
            .to_string()
    }

    /// Only agent-pinball's own entries that would be added or removed, as
    /// diff lines. Safe to show on the setup page or hand to another agent:
    /// nothing else from the config appears, whatever its formatting.
    #[must_use]
    pub fn own_changes(&self) -> String {
        let spec = hooks::Spec::of(self.kind);
        let before = hooks::parse(self.before.as_deref()).unwrap_or_default();
        let after = hooks::parse(Some(&self.after)).unwrap_or_default();
        let label = self.path.display();
        let mut out = format!("--- {label}\n+++ {label}\n");
        for line in hooks::own_changes(spec, &before, &after) {
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}

/// Reads an agent config with a size cap. `Ok(None)`: the file does not exist.
fn read_config(path: &Path) -> Result<Option<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("cannot read it ({err})")),
    };
    let mut text = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| format!("cannot read it ({err})"))?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err("it is larger than 1 MiB".to_owned());
    }
    Ok(Some(text))
}

/// The agent's current state and, when something would change, the plan.
fn inspect(env: &SetupEnv, kind: AgentKind, action: Action) -> (HookState, Option<Plan>) {
    let spec = hooks::Spec::of(kind);
    let path = env.config_path(kind);
    let command = |event| env.hook_command(kind, event);
    let parsed =
        read_config(&path).and_then(|before| Ok((hooks::parse(before.as_deref())?, before)));
    let (doc, before) = match parsed {
        Ok(parsed) => parsed,
        Err(reason) => return (HookState::Invalid { reason }, None),
    };
    let state = hooks::state(spec, &doc, command);
    let mut edited = doc.clone();
    let edit = match action {
        Action::Install => hooks::install(spec, &mut edited, command),
        Action::Remove => hooks::remove(spec, &mut edited),
    };
    if let Err(reason) = edit {
        return (HookState::Invalid { reason }, None);
    }
    let plan = (edited != doc).then(|| Plan {
        kind,
        path,
        before,
        after: hooks::render(&edited),
    });
    (state, plan)
}

/// Plans for `only` (every agent when empty). Agents whose root does not
/// exist are skipped: setup never creates an agent's directory.
#[must_use]
pub fn plan(env: &SetupEnv, action: Action, only: &[AgentKind]) -> Vec<Plan> {
    AgentKind::ALL
        .into_iter()
        .filter(|kind| only.is_empty() || only.contains(kind))
        .filter(|kind| env.root(*kind).is_dir())
        .filter_map(|kind| inspect(env, kind, action).1)
        .collect()
}

/// Full report, with the install diff each agent would get.
#[must_use]
pub fn report(env: &SetupEnv) -> SetupReport {
    let agents = AgentKind::ALL
        .into_iter()
        .map(|kind| agent_report(env, kind))
        .collect();
    SetupReport {
        executable: env.executable.display().to_string(),
        state_dir: env.state_dir.display().to_string(),
        agents,
    }
}

fn agent_report(env: &SetupEnv, kind: AgentKind) -> AgentReport {
    let root = env.root(kind);
    let root_found = root.is_dir();
    let (hooks, plan) = if root_found {
        inspect(env, kind, Action::Install)
    } else {
        (HookState::Missing, None)
    };
    let note = match kind {
        AgentKind::Codex if root_found => Some(
            "Codex skips new or changed hooks until you trust them: run /hooks in Codex."
                .to_owned(),
        ),
        _ => None,
    };
    AgentReport {
        kind,
        binary: find_binary(&env.path_dirs, kind).map(|path| path.display().to_string()),
        root: root.display().to_string(),
        root_found,
        config_path: env.config_path(kind).display().to_string(),
        hooks,
        pending_diff: plan.map(|plan| plan.own_changes()),
        last_event: spool::last_event(&env.state_dir, kind),
        note,
    }
}

/// First executable for `kind` on the search path. Only checks that the file
/// exists; agents are never run.
fn find_binary(path_dirs: &[PathBuf], kind: AgentKind) -> Option<PathBuf> {
    let names: &[&str] = match kind {
        AgentKind::Claude => &["claude"],
        AgentKind::Codex => &["codex"],
        AgentKind::Cursor => &["cursor-agent", "cursor"],
    };
    path_dirs
        .iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../tests/suite/mod.rs"]
mod suite;
