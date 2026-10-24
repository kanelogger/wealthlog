//! `mcp.lock` discovery file (v2).
//!
//! Written to the app data directory when the embedded service starts and
//! updated on ledger changes and locks. It is a discovery hint only — it never
//! contains a credential. Replaced atomically with mode `0600`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LOCK_FILE_NAME: &str = "mcp.lock";
pub const LOCK_FILE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpLockFile {
    pub lock_file_version: u32,
    /// Random per-listener id; distinguishes a restarted instance from a
    /// stale file left by an earlier one.
    pub instance_id: String,
    pub port: u16,
    pub pid: u32,
    /// RFC3339 timestamp of when the server started.
    pub started_at: String,
    /// Ledger currently served, or `null` while no runtime is attached.
    pub active_ledger_id: Option<String>,
    /// `"ready"` or `"locked"`.
    pub status: String,
    /// Unix-domain socket used by the CLI.
    pub socket_path: String,
}

fn lock_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(LOCK_FILE_NAME)
}

pub fn write(app_data_dir: &Path, lock: &McpLockFile) -> io::Result<()> {
    let path = lock_path(app_data_dir);
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "lock path has no parent"))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp = directory.join(format!(".mcp.lock.{}.{}.tmp", std::process::id(), nonce));
    let result = (|| -> io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        let json = serde_json::to_vec_pretty(lock)?;
        file.write_all(&json)?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if result.is_err() {
        // Cover every failure path, not only rename: a leftover temp file must
        // not make every later publication fail.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Removes the lock file; missing files are not an error.
pub fn remove(app_data_dir: &Path) -> io::Result<()> {
    match std::fs::remove_file(lock_path(app_data_dir)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> McpLockFile {
        McpLockFile {
            lock_file_version: LOCK_FILE_VERSION,
            instance_id: "instance-1".to_string(),
            port: 8639,
            pid: 12345,
            started_at: "2026-05-17T00:00:00Z".to_string(),
            active_ledger_id: Some("ledger-a".to_string()),
            status: "ready".to_string(),
            socket_path: "/tmp/wl-1/mcp.sock".to_string(),
        }
    }

    #[test]
    fn writes_camel_case_v2_atomically_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &sample()).unwrap();

        let raw = std::fs::read_to_string(dir.path().join(LOCK_FILE_NAME)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["lockFileVersion"], 2);
        assert_eq!(value["instanceId"], "instance-1");
        assert_eq!(value["port"], 8639);
        assert_eq!(value["activeLedgerId"], "ledger-a");
        assert_eq!(value["status"], "ready");
        assert_eq!(value["socketPath"], "/tmp/wl-1/mcp.sock");

        let parsed: McpLockFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.active_ledger_id.as_deref(), Some("ledger-a"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(LOCK_FILE_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no temp file remains");
    }

    #[test]
    fn remove_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        remove(dir.path()).unwrap();
        write(dir.path(), &sample()).unwrap();
        remove(dir.path()).unwrap();
        assert!(!dir.path().join(LOCK_FILE_NAME).exists());
        remove(dir.path()).unwrap();
    }
}
