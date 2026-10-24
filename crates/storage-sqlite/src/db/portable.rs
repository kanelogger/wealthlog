//! Password-protected, portable database copies. The installation key never
//! leaves the source; SQLCipher performs password derivation and page encryption.

use super::{copy_database, DbAccess, DbEncryptionKey};
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OpenFlags};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroizing;

// V1 pins SQLCipher 4 defaults explicitly. Unknown profiles are never guessed.
const HEADER: &[u8; 16] = b"WFOLIOBACKUP\0\0\0\x01";
pub const MAX_BACKUP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct PortableExport {
    pub path: PathBuf,
    pub filename: String,
    _workspace: tempfile::TempDir,
}

pub fn validate_password(password: &str) -> anyhow::Result<()> {
    ensure!(
        (12..=1024).contains(&password.chars().count()) && password.len() <= 4096,
        "Use a backup password between 12 and 1024 characters (at most 4096 UTF-8 bytes)"
    );
    Ok(())
}

// A fixed prefix ensures SQLCipher never interprets a user-entered x'...' value
// as a raw key, bypassing password derivation. Preserve the user's bytes exactly.
fn password_key(password: &str) -> Zeroizing<String> {
    Zeroizing::new(format!("wealthfolio-portable-v1:{password}"))
}

#[allow(unsafe_code, reason = "SQLCipher's sqlite3_key has no safe binding")]
fn password_connection(path: &Path, password: &str) -> anyhow::Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let key = password_key(password);
    // SAFETY: conn owns a live SQLite handle, and key remains valid for this
    // synchronous call. SQLCipher copies the explicitly sized key material.
    // Do not interpolate passwords into PRAGMA SQL (including error messages).
    let result = unsafe {
        libsqlite3_sys::sqlite3_key(conn.handle(), key.as_ptr().cast(), key.len().try_into()?)
    };
    ensure!(result == libsqlite3_sys::SQLITE_OK, "Cannot unlock backup");
    conn.execute_batch(
        "PRAGMA cipher_compatibility = 4;
         PRAGMA kdf_iter = 256000;
         PRAGMA trusted_schema = OFF;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(conn)
}

fn copy_with_key(source: &Connection, destination: &Path, key: &str) -> anyhow::Result<()> {
    // A read/write source opened without CREATE also prevents ATTACH from
    // creating files. Reserve the unique output explicitly and never overwrite.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    source.execute(
        "ATTACH DATABASE ?1 AS portable KEY ?2",
        params![destination.to_str().context("Invalid backup path")?, key],
    )?;
    let copied = source.execute_batch(
        "PRAGMA portable.cipher_compatibility = 4;
         PRAGMA portable.kdf_iter = 256000;
         SELECT sqlcipher_export('portable');",
    );
    let detached = source.execute_batch("DETACH DATABASE portable;");
    copied?;
    detached?;
    Ok(())
}

fn workspace(root: &Path) -> anyhow::Result<tempfile::TempDir> {
    fs::create_dir_all(root)?;
    Ok(tempfile::Builder::new()
        .prefix("portable-")
        .tempdir_in(root)?)
}

fn encrypted_access(path: &Path) -> anyhow::Result<DbAccess> {
    Ok(DbAccess::encrypted(
        path.to_str().context("Invalid backup path")?,
        Arc::new(DbEncryptionKey::generate()),
    ))
}

fn validate_integrity(conn: &Connection, encrypted: bool, file_len: u64) -> anyhow::Result<()> {
    conn.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA temp_store = MEMORY;")?;
    super::maintenance::integrity_check(conn)
        .map_err(|_| anyhow::anyhow!("The backup is damaged or the password is incorrect"))?;
    if encrypted {
        super::maintenance::cipher_integrity_check(conn)
            .map_err(|_| anyhow::anyhow!("The backup is damaged or the password is incorrect"))?;
    }
    let pages: u32 = conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let size = conn.pragma_query_value(None, "page_size", |row| {
        row.get::<_, rusqlite::types::Value>(0)
    })?;
    let size: u32 = match size {
        rusqlite::types::Value::Integer(value) => value.try_into()?,
        rusqlite::types::Value::Text(value) => value.parse()?,
        _ => anyhow::bail!("Invalid database page size"),
    };
    ensure!(
        u64::from(pages).checked_mul(u64::from(size)) == Some(file_len),
        "The backup has an invalid length"
    );
    Ok(())
}

/// Creates a faithful portable copy. All intermediate databases are encrypted,
/// including when the final output is explicitly requested as plaintext.
/// Preserve the snapshot schema and migration history; compatibility checks and
/// pending migrations belong to restore, not export.
pub fn export(
    source: &DbAccess,
    root: &Path,
    password: Option<&str>,
) -> anyhow::Result<PortableExport> {
    if let Some(password) = password {
        validate_password(password)?;
    }
    let work = workspace(root)?;
    let clean = encrypted_access(&work.path().join("clean.db"))?;
    copy_database(source, clean.path(), clean.key().map(Arc::as_ref))?;
    validate_integrity(
        &clean.connect_rusqlite()?,
        true,
        fs::metadata(clean.path())?.len(),
    )?;
    let conn = clean.connect_rusqlite()?;
    conn.execute_batch("DROP TABLE IF EXISTS wealthfolio_portable_backup;
        CREATE TABLE wealthfolio_portable_backup(id INTEGER PRIMARY KEY CHECK(id=1), profile INTEGER NOT NULL, created_at TEXT NOT NULL, app_version TEXT NOT NULL);")?;
    conn.execute(
        "INSERT INTO wealthfolio_portable_backup VALUES (1,1,?1,?2)",
        params![chrono::Utc::now().to_rfc3339(), env!("CARGO_PKG_VERSION")],
    )?;
    let filename = format!(
        "wealthfolio-{}.{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        if password.is_some() { "wfbackup" } else { "db" }
    );
    let output = work.path().join(&filename);
    if let Some(password) = password {
        let payload = work.path().join("payload.db");
        copy_with_key(&conn, &payload, &password_key(password))?;
        check_export_size(fs::metadata(&payload)?.len(), HEADER.len() as u64)?;
        let verified = password_connection(&payload, password)?;
        validate_integrity(&verified, true, fs::metadata(&payload)?.len())?;
        drop(verified);
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        out.write_all(HEADER)?;
        std::io::copy(&mut fs::File::open(&payload)?, &mut out)?;
        out.sync_all()?;
    } else {
        copy_with_key(&conn, &output, "")?;
        check_export_size(fs::metadata(&output)?.len(), 0)?;
    }
    Ok(PortableExport {
        path: output,
        filename,
        _workspace: work,
    })
}

fn check_export_size(payload: u64, header: u64) -> anyhow::Result<()> {
    ensure!(
        payload
            .checked_add(header)
            .is_some_and(|size| size <= MAX_BACKUP_BYTES),
        "Backup exceeds the supported 2 GiB limit"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(dir: &Path, encrypted: bool) -> DbAccess {
        let access = DbAccess::new(
            dir.join("source.db").to_str().unwrap(),
            encrypted.then(|| Arc::new(DbEncryptionKey::generate())),
        );
        access.prepare().unwrap();
        access.run_migrations().unwrap();
        let conn = access.connect_rusqlite().unwrap();
        conn.execute_batch("INSERT INTO app_settings(setting_key,setting_value) VALUES ('theme','dark') ON CONFLICT(setting_key) DO UPDATE SET setting_value='dark';
            INSERT INTO app_settings(setting_key,setting_value) VALUES ('sync_enabled','true') ON CONFLICT(setting_key) DO UPDATE SET setting_value='true';
            INSERT INTO personal_access_tokens(id,name,token_prefix,token_hash) VALUES ('test','Synthetic token','wf','SYNTHETIC_TOKEN_HASH');").unwrap();
        access
    }

    fn assert_export_preserves_snapshot(
        source: &DbAccess,
        root: &Path,
        password: Option<&str>,
    ) -> PortableExport {
        let original = source.connect_rusqlite().unwrap();
        let schema = |conn: &Connection| {
            conn.prepare("SELECT type, name, sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' AND name != 'wealthfolio_portable_backup' ORDER BY type, name")
                .unwrap()
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        let before = schema(&original);
        let rows = before
            .iter()
            .filter(|(kind, _, _)| kind == "table")
            .map(|(_, name, _)| (name.clone(), table_rows(&original, name)))
            .collect::<Vec<_>>();
        let output = export(source, root, password).unwrap();
        let exported = if let Some(password) = password {
            let payload = root.join("exported-payload.db");
            let bytes = fs::read(&output.path).unwrap();
            assert_eq!(&bytes[..HEADER.len()], HEADER);
            fs::write(&payload, &bytes[HEADER.len()..]).unwrap();
            password_connection(&payload, password).unwrap()
        } else {
            Connection::open(&output.path).unwrap()
        };
        assert_eq!(schema(&exported), before);
        assert_eq!(schema(&original), before);
        for (table, expected) in rows {
            assert_eq!(table_rows(&exported, &table), expected, "exported {table}");
            assert_eq!(table_rows(&original, &table), expected, "source {table}");
        }
        output
    }

    #[test]
    fn export_preserves_unknown_migrations() {
        for version in ["20260704000002", "20990101000001"] {
            for encrypted in [false, true] {
                for password in [None, Some("portable backup password")] {
                    let dir = tempfile::tempdir().unwrap();
                    let source = seeded(dir.path(), encrypted);
                    source
                        .connect_rusqlite()
                        .unwrap()
                        .execute(
                            "INSERT INTO __diesel_schema_migrations(version) VALUES (?1)",
                            [version],
                        )
                        .unwrap();
                    assert_export_preserves_snapshot(&source, dir.path(), password);
                }
            }
        }
    }
    #[test]
    fn export_preserves_older_schema_without_applying_pending_migrations() {
        use diesel::migration::{MigrationConnection, MigrationSource};
        use diesel_migrations::MigrationHarness;
        for encrypted in [false, true] {
            for password in [None, Some("portable backup password")] {
                let dir = tempfile::tempdir().unwrap();
                let source = DbAccess::new(
                    dir.path().join("source.db").to_str().unwrap(),
                    encrypted.then(|| Arc::new(DbEncryptionKey::generate())),
                );
                source.prepare().unwrap();
                {
                    let mut conn = source.connect().unwrap();
                    conn.setup().unwrap();
                    let mut migrations =
                        <diesel_migrations::EmbeddedMigrations as MigrationSource<
                            diesel::sqlite::Sqlite,
                        >>::migrations(&super::super::MIGRATIONS)
                        .unwrap();
                    migrations.sort_by(|a, b| a.name().version().cmp(&b.name().version()));
                    conn.run_migrations(&migrations[..2]).unwrap();
                }
                source.connect_rusqlite().unwrap().execute_batch(
                    "INSERT INTO settings(theme,font,base_currency) VALUES('dark','font-mono','USD');
                     INSERT INTO accounts(id,name,currency) VALUES('legacy-account','Legacy','USD');"
                ).unwrap();
                assert_export_preserves_snapshot(&source, dir.path(), password);
            }
        }
    }
    fn table_rows(conn: &Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns = stmt.column_count();
        let order = (1..=columns)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        stmt = conn
            .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY {order}"))
            .unwrap();
        stmt.query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn export_size_includes_format_header() {
        assert!(check_export_size(MAX_BACKUP_BYTES - 16, 16).is_ok());
        assert!(check_export_size(MAX_BACKUP_BYTES - 15, 16).is_err());
        assert!(check_export_size(MAX_BACKUP_BYTES, 0).is_ok());
        assert!(check_export_size(u64::MAX, 16).is_err());
    }
    #[test]
    fn sqlcipher_password_export_is_independent_of_installation_keys() {
        let dir = tempfile::tempdir().unwrap();
        let source = DbAccess::encrypted(
            dir.path().join("source.db").to_str().unwrap(),
            Arc::new(DbEncryptionKey::generate()),
        );
        source.prepare().unwrap();
        let conn = source.connect_rusqlite().unwrap();
        conn.execute_batch(
            "CREATE TABLE proof(value TEXT); INSERT INTO proof VALUES ('preserved');",
        )
        .unwrap();
        let exported = dir.path().join("password.db");
        let password = "  user's long 日本語 passphrase  ";
        copy_with_key(&conn, &exported, &password_key(password)).unwrap();
        assert!(!std::fs::read(&exported)
            .unwrap()
            .starts_with(b"SQLite format 3\0"));
        let imported = password_connection(&exported, password).unwrap();
        let value: String = imported
            .query_row("SELECT value FROM proof", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "preserved");
        let wrong = password_connection(&exported, password.trim()).unwrap();
        assert!(wrong
            .query_row("SELECT value FROM proof", [], |row| row.get::<_, String>(0))
            .is_err());
    }
}
