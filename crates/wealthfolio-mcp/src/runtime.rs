//! Per-request runtime resolution.
//!
//! The listener outlives any one ledger runtime and must not hold business
//! services, repositories or a connection pool. Each request asks the host for
//! a short-lived lease instead; the host supplies the current environment,
//! PAT repository, ledger id and audit sink. Dropping the lease ends the
//! request's claim on the runtime, which is what makes profiled shutdown
//! waitable.

use std::sync::Arc;

use wealthfolio_agent_tools::AgentEnvironment;
use wealthfolio_storage_sqlite::agent::PatRepository;

use crate::audit::AuditSink;

/// Why a request cannot reach a ledger right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAvailability {
    /// The listener has no active, unlocked ledger.
    Locked,
    /// No listener/runtime is serving this host at all.
    Unavailable,
}

/// The services one request may use. Every field is a clone owned by the
/// request for as long as its lease lives.
#[derive(Clone)]
pub struct McpCallRuntime {
    pub ledger_id: String,
    pub env: Arc<dyn AgentEnvironment>,
    pub pat_repository: Arc<PatRepository>,
    pub audit: Option<Arc<dyn AuditSink>>,
    /// Increases whenever the ledger locks. Sessions created under an older
    /// epoch are revoked even if the process keeps the listener up.
    pub session_epoch: u64,
}

impl std::fmt::Debug for McpCallRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpCallRuntime")
            .field("ledger_id", &self.ledger_id)
            .finish_non_exhaustive()
    }
}

/// A request's claim on one ledger runtime. Releasing it (drop) lets profile
/// shutdown proceed.
pub trait McpLease: Send + Sync {
    fn runtime(&self) -> &McpCallRuntime;
}

/// Host-side lease factory. Implementations must register the lease before
/// returning and release it when the lease is dropped.
pub trait McpRuntimeProvider: Send + Sync {
    fn acquire(&self) -> Result<Box<dyn McpLease>, McpAvailability>;

    /// State for discovery/status without acquiring a lease.
    fn status(&self) -> McpAvailability;
}

/// A provider over one already-installed runtime. Used by tests and by hosts
/// that intentionally bind a listener to a single ledger.
pub struct StaticRuntimeProvider {
    runtime: McpCallRuntime,
}

impl StaticRuntimeProvider {
    pub fn new(runtime: McpCallRuntime) -> Self {
        Self { runtime }
    }

    pub fn from_parts(
        ledger_id: impl Into<String>,
        env: Arc<dyn AgentEnvironment>,
        pat_repository: Arc<PatRepository>,
        audit: Option<Arc<dyn AuditSink>>,
    ) -> Self {
        Self::new(McpCallRuntime {
            ledger_id: ledger_id.into(),
            env,
            pat_repository,
            audit,
            session_epoch: 0,
        })
    }
}

pub struct StaticLease {
    runtime: McpCallRuntime,
}

impl McpLease for StaticLease {
    fn runtime(&self) -> &McpCallRuntime {
        &self.runtime
    }
}

impl McpRuntimeProvider for StaticRuntimeProvider {
    fn acquire(&self) -> Result<Box<dyn McpLease>, McpAvailability> {
        Ok(Box::new(StaticLease {
            runtime: self.runtime.clone(),
        }))
    }

    fn status(&self) -> McpAvailability {
        McpAvailability::Locked
    }
}
