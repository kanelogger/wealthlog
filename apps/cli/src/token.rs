//! Credential sources: a private token file or `WEALTHLOG_TOKEN`.
//!
//! The token is read without following links and validated on the opened file
//! descriptor. It is never echoed to stdout, stderr or help output.

use std::io::Read;
use std::path::Path;

use crate::error::CliError;

const MAX_TOKEN_BYTES: usize = 4096;

pub fn resolve(explicit_file: Option<&Path>) -> Result<Option<String>, CliError> {
    let environment = std::env::var_os("WEALTHLOG_TOKEN");
    if explicit_file.is_some() && environment.is_some() {
        return Err(CliError::usage(
            "--token-file and WEALTHLOG_TOKEN are mutually exclusive",
        ));
    }
    if let Some(path) = explicit_file {
        if path.as_os_str().is_empty() {
            return Err(CliError::usage("--token-file must not be empty"));
        }
        return read_file(path).map(Some);
    }
    match environment {
        None => Ok(None),
        Some(value) => {
            let value = value
                .into_string()
                .map_err(|_| CliError::usage("WEALTHLOG_TOKEN is not valid UTF-8"))?;
            normalize(&value).map(Some)
        }
    }
}

#[cfg(unix)]
fn read_file(path: &Path) -> Result<String, CliError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut file = options
        .open(path)
        .map_err(|error| CliError::usage(format!("cannot read the token file: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| CliError::usage(format!("cannot inspect the token file: {error}")))?;
    if !metadata.is_file() {
        return Err(CliError::usage("the token file must be a regular file"));
    }
    if metadata.uid() != effective_uid() {
        return Err(CliError::usage(
            "the token file must be owned by the current user",
        ));
    }
    let mode = metadata.mode() & 0o777;
    if mode != 0o400 && mode != 0o600 {
        return Err(CliError::usage(format!(
            "the token file mode is {mode:o}; only 0400 or 0600 are accepted"
        )));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_TOKEN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::usage(format!("cannot read the token file: {error}")))?;
    if bytes.len() > MAX_TOKEN_BYTES {
        return Err(CliError::usage(format!(
            "the token file exceeds {MAX_TOKEN_BYTES} bytes"
        )));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| CliError::usage("the token file is not valid UTF-8"))?;
    normalize(&text)
}

#[cfg(not(unix))]
fn read_file(path: &Path) -> Result<String, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::usage(format!("cannot read the token file: {error}")))?;
    if text.len() > MAX_TOKEN_BYTES {
        return Err(CliError::usage(format!(
            "the token file exceeds {MAX_TOKEN_BYTES} bytes"
        )));
    }
    normalize(&text)
}

fn normalize(raw: &str) -> Result<String, CliError> {
    if raw.len() > MAX_TOKEN_BYTES {
        return Err(CliError::usage(format!(
            "the token exceeds {MAX_TOKEN_BYTES} bytes"
        )));
    }
    let value = raw
        .strip_suffix("\r\n")
        .or_else(|| raw.strip_suffix('\n'))
        .unwrap_or(raw);
    if value.is_empty() {
        return Err(CliError::usage("the token is empty"));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(CliError::usage(
            "the token must be one line with no whitespace",
        ));
    }
    Ok(value.to_string())
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
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn write_token(mode: u32, content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        (dir, path)
    }

    #[test]
    fn accepts_one_trailing_newline_and_rejects_other_whitespace() {
        assert_eq!(normalize("wfp_abc\n").unwrap(), "wfp_abc");
        assert_eq!(normalize("wfp_abc\r\n").unwrap(), "wfp_abc");
        assert_eq!(normalize(" wfp_abc").unwrap_err().exit, 2);
        assert_eq!(normalize("wfp_abc\nwfp_def").unwrap_err().exit, 2);
        assert_eq!(normalize("").unwrap_err().exit, 2);
        assert_eq!(normalize(" \n").unwrap_err().exit, 2);
    }

    #[test]
    fn file_must_be_private_regular_and_not_a_symlink() {
        let (_dir, path) = write_token(0o644, "wfp_abc\n");
        assert!(read_file(&path).is_err());

        let (_dir, path) = write_token(0o600, "wfp_abc\n");
        assert_eq!(read_file(&path).unwrap(), "wfp_abc");

        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_file(&link).is_err());
    }

    #[test]
    fn oversized_files_are_rejected() {
        let (_dir, path) = write_token(0o600, &"a".repeat(MAX_TOKEN_BYTES + 1));
        assert!(read_file(&path).is_err());
    }
}
