//! Command-line arguments. Std-only until the surface outgrows it.

use std::fmt;
use std::net::SocketAddr;

/// Help text printed by `--help`.
pub const USAGE: &str = "\
agent-troops: monitoring panel for Codex, Cursor and Claude agents

Usage:
  agent-troops serve [--bind ADDR]   serve the panel (default 127.0.0.1:4242)
  agent-troops status                probe /healthz on the configured bind
  agent-troops --version
  agent-troops --help

Environment:
  AGENT_TROOPS_BIND            bind address (default 127.0.0.1:4242)
  AGENT_TROOPS_ALLOWED_HOSTS   comma-separated Host allowlist
  AGENT_TROOPS_IN_CONTAINER    1 inside a container (allows a wildcard bind)
";

/// What the user asked the binary to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Serve the panel, optionally overriding the configured bind.
    Serve {
        /// `--bind` value, when given.
        bind: Option<SocketAddr>,
    },
    /// Probe `/healthz`.
    Status,
    /// Print the version.
    Version,
    /// Print usage.
    Help,
}

/// Invalid command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgsError(String);

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (see --help)", self.0)
    }
}

impl std::error::Error for ArgsError {}

/// Parses arguments, without the program name.
pub fn parse_args<I>(args: I) -> Result<Command, ArgsError>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Help);
    };
    let command = match first.as_str() {
        "-h" | "--help" | "help" => Command::Help,
        "-V" | "--version" => Command::Version,
        "status" => Command::Status,
        "serve" => Command::Serve {
            bind: parse_serve_flags(&mut args)?,
        },
        other => return Err(ArgsError(format!("unknown command: {other}"))),
    };
    match args.next() {
        Some(extra) => Err(ArgsError(format!("unexpected argument: {extra}"))),
        None => Ok(command),
    }
}

fn parse_serve_flags(
    args: &mut impl Iterator<Item = String>,
) -> Result<Option<SocketAddr>, ArgsError> {
    let mut bind = None;
    while let Some(flag) = args.next() {
        if flag != "--bind" {
            return Err(ArgsError(format!("unexpected argument: {flag}")));
        }
        let value = args
            .next()
            .ok_or_else(|| ArgsError("--bind needs an address".to_owned()))?;
        let addr = value
            .parse()
            .map_err(|_| ArgsError(format!("invalid --bind address: {value}")))?;
        bind = Some(addr);
    }
    Ok(bind)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, ArgsError> {
        parse_args(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse(&[]).unwrap(), Command::Help);
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
        assert_eq!(parse(&["--version"]).unwrap(), Command::Version);
        assert_eq!(parse(&["-h"]).unwrap(), Command::Help);
    }

    #[test]
    fn invalid_input_is_rejected() {
        assert!(parse(&["deploy"]).is_err());
        assert!(parse(&["status", "extra"]).is_err());
        assert!(parse(&["serve", "--bind"]).is_err());
        assert!(parse(&["serve", "--bind", "localhost"]).is_err());
        assert!(parse(&["serve", "--port", "1"]).is_err());
    }
}
