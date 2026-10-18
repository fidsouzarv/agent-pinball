//! Settings, read once at startup and passed down. Nothing else reads the
//! environment (AGENTS.md: one config-read path).

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Loopback by default: the panel has no authentication.
pub const DEFAULT_BIND: &str = "127.0.0.1:4242";
/// `Host` values accepted by default (DNS-rebinding guard).
pub const DEFAULT_ALLOWED_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Resolved settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Address `serve` binds and `status` probes.
    pub bind: SocketAddr,
    /// Accepted `Host` header values, without port.
    pub allowed_hosts: Vec<String>,
    /// Running inside a container, where a wildcard bind is a namespace
    /// requirement rather than network exposure.
    pub in_container: bool,
    /// Claude Code root (`AGENT_PINBALL_CLAUDE_ROOT`, default `$HOME/.claude`).
    pub claude_root: Option<PathBuf>,
    /// Codex root (`AGENT_PINBALL_CODEX_ROOT`, default `$HOME/.codex`).
    pub codex_root: Option<PathBuf>,
    /// Cursor's dot directory, where its hooks live (`$HOME/.cursor`).
    pub cursor_home: Option<PathBuf>,
    /// agent-pinball's own state (`AGENT_PINBALL_STATE_DIR`, default
    /// `$HOME/.local/state/agent-pinball`).
    pub state_dir: Option<PathBuf>,
    /// `PATH`, split, for finding agent binaries.
    pub path_dirs: Vec<PathBuf>,
}

/// Invalid setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `AGENT_PINBALL_BIND` is not a socket address.
    InvalidBind(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBind(value) => write!(f, "invalid AGENT_PINBALL_BIND: {value:?}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Builds the config from an environment lookup. Injectable, so tests never
    /// touch the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind_raw = lookup("AGENT_PINBALL_BIND").unwrap_or_else(|| DEFAULT_BIND.to_owned());
        let bind = bind_raw
            .parse()
            .map_err(|_| ConfigError::InvalidBind(bind_raw.clone()))?;
        let allowed_hosts = lookup("AGENT_PINBALL_ALLOWED_HOSTS").map_or_else(
            || DEFAULT_ALLOWED_HOSTS.map(str::to_owned).to_vec(),
            |raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|host| !host.is_empty())
                    .map(str::to_owned)
                    .collect()
            },
        );
        let in_container = lookup("AGENT_PINBALL_IN_CONTAINER")
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        // A missing HOME only matters to `setup`, which reports it; `serve`
        // and `status` must keep working without one.
        let home = lookup("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from);
        let path_or_home = |key: &str, under_home: &str| {
            lookup(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| home.join(under_home)))
        };
        let path_dirs = lookup("PATH")
            .map(|raw| std::env::split_paths(&raw).collect())
            .unwrap_or_default();
        Ok(Self {
            bind,
            allowed_hosts,
            in_container,
            claude_root: path_or_home("AGENT_PINBALL_CLAUDE_ROOT", ".claude"),
            codex_root: path_or_home("AGENT_PINBALL_CODEX_ROOT", ".codex"),
            cursor_home: home.as_ref().map(|home| home.join(".cursor")),
            state_dir: path_or_home("AGENT_PINBALL_STATE_DIR", ".local/state/agent-pinball"),
            path_dirs,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn defaults_are_loopback_only() {
        let config = Config::from_lookup(lookup(&[])).unwrap();
        assert_eq!(config.bind, DEFAULT_BIND.parse().unwrap());
        assert!(config.bind.ip().is_loopback());
        assert_eq!(
            config.allowed_hosts,
            DEFAULT_ALLOWED_HOSTS.map(str::to_owned)
        );
        assert!(!config.in_container);
    }

    #[test]
    fn environment_overrides() {
        let config = Config::from_lookup(lookup(&[
            ("AGENT_PINBALL_BIND", "0.0.0.0:4242"),
            (
                "AGENT_PINBALL_ALLOWED_HOSTS",
                " localhost , host.docker.internal ,,",
            ),
            ("AGENT_PINBALL_IN_CONTAINER", "1"),
        ]))
        .unwrap();
        assert_eq!(config.bind, "0.0.0.0:4242".parse().unwrap());
        assert_eq!(config.allowed_hosts, ["localhost", "host.docker.internal"]);
        assert!(config.in_container);
    }

    #[test]
    fn agent_roots_default_under_home() {
        let config = Config::from_lookup(lookup(&[
            ("HOME", "/home/u"),
            ("PATH", "/usr/bin:/opt/bin"),
        ]))
        .unwrap();
        assert_eq!(config.claude_root, Some(PathBuf::from("/home/u/.claude")));
        assert_eq!(config.codex_root, Some(PathBuf::from("/home/u/.codex")));
        assert_eq!(config.cursor_home, Some(PathBuf::from("/home/u/.cursor")));
        assert_eq!(
            config.state_dir,
            Some(PathBuf::from("/home/u/.local/state/agent-pinball"))
        );
        assert_eq!(
            config.path_dirs,
            ["/usr/bin", "/opt/bin"].map(PathBuf::from)
        );
    }

    #[test]
    fn agent_roots_can_be_overridden_and_need_no_home() {
        let config = Config::from_lookup(lookup(&[
            ("AGENT_PINBALL_CLAUDE_ROOT", "/c"),
            ("AGENT_PINBALL_CODEX_ROOT", "/x"),
            ("AGENT_PINBALL_STATE_DIR", "/s"),
        ]))
        .unwrap();
        assert_eq!(config.claude_root, Some(PathBuf::from("/c")));
        assert_eq!(config.codex_root, Some(PathBuf::from("/x")));
        assert_eq!(config.state_dir, Some(PathBuf::from("/s")));
        assert_eq!(config.cursor_home, None);
        assert!(config.path_dirs.is_empty());
    }

    #[test]
    fn invalid_bind_is_an_error() {
        let err = Config::from_lookup(lookup(&[("AGENT_PINBALL_BIND", "nope")])).unwrap_err();
        assert_eq!(err, ConfigError::InvalidBind("nope".to_owned()));
    }

    #[test]
    fn in_container_needs_an_explicit_true() {
        for value in ["0", "", "yes"] {
            let config =
                Config::from_lookup(lookup(&[("AGENT_PINBALL_IN_CONTAINER", value)])).unwrap();
            assert!(!config.in_container, "{value:?}");
        }
    }
}
