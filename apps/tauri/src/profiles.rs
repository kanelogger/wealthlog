//! Native profile ownership. IPC arguments capture a runtime before execution.
use crate::{
    context::ServiceContext, database::DatabaseRuntime, secret_store::shared_secret_store,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use wealthfolio_core::profiles::{ProfileError, ProfileRegistry, ProfileSession, ProfileSummary};
use zeroize::Zeroizing;

pub const NATIVE_OWNER: &str = "main";
pub const PROFILE_CHANGED: &str = "profile-session-changed";
/// The longest quitting waits for the active profile to release its database.
/// Exit proceeds after it even if the release is still running.
#[cfg(desktop)]
const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// One absolute lock/switch drain budget for every stage.
const DRAIN_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

pub struct NativeProfiles {
    pub registry: Arc<ProfileRegistry>,
    active: Mutex<Option<Arc<DatabaseRuntime>>>,
    // Serializes profile unlock, lock and deletion.
    lifecycle: tokio::sync::Mutex<()>,
    // Admission and lease registration share this mutex with lock(), so a
    // command admitted just before a lock is always counted before the drain.
    admission: Mutex<()>,
    leases: Arc<LeaseCounter>,
    starting: AtomicBool,
    lock_epoch: AtomicU64,
}

/// In-flight command leases for the active runtime. Dropping a lease wakes
/// the lock drain.
struct LeaseCounter {
    count: std::sync::atomic::AtomicUsize,
    changed: tokio::sync::Notify,
}

impl LeaseCounter {
    fn new() -> Self {
        Self {
            count: std::sync::atomic::AtomicUsize::new(0),
            changed: tokio::sync::Notify::new(),
        }
    }
}

pub struct ProfileLease {
    runtime: Arc<DatabaseRuntime>,
    counter: Arc<LeaseCounter>,
}

impl std::ops::Deref for ProfileLease {
    type Target = Arc<DatabaseRuntime>;
    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        self.counter
            .count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        self.counter.changed.notify_waiters();
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileState {
    pub profiles: Vec<ProfileSummary>,
    pub pending_deletions: Vec<ProfileSummary>,
    pub session: Option<ProfileSession>,
    pub starting: bool,
    pub startup_error: Option<String>,
}

/// Construct a runtime over the selected profile's database and scoped secrets.
fn profile_runtime(registry: &Arc<ProfileRegistry>, id: Uuid) -> Result<DatabaseRuntime, String> {
    let profile = registry.profile(id).map_err(|e| e.to_string())?;
    let mut runtime = DatabaseRuntime::for_profile(
        id,
        registry.paths(&profile),
        registry.secret_store(&profile),
    );
    runtime.profile_registry = Some(registry.clone());
    Ok(runtime)
}

fn portfolio_history_backfill_needed(context: &Arc<ServiceContext>) -> bool {
    let accounts = match context.account_service().get_non_archived_accounts() {
        Ok(accounts) => accounts,
        Err(err) => {
            log::error!("Failed to inspect accounts for valuation backfill: {}", err);
            return false;
        }
    };
    let account_ids: Vec<String> = accounts.into_iter().map(|account| account.id).collect();
    if account_ids.is_empty() {
        return false;
    }

    let latest = match context
        .valuation_service()
        .get_latest_valuations(&account_ids)
    {
        Ok(latest) => latest,
        Err(err) => {
            log::error!("Failed to inspect valuation history for backfill: {}", err);
            return false;
        }
    };
    let accounts_with_valuations: std::collections::HashSet<_> = latest
        .into_iter()
        .map(|valuation| valuation.account_id)
        .collect();
    let missing_ids: Vec<String> = account_ids
        .into_iter()
        .filter(|account_id| !accounts_with_valuations.contains(account_id))
        .collect();
    if missing_ids.is_empty() {
        return false;
    }

    if matches!(
        context
            .activity_service()
            .get_first_activity_date(Some(&missing_ids)),
        Ok(Some(_))
    ) {
        return true;
    }

    missing_ids.iter().any(|account_id| {
        matches!(
            context
                .snapshot_service()
                .get_latest_holdings_snapshot(account_id),
            Ok(Some(_))
        )
    })
}

impl NativeProfiles {
    pub fn new(root: String, identifier: &str) -> Result<Self, String> {
        Self::open(root, identifier, false)
    }

    pub fn start_new(root: String, identifier: &str) -> Result<Self, String> {
        Self::open(root, identifier, true)
    }

    fn open(root: String, identifier: &str, start_new: bool) -> Result<Self, String> {
        let (root, legacy) =
            crate::data_dir::profile_paths(root, identifier, std::env::var_os("WF_DATA_DIR"))?;
        let registry = if start_new {
            ProfileRegistry::start_new(root, legacy, shared_secret_store(identifier))
        } else {
            ProfileRegistry::open(root, legacy, shared_secret_store(identifier))
        }
        .map_err(|e| e.to_string())?;
        for id in registry.pending_deletions().map_err(|e| e.to_string())? {
            if let Err(error) = registry.finish_delete(id) {
                let (kind, io_kind, os_code) = match error {
                    ProfileError::StorageIo { kind, os_code } => {
                        ("storage_io", Some(kind), os_code)
                    }
                    ProfileError::Unavailable(_) => ("storage_unavailable", None, None),
                    ProfileError::Invalid(_) => ("invalid_deletion_record", None, None),
                    _ => ("other", None, None),
                };
                log::warn!(
                    "Profile deletion cleanup needs a retry (profile={id}, kind={kind}, ioKind={io_kind:?}, osCode={os_code:?})"
                );
            }
        }
        Ok(Self {
            registry: Arc::new(registry),
            active: Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
            admission: Mutex::new(()),
            leases: Arc::new(LeaseCounter::new()),
            starting: AtomicBool::new(true),
            lock_epoch: AtomicU64::new(0),
        })
    }

    pub fn active(&self) -> Result<Option<Arc<DatabaseRuntime>>, String> {
        self.active
            .lock()
            .map(|r| r.clone())
            .map_err(|_| "Profile runtime is unavailable.".into())
    }

    pub fn try_context(&self) -> Option<Arc<ServiceContext>> {
        let session = self.registry.sessions.current(NATIVE_OWNER).ok()??;
        let runtime = self.active().ok()??;
        (runtime.profile_id == session.profile_id && runtime.generation() == session.generation)
            .then(|| runtime.try_context())
            .flatten()
    }

    /// Capture the original grant before checking context identity. A queued
    /// event keeps this scope even if a switch happens immediately afterward.
    pub fn event_scope(&self, context: &ServiceContext) -> Option<Uuid> {
        let session = self.registry.sessions.current(NATIVE_OWNER).ok()??;
        let runtime = self.admit(session.scope_id).ok()?;
        let current = runtime.try_context()?;
        if context.profile_id != session.profile_id
            || !context.is_active()
            || !std::ptr::eq(current.as_ref(), context)
        {
            return None;
        }
        Some(session.scope_id)
    }

    pub async fn startup(&self, handle: &AppHandle) -> Result<Option<Arc<ServiceContext>>, String> {
        let result = async {
            let list = self.registry.list().map_err(|e| e.to_string())?;
            if list.len() == 1 && !list[0].lock_enabled {
                match self.unlock(handle, list[0].id, None).await {
                    Ok(_) => return Ok(self.try_context()),
                    Err(error) if error.starts_with("PROFILE_LOCKED") => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
            Ok(None)
        }
        .await;
        self.starting.store(false, Ordering::SeqCst);
        result
    }

    pub fn state(&self) -> Result<ProfileState, String> {
        let mut session = self
            .registry
            .sessions
            .current(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
        // A successful rebuild invalidates old requests. Only an already
        // authorized session may receive the replacement scope.
        if let (Some(current), Some(runtime)) = (&session, self.active()?) {
            let status = runtime.startup_status();
            if current.profile_id == runtime.profile_id
                && !status.maintenance
                && (status.ready || status.error.is_some())
                && current.generation != status.generation
            {
                session = Some(
                    self.registry
                        .sessions
                        .rotate(NATIVE_OWNER, current.scope_id, status.generation)
                        .map_err(|e| e.to_string())?,
                );
            }
        }
        Ok(ProfileState {
            profiles: self.registry.list().map_err(|e| e.to_string())?,
            pending_deletions: self
                .registry
                .pending_profiles()
                .map_err(|e| e.to_string())?,
            session,
            starting: self.starting.load(Ordering::SeqCst),
            startup_error: None,
        })
    }

    pub async fn unlock(
        &self,
        handle: &AppHandle,
        id: Uuid,
        proof: Option<String>,
    ) -> Result<ProfileSession, String> {
        let _transition = self.lifecycle.lock().await;
        // Close admission before anything else: an in-flight command keeps its
        // lease, but no new command may enter while this profile is leaving.
        let epoch = {
            let _admission = self
                .admission
                .lock()
                .map_err(|_| "Profile admission is unavailable.")?;
            let epoch = self.lock_epoch.fetch_add(1, Ordering::SeqCst) + 1;
            self.registry
                .sessions
                .revoke(NATIVE_OWNER)
                .map_err(|e| e.to_string())?;
            if let Some(runtime) = self.active()? {
                runtime.suspend();
            }
            epoch
        };
        let _ = handle.emit(PROFILE_CHANGED, ());
        crate::mcp::on_profile_locking(handle);

        let registry = self.registry.clone();
        let protected = tauri::async_runtime::spawn_blocking(move || {
            let proof = proof.map(Zeroizing::new);
            registry.verify(id, proof.as_deref().map(String::as_str))
        })
        .await
        .map_err(|error| {
            log::error!(
                "Profile verification task failed (panic={})",
                matches!(error, tauri::Error::JoinError(ref join) if join.is_panic())
            );
            "Profile verification failed.".to_string()
        })?
        .map_err(|e| e.to_string())?;

        // One absolute drain budget covers waiting for admitted work and for
        // the writer/pool to close. Credential verification is not part of it.
        let deadline = std::time::Instant::now() + DRAIN_WAIT;
        let (ipc_drained, mcp_drained) = tokio::join!(
            self.wait_for_leases(deadline),
            crate::mcp::drain_requests(handle, deadline)
        );
        if !ipc_drained || !mcp_drained {
            return Err(drain_timeout());
        }
        if let Some(old) = self.active()? {
            old.shutdown_until(handle, deadline)
                .await
                .map_err(profile_drain_error)?;
        }
        if epoch != self.lock_epoch.load(Ordering::SeqCst) {
            return Err(ProfileError::Locked.to_string());
        }

        let runtime = profile_runtime(&self.registry, id)?;
        let runtime = Arc::new(runtime);
        *self
            .active
            .lock()
            .map_err(|_| "Profile runtime is unavailable.")? = Some(runtime.clone());
        // Failed database startup still gets an authorized session for status and retry.
        if let Err(error) = runtime.initialize(handle).await {
            log::warn!("Profile database startup failed: {error}");
        }
        if epoch != self.lock_epoch.load(Ordering::SeqCst) {
            let deadline = std::time::Instant::now() + DRAIN_WAIT;
            runtime
                .shutdown_until(handle, deadline)
                .await
                .map_err(profile_drain_error)?;
            return Err(ProfileError::Locked.to_string());
        }
        let session = self
            .registry
            .sessions
            .issue(NATIVE_OWNER, id, protected, runtime.generation())
            .map_err(|e| e.to_string())?;
        // Run this for every activated profile, including one opened from the
        // chooser after startup. Issue its scope first so job events are scoped.
        if let Some(context) = runtime.try_context() {
            if portfolio_history_backfill_needed(&context) {
                crate::events::emit_portfolio_trigger_recalculate(
                    handle,
                    crate::events::PortfolioRequestPayload::builder().build(),
                    &context,
                );
            }
            crate::mcp::on_profile_ready(handle, &context).await;
        }
        Ok(session)
    }

    pub async fn lock<R: tauri::Runtime>(&self, handle: &AppHandle<R>) -> Result<(), String> {
        // Close admission and start the absolute budget before waiting for any
        // lifecycle transition. A stuck transition must not keep the old
        // session usable. The request returns profile_drain_timeout while the
        // admitted work and ownership stay tracked.
        let deadline = std::time::Instant::now() + DRAIN_WAIT;
        self.close_admission()?;
        let _ = handle.emit(PROFILE_CHANGED, ());
        // Reject external calls first; the listener itself stays minimal.
        crate::mcp::on_profile_locking(handle);

        let _transition =
            match tokio::time::timeout_at(deadline.into(), self.lifecycle.lock()).await {
                Ok(guard) => guard,
                Err(_) => return Err(drain_timeout()),
            };
        // A transition that won the race before we acquired the mutex has
        // already been invalidated by the epoch bump; close again defensively.
        self.close_admission()?;
        let (ipc_drained, mcp_drained) = tokio::join!(
            self.wait_for_leases(deadline),
            crate::mcp::drain_requests(handle, deadline)
        );
        if !ipc_drained || !mcp_drained {
            return Err(drain_timeout());
        }
        if let Some(runtime) = self.active()? {
            runtime
                .shutdown_until(handle, deadline)
                .await
                .map_err(profile_drain_error)?;
        }
        *self
            .active
            .lock()
            .map_err(|_| "Profile runtime is unavailable.")? = None;
        Ok(())
    }

    /// Close IPC admission, revoke sessions and suspend the active runtime.
    fn close_admission(&self) -> Result<u64, String> {
        let _admission = self
            .admission
            .lock()
            .map_err(|_| "Profile admission is unavailable.")?;
        let epoch = self.lock_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.registry
            .sessions
            .revoke(NATIVE_OWNER)
            .map_err(|e| e.to_string())?;
        if let Some(runtime) = self.active()? {
            runtime.suspend();
        }
        Ok(epoch)
    }

    /// Synchronously close admission for a native auto-lock. This runs before
    /// any awaited lifecycle transition, so an unlock already in progress can
    /// never issue a fresh session after the lock request.
    pub fn begin_auto_lock(&self) -> Result<bool, String> {
        let _admission = self
            .admission
            .lock()
            .map_err(|_| "Profile admission is unavailable.")?;
        if !self
            .registry
            .sessions
            .revoke_for_auto_lock(NATIVE_OWNER)
            .map_err(|e| e.to_string())?
        {
            return Ok(false);
        }
        self.lock_epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(runtime) = self.active()? {
            runtime.suspend();
        }
        Ok(true)
    }

    /// Admit an IPC command and register its lease atomically with the gate
    /// and generation check.
    pub fn lease(&self, scope: Uuid) -> Result<ProfileLease, String> {
        let _admission = self
            .admission
            .lock()
            .map_err(|_| "Profile admission is unavailable.")?;
        let session = self
            .registry
            .sessions
            .admit(NATIVE_OWNER, scope)
            .map_err(|e| e.to_string())?;
        let runtime = self
            .active()?
            .ok_or_else(|| ProfileError::Locked.to_string())?;
        if runtime.profile_id != session.profile_id || runtime.generation() != session.generation {
            return Err(ProfileError::Stale.to_string());
        }
        // Startup/status commands are admitted before the database is ready;
        // the command layer decides which of them may run without a context.
        self.leases.count.fetch_add(1, Ordering::SeqCst);
        Ok(ProfileLease {
            runtime,
            counter: Arc::clone(&self.leases),
        })
    }

    async fn wait_for_leases(&self, deadline: std::time::Instant) -> bool {
        loop {
            let notified = self.leases.changed.notified();
            if self.leases.count.load(Ordering::SeqCst) == 0 {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            if tokio::time::timeout_at(deadline.into(), notified)
                .await
                .is_err()
            {
                return self.leases.count.load(Ordering::SeqCst) == 0;
            }
        }
    }

    pub fn admit(&self, scope: Uuid) -> Result<Arc<DatabaseRuntime>, String> {
        let session = self
            .registry
            .sessions
            .admit(NATIVE_OWNER, scope)
            .map_err(|e| e.to_string())?;
        let runtime = self
            .active()?
            .ok_or_else(|| ProfileError::Locked.to_string())?;
        if runtime.profile_id != session.profile_id || runtime.generation() != session.generation {
            return Err(ProfileError::Stale.to_string());
        }
        Ok(runtime)
    }
}

/// Normalize every cleanup stage that consumes the absolute drain budget to
/// the same GUI-facing code. Unrelated failures keep their original text.
fn profile_drain_error(error: String) -> String {
    if error.starts_with("profile_drain_timeout") {
        return error;
    }
    if error.contains("still completing")
        || error.contains("still using")
        || error.contains("still running")
        || error.contains("maintenance")
        || error.contains("background database work")
    {
        format!("profile_drain_timeout: {error}")
    } else {
        error
    }
}

fn drain_timeout() -> String {
    "profile_drain_timeout: the profile is locked; background database work is still completing. Try unlocking again when it finishes.".to_string()
}

/// Command-boundary access to the runtime admitted for the caller's profile session.
/// Extraction checks window identity, scope and runtime generation once. The captured
/// runtime never changes when another profile opens; this is not a revocable lease
/// and does not cancel work already admitted. Database operations separately enforce
/// suspension and maintenance gates. Internal helpers should accept only the service,
/// runtime or path they need, retaining runtime/file leases for database work.
pub struct ProfileAccess(pub ProfileLease);

impl std::ops::Deref for ProfileAccess {
    type Target = Arc<DatabaseRuntime>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de, R: tauri::Runtime> tauri::ipc::CommandArg<'de, R> for ProfileAccess {
    fn from_command(
        command: tauri::ipc::CommandItem<'de, R>,
    ) -> Result<Self, tauri::ipc::InvokeError> {
        let webview = command.message.webview();
        if webview.label() != NATIVE_OWNER {
            return Err(ProfileError::Locked.to_string().into());
        }
        let scope = match command.message.payload() {
            tauri::ipc::InvokeBody::Json(body) => body
                .get("scopeId")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok()),
            _ => None,
        }
        .ok_or_else(|| tauri::ipc::InvokeError::from(ProfileError::Locked.to_string()))?;
        let profiles = webview
            .try_state::<NativeProfiles>()
            .ok_or_else(|| tauri::ipc::InvokeError::from(ProfileError::Locked.to_string()))?;
        let runtime = profiles
            .lease(scope)
            .map_err(tauri::ipc::InvokeError::from)?;
        if runtime.generation().is_none()
            && ![
                "get_database_startup_status",
                "retry_database_startup",
                "get_database_encryption_status",
            ]
            .contains(&command.message.command())
        {
            return Err("Profile database is unavailable; retry startup first".into());
        }
        Ok(Self(runtime))
    }
}

#[tauri::command]
pub async fn delete_profile(
    handle: AppHandle,
    profile_id: Uuid,
    scope_id: Option<Uuid>,
    confirmation: String,
    proof: Option<String>,
) -> Result<(), String> {
    // Keep cleanup running even if the renderer reloads or drops its IPC future.
    tauri::async_runtime::spawn(async move {
        let state = handle
            .try_state::<NativeProfiles>()
            .ok_or_else(|| ProfileError::Locked.to_string())?;
        let _transition = state.lifecycle.lock().await;
        if !state.registry.is_deleting(profile_id) {
            let session = state
                .registry
                .sessions
                .admit(
                    NATIVE_OWNER,
                    scope_id.ok_or_else(|| ProfileError::Locked.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            if session.profile_id != profile_id {
                return Err(ProfileError::Stale.to_string());
            }
            let registry = state.registry.clone();
            tauri::async_runtime::spawn_blocking(move || {
                registry.begin_delete(profile_id, &confirmation, proof.as_deref())
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        }
        if let Some(runtime) = state.active()? {
            if runtime.profile_id == profile_id {
                crate::mcp::on_profile_locking(&handle);
                let deadline = std::time::Instant::now() + DRAIN_WAIT;
                let (ipc_drained, mcp_drained) = tokio::join!(
                    state.wait_for_leases(deadline),
                    crate::mcp::drain_requests(&handle, deadline)
                );
                if !ipc_drained || !mcp_drained {
                    return Err(drain_timeout());
                }
                runtime
                    .shutdown_until(&handle, deadline)
                    .await
                    .map_err(profile_drain_error)?;
            }
        }
        if state.active()?.is_some_and(|r| r.profile_id == profile_id) {
            *state
                .active
                .lock()
                .map_err(|_| "Profile runtime is unavailable.")? = None;
            crate::mcp::stop_server(&handle).await;
        }
        let registry = state.registry.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || registry.finish_delete(profile_id))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string());
        let _ = handle.emit(PROFILE_CHANGED, ());
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_profile_state(handle: AppHandle) -> Result<ProfileState, String> {
    if let Some(profiles) = handle.try_state::<NativeProfiles>() {
        return profiles.state();
    }
    let error = handle
        .state::<crate::profile_startup::ProfileStartup>()
        .error()?;
    Ok(ProfileState {
        profiles: vec![],
        pending_deletions: vec![],
        session: None,
        starting: error.is_none(),
        startup_error: error,
    })
}

#[tauri::command]
pub async fn create_profile(
    state: tauri::State<'_, NativeProfiles>,
    name: String,
    avatar_id: String,
    password: Option<String>,
) -> Result<serde_json::Value, String> {
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let password = password.map(Zeroizing::new);
        let (profile, recovery_code) = registry
            .create_with_password(&name, &avatar_id, password.as_deref().map(String::as_str))
            .map_err(|e| e.to_string())?;
        let mut result = serde_json::to_value(profile).map_err(|e| e.to_string())?;
        result["recoveryCode"] = serde_json::json!(recovery_code);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unlock_profile(
    handle: AppHandle,
    state: tauri::State<'_, NativeProfiles>,
    profile_id: Uuid,
    proof: Option<String>,
) -> Result<ProfileSession, String> {
    state.unlock(&handle, profile_id, proof).await
}

#[tauri::command]
pub async fn lock_profile(
    handle: AppHandle,
    state: tauri::State<'_, NativeProfiles>,
) -> Result<(), String> {
    let result = state.lock(&handle).await;
    if let Err(error) = &result {
        log::warn!("Profile lock failed: {error}");
    }
    result
}

#[tauri::command]
pub fn profile_activity(
    state: tauri::State<'_, NativeProfiles>,
    scope_id: Uuid,
) -> Result<(), String> {
    state
        .registry
        .sessions
        .activity(NATIVE_OWNER, scope_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_profile(
    handle: AppHandle,
    state: ProfileAccess,
    name: String,
    avatar_id: String,
) -> Result<(), String> {
    let registry = handle.state::<NativeProfiles>().registry.clone();
    let id = state.profile_id;
    tauri::async_runtime::spawn_blocking(move || registry.update(id, &name, &avatar_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_profile_password(
    handle: AppHandle,
    state: ProfileAccess,
    proof: Option<String>,
    password: Option<String>,
) -> Result<Option<String>, String> {
    let registry = handle.state::<NativeProfiles>().registry.clone();
    let id = state.profile_id;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let proof = proof.map(Zeroizing::new);
        let password = password.map(Zeroizing::new);
        registry.set_password(
            id,
            proof.as_deref().map(String::as_str),
            password.as_deref().map(String::as_str),
        )
    })
    .await
    .map_err(|error| {
        log::error!(
            "Profile password update task failed (panic={})",
            matches!(error, tauri::Error::JoinError(ref join) if join.is_panic())
        );
        "Profile password update failed.".to_string()
    })?
    .map_err(|e| e.to_string())?;
    drop(state);
    let _ = handle.state::<NativeProfiles>().lock(&handle).await;
    Ok(result)
}

#[tauri::command]
pub async fn recover_profile_password(
    handle: AppHandle,
    state: tauri::State<'_, NativeProfiles>,
    profile_id: Uuid,
    recovery_code: String,
    password: String,
) -> Result<String, String> {
    // Recovery is an authentication operation; initial setup still requires an
    // admitted session and cannot be reached through this unauthenticated route.
    let registry = state.registry.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let recovery = Zeroizing::new(recovery_code);
        let password = Zeroizing::new(password);
        let protected = registry.verify(profile_id, Some(&recovery))?;
        if !protected {
            return Err(ProfileError::Locked);
        }
        registry.set_password(profile_id, Some(&recovery), Some(&password))
    })
    .await
    .map_err(|error| {
        log::error!(
            "Profile recovery task failed (panic={})",
            matches!(error, tauri::Error::JoinError(ref join) if join.is_panic())
        );
        "Profile recovery failed.".to_string()
    })?
    .map_err(|e| e.to_string())?;
    let _ = state.lock(&handle).await;
    result.ok_or_else(|| "Profile recovery failed.".into())
}

// Backend time is authoritative; network polling never counts as user activity.
pub fn start_lock_monitor(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let Some(profiles) = handle.try_state::<NativeProfiles>() else {
                continue;
            };
            if profiles.lifecycle.try_lock().is_ok()
                && !profiles.starting.load(Ordering::SeqCst)
                && profiles.active().ok().flatten().is_some()
                && profiles
                    .registry
                    .sessions
                    .current(NATIVE_OWNER)
                    .ok()
                    .flatten()
                    .is_none()
            {
                if let Err(error) = profiles.lock(&handle).await {
                    log::warn!("Profile remains locked: {error}");
                }
            }
        }
    });
}

/// Locks the active profile before the process exits, so its database is
/// closed and its ownership released first.
///
/// Tauri ends the process with `std::process::exit` right after
/// `RunEvent::Exit`. A pool that is still open then races SQLCipher's
/// exit-time cleanup on r2d2's threads, which can crash the process.
#[cfg(desktop)]
pub fn release_for_exit<R: tauri::Runtime>(handle: &AppHandle<R>) {
    release_within(handle, EXIT_WAIT);
}

// The lock runs on the async runtime while this thread waits on a plain
// channel, so a stuck transition, command or worker cannot stop the app from
// quitting. A lock that misses the deadline keeps running until the exit.
#[cfg(desktop)]
fn release_within<R: tauri::Runtime>(handle: &AppHandle<R>, limit: std::time::Duration) {
    let (released, wait) = std::sync::mpsc::sync_channel(1);
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(profiles) = handle.try_state::<NativeProfiles>() {
            if let Err(error) = profiles.lock(&handle).await {
                log::warn!("Profile database was not released before exit: {error}");
            }
        }
        // Teardown stops the MCP server only while a profile runtime exists.
        if handle.try_state::<crate::mcp::McpServerState>().is_some() {
            crate::mcp::stop_server(&handle).await;
        }
        let _ = released.send(());
    });
    if wait.recv_timeout(limit).is_err() {
        log::warn!("Exiting before the profile database was released");
    }
}

#[cfg(test)]
mod profile_access_tests {
    use super::*;
    use std::collections::HashMap;
    use wealthfolio_core::secrets::SecretStore;

    #[derive(Default)]
    struct Secrets(Mutex<HashMap<String, String>>);

    impl SecretStore for Secrets {
        fn get_secret(&self, key: &str) -> wealthfolio_core::Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set_secret(&self, key: &str, value: &str) -> wealthfolio_core::Result<()> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete_secret(&self, key: &str) -> wealthfolio_core::Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn native_profile() -> (
        tempfile::TempDir,
        NativeProfiles,
        Arc<DatabaseRuntime>,
        Uuid,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let registry = Arc::new(
            ProfileRegistry::open(
                directory.path().to_path_buf(),
                directory.path().join("app.db"),
                Arc::new(Secrets::default()),
            )
            .unwrap(),
        );
        let id = registry
            .create("Test", wealthfolio_core::profiles::PROFILE_AVATARS[0])
            .unwrap()
            .id;
        let runtime = Arc::new(profile_runtime(&registry, id).unwrap());
        let session = registry
            .sessions
            .issue(NATIVE_OWNER, id, false, None)
            .unwrap();
        let profiles = NativeProfiles {
            registry,
            active: Mutex::new(Some(runtime.clone())),
            lifecycle: tokio::sync::Mutex::new(()),
            admission: Mutex::new(()),
            leases: Arc::new(LeaseCounter::new()),
            starting: AtomicBool::new(false),
            lock_epoch: AtomicU64::new(0),
        };
        (directory, profiles, runtime, session.scope_id)
    }

    #[test]
    fn auto_lock_closes_admission_synchronously() {
        let (directory, profiles, _runtime, _scope) = native_profile();
        let id = profiles.active().unwrap().unwrap().profile_id;
        let unprotected = profiles
            .registry
            .sessions
            .issue(NATIVE_OWNER, id, false, None)
            .unwrap();
        assert!(!profiles.begin_auto_lock().unwrap());
        profiles
            .lease(unprotected.scope_id)
            .expect("unprotected session stays admitted");

        let session = profiles
            .registry
            .sessions
            .issue(NATIVE_OWNER, id, true, None)
            .unwrap();
        assert!(profiles.begin_auto_lock().unwrap());
        assert!(profiles.lease(session.scope_id).is_err());
        assert!(profiles
            .registry
            .sessions
            .admit(NATIVE_OWNER, session.scope_id)
            .is_err());
        drop(directory);
    }

    #[tokio::test]
    async fn an_explicit_lock_closes_admission_before_a_stuck_transition() {
        let (directory, profiles, _runtime, _scope) = native_profile();
        let profiles = Arc::new(profiles);
        let id = profiles.active().unwrap().unwrap().profile_id;
        let session = profiles
            .registry
            .sessions
            .issue(NATIVE_OWNER, id, true, None)
            .unwrap();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        // Simulate a transition (unlock/delete) that never finishes.
        let transition = profiles.lifecycle.try_lock().unwrap();

        let locking = Arc::clone(&profiles);
        let handle = app.handle().clone();
        let started = std::time::Instant::now();
        let task = tokio::spawn(async move { locking.lock(&handle).await });

        // Admission is closed immediately, before the lifecycle mutex.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(profiles
            .registry
            .sessions
            .admit(NATIVE_OWNER, session.scope_id)
            .is_err());
        assert!(profiles.lease(session.scope_id).is_err());
        assert!(!task.is_finished());

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("lock must respect the absolute deadline")
            .unwrap();
        let error = result.expect_err("a stuck transition must return a drain timeout");
        assert!(error.starts_with("profile_drain_timeout"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
        drop(transition);
        drop(directory);
    }

    #[tokio::test]
    async fn holdings_ipc_requires_a_ready_database_and_authorized_profile() {
        use crate::commands::{portfolio, utilities};
        use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets};

        let (_directory, profiles, runtime, scope) = native_profile();
        let app = mock_builder()
            .manage(profiles)
            .invoke_handler(tauri::generate_handler![
                portfolio::get_holdings_list,
                utilities::get_database_startup_status,
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, NATIVE_OWNER, Default::default())
            .build()
            .unwrap();
        let invoke = |command: &str, scope: Uuid| {
            get_ipc_response(
                &window,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: if cfg!(windows) {
                        "http://tauri.localhost"
                    } else {
                        "tauri://localhost"
                    }
                    .parse()
                    .unwrap(),
                    body: tauri::ipc::InvokeBody::Json(serde_json::json!({
                        "scopeId": scope, "filter": {"type": "all"}
                    })),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
            )
            .map(|body| {
                body.deserialize::<serde_json::Value>()
                    .expect("JSON IPC response")
            })
        };

        assert_eq!(
            invoke("get_holdings_list", scope).unwrap_err(),
            "Profile database is unavailable; retry startup first"
        );
        let status = invoke("get_database_startup_status", scope).unwrap();
        assert_eq!(status["ready"], false);
        assert_eq!(status["generation"], serde_json::Value::Null);

        runtime.initialize_for_test(None).await;
        let profiles = app.state::<NativeProfiles>();
        let ready_scope = profiles
            .registry
            .sessions
            .rotate(NATIVE_OWNER, scope, runtime.generation())
            .unwrap()
            .scope_id;
        assert!(invoke("get_holdings_list", scope).is_err());
        let holdings = invoke("get_holdings_list", ready_scope).unwrap();
        assert_eq!(holdings, serde_json::json!([]));
        assert!(invoke("get_holdings_list", Uuid::new_v4()).is_err());
        profiles.registry.sessions.revoke(NATIVE_OWNER).unwrap();
        assert!(invoke("get_holdings_list", ready_scope).is_err());
        runtime.shutdown_for_test().await;
    }

    #[cfg(desktop)]
    mod exit_tests {
        use super::*;
        use std::time::{Duration, Instant};
        use wealthfolio_storage_sqlite::db::{
            maintenance, DatabaseOwner, DbAccess, DbEncryptionKey,
        };

        /// The managed state that exit teardown reaches, with the app directories
        /// kept inside the test's own directory.
        fn exit_app(
            profiles: NativeProfiles,
            root: &std::path::Path,
        ) -> tauri::App<tauri::test::MockRuntime> {
            let mut context = tauri::test::mock_context(tauri::test::noop_assets());
            context.config_mut().app.app_directories_override = Some(
                tauri::utils::config::AppDirectoriesOverride::Root(root.to_path_buf()),
            );
            tauri::test::mock_builder()
                .manage(profiles)
                .manage(crate::mcp::McpServerState::default())
                .build(context)
                .unwrap()
        }

        #[test]
        fn quitting_releases_the_profile_database_before_the_process_exits() {
            let (directory, profiles, runtime, _scope) = native_profile();
            let key = Arc::new(DbEncryptionKey::generate());
            tauri::async_runtime::block_on(runtime.initialize_for_test(Some(key.clone())));
            let database = DbAccess::encrypted(runtime.database_path(), key);
            drop(runtime);
            let app = exit_app(profiles, directory.path());
            assert!(DatabaseOwner::acquire(database.path()).is_err());

            release_for_exit(app.handle());

            // Tauri calls `std::process::exit` next. Ownership is released only
            // once every pooled connection has closed.
            DatabaseOwner::acquire(database.path()).expect("quitting must release the database");
            maintenance::check_sqlite_locks(&database).unwrap();
            assert!(app.state::<NativeProfiles>().active().unwrap().is_none());
        }

        #[test]
        fn quitting_does_not_wait_for_a_stuck_profile_transition() {
            let (directory, profiles, _runtime, _scope) = native_profile();
            let app = exit_app(profiles, directory.path());
            let profiles = app.state::<NativeProfiles>();
            // An unlock or deletion that never finishes keeps the lifecycle lock.
            let transition = profiles.lifecycle.try_lock().unwrap();

            let started = Instant::now();
            release_within(app.handle(), Duration::from_millis(200));

            assert!(started.elapsed() < Duration::from_secs(2));
            assert!(profiles.active().unwrap().is_some());
            drop(transition);
        }
    }
}
