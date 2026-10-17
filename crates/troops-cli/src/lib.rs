//! Argument parsing and configuration for the `agent-troops` binary.
//!
//! Kept in a library so it is unit-tested; `main.rs` only wires these together.

mod args;
mod config;

pub use args::{ArgsError, Command, USAGE, parse_args};
pub use config::{Config, ConfigError, DEFAULT_ALLOWED_HOSTS, DEFAULT_BIND};
