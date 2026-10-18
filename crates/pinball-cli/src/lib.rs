//! Argument parsing, configuration and the setup conversation for the
//! `agent-pinball` binary.
//!
//! Kept in a library so it is unit-tested; `main.rs` only wires these together.

mod args;
mod config;
mod setup;

pub use args::{Command, SetupArgs, parse_args, usage};
pub use config::{Config, ConfigError, DEFAULT_ALLOWED_HOSTS, DEFAULT_BIND};
pub use setup::{SetupEnvError, Terminal, converse, setup_env};
