//! `mcp.lock` discovery and Unix-socket validation.
//!
//! The CLI never derives the socket path and never accepts an endpoint
//! argument: it reads the app-published v2 file at the system application
//! support directory for the selected identifier, then verifies that the
//! socket sits in a private, current-user directory.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::CliError;

pub const PRODUCTION_APP_ID: &str = "local.kanelogger.wealthlog";
pub const DEVELOPMENT_APP_ID: &str = "local.kanelogger.wealthlog.dev";
pub const LOCK_FILE_NAME: &str = "mcp.lock";
const LOCK_FILE_VERSION: u32 = 2;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockFile {
    pub lock_file_version: u32,
    pub instance_id: String,
    pub port: u16,
    pub pid: u32,
    pub started_at: String,
    pub active_ledger_id: Option<String>,
    pub status: String,
    pub socket_path: String,
}

pub struct Discovery {
    pub lock: LockFile,
    pub socket: PathBuf,
}

pub fn validate_app_id(app_id: &str) -> Result<(), CliError> {
    if app_id == PRODUCTION_APP_ID || app_id == DEVELOPMENT_APP_ID {
        Ok(())
    } else {
        Err(CliError::usage(format!(
            "unknown --app-id '{app_id}'; expected {PRODUCTION_APP_ID} or {DEVELOPMENT_APP_ID}"
        )))
    }
}

pub fn lock_path(app_id: &str) -> Result<PathBuf, CliError> {
    let data = dirs::data_dir().ok_or_else(|| {
        CliError::unavailable("the system application support directory is unavailable")
    })?;
    Ok(data.join(app_id).join(LOCK_FILE_NAME))
}

pub fn load(app_id: &str) -> Result<Discovery, CliError> {
    validate_app_id(app_id)?;
    let lock_path = lock_path(app_id)?;
    let raw = match std::fs::read_to_string(&lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CliError::unavailable(format!(
                "no Wealthlog service is running (missing {})",
                lock_path.display()
            )))
        }
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot read {}: {error}",
                lock_path.display()
            )))
        }
    };
    let lock: LockFile = serde_json::from_str(&raw).map_err(|error| {
        CliError::unavailable(format!(
            "{} is not a valid discovery file: {error}",
            lock_path.display()
        ))
    })?;
    if lock.lock_file_version != LOCK_FILE_VERSION {
        return Err(CliError::unavailable(format!(
            "unsupported discovery file version {}",
            lock.lock_file_version
        )));
    }
    let socket = PathBuf::from(&lock.socket_path);
    validate_socket(&socket)?;
    Ok(Discovery { lock, socket })
}

#[cfg(unix)]
pub(crate) fn validate_socket(socket: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let metadata = std::fs::symlink_metadata(socket).map_err(|error| {
        CliError::unavailable(format!(
            "the published socket {} is unavailable: {error}",
            socket.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(CliError::unavailable(
            "the published socket is not a real socket",
        ));
    }
    if metadata.uid() != effective_uid() {
        return Err(CliError::unavailable(
            "the published socket is not owned by the current user",
        ));
    }
    if metadata.mode() & 0o777 != 0o600 {
        return Err(CliError::unavailable(format!(
            "the published socket mode is {:o}, expected 600",
            metadata.mode() & 0o777
        )));
    }
    let parent = socket
        .parent()
        .ok_or_else(|| CliError::unavailable("the published socket has no parent directory"))?;
    let parent_metadata = std::fs::symlink_metadata(parent).map_err(|error| {
        CliError::unavailable(format!("cannot inspect the socket directory: {error}"))
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(CliError::unavailable(
            "the socket directory is not a real directory",
        ));
    }
    if parent_metadata.uid() != effective_uid() {
        return Err(CliError::unavailable(
            "the socket directory is not owned by the current user",
        ));
    }
    if parent_metadata.mode() & 0o077 != 0 {
        return Err(CliError::unavailable(format!(
            "the socket directory mode is {:o}; it must be private",
            parent_metadata.mode() & 0o777
        )));
    }
    let parent = std::fs::canonicalize(parent).map_err(|error| {
        CliError::unavailable(format!("cannot resolve the socket directory: {error}"))
    })?;
    let name = parent
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !name.starts_with("wl-") || name.len() > 16 {
        return Err(CliError::unavailable(
            "the published socket directory is not a Wealthlog instance directory",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn validate_socket(_socket: &Path) -> Result<(), CliError> {
    Err(CliError::unavailable(
        "the CLI Unix-socket transport is only supported on Unix hosts",
    ))
}

#[cfg(unix)]
#[allow(unsafe_code, reason = "geteuid is a parameterless libc read")]
fn effective_uid() -> u32 {
    // SAFETY: parameterless libc read.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_is_an_exact_allowlist() {
        assert!(validate_app_id(PRODUCTION_APP_ID).is_ok());
        assert!(validate_app_id(DEVELOPMENT_APP_ID).is_ok());
        for value in [
            "",
            "../x",
            "local.kanelogger.wealthlog/evil",
            "local.kanelogger.wealthlog.devx",
        ] {
            assert!(validate_app_id(value).is_err(), "{value}");
        }
    }

    #[test]
    fn production_and_development_lock_paths_differ() {
        let production = lock_path(PRODUCTION_APP_ID).unwrap();
        let development = lock_path(DEVELOPMENT_APP_ID).unwrap();
        assert_ne!(production, development);
        assert!(production.ends_with(std::path::Path::new(PRODUCTION_APP_ID).join(LOCK_FILE_NAME)));
    }
}
