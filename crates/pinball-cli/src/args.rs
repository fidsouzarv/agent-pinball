//! Command-line arguments.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand};
use pinball_core::{AgentKind, HookEventKind};

const ENV_HELP: &str = "\
Environment:
  AGENT_PINBALL_BIND            bind address (default 127.0.0.1:4242)
  AGENT_PINBALL_ALLOWED_HOSTS   comma-separated Host allowlist
  AGENT_PINBALL_IN_CONTAINER    1 inside a container (allows a wildcard bind)
  AGENT_PINBALL_CLAUDE_ROOT     Claude Code root (default $HOME/.claude)
  AGENT_PINBALL_CODEX_ROOT      Codex root (default $HOME/.codex)
  AGENT_PINBALL_STATE_DIR       agent-pinball state (default $HOME/.local/state/agent-pinball)";

#[derive(Debug, Parser)]
#[command(
    name = "agent-pinball",
    version,
    about = "agent-pinball: monitoring panel for Codex, Cursor and Claude agents",
    after_help = ENV_HELP
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// What the user asked the binary to do.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Serve the panel (default 127.0.0.1:4242).
    Serve {
        /// Bind address, overriding AGENT_PINBALL_BIND.
        #[arg(long)]
        bind: Option<SocketAddr>,
    },
    /// Probe /healthz on the configured bind.
    Status,
    /// Find Codex, Cursor and Claude and install agent-pinball's hooks.
    Setup(SetupArgs),
    /// Record one agent hook event. Agents run this; people do not.
    #[command(hide = true)]
    Hook {
        /// agent-pinball state directory, spelled out by setup.
        #[arg(long)]
        state_dir: PathBuf,
        /// Agent running the hook.
        kind: AgentKind,
        /// Event that fired.
        event: HookEventKind,
    },
    /// Print usage. Chosen when no command is given.
    #[command(skip)]
    Help,
}

/// `setup` options. Without `--yes`, every change is shown and confirmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct SetupArgs {
    /// Also serve a live setup page; confirmation stays in this terminal.
    #[arg(long, conflicts_with = "agent")]
    pub web: bool,
    /// Bind address for --web, overriding AGENT_PINBALL_BIND.
    #[arg(long, requires = "web")]
    pub bind: Option<SocketAddr>,
    /// Print a Markdown brief for another coding agent. Writes nothing.
    #[arg(long, conflicts_with_all = ["yes", "remove"])]
    pub agent: bool,
    /// Apply without asking (after reviewing the changes).
    #[arg(long)]
    pub yes: bool,
    /// Remove agent-pinball's hooks instead of installing them.
    #[arg(long)]
    pub remove: bool,
    /// Limit to these agents (comma-separated: claude,codex,cursor).
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<AgentKind>,
}

/// Parses arguments, including the program name.
pub fn parse_args<I>(args: I) -> Result<Command, clap::Error>
where
    I: IntoIterator<Item = String>,
{
    Ok(Cli::try_parse_from(args)?.command.unwrap_or(Command::Help))
}

/// Full help text.
#[must_use]
pub fn usage() -> String {
    Cli::command().render_help().to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn parse(args: &[&str]) -> Result<Command, clap::Error> {
        parse_args(
            std::iter::once("agent-pinball")
                .chain(args.iter().copied())
                .map(str::to_owned),
        )
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse(&[]).unwrap(), Command::Help);
        assert!(usage().contains("setup"));
        assert!(usage().contains("AGENT_PINBALL_STATE_DIR"));
    }

    #[test]
    fn serve_with_and_without_bind() {
        assert_eq!(parse(&["serve"]).unwrap(), Command::Serve { bind: None });
        assert_eq!(
            parse(&["serve", "--bind", "127.0.0.1:9000"]).unwrap(),
            Command::Serve {
                bind: Some("127.0.0.1:9000".parse().unwrap())
            }
        );
    }

    #[test]
    fn simple_commands() {
        assert_eq!(parse(&["status"]).unwrap(), Command::Status);
        assert_eq!(
            parse(&["--version"]).unwrap_err().kind(),
            ErrorKind::DisplayVersion
        );
        assert_eq!(parse(&["-h"]).unwrap_err().kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn invalid_input_is_rejected() {
        assert!(parse(&["deploy"]).is_err());
        assert!(parse(&["status", "extra"]).is_err());
        assert!(parse(&["serve", "--bind"]).is_err());
        assert!(parse(&["serve", "--bind", "localhost"]).is_err());
        assert!(parse(&["serve", "--port", "1"]).is_err());
    }

    #[test]
    fn setup_flags() {
        assert_eq!(
            parse(&["setup"]).unwrap(),
            Command::Setup(SetupArgs::default())
        );
        let Command::Setup(args) = parse(&["setup", "--yes", "--only", "claude,codex"]).unwrap()
        else {
            panic!("not setup");
        };
        assert!(args.yes);
        assert_eq!(args.only, [AgentKind::Claude, AgentKind::Codex]);
    }

    #[test]
    fn contradictory_setup_flags_are_rejected() {
        assert!(parse(&["setup", "--agent", "--yes"]).is_err());
        assert!(parse(&["setup", "--agent", "--web"]).is_err());
        assert!(parse(&["setup", "--bind", "127.0.0.1:1"]).is_err());
        assert!(parse(&["setup", "--only", "copilot"]).is_err());
    }

    #[test]
    fn hook_takes_typed_kind_and_event() {
        assert_eq!(
            parse(&["hook", "--state-dir", "/s", "cursor", "pre_compact"]).unwrap(),
            Command::Hook {
                state_dir: "/s".into(),
                kind: AgentKind::Cursor,
                event: HookEventKind::PreCompact,
            }
        );
        assert!(parse(&["hook", "--state-dir", "/s", "claude", "user_prompt_submit"]).is_err());
    }
}
