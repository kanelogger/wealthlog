//! Embedded local MCP server (Phase D of the agent-access design).
//!
//! Hosts the `wealthfolio-mcp` Streamable HTTP service on loopback when
//! enabled in settings, guarded by per-client Personal Access Tokens and
//! Origin validation, discovered through `<app_data>/mcp.lock`.

#[cfg(desktop)]
pub mod audit_sink;
#[cfg(desktop)]
pub mod gate;
#[cfg(desktop)]
pub mod lockfile;
#[cfg(desktop)]
pub mod middleware;
#[cfg(desktop)]
pub mod server;

#[cfg(all(test, desktop))]
mod tests;

#[cfg(desktop)]
use std::path::PathBuf;
#[cfg(desktop)]
use std::sync::Arc;
#[cfg(desktop)]
use std::sync::Mutex as StdMutex;
#[cfg(desktop)]
use uuid::Uuid;

use chrono::{DateTime, Utc};
#[cfg(desktop)]
use std::collections::HashMap;
use tauri::AppHandle;
#[cfg(desktop)]
use tauri::Manager;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
#[cfg(desktop)]
use wealthfolio_mcp::runtime::McpRuntimeProvider;
#[cfg(desktop)]
use wealthfolio_mcp::AuditSink;

use crate::context::ServiceContext;
#[cfg(desktop)]
use audit_sink::RepoAuditSink;

/// Settings keys (app_settings k/v; values are "true"/"false").
pub const SETTING_ENABLED: &str = "mcp_server_enabled";
pub const SETTING_AUTO_START: &str = "mcp_server_auto_start";
/// Optional port override; defaults to [`server::DEFAULT_PORT`].
pub const SETTING_PORT: &str = "mcp_server_port";
/// Audit logging of agent tool calls; defaults to enabled when unset.
pub const SETTING_AUDIT_ENABLED: &str = "mcp_audit_enabled";

/// Managed Tauri state holding the running server, if any.
#[cfg(desktop)]
#[derive(Default, Clone, Copy)]
struct LedgerIntent {
    started: bool,
}

/// Whether the service should run for a ledger, given its persisted flags and
/// this session's intent. Disabled always wins; an explicit stop is not
/// undone by unlocking or switching back.
#[cfg(desktop)]
fn should_run_service(enabled: bool, auto_start: bool, prior: Option<LedgerIntent>) -> bool {
    enabled
        && match prior {
            Some(intent) => intent.started,
            None => auto_start,
        }
}

#[cfg(desktop)]
pub struct McpServerState {
    inner: tokio::sync::Mutex<Option<RunningServer>>,
    /// Serializes start/stop orchestration end-to-end so concurrent
    /// operations cannot interleave (e.g. a start racing a slow stop and
    /// landing on a random fallback port).
    ops: tokio::sync::Mutex<()>,
    /// Per-ledger in-memory run intent. Cleared on app restart.
    intents: StdMutex<HashMap<Uuid, LedgerIntent>>,
    /// Request leases over the active ledger runtime.
    pub gate: std::sync::Arc<gate::LedgerGate>,
}

#[cfg(desktop)]
impl Default for McpServerState {
    fn default() -> Self {
        Self {
            inner: tokio::sync::Mutex::new(None),
            ops: tokio::sync::Mutex::new(()),
            intents: StdMutex::new(HashMap::new()),
            gate: std::sync::Arc::new(gate::LedgerGate::new()),
        }
    }
}

#[cfg(not(desktop))]
#[derive(Default)]
pub struct McpServerState;

#[cfg(desktop)]
impl McpServerState {
    /// Snapshot of the running server: `(port, started_at_rfc3339)`.
    fn intent(&self, ledger: Uuid) -> Option<LedgerIntent> {
        self.intents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&ledger)
            .copied()
    }

    fn set_intent(&self, ledger: Uuid, intent: LedgerIntent) {
        self.intents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(ledger, intent);
    }

    pub async fn running_info(&self) -> Option<(u16, String)> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|server| (server.port, server.started_at.to_rfc3339()))
    }
}

/// A running embedded MCP listener owned by the desktop runtime.
pub struct RunningServer {
    pub port: u16,
    pub started_at: DateTime<Utc>,
    pub instance_id: String,
    socket: server::SocketLocation,
    cancel: CancellationToken,
    joins: Vec<JoinHandle<()>>,
}

impl RunningServer {
    #[cfg(test)]
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket.path
    }

    /// Cancels in-flight MCP sessions and waits for both listeners to exit.
    /// The private socket directory is removed only after the listener stopped.
    pub async fn stop(self) {
        self.cancel.cancel();
        for join in self.joins {
            let _ = join.await;
        }
        self.socket.cleanup();
    }
}

fn setting_is_true(ctx: &ServiceContext, key: &str) -> bool {
    matches!(
        ctx.settings_service().get_setting_value(key),
        Ok(Some(value)) if value == "true"
    )
}

/// Current persisted flags: `(enabled, auto_start)`.
pub fn flags(ctx: &ServiceContext) -> (bool, bool) {
    (
        setting_is_true(ctx, SETTING_ENABLED),
        setting_is_true(ctx, SETTING_AUTO_START),
    )
}

/// Whether agent tool calls are written to the audit log. Defaults to
/// `true` when the setting is unset; only an explicit "false" disables it.
pub fn audit_enabled(ctx: &ServiceContext) -> bool {
    !matches!(
        ctx.settings_service().get_setting_value(SETTING_AUDIT_ENABLED),
        Ok(Some(value)) if value == "false"
    )
}

/// Audit sink for the MCP service, or `None` when audit logging is off.
#[cfg(desktop)]
pub(crate) fn build_audit_sink(ctx: &ServiceContext) -> Option<Arc<dyn AuditSink>> {
    audit_enabled(ctx)
        .then(|| Arc::new(RepoAuditSink(ctx.mcp_audit_repository())) as Arc<dyn AuditSink>)
}

#[cfg(desktop)]
fn configured_port(ctx: &ServiceContext) -> Option<u16> {
    ctx.settings_service()
        .get_setting_value(SETTING_PORT)
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
}

#[cfg(desktop)]
fn app_data_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))
}

/// Removes a stale discovery file and the private socket directory it names.
/// Only paths under the system temporary directory with our generated prefix
/// are touched; a mismatched or symlinked path is left alone.
pub fn remove_stale_lock<R: tauri::Runtime>(app: &AppHandle<R>) {
    #[cfg(desktop)]
    {
        let Ok(dir) = app_data_dir(app) else {
            return;
        };
        if let Ok(raw) = std::fs::read_to_string(dir.join(lockfile::LOCK_FILE_NAME)) {
            if let Ok(lock) = serde_json::from_str::<lockfile::McpLockFile>(&raw) {
                // Never remove a socket that a live process still owns. A
                // stale file from an earlier crash is the only cleanup target.
                if !process_is_alive(lock.pid) {
                    cleanup_published_socket(&lock.socket_path);
                }
            }
        }
        if let Err(err) = lockfile::remove(&dir) {
            log::warn!("Failed to remove stale mcp.lock: {err}");
        }
    }
}

#[cfg(all(desktop, unix))]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: signal 0 performs permission/existence checking only.
    let result = libc_kill(pid as i32, 0);
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc_eperm())
}

#[cfg(all(desktop, unix))]
#[allow(unsafe_code, reason = "kill(pid, 0) is a parameter-checking libc call")]
fn libc_kill(pid: i32, signal: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, signal) }
}

#[cfg(all(desktop, unix))]
fn libc_eperm() -> i32 {
    1
}

#[cfg(all(desktop, not(unix)))]
fn process_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(desktop)]
fn cleanup_published_socket(path: &str) {
    #[cfg(unix)]
    use std::os::unix::fs::FileTypeExt;
    let path = std::path::Path::new(path);
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(temp_root) = std::fs::canonicalize(std::env::temp_dir()) else {
        return;
    };
    let Ok(parent) = std::fs::canonicalize(parent) else {
        return;
    };
    if parent.parent() != Some(temp_root.as_path()) {
        return;
    }
    let Some(name) = parent.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if !name.starts_with("wl-") || name.len() > 16 {
        return;
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_socket() {
            let _ = std::fs::remove_file(path);
        }
    }
    let _ = std::fs::remove_dir(parent);
}

#[cfg(desktop)]
fn active_runtime(
    app: &AppHandle,
    ctx: &ServiceContext,
) -> Result<std::sync::Arc<crate::database::DatabaseRuntime>, String> {
    let profiles = app
        .try_state::<crate::profiles::NativeProfiles>()
        .ok_or_else(|| "Profile state is unavailable".to_string())?;
    let runtime = profiles
        .active()?
        .ok_or_else(|| "No profile runtime is active".to_string())?;
    if runtime.profile_id != ctx.profile_id || !ctx.is_active() {
        return Err("PROFILE_LOCKED".to_string());
    }
    Ok(runtime)
}

/// Starts the server (no-op when already running) and writes `mcp.lock`.
pub async fn start_server(app: &AppHandle, ctx: &ServiceContext) -> Result<(), String> {
    #[cfg(desktop)]
    {
        if !flags(ctx).0 {
            return Err("Enable AI Agent Access before starting the server".to_string());
        }
        let state = app.state::<McpServerState>();
        let _ops = state.ops.lock().await;
        let runtime = active_runtime(app, ctx)?;
        state.set_intent(ctx.profile_id, LedgerIntent { started: true });
        start_server_locked(app, ctx, Some(runtime)).await
    }
}

/// Start implementation; callers must hold the `ops` mutex. The listener only
/// holds the gate, never the business services.
#[cfg(desktop)]
async fn start_server_locked(
    app: &AppHandle,
    ctx: &ServiceContext,
    runtime: Option<std::sync::Arc<crate::database::DatabaseRuntime>>,
) -> Result<(), String> {
    if !ctx.is_active() {
        return Err("PROFILE_LOCKED".into());
    }
    let state = app.state::<McpServerState>();
    let Some(runtime) = runtime.or_else(|| active_runtime(app, ctx).ok()) else {
        return Err("PROFILE_LOCKED".into());
    };
    let ledger_id = ctx.business_service.ledger_id().to_string();
    state.gate.activate(&runtime, ledger_id.clone());

    let mut guard = state.inner.lock().await;
    if guard.is_some() {
        update_lock_file(app, "ready").ok();
        return Ok(());
    }

    let provider: std::sync::Arc<dyn McpRuntimeProvider> = state.gate.clone();
    let running = server::start(provider, configured_port(ctx)).await?;
    if let Err(err) = write_lock_file(app, &running, &ledger_id, "ready") {
        running.stop().await;
        state.gate.lock();
        return Err(err);
    }
    *guard = Some(running);
    Ok(())
}

/// Record an explicit stop for the active ledger. Unlike an unlock or a
/// switch, this intent survives until the user starts the service again.
pub fn mark_stopped<R: tauri::Runtime>(app: &AppHandle<R>, ctx: &ServiceContext) {
    #[cfg(desktop)]
    {
        if let Some(state) = app.try_state::<McpServerState>() {
            state.set_intent(ctx.profile_id, LedgerIntent { started: false });
        }
    }
}

/// Stops the server when running and removes `mcp.lock`.
pub async fn stop_server<R: tauri::Runtime>(app: &AppHandle<R>) {
    #[cfg(desktop)]
    {
        let state = app.state::<McpServerState>();
        let _ops = state.ops.lock().await;
        stop_server_locked(app).await;
    }
}

/// Stop implementation; callers must hold the `ops` mutex.
#[cfg(desktop)]
async fn stop_server_locked<R: tauri::Runtime>(app: &AppHandle<R>) {
    let state = app.state::<McpServerState>();
    state.gate.clear();
    let running = state.inner.lock().await.take();
    if let Some(running) = running {
        running.stop().await;
        log::info!("MCP server stopped");
    }
    remove_stale_lock(app);
}

/// Called when a ledger runtime is installed and unlocked. Reads that ledger's
/// settings and its in-memory intent, starts or stops the listener, and points
/// the gate at the runtime.
pub async fn on_profile_ready(app: &AppHandle, ctx: &ServiceContext) {
    #[cfg(desktop)]
    {
        let state = app.state::<McpServerState>();
        let _ops = state.ops.lock().await;
        let (enabled, auto_start) = flags(ctx);
        let ledger = ctx.profile_id;
        let prior = state.intent(ledger);
        let should_run = should_run_service(enabled, auto_start, prior);
        state.set_intent(
            ledger,
            LedgerIntent {
                started: should_run,
            },
        );
        if !should_run {
            stop_server_locked(app).await;
            return;
        }
        let Ok(runtime) = active_runtime(app, ctx) else {
            return;
        };
        // Rebind per ledger: its configured port may differ.
        let existing = state.inner.lock().await.take();
        if let Some(existing) = existing {
            existing.stop().await;
            // The old port/socket no longer exist; a failed restart must not
            // leave a discovery file pointing at them.
            if let Ok(dir) = app_data_dir(app) {
                let _ = lockfile::remove(&dir);
            }
        }
        if let Err(error) = start_server_locked(app, ctx, Some(runtime)).await {
            log::error!("Failed to start MCP server for the active ledger: {error}");
        }
    }
}

/// Wait for admitted MCP requests to finish before database teardown. Shares
/// the caller's absolute deadline with IPC lease draining.
pub async fn drain_requests<R: tauri::Runtime>(
    app: &AppHandle<R>,
    deadline: std::time::Instant,
) -> bool {
    #[cfg(desktop)]
    {
        match app.try_state::<McpServerState>() {
            Some(state) => state.gate.drain(deadline).await,
            None => true,
        }
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, deadline);
        true
    }
}

/// Called synchronously when a ledger starts locking. New requests are
/// rejected immediately; admitted leases finish before database teardown.
pub fn on_profile_locking<R: tauri::Runtime>(app: &AppHandle<R>) {
    #[cfg(desktop)]
    {
        if let Some(state) = app.try_state::<McpServerState>() {
            state.gate.lock();
            update_lock_file(app, "locked").ok();
        }
    }
}

/// The database runtime is being torn down (maintenance, switch, or exit).
/// The listener stays up in the minimal locked state; it holds no runtime.
pub fn on_database_teardown<R: tauri::Runtime>(app: &AppHandle<R>) {
    on_profile_locking(app);
}

/// Persists the feature-enabled flag. Disabling stops a running server and
/// remains authoritative; enabling only makes the feature available.
pub async fn set_enabled(
    app: &AppHandle,
    ctx: &ServiceContext,
    enabled: bool,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        let state = app.state::<McpServerState>();
        let _ops = state.ops.lock().await;

        ctx.settings_service()
            .set_setting_value(SETTING_ENABLED, if enabled { "true" } else { "false" })
            .await
            .map_err(|e| e.to_string())?;

        let prior = state.intent(ctx.profile_id);
        if !enabled {
            state.set_intent(ctx.profile_id, LedgerIntent { started: false });
            stop_server_locked(app).await;
        } else if let Some(prior) = prior {
            state.set_intent(
                ctx.profile_id,
                LedgerIntent {
                    started: prior.started,
                },
            );
        }
        Ok(())
    }
}

/// Persists the auto-start flag. Takes effect on next launch; does not
/// start or stop the currently running server.
pub async fn set_auto_start(
    app: &AppHandle,
    ctx: &ServiceContext,
    auto_start: bool,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        let state = app.state::<McpServerState>();
        let _ops = state.ops.lock().await;

        ctx.settings_service()
            .set_setting_value(
                SETTING_AUTO_START,
                if auto_start { "true" } else { "false" },
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Persists the audit-logging flag. When the server is running it is
/// restarted so the change takes effect immediately.
pub async fn set_audit_enabled(
    app: &AppHandle,
    ctx: &ServiceContext,
    enabled: bool,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        let state = app.state::<McpServerState>();
        let _ops = state.ops.lock().await;

        ctx.settings_service()
            .set_setting_value(
                SETTING_AUDIT_ENABLED,
                if enabled { "true" } else { "false" },
            )
            .await
            .map_err(|e| e.to_string())?;

        let running = state.inner.lock().await.is_some();
        if running {
            stop_server_locked(app).await;
            start_server_locked(app, ctx, None).await?;
        }
        Ok(())
    }
}

#[cfg(desktop)]
fn write_lock_file(
    app: &AppHandle,
    running: &RunningServer,
    ledger_id: &str,
    status: &str,
) -> Result<(), String> {
    let dir = app_data_dir(app)?;
    let lock = lockfile::McpLockFile {
        lock_file_version: lockfile::LOCK_FILE_VERSION,
        instance_id: running.instance_id.clone(),
        port: running.port,
        pid: std::process::id(),
        started_at: running.started_at.to_rfc3339(),
        active_ledger_id: Some(ledger_id.to_string()),
        status: status.to_string(),
        socket_path: running.socket.path.to_string_lossy().into_owned(),
    };
    lockfile::write(&dir, &lock).map_err(|e| format!("Failed to write mcp.lock: {e}"))
}

#[cfg(desktop)]
fn update_lock_file<R: tauri::Runtime>(app: &AppHandle<R>, status: &str) -> Result<(), String> {
    let dir = app_data_dir(app)?;
    let path = dir.join(lockfile::LOCK_FILE_NAME);
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut lock: lockfile::McpLockFile =
        serde_json::from_str(&raw).map_err(|e| format!("mcp.lock is unreadable: {e}"))?;
    lock.status = status.to_string();
    let state = app.state::<McpServerState>();
    if state.gate.is_locked() {
        lock.status = "locked".to_string();
    }
    lock.active_ledger_id = state.gate.ledger_id();
    lockfile::write(&dir, &lock).map_err(|e| format!("Failed to update mcp.lock: {e}"))
}

#[cfg(all(test, desktop))]
mod intent_tests {
    use super::*;

    #[test]
    fn first_visit_uses_its_own_auto_start_flag() {
        assert!(!should_run_service(true, false, None));
        assert!(should_run_service(true, true, None));
        assert!(!should_run_service(false, true, None));
    }

    #[test]
    fn disabled_wins_over_a_running_intent() {
        assert!(!should_run_service(
            false,
            true,
            Some(LedgerIntent { started: true })
        ));
    }

    #[test]
    fn an_explicit_stop_survives_unlock_and_switch_back() {
        assert!(!should_run_service(
            true,
            true,
            Some(LedgerIntent { started: false })
        ));
        assert!(should_run_service(
            true,
            false,
            Some(LedgerIntent { started: true })
        ));
    }
}
