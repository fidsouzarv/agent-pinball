//! The `setup` conversation in the terminal: show what was found and what
//! would change, then write only after a clear yes (ADR 0006).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use pinball_core::{AgentKind, HookState};
use pinball_setup::{Action, Plan, SetupEnv};

use crate::{Config, SetupArgs};

/// Why setup cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupEnvError(&'static str);

impl std::fmt::Display for SetupEnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is unknown: set HOME or {}", self.0, self.0)
    }
}

impl std::error::Error for SetupEnvError {}

/// Builds the setup environment from config and this binary's path.
pub fn setup_env(config: &Config, executable: PathBuf) -> Result<SetupEnv, SetupEnvError> {
    let need = |value: &Option<PathBuf>, name| value.clone().ok_or(SetupEnvError(name));
    Ok(SetupEnv {
        executable,
        state_dir: need(&config.state_dir, "AGENT_PINBALL_STATE_DIR")?,
        path_dirs: config.path_dirs.clone(),
        claude_root: need(&config.claude_root, "AGENT_PINBALL_CLAUDE_ROOT")?,
        codex_root: need(&config.codex_root, "AGENT_PINBALL_CODEX_ROOT")?,
        cursor_root: need(&config.cursor_home, "HOME")?,
    })
}

/// The user's terminal, injectable so tests drive the conversation.
pub struct Terminal<R, W> {
    /// Where answers come from.
    pub input: R,
    /// Where the summary and diffs go.
    pub output: W,
    /// Whether a person can answer. Without one, nothing is written unless
    /// `--yes` was given.
    pub interactive: bool,
}

/// Runs the install or remove conversation. Returns an error message when
/// setup refused or failed; files are untouched unless the user agreed.
pub fn converse<R: BufRead, W: Write>(
    args: &SetupArgs,
    env: &SetupEnv,
    term: &mut Terminal<R, W>,
    now_ms: u64,
) -> Result<(), String> {
    let io_err = |err: io::Error| err.to_string();
    let action = if args.remove {
        Action::Remove
    } else {
        Action::Install
    };
    print_summary(env, &mut term.output).map_err(io_err)?;
    let plans = pinball_setup::plan(env, action, &args.only);
    if plans.is_empty() {
        writeln!(term.output, "\nNothing to change.").map_err(io_err)?;
        return Ok(());
    }
    for plan in &plans {
        write!(term.output, "\n{}", plan.diff()).map_err(io_err)?;
    }
    if !args.yes {
        if !term.interactive {
            return Err("not a terminal: review the changes above, then rerun with --yes".into());
        }
        if !ask(term, &plans).map_err(io_err)? {
            writeln!(term.output, "Nothing written.").map_err(io_err)?;
            return Ok(());
        }
    }
    let applied = pinball_setup::apply(&plans, now_ms).map_err(|err| err.to_string())?;
    for done in applied {
        let backup = done
            .backup
            .map(|b| format!(" (backup: {})", b.display()))
            .unwrap_or_default();
        writeln!(term.output, "wrote {}{backup}", done.path.display()).map_err(io_err)?;
    }
    if action == Action::Install && plans.iter().any(|p| p.kind == AgentKind::Codex) {
        writeln!(
            term.output,
            "Codex: open Codex and run /hooks to trust the new hooks."
        )
        .map_err(io_err)?;
    }
    Ok(())
}

fn ask<R: BufRead, W: Write>(term: &mut Terminal<R, W>, plans: &[Plan]) -> io::Result<bool> {
    let files = if plans.len() == 1 { "file" } else { "files" };
    write!(
        term.output,
        "\nWrite these changes to {} {files}? [y/N] ",
        plans.len()
    )?;
    term.output.flush()?;
    let mut answer = String::new();
    term.input.read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

fn print_summary(env: &SetupEnv, out: &mut impl Write) -> io::Result<()> {
    let report = pinball_setup::report(env);
    writeln!(out, "agent-pinball setup\n")?;
    for agent in &report.agents {
        let found = if agent.root_found {
            agent.root.clone()
        } else {
            format!("{} (not found)", agent.root)
        };
        let hooks = match &agent.hooks {
            HookState::Missing => "hooks missing".to_owned(),
            HookState::Installed => "hooks installed".to_owned(),
            HookState::Outdated => "hooks outdated".to_owned(),
            HookState::Invalid { reason } => format!("left alone: {reason}"),
        };
        writeln!(out, "  {:<7} {found:<40} {hooks}", agent.kind.as_str())?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;

    fn env() -> (tempfile::TempDir, SetupEnv) {
        let home = tempfile::tempdir().unwrap();
        for dir in [".claude", ".codex", ".cursor"] {
            fs::create_dir_all(home.path().join(dir)).unwrap();
        }
        let env = SetupEnv {
            executable: home.path().join("agent-pinball"),
            state_dir: home.path().join("state"),
            path_dirs: Vec::new(),
            claude_root: home.path().join(".claude"),
            codex_root: home.path().join(".codex"),
            cursor_root: home.path().join(".cursor"),
        };
        (home, env)
    }

    fn run(
        args: &SetupArgs,
        env: &SetupEnv,
        answer: &str,
        interactive: bool,
    ) -> (Result<(), String>, String) {
        let mut term = Terminal {
            input: answer.as_bytes(),
            output: Vec::new(),
            interactive,
        };
        let result = converse(args, env, &mut term, 1);
        (result, String::from_utf8(term.output).unwrap())
    }

    fn written(env: &SetupEnv) -> bool {
        env.claude_root.join("settings.json").exists()
    }

    // Adversarial: piped into another tool, setup must not write by default.
    #[test]
    fn no_terminal_and_no_yes_writes_nothing() {
        let (_home, env) = env();
        let (result, out) = run(&SetupArgs::default(), &env, "y\n", false);
        assert!(result.unwrap_err().contains("--yes"));
        assert!(out.contains("+++"), "the diff is still shown");
        assert!(!written(&env));
    }

    #[test]
    fn declining_writes_nothing() {
        let (_home, env) = env();
        for answer in ["\n", "n\n", "yep\n", ""] {
            let (result, out) = run(&SetupArgs::default(), &env, answer, true);
            assert!(result.is_ok());
            assert!(out.contains("Nothing written."), "{answer:?}");
            assert!(!written(&env), "{answer:?}");
        }
    }

    #[test]
    fn yes_answer_writes_and_reports_codex_trust() {
        let (_home, env) = env();
        let (result, out) = run(&SetupArgs::default(), &env, "y\n", true);
        assert!(result.is_ok(), "{result:?}");
        assert!(written(&env));
        assert!(out.contains("/hooks"));
        let (_, again) = run(&SetupArgs::default(), &env, "", true);
        assert!(again.contains("Nothing to change."));
    }

    #[test]
    fn yes_flag_needs_no_terminal_and_remove_undoes() {
        let (_home, env) = env();
        let yes = SetupArgs {
            yes: true,
            ..SetupArgs::default()
        };
        assert!(run(&yes, &env, "", false).0.is_ok());
        let remove = SetupArgs {
            yes: true,
            remove: true,
            ..SetupArgs::default()
        };
        assert!(run(&remove, &env, "", false).0.is_ok());
        let settings = fs::read_to_string(env.claude_root.join("settings.json")).unwrap();
        assert!(!settings.contains("agent-pinball"), "{settings}");
    }

    #[test]
    fn setup_env_needs_home_or_explicit_roots() {
        let config = Config::from_lookup(|_| None).unwrap();
        assert!(setup_env(&config, "/x".into()).is_err());
        let config = Config::from_lookup(|key| (key == "HOME").then(|| "/h".to_owned())).unwrap();
        let env = setup_env(&config, "/x".into()).unwrap();
        assert_eq!(env.cursor_root, PathBuf::from("/h/.cursor"));
    }
}
