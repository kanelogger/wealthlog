//! Activities module - domain models, services, and traits.

mod activities_constants;
mod activities_errors;
mod activities_model;
mod activities_service;
mod activities_traits;
mod activity_cash_migration;
mod compiler;
mod idempotency;
mod transfer_pairs;

#[cfg(test)]
pub(crate) mod activities_service_tests;

#[cfg(test)]
mod activities_model_tests;

pub use activities_constants::*;
pub use activities_errors::ActivityError;
pub use activities_model::{effective_activity_type, stored_type_override, type_override};
pub use activities_model::{
    parse_decimal_string_tolerant, Activity, ActivityBulkIdentifierMapping,
    ActivityBulkMutationError, ActivityBulkMutationRequest, ActivityBulkMutationResult,
    ActivityDetails, ActivityFinalCashMigrationResult, ActivityFinalCashMigrationUpdate,
    ActivityFinalCashMigrationWriteResult, ActivitySearchResponse, ActivitySearchResponseMeta,
    ActivityStatus, ActivityType, ActivityUpdate, ActivityUpdatePreview, ActivityUpsert,
    AssetResolutionInput, BulkUpsertResult, IncomeData, InternalTransferPairRequest,
    InternalTransferPairResponse, LinkedTransfer, NewActivity, PrepareActivitiesResult,
    PreviewedActivityUpdate, Sort, TransferMatchCandidate, TransferMatchCandidateRequest,
    UnlinkedTransfer, UnlinkedTransfers, UnlinkedTransfersRequest,
};
pub use activities_service::ActivityService;
pub use activities_traits::{ActivityRepositoryTrait, ActivityServiceTrait};
pub use activity_cash_migration::{
    get_final_cash_migration_status, rebuild_pending_final_cash_accounts,
    record_final_cash_rebuild_attempt, run_final_cash_migration, ActivityFinalCashMigrationStatus,
};
pub use compiler::{ActivityCompiler, DefaultActivityCompiler};
pub use idempotency::{
    compute_activity_idempotency_key, compute_idempotency_key, generate_manual_idempotency_key,
};
pub use transfer_pairs::{
    is_contribution_neutral_same_account_cash_fx_conversion, is_same_account_cash_fx_conversion,
    InvalidTransferGroup, TransferLinkState, TransferPair, TransferPairResolution,
};
