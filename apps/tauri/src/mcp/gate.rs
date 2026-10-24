//! Per-request leases over the active ledger runtime.
//!
//! The MCP listener lives across ledger switches and locks. It never owns the
//! business services or the connection pool: every request asks this gate for
//! a short lease, and profile shutdown waits for those leases before closing
//! the writer and releasing database ownership.
//!
//! Admission, generation confirmation and lease registration happen under one
//! mutex, so a lock that starts after a request was admitted cannot miss its
//! lease, and a request that starts after the lock cannot slip in.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use uuid::Uuid;

use wealthfolio_mcp::runtime::{McpAvailability, McpCallRuntime, McpLease, McpRuntimeProvider};

use crate::database::DatabaseRuntime;

struct Active {
    ledger_id: String,
    runtime: Weak<DatabaseRuntime>,
    generation: Option<Uuid>,
    locked: bool,
}

pub struct LedgerGate {
    inner: Arc<GateInner>,
}

struct GateInner {
    active: Mutex<Option<Active>>,
    leases: AtomicUsize,
    session_epoch: std::sync::atomic::AtomicU64,
    changed: tokio::sync::Notify,
}

impl Clone for LedgerGate {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Default for LedgerGate {
    fn default() -> Self {
        Self::new()
    }
}

impl LedgerGate {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(GateInner {
                active: Mutex::new(None),
                leases: AtomicUsize::new(0),
                session_epoch: std::sync::atomic::AtomicU64::new(0),
                changed: tokio::sync::Notify::new(),
            }),
        }
    }

    /// Point the listener at a freshly installed runtime. Clears the lock.
    pub fn activate(&self, runtime: &Arc<DatabaseRuntime>, ledger_id: impl Into<String>) {
        let generation = runtime.generation();
        *self.inner.lock_active() = Some(Active {
            ledger_id: ledger_id.into(),
            runtime: Arc::downgrade(runtime),
            generation,
            locked: false,
        });
        self.inner.changed.notify_waiters();
    }

    /// Reject new requests immediately. Already-admitted leases keep their
    /// claim until they finish.
    pub fn lock(&self) {
        self.inner.session_epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(active) = self.inner.lock_active().as_mut() {
            active.locked = true;
        }
        self.inner.changed.notify_waiters();
    }

    pub fn clear(&self) {
        *self.inner.lock_active() = None;
        self.inner.changed.notify_waiters();
    }

    pub fn is_locked(&self) -> bool {
        match &*self.inner.lock_active() {
            Some(active) => active.locked,
            None => true,
        }
    }

    pub fn ledger_id(&self) -> Option<String> {
        self.inner
            .lock_active()
            .as_ref()
            .map(|active| active.ledger_id.clone())
    }

    #[cfg(test)]
    pub fn lease_count(&self) -> usize {
        self.inner.leases.load(Ordering::SeqCst)
    }

    /// Wait until no admitted request remains, or the absolute deadline passes.
    /// Returns false on timeout; callers keep ownership and must not force
    /// release anything.
    pub async fn drain(&self, deadline: Instant) -> bool {
        loop {
            let notified = self.inner.changed.notified();
            if self.inner.leases.load(Ordering::SeqCst) == 0 {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            if tokio::time::timeout_at(deadline.into(), notified)
                .await
                .is_err()
            {
                return self.inner.leases.load(Ordering::SeqCst) == 0;
            }
        }
    }

    fn acquire_lease(&self) -> Result<GateLease, McpAvailability> {
        let mut guard = self.inner.lock_active();
        let active = guard.as_mut().ok_or(McpAvailability::Unavailable)?;
        if active.locked {
            return Err(McpAvailability::Locked);
        }
        let runtime = active
            .runtime
            .upgrade()
            .ok_or(McpAvailability::Unavailable)?;
        // Confirming the generation under the same lock is what makes a stale
        // runtime unrepresentable after a rebuild.
        if runtime.generation() != active.generation {
            return Err(McpAvailability::Locked);
        }
        let context = runtime
            .try_context()
            .filter(|context| context.is_active())
            .ok_or(McpAvailability::Locked)?;
        // Register before releasing the mutex so `lock()` can never observe
        // zero leases between admission and registration.
        self.inner.leases.fetch_add(1, Ordering::SeqCst);

        let audit = crate::mcp::build_audit_sink(&context);
        let call = McpCallRuntime {
            ledger_id: active.ledger_id.clone(),
            env: context.agent_environment(),
            pat_repository: context.pat_repository(),
            audit,
            session_epoch: self.inner.session_epoch.load(Ordering::SeqCst),
        };
        drop(guard);
        Ok(GateLease {
            gate: Arc::clone(&self.inner),
            runtime: call,
        })
    }
}

struct GateLease {
    gate: Arc<GateInner>,
    runtime: McpCallRuntime,
}

impl GateInner {
    fn lock_active(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl McpLease for GateLease {
    fn runtime(&self) -> &McpCallRuntime {
        &self.runtime
    }
}

impl Drop for GateLease {
    fn drop(&mut self) {
        self.gate.leases.fetch_sub(1, Ordering::SeqCst);
        self.gate.changed.notify_waiters();
    }
}

impl McpRuntimeProvider for LedgerGate {
    fn acquire(&self) -> Result<Box<dyn McpLease>, McpAvailability> {
        self.acquire_lease()
            .map(|lease| Box::new(lease) as Box<dyn McpLease>)
    }

    fn status(&self) -> McpAvailability {
        let guard = self.inner.lock_active();
        match guard.as_ref() {
            None => McpAvailability::Unavailable,
            Some(active) if active.locked => McpAvailability::Locked,
            Some(active) if active.runtime.strong_count() == 0 => McpAvailability::Unavailable,
            Some(_) => McpAvailability::Locked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn runtime() -> (tempfile::TempDir, Arc<DatabaseRuntime>) {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(DatabaseRuntime::new(
            dir.path().to_string_lossy().into_owned(),
        ));
        (dir, runtime)
    }

    #[tokio::test]
    async fn a_lock_blocks_new_leases_and_waits_for_admitted_work() {
        let (_dir, runtime) = runtime();
        runtime.initialize_for_test(None).await;
        let gate = Arc::new(LedgerGate::new());
        gate.activate(&runtime, "ledger-a");

        let lease = gate.acquire_lease().expect("first lease");
        gate.lock();
        assert!(gate.acquire_lease().is_err());
        assert_eq!(gate.lease_count(), 1);

        let drain_gate = Arc::clone(&gate);
        let deadline = Instant::now() + Duration::from_secs(1);
        let drain = tokio::spawn(async move { drain_gate.drain(deadline).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!drain.is_finished());
        drop(lease);
        assert!(drain.await.unwrap());
        assert_eq!(gate.lease_count(), 0);
        runtime.shutdown_for_test().await;
    }

    #[tokio::test]
    async fn a_timed_out_drain_keeps_the_lease_and_reports_timeout() {
        let (_dir, runtime) = runtime();
        runtime.initialize_for_test(None).await;
        let gate = Arc::new(LedgerGate::new());
        gate.activate(&runtime, "ledger-a");
        let lease = gate.acquire_lease().expect("lease");
        gate.lock();
        let deadline = Instant::now() + Duration::from_millis(50);
        assert!(!gate.drain(deadline).await);
        assert_eq!(gate.lease_count(), 1);
        drop(lease);
        assert!(gate.drain(Instant::now() + Duration::from_secs(1)).await);
        runtime.shutdown_for_test().await;
    }

    #[tokio::test]
    async fn a_stale_runtime_generation_cannot_acquire_a_lease() {
        let (_dir, runtime) = runtime();
        runtime.initialize_for_test(None).await;
        let gate = Arc::new(LedgerGate::new());
        gate.activate(&runtime, "ledger-a");
        runtime.shutdown_for_test().await;
        assert!(gate.acquire_lease().is_err());
    }
}
