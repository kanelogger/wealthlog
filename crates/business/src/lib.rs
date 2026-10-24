//! Unified business interface for Wealthlog.
//!
//! GUI, REST, CLI and MCP share the use cases here instead of re-implementing
//! accounting rules per transport. The plan this implements is
//! `docs/business-interface-plan.md`; the matrices it relies on are in
//! `docs/business-interface-matrices.md`.
//!
//! Two entry layers exist:
//!
//! - [`service::BusinessService`] — the application layer. It owns the
//!   scope checks, versioned writes, request replay and audit trail for
//!   ordinary income/expense activity.
//! - [`pending::PendingService`] — pending entries (待确认收支) that may be
//!   incomplete and become a single `POSTED` activity when confirmed.
//!
//! The core accounting service remains the only place money is computed; this
//! crate only sequences it with the extra records the plan requires.

pub mod audit;
pub mod pending;
pub mod requests;
pub mod service;
pub mod store;

pub use audit::{BusinessAuditEntry, BusinessChannel, BusinessOutcome};
pub use pending::{
    PendingActivity, PendingActivityStatus, PendingConfirmResult, PendingCreate, PendingService,
    PendingUpdate,
};
pub use requests::{RequestOutcome, RequestRecord, RequestReplay};
pub use service::{
    ActivityVersion, BusinessService, CreateOrdinaryActivity, OrdinaryActivityDetail,
    OrdinaryActivityError, OrdinaryActivityPage, OrdinaryActivityQuery, OrdinaryActivityResult,
    OrdinaryActivityRow, UpdateOrdinaryActivity,
};
pub use store::BusinessStore;
