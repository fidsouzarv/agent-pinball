//! The only code that writes into agent files (ADR 0006): backup, then an
//! atomic replace that keeps the file's permissions.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::{Plan, read_config};

/// A file that was rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The config file.
    pub path: PathBuf,
    /// Copy of the previous version, when there was one.
    pub backup: Option<PathBuf>,
}

/// Why a plan was not applied. Files before the failing one stay applied,
/// each with its backup.
#[derive(Debug)]
pub enum ApplyError {
    /// The file changed after the plan was shown; the user must review again.
    ChangedSincePlan(PathBuf),
    /// Reading, backing up or writing failed.
    Io(PathBuf, io::Error),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChangedSincePlan(path) => write!(
                f,
                "{} changed after the plan was shown; run setup again to review it",
                path.display()
            ),
            Self::Io(path, err) => write!(f, "{}: {err}", path.display()),
        }
    }
}

impl std::error::Error for ApplyError {}

/// Writes every plan. `now_ms` names the backups, so the clock stays with
/// the caller.
pub fn apply(plans: &[Plan], now_ms: u64) -> Result<Vec<Applied>, ApplyError> {
    plans.iter().map(|plan| apply_one(plan, now_ms)).collect()
}

fn apply_one(plan: &Plan, now_ms: u64) -> Result<Applied, ApplyError> {
    let path = &plan.path;
    let io_err = |err| ApplyError::Io(path.clone(), err);
    // What the user approved is the diff against `before`; anything else
    // would write changes they never saw.
    let current = read_config(path).map_err(|reason| io_err(io::Error::other(reason)))?;
    if current != plan.before {
        return Err(ApplyError::ChangedSincePlan(path.clone()));
    }
    let backup = match plan.before {
        Some(_) => {
            let backup = backup_path(path, now_ms);
            fs::copy(path, &backup).map_err(io_err)?;
            Some(backup)
        }
        None => None,
    };
    write_atomic(path, plan.after.as_bytes()).map_err(io_err)?;
    Ok(Applied {
        path: path.clone(),
        backup,
    })
}

fn backup_path(path: &Path, now_ms: u64) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".agent-pinball-bak-{now_ms}"));
    path.with_file_name(name)
}

/// A temp file in the same directory, renamed over the target, so the agent
/// never reads a half-written config.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(temp.path(), meta.permissions())?;
    }
    temp.persist(path).map_err(|err| err.error)?;
    Ok(())
}
