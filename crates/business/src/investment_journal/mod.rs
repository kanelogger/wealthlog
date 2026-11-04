//! Investment records and ownership, separate from the ledger's asset facts.
//!
//! This module is internal until all three implementation stages pass. Money
//! and quantities never travel through a floating point DTO.

mod accounting;
mod context;
mod decisions;
mod model;
mod ownership;
mod preview;
mod principles;
mod projection;
mod protocol;
mod reconciliation;
mod reviews;
mod service;
mod sources;
mod transfers;

pub use accounting::AccountingResourceReader;
pub use context::*;
pub use decisions::*;
pub use model::*;
pub use ownership::*;
pub use preview::*;
pub use principles::*;
pub use projection::*;
pub use protocol::*;
pub use reconciliation::*;
pub use reviews::*;
pub use service::*;
pub use sources::{CashTransferSnapshot, SourceReferenceState, UsedSourceReference};
pub use transfers::{apportion_transfer, TransferShare};

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("investment permission denied")]
    PermissionDenied,
    #[error("investment state changed; preview again")]
    StateConflict,
    #[error("requestKey already identifies different content")]
    RequestConflict,
    #[error("resource balance is unavailable")]
    ResourceUnavailable,
    #[error("allocation would exceed the available resource")]
    InsufficientResource,
    #[error("purpose ownership cannot become negative")]
    NegativeAllocation,
    #[error("trade requires reconciliation before ownership conversion")]
    ReconciliationRequired,
    #[error("this source already has an ownership application")]
    AlreadyApplied,
    #[error("an activity with this deduplication identity already exists")]
    DuplicateActivity,
    #[error(transparent)]
    Storage(#[from] wealthfolio_core::errors::Error),
}

pub type JournalResult<T> = std::result::Result<T, JournalError>;

impl JournalError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::PermissionDenied => "scope_denied",
            Self::StateConflict => "state_conflict",
            Self::RequestConflict => "request_conflict",
            Self::ResourceUnavailable => "resource_unavailable",
            Self::InsufficientResource => "insufficient_resource",
            Self::NegativeAllocation => "negative_allocation",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::AlreadyApplied => "already_applied",
            Self::DuplicateActivity => "duplicate_activity",
            Self::Storage(_) => "storage_error",
        }
    }
}

impl From<crate::investments::InvestmentError> for JournalError {
    fn from(error: crate::investments::InvestmentError) -> Self {
        match error {
            crate::investments::InvestmentError::StateConflict => Self::StateConflict,
            crate::investments::InvestmentError::RequestConflict => Self::RequestConflict,
            crate::investments::InvestmentError::DuplicateActivity => Self::DuplicateActivity,
            crate::investments::InvestmentError::InvalidInput(message)
            | crate::investments::InvestmentError::OutOfScope(message) => {
                Self::InvalidInput(message)
            }
            crate::investments::InvestmentError::Storage(error) => Self::Storage(error),
        }
    }
}

impl From<diesel::result::Error> for JournalError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Storage(wealthfolio_storage_sqlite::errors::StorageError::from(error).into())
    }
}
