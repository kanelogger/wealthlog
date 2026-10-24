//! Ordinary income/expense use cases.
//!
//! The scope and branch rules are the ones in
//! `docs/business-interface-plan.md` §2. The core [`ActivityServiceTrait`]
//! remains the only accounting engine: this layer validates the ordinary
//! income/expense envelope, sequences the write, and records the request,
//! version and audit facts the plan requires.

use std::sync::Arc;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use wealthfolio_core::accounts::{AccountPurpose, AccountServiceTrait};
use wealthfolio_core::activities::{
    Activity, ActivityServiceTrait, ActivityStatus, NewActivity, ACTIVITY_TYPE_DEPOSIT,
    ACTIVITY_TYPE_WITHDRAWAL,
};
use wealthfolio_spending::settings::SpendingSettingsService;

use crate::audit::{BusinessAuditEntry, BusinessChannel};
use crate::requests::{RequestOutcome, RequestRecord};
use crate::store::BusinessStore;

/// One ordinary activity row returned by the scoped query.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryActivityRow {
    pub id: String,
    pub account_id: String,
    pub activity_type: String,
    pub activity_date: String,
    pub currency: String,
    pub amount: Option<String>,
    pub fee: Option<String>,
    pub tax: Option<String>,
    pub notes: Option<String>,
    pub status: String,
    pub version: i32,
}

/// One ordinary activity with every field the edit form needs.
///
/// Query rows stay lean; the single read is the contract an editor loads
/// before an update, so it includes the optional FX, classification and
/// spending-event fields too.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryActivityDetail {
    pub id: String,
    pub account_id: String,
    pub activity_type: String,
    pub activity_date: String,
    pub currency: String,
    pub amount: Option<String>,
    pub fee: Option<String>,
    pub tax: Option<String>,
    pub notes: Option<String>,
    pub status: String,
    pub version: i32,
    pub fx_rate: Option<String>,
    pub taxonomy_id: Option<String>,
    pub category_id: Option<String>,
    pub event_id: Option<String>,
}

/// Paginated result with stable ordering and a pre-filter total.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryActivityPage {
    pub rows: Vec<OrdinaryActivityRow>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
}

/// Query for ordinary activities: scope and status filter first, then count
/// and page. Defaults to 50, capped at 200.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryActivityQuery {
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
    #[serde(default)]
    pub account_ids: Option<Vec<String>>,
    #[serde(default)]
    pub date_from: Option<String>,
    #[serde(default)]
    pub date_to: Option<String>,
}

/// The version returned with every successful write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityVersion {
    pub version: i32,
}

/// One ordinary income/expense creation.
///
/// Amounts are decimal strings end to end; parsing happens here, never in the
/// transport.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrdinaryActivity {
    pub account_id: String,
    /// `DEPOSIT` or `WITHDRAWAL` only.
    pub activity_type: String,
    pub activity_date: String,
    pub currency: String,
    pub amount: Decimal,
    #[serde(default)]
    pub fee: Option<Decimal>,
    #[serde(default)]
    pub tax: Option<Decimal>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Frozen by the form on open; the same key retries to the same result.
    pub request_key: String,
    /// Stable idempotency key derived from the record identity, so the old
    /// path's duplicate protection keeps working across retries.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Optional classification follow-up written in the same transaction.
    /// Omitted or null means no assignment.
    #[serde(default, deserialize_with = "deserialize_optional_string_patch")]
    pub taxonomy_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_string_patch")]
    pub category_id: Option<Option<String>>,
    /// Optional spending event tag written in the same transaction.
    #[serde(default)]
    pub event_id: Option<String>,
    /// Decimal-string FX rate, when the amount is not in the account currency.
    #[serde(default)]
    pub fx_rate: Option<Decimal>,
}

/// One ordinary income/expense modification.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrdinaryActivity {
    pub activity_id: String,
    /// Version the editor loaded; a mismatch is a conflict.
    pub expected_version: i32,
    pub account_id: String,
    /// The new direction; DEPOSIT and WITHDRAWAL may swap.
    pub activity_type: String,
    pub activity_date: String,
    pub currency: String,
    pub amount: Decimal,
    /// On update: omitted preserves, null clears.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub fee: Option<Option<Decimal>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub tax: Option<Option<Decimal>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub request_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Classification patch: omitted preserves, null clears.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub taxonomy_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub category_id: Option<Option<String>>,
    /// Spending event tag; `clear_event` removes an existing one.
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub clear_event: bool,
    /// Decimal-string FX rate. On update: omitted preserves, null clears.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub fx_rate: Option<Option<Decimal>>,
}

/// Distinguishes an omitted field (`None`) from an explicit JSON null
/// (`Some(None)`) for patch semantics.
#[doc(hidden)]
pub fn deserialize_optional_decimal_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<Decimal>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::String(text)) => text
            .parse::<Decimal>()
            .map(|value| Some(Some(value)))
            .map_err(serde::de::Error::custom),
        Some(serde_json::Value::Number(number)) => number
            .to_string()
            .parse::<Decimal>()
            .map(|value| Some(Some(value)))
            .map_err(serde::de::Error::custom),
        Some(_) => Err(serde::de::Error::custom(
            "fxRate must be a decimal string or null",
        )),
    }
}

/// Distinguishes an omitted string field (`None`) from an explicit JSON null
/// (`Some(None)`) for patch semantics.
#[doc(hidden)]
pub fn deserialize_optional_string_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Ok(Some(None))
            } else {
                Ok(Some(Some(trimmed.to_string())))
            }
        }
        Some(_) => Err(serde::de::Error::custom(
            "expected a string or null for this field",
        )),
    }
}

/// Successful result, including the facts the plan says to return.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryActivityResult {
    #[schema(value_type = Object)]
    pub activity: Activity,
    pub version: i32,
    pub request_id: String,
    /// True when this call returned a stored result instead of writing.
    pub replayed: bool,
}

/// Stable error codes.
#[derive(Debug, Error)]
pub enum OrdinaryActivityError {
    #[error("unknown activity: {0}")]
    UnknownActivity(String),
    #[error("activity {activity_id} is not scope-eligible: {reason}")]
    OutOfScope { activity_id: String, reason: String },
    #[error("version conflict: expected {expected}, stored {actual}")]
    VersionConflict { expected: i32, actual: i32 },
    #[error("request key {key} was already used with different content")]
    RequestConflict { key: String },
    #[error("invalid amount: {0}")]
    InvalidAmount(String),
    /// A create matched an activity that already carries this dedupe identity.
    ///
    /// The message deliberately does not claim the existing row still matches
    /// the submitted fields: the key identifies the row, not its current state.
    #[error("duplicate activity: an activity with this deduplication key already exists")]
    DuplicateActivity {
        existing_activity_id: Option<String>,
    },
    #[error("validation failed: {0}")]
    InvalidInput(String),
    #[error("busy: {0}")]
    Conflict(String),
    #[error("storage error: {0}")]
    Storage(wealthfolio_core::errors::Error),
}

impl OrdinaryActivityError {
    /// Stable code consumers switch on.
    pub fn code(&self) -> &'static str {
        match self {
            OrdinaryActivityError::UnknownActivity(_) => "activity_not_found",
            OrdinaryActivityError::OutOfScope { .. } => "activity_out_of_scope",
            OrdinaryActivityError::VersionConflict { .. } => "version_conflict",
            OrdinaryActivityError::RequestConflict { .. } => "request_conflict",
            OrdinaryActivityError::InvalidAmount(_) => "invalid_amount",
            OrdinaryActivityError::DuplicateActivity { .. } => "duplicate_activity",
            OrdinaryActivityError::InvalidInput(_) => "invalid_input",
            OrdinaryActivityError::Conflict(_) => "request_in_progress",
            OrdinaryActivityError::Storage(_) => "storage_error",
        }
    }

    /// Whether submitting the same request again could succeed. A duplicate
    /// never can: the dedupe identity already exists and clients must not
    /// silently mint a new key.
    pub fn retryable(&self) -> bool {
        match self {
            OrdinaryActivityError::DuplicateActivity { .. }
            | OrdinaryActivityError::InvalidAmount(_)
            | OrdinaryActivityError::InvalidInput(_)
            | OrdinaryActivityError::UnknownActivity(_)
            | OrdinaryActivityError::OutOfScope { .. }
            | OrdinaryActivityError::VersionConflict { .. }
            | OrdinaryActivityError::RequestConflict { .. } => false,
            OrdinaryActivityError::Conflict(_) | OrdinaryActivityError::Storage(_) => true,
        }
    }

    /// The existing activity id when a duplicate was identified reliably.
    pub fn existing_activity_id(&self) -> Option<&str> {
        match self {
            OrdinaryActivityError::DuplicateActivity {
                existing_activity_id,
            } => existing_activity_id.as_deref(),
            _ => None,
        }
    }

    /// Legacy GUI/IPC text for the pre-existing ordinary endpoints. A
    /// duplicate used to surface as a storage-wrapped core error; keep that
    /// exact shape while CLI/MCP use the typed structured error.
    pub fn legacy_entry_message(&self) -> String {
        match self {
            OrdinaryActivityError::DuplicateActivity {
                existing_activity_id,
            } => format!(
                "storage error: Activity error: Invalid data: Duplicate activity detected. A matching activity already exists{}.",
                existing_activity_id
                    .as_ref()
                    .map(|id| format!(" (id: {id})"))
                    .unwrap_or_default()
            ),
            other => other.to_string(),
        }
    }

    /// Legacy REST body: the old adapter prefixed the business code and the
    /// duplicate case used `storage_error`.
    pub fn legacy_rest_message(&self) -> String {
        match self {
            OrdinaryActivityError::DuplicateActivity { .. } => {
                format!("storage_error: {}", self.legacy_entry_message())
            }
            other => format!("{}: {other}", other.code()),
        }
    }

    /// Stable structured error body for CLI and MCP.
    pub fn structured_value(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code(),
            "message": self.to_string(),
            "retryable": self.retryable(),
            "requestId": serde_json::Value::Null,
            "data": match self {
                OrdinaryActivityError::DuplicateActivity { .. } => serde_json::json!({
                    "existingActivityId": self.existing_activity_id(),
                }),
                OrdinaryActivityError::VersionConflict { expected, actual } => serde_json::json!({
                    "expectedVersion": expected,
                    "actualVersion": actual,
                }),
                OrdinaryActivityError::OutOfScope { activity_id, reason } => serde_json::json!({
                    "activityId": activity_id,
                    "reason": reason,
                }),
                _ => serde_json::Value::Null,
            },
        })
    }
}

impl From<wealthfolio_core::errors::Error> for OrdinaryActivityError {
    fn from(error: wealthfolio_core::errors::Error) -> Self {
        match &error {
            wealthfolio_core::errors::Error::Activity(
                wealthfolio_core::activities::ActivityError::DuplicateActivity {
                    existing_activity_id,
                },
            ) => OrdinaryActivityError::DuplicateActivity {
                existing_activity_id: existing_activity_id.clone(),
            },
            // The core pre-check can miss a concurrent writer; the unique
            // index on `activities.idempotency_key` is the authoritative
            // duplicate signal. Only that constraint maps here — other unique
            // violations stay storage errors.
            wealthfolio_core::errors::Error::Database(
                wealthfolio_core::errors::DatabaseError::UniqueViolation(message),
            ) if is_idempotency_unique_violation(message) => {
                OrdinaryActivityError::DuplicateActivity {
                    existing_activity_id: None,
                }
            }
            wealthfolio_core::errors::Error::Database(
                wealthfolio_core::errors::DatabaseError::Internal(message),
            ) if is_idempotency_unique_violation(message) => {
                OrdinaryActivityError::DuplicateActivity {
                    existing_activity_id: None,
                }
            }
            _ => OrdinaryActivityError::Storage(error),
        }
    }
}

fn is_idempotency_unique_violation(message: &str) -> bool {
    message.contains("activities.idempotency_key")
        && (message.contains("UNIQUE constraint failed")
            || message.contains("Unique constraint violation"))
}

impl From<anyhow::Error> for OrdinaryActivityError {
    fn from(error: anyhow::Error) -> Self {
        OrdinaryActivityError::InvalidInput(error.to_string())
    }
}

/// Application-layer service for ordinary income/expense.
pub struct BusinessService {
    activity_service: Arc<dyn ActivityServiceTrait>,
    account_service: Arc<dyn AccountServiceTrait>,
    spending_settings: Arc<SpendingSettingsService>,
    store: BusinessStore,
    ledger_id: String,
}

/// Reads the classification patch off a caller payload. A null on either side
/// clears the single-select assignment; both sides present assign.
fn assignment_patch(
    taxonomy_id: &Option<Option<String>>,
    category_id: &Option<Option<String>>,
) -> Option<Option<(String, String)>> {
    match (taxonomy_id, category_id) {
        (None, None) => None,
        (Some(Some(taxonomy)), Some(Some(category))) => {
            Some(Some((taxonomy.clone(), category.clone())))
        }
        _ => Some(None),
    }
}

impl BusinessService {
    pub fn new(
        activity_service: Arc<dyn ActivityServiceTrait>,
        account_service: Arc<dyn AccountServiceTrait>,
        spending_settings: Arc<SpendingSettingsService>,
        store: BusinessStore,
        ledger_id: impl Into<String>,
    ) -> Self {
        Self {
            activity_service,
            account_service,
            spending_settings,
            store,
            ledger_id: ledger_id.into(),
        }
    }

    pub fn ledger_id(&self) -> &str {
        &self.ledger_id
    }

    /// Create one ordinary income/expense activity.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_ordinary(
        &self,
        mut input: CreateOrdinaryActivity,
        channel: BusinessChannel,
        actor: &str,
        authenticated: bool,
    ) -> Result<RequestOutcome<OrdinaryActivityResult>, OrdinaryActivityError> {
        validate_request_key(&input.request_key)?;
        if let Some(key) = input.idempotency_key.as_deref() {
            let trimmed = key.trim();
            if trimmed.is_empty() {
                return Err(OrdinaryActivityError::InvalidInput(
                    "idempotencyKey must not be blank".to_string(),
                ));
            }
            input.idempotency_key = Some(trimmed.to_string());
        }
        validate_activity_type(&input.activity_type)?;
        validate_amount(input.amount, "amount")?;
        if let Some(fee) = input.fee {
            validate_optional_amount(fee, "fee")?;
        }
        if let Some(tax) = input.tax {
            validate_optional_amount(tax, "tax")?;
        }

        let snapshot = serde_json::to_value(&input).unwrap_or(serde_json::Value::Null);
        let record = RequestRecord::new(
            &self.ledger_id,
            actor,
            "ordinary_activity_create",
            &input.request_key,
            snapshot,
        );
        if let Some(replayed) = self.replay_completed(&record)? {
            return Ok(replayed);
        }
        // New work only: a completed request replays above even if the account
        // has since been archived or dropped from spending.
        self.ensure_account_eligible(&input.account_id).await?;

        let mut new_activity = NewActivity {
            id: None,
            account_id: input.account_id.clone(),
            asset: None,
            activity_type: input.activity_type.clone(),
            subtype: None,
            activity_date: input.activity_date.clone(),
            quantity: None,
            unit_price: None,
            currency: input.currency.clone(),
            fee: input.fee,
            tax: input.tax,
            amount: Some(input.amount),
            status: Some(ActivityStatus::Posted),
            notes: input.notes.clone(),
            fx_rate: input.fx_rate,
            metadata: None,
            source_system: Some("MANUAL".to_string()),
            source_record_id: None,
            source_group_id: None,
            idempotency_key: input.idempotency_key.clone(),
            needs_review: Some(false),
        };
        new_activity.amount = Some(input.amount.abs());
        let prepared = self
            .activity_service
            .prepare_activity_for_commit(new_activity)
            .await?;
        let extras = crate::store::ActivityExtras {
            assignment: assignment_patch(&input.taxonomy_id, &input.category_id),
            event_id: input.event_id.clone(),
            clear_event: false,
        };
        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            channel,
            actor,
            authenticated,
            "ordinary_activity_create",
            "activity",
        )
        .detail(serde_json::json!({ "activityType": input.activity_type }));
        let plan = crate::store::OrdinaryCommitPlan {
            ledger_id: record.ledger_id.clone(),
            subject: record.subject.clone(),
            operation: record.operation.clone(),
            request_key: record.request_key.clone(),
            request_hash: record.request_hash.clone(),
            request_snapshot: serde_json::to_string(&record.request_snapshot).unwrap_or_default(),
            kind: crate::store::OrdinaryCommitKind::Create(prepared),
            extras,
            audit: audit.to_row(),
            build_result: Box::new(|activity, version, request_id| {
                serde_json::to_value(OrdinaryActivityResult {
                    activity: activity.clone(),
                    version,
                    request_id: request_id.to_string(),
                    replayed: false,
                })
                .unwrap_or_default()
            }),
        };
        match self.store.commit_ordinary(plan).await? {
            crate::store::OrdinaryCommitOutcome::Committed {
                activity,
                version,
                request_id,
            } => {
                self.activity_service
                    .notify_activity_committed(None, &activity);
                Ok(RequestOutcome::Executed(OrdinaryActivityResult {
                    activity: *activity,
                    version,
                    request_id,
                    replayed: false,
                }))
            }
            crate::store::OrdinaryCommitOutcome::Replayed(stored) => decode_stored_result(stored),
            crate::store::OrdinaryCommitOutcome::RequestConflict => {
                Err(OrdinaryActivityError::RequestConflict {
                    key: input.request_key,
                })
            }
            crate::store::OrdinaryCommitOutcome::VersionConflict { expected, actual } => {
                Err(OrdinaryActivityError::VersionConflict { expected, actual })
            }
        }
    }

    /// Update one ordinary income/expense activity.
    pub async fn update_ordinary(
        &self,
        input: UpdateOrdinaryActivity,
        channel: BusinessChannel,
        actor: &str,
        authenticated: bool,
    ) -> Result<RequestOutcome<OrdinaryActivityResult>, OrdinaryActivityError> {
        validate_request_key(&input.request_key)?;
        // Updates always keep the existing dedupe key; accepting one here
        // would let a client rewrite identity after the fact.
        if input.idempotency_key.is_some() {
            return Err(OrdinaryActivityError::InvalidInput(
                "idempotencyKey is not accepted when updating an activity".to_string(),
            ));
        }
        validate_activity_type(&input.activity_type)?;
        validate_amount(input.amount, "amount")?;
        if let Some(Some(fee)) = input.fee {
            validate_optional_amount(fee, "fee")?;
        }
        if let Some(Some(tax)) = input.tax {
            validate_optional_amount(tax, "tax")?;
        }
        let snapshot = serde_json::to_value(&input).unwrap_or(serde_json::Value::Null);
        let record = RequestRecord::new(
            &self.ledger_id,
            actor,
            "ordinary_activity_update",
            &input.request_key,
            snapshot,
        );
        // Replay first: a retry after a broker-direction correction must return
        // the stored result even though the record now carries an override.
        if let Some(replayed) = self.replay_completed(&record)? {
            return Ok(replayed);
        }

        let existing = self
            .activity_service
            .get_activity(&input.activity_id)
            .map_err(|_| OrdinaryActivityError::UnknownActivity(input.activity_id.clone()))?;
        self.ensure_activity_in_scope(&existing)?;
        if existing.has_override() {
            // A record with an effective override keeps the original cash edit
            // path; the new use case must not bypass source protection.
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: input.activity_id.clone(),
                reason: "activity has an effective type override".to_string(),
            });
        }
        if existing.status != ActivityStatus::Posted {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: input.activity_id.clone(),
                reason: "only POSTED activities are updated here".to_string(),
            });
        }
        self.ensure_account_eligible(&input.account_id).await?;

        let update = wealthfolio_core::activities::ActivityUpdate {
            id: input.activity_id.clone(),
            account_id: input.account_id.clone(),
            asset: None,
            activity_type: input.activity_type.clone(),
            subtype: None,
            activity_date: input.activity_date.clone(),
            quantity: None,
            unit_price: None,
            currency: input.currency.clone(),
            fee: input.fee,
            tax: input.tax,
            amount: Some(Some(input.amount.abs())),
            status: Some(ActivityStatus::Posted),
            needs_review: Some(false),
            notes: input.notes.clone(),
            fx_rate: input.fx_rate,
            metadata: None,
        };
        let (prepared, previous) = self
            .activity_service
            .prepare_activity_update_for_commit(update)
            .await?;
        let extras = crate::store::ActivityExtras {
            assignment: assignment_patch(&input.taxonomy_id, &input.category_id),
            event_id: input.event_id.clone(),
            clear_event: input.clear_event,
        };
        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            channel,
            actor,
            authenticated,
            "ordinary_activity_update",
            "activity",
        )
        .detail(serde_json::json!({
            "activityType": input.activity_type,
            "directionSwapped": previous.activity_type != input.activity_type,
            "previousVersion": input.expected_version,
        }));
        let plan = crate::store::OrdinaryCommitPlan {
            ledger_id: record.ledger_id.clone(),
            subject: record.subject.clone(),
            operation: record.operation.clone(),
            request_key: record.request_key.clone(),
            request_hash: record.request_hash.clone(),
            request_snapshot: serde_json::to_string(&record.request_snapshot).unwrap_or_default(),
            kind: crate::store::OrdinaryCommitKind::Update {
                update: prepared,
                expected_version: input.expected_version,
            },
            extras,
            audit: audit.to_row(),
            build_result: Box::new(|activity, version, request_id| {
                serde_json::to_value(OrdinaryActivityResult {
                    activity: activity.clone(),
                    version,
                    request_id: request_id.to_string(),
                    replayed: false,
                })
                .unwrap_or_default()
            }),
        };
        match self.store.commit_ordinary(plan).await? {
            crate::store::OrdinaryCommitOutcome::Committed {
                activity,
                version,
                request_id,
            } => {
                self.activity_service
                    .notify_activity_committed(Some(&previous), &activity);
                Ok(RequestOutcome::Executed(OrdinaryActivityResult {
                    activity: *activity,
                    version,
                    request_id,
                    replayed: false,
                }))
            }
            crate::store::OrdinaryCommitOutcome::Replayed(stored) => decode_stored_result(stored),
            crate::store::OrdinaryCommitOutcome::RequestConflict => {
                Err(OrdinaryActivityError::RequestConflict {
                    key: input.request_key,
                })
            }
            crate::store::OrdinaryCommitOutcome::VersionConflict { expected, actual } => {
                Err(OrdinaryActivityError::VersionConflict { expected, actual })
            }
        }
    }

    /// Read one ordinary activity with the complete editable field set.
    ///
    /// Out-of-scope rows (assets, transfers, special subtypes, non-ordinary
    /// types or non-POSTED status) are rejected exactly like writes are.
    pub async fn get_ordinary(
        &self,
        activity_id: &str,
    ) -> Result<OrdinaryActivityDetail, OrdinaryActivityError> {
        let mut conn = wealthfolio_storage_sqlite::db::get_connection(self.store.pool())
            .map_err(OrdinaryActivityError::Storage)?;
        use diesel::prelude::*;
        use wealthfolio_storage_sqlite::activities::ActivityDB;
        use wealthfolio_storage_sqlite::schema::{
            activities, activity_taxonomy_assignments, spending_activity_events,
        };
        // One read transaction gives the activity, its version, classification
        // and event from the same snapshot, so a concurrent update cannot pair
        // stale fields with a newer version.
        let (activity, version, assignment, event_id) = conn
            .transaction::<_, wealthfolio_storage_sqlite::errors::StorageError, _>(|conn| {
                let stored = activities::table
                    .find(activity_id)
                    .first::<ActivityDB>(conn)
                    .optional()
                    .map_err(wealthfolio_storage_sqlite::errors::StorageError::from)?;
                let Some(stored) = stored else {
                    return Err(wealthfolio_storage_sqlite::errors::StorageError::CoreError(
                        wealthfolio_core::errors::Error::Database(
                            wealthfolio_core::errors::DatabaseError::NotFound(format!(
                                "activity {activity_id}"
                            )),
                        ),
                    ));
                };
                let activity = Activity::from(stored);
                let version = crate::store::activity_version(conn, activity_id)?;
                let assignment: Option<(String, String)> = activity_taxonomy_assignments::table
                    .filter(activity_taxonomy_assignments::activity_id.eq(activity_id))
                    .select((
                        activity_taxonomy_assignments::taxonomy_id,
                        activity_taxonomy_assignments::category_id,
                    ))
                    .first(conn)
                    .optional()
                    .map_err(wealthfolio_storage_sqlite::errors::StorageError::from)?;
                let event_id: Option<String> = spending_activity_events::table
                    .filter(spending_activity_events::activity_id.eq(activity_id))
                    .select(spending_activity_events::event_id)
                    .first(conn)
                    .optional()
                    .map_err(wealthfolio_storage_sqlite::errors::StorageError::from)?;
                Ok((activity, version, assignment, event_id))
            })
            .map_err(|error| {
                let error = wealthfolio_core::errors::Error::from(error);
                match error {
                    wealthfolio_core::errors::Error::Database(
                        wealthfolio_core::errors::DatabaseError::NotFound(_),
                    ) => OrdinaryActivityError::UnknownActivity(activity_id.to_string()),
                    other => OrdinaryActivityError::from(other),
                }
            })?;
        self.ensure_activity_in_scope(&activity)?;
        if activity.status != ActivityStatus::Posted {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: activity_id.to_string(),
                reason: "only POSTED activities are read here".to_string(),
            });
        }
        // Read and write share the same account eligibility: an archived
        // account, disabled spending module or account removed from the
        // tracked set is out of scope for both.
        self.ensure_account_eligible(&activity.account_id).await?;
        let (taxonomy_id, category_id) = match assignment {
            Some((taxonomy_id, category_id)) => (Some(taxonomy_id), Some(category_id)),
            None => (None, None),
        };
        Ok(OrdinaryActivityDetail {
            id: activity.id.clone(),
            account_id: activity.account_id.clone(),
            activity_type: activity.effective_type().to_string(),
            activity_date: activity.activity_date.to_rfc3339(),
            currency: activity.currency.clone(),
            amount: activity.amount.map(|value| value.to_string()),
            fee: activity.fee.map(|value| value.to_string()),
            tax: activity.tax.map(|value| value.to_string()),
            notes: activity.notes.clone(),
            status: activity_status_str(&activity.status).to_string(),
            version,
            fx_rate: activity.fx_rate.map(|value| value.to_string()),
            taxonomy_id,
            category_id,
            event_id,
        })
    }

    /// Scoped query for ordinary income/expense.
    ///
    /// The old activity search is unchanged; this one filters to spending
    /// accounts, POSTED status, DEPOSIT/WITHDRAWAL and no asset/transfer/subtype,
    /// then counts and pages over exactly that set.
    pub async fn query_ordinary(
        &self,
        query: OrdinaryActivityQuery,
    ) -> Result<OrdinaryActivityPage, OrdinaryActivityError> {
        const DEFAULT_PAGE_SIZE: i64 = 50;
        const MAX_PAGE_SIZE: i64 = 200;
        let page_size = query
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        let page = query.page.unwrap_or(0).max(0);
        let settings = self
            .spending_settings
            .get()
            .await
            .map_err(spending_settings_error)?;
        if !settings.enabled {
            return Ok(OrdinaryActivityPage {
                rows: Vec::new(),
                total_count: 0,
                page,
                page_size,
            });
        }
        let requested = query
            .account_ids
            .unwrap_or_else(|| settings.account_ids.clone());
        let account_ids: Vec<String> = requested
            .into_iter()
            .filter(|id| settings.account_ids.iter().any(|opted| opted == id))
            .collect();
        if account_ids.is_empty() {
            return Ok(OrdinaryActivityPage {
                rows: Vec::new(),
                total_count: 0,
                page,
                page_size,
            });
        }

        let mut conn = wealthfolio_storage_sqlite::db::get_connection(self.store.pool())
            .map_err(OrdinaryActivityError::Storage)?;
        use diesel::prelude::*;
        use wealthfolio_storage_sqlite::schema::{accounts, activities};

        let scoped_query = || {
            let mut filter = activities::table
                .inner_join(accounts::table.on(activities::account_id.eq(accounts::id)))
                .filter(accounts::is_archived.eq(false))
                .filter(activities::status.eq("POSTED"))
                .filter(activities::account_id.eq_any(account_ids.clone()))
                .filter(activities::asset_id.is_null())
                .filter(activities::source_group_id.is_null())
                .filter(activities::subtype.is_null().or(activities::subtype.eq("")))
                .filter(
                    activities::activity_type
                        .eq_any([ACTIVITY_TYPE_DEPOSIT, ACTIVITY_TYPE_WITHDRAWAL]),
                )
                .into_boxed();
            if let Some(from) = query.date_from.as_deref() {
                filter = filter.filter(activities::activity_date.ge(format!("{from}T00:00:00")));
            }
            if let Some(to) = query.date_to.as_deref() {
                filter = filter
                    .filter(activities::activity_date.lt(format!("{to}T23:59:59.999999+00:00")));
            }
            filter
        };

        let total_count: i64 = scoped_query()
            .count()
            .get_result(&mut conn)
            .map_err(|error| {
                OrdinaryActivityError::Storage(
                    wealthfolio_storage_sqlite::errors::StorageError::from(error).into(),
                )
            })?;

        // Stable order: by date, then id, so paging cannot repeat or skip rows.
        let rows: Vec<crate::store::OrdinaryRowTuple> = scoped_query()
            .select((
                activities::id,
                activities::account_id,
                activities::activity_type,
                activities::activity_date,
                activities::currency,
                activities::amount,
                activities::fee,
                activities::tax,
                activities::notes,
                activities::status,
                activities::version,
            ))
            .order((activities::activity_date.desc(), activities::id.desc()))
            .limit(page_size)
            .offset(page * page_size)
            .load(&mut conn)
            .map_err(|error| {
                OrdinaryActivityError::Storage(
                    wealthfolio_storage_sqlite::errors::StorageError::from(error).into(),
                )
            })?;

        Ok(OrdinaryActivityPage {
            rows: rows
                .into_iter()
                .map(
                    |(
                        id,
                        account_id,
                        activity_type,
                        activity_date,
                        currency,
                        amount,
                        fee,
                        tax,
                        notes,
                        status,
                        version,
                    )| {
                        OrdinaryActivityRow {
                            id,
                            account_id,
                            activity_type,
                            activity_date,
                            currency,
                            amount,
                            fee,
                            tax,
                            notes,
                            status,
                            version,
                        }
                    },
                )
                .collect(),
            total_count,
            page,
            page_size,
        })
    }

    /// Read the stored store for tests and host wiring.
    pub fn store(&self) -> &BusinessStore {
        &self.store
    }

    /// Fast replay check before preparing the write: a completed request key
    /// returns the stored result, and different content conflicts. The commit
    /// transaction re-checks the same row authoritatively.
    fn replay_completed(
        &self,
        record: &RequestRecord,
    ) -> Result<Option<RequestOutcome<OrdinaryActivityResult>>, OrdinaryActivityError> {
        let existing = self.store.request(
            &record.ledger_id,
            &record.subject,
            &record.operation,
            &record.request_key,
        )?;
        match crate::requests::RequestReplay::decide(existing.as_ref(), &record.request_hash) {
            crate::requests::RequestReplay::Completed(stored) => {
                Ok(Some(decode_stored_result(stored)?))
            }
            crate::requests::RequestReplay::Conflict => {
                Err(OrdinaryActivityError::RequestConflict {
                    key: record.request_key.clone(),
                })
            }
            _ => Ok(None),
        }
    }

    async fn ensure_account_eligible(&self, account_id: &str) -> Result<(), OrdinaryActivityError> {
        let account = self.account_service.get_account(account_id).map_err(|_| {
            OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: format!("account {account_id} was not found"),
            }
        })?;
        if account.is_archived {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: "account is archived".to_string(),
            });
        }
        if !wealthfolio_core::accounts::account_supports_purpose(
            &account.account_type,
            AccountPurpose::Spending,
        ) {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: format!(
                    "account type {} is not a spending account",
                    account.account_type
                ),
            });
        }
        let settings = self
            .spending_settings
            .get()
            .await
            .map_err(spending_settings_error)?;
        if !settings.enabled {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: "spending module is disabled".to_string(),
            });
        }
        if !settings.account_ids.iter().any(|id| id == account_id) {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: "account is not included in spending".to_string(),
            });
        }
        Ok(())
    }

    fn ensure_activity_in_scope(&self, activity: &Activity) -> Result<(), OrdinaryActivityError> {
        let is_ordinary = matches!(
            activity.effective_type(),
            ACTIVITY_TYPE_DEPOSIT | ACTIVITY_TYPE_WITHDRAWAL
        );
        if !is_ordinary {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: activity.id.clone(),
                reason: format!(
                    "type {} is not ordinary income/expense",
                    activity.effective_type()
                ),
            });
        }
        if activity.asset_id.is_some() || activity.source_group_id.is_some() {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: activity.id.clone(),
                reason: "activity carries an asset or transfer association".to_string(),
            });
        }
        if activity
            .subtype
            .as_deref()
            .is_some_and(|subtype| !subtype.trim().is_empty())
        {
            return Err(OrdinaryActivityError::OutOfScope {
                activity_id: activity.id.clone(),
                reason: "activity carries a special subtype".to_string(),
            });
        }
        Ok(())
    }
}

/// Convert a spending-settings failure into the business error surface.
fn spending_settings_error(error: anyhow::Error) -> OrdinaryActivityError {
    OrdinaryActivityError::InvalidInput(error.to_string())
}

fn activity_status_str(status: &ActivityStatus) -> &'static str {
    match status {
        ActivityStatus::Posted => "POSTED",
        ActivityStatus::Pending => "PENDING",
        ActivityStatus::Draft => "DRAFT",
        ActivityStatus::Void => "VOID",
    }
}

fn validate_request_key(key: &str) -> Result<(), OrdinaryActivityError> {
    if key.trim().is_empty() {
        return Err(OrdinaryActivityError::InvalidInput(
            "requestKey must not be blank".to_string(),
        ));
    }
    Ok(())
}

fn validate_activity_type(activity_type: &str) -> Result<(), OrdinaryActivityError> {
    if matches!(
        activity_type,
        ACTIVITY_TYPE_DEPOSIT | ACTIVITY_TYPE_WITHDRAWAL
    ) {
        Ok(())
    } else {
        Err(OrdinaryActivityError::InvalidInput(format!(
            "{activity_type} is not an ordinary income/expense type"
        )))
    }
}

/// Fee and tax must be valid decimals but may legitimately be zero.
fn validate_optional_amount(amount: Decimal, field: &str) -> Result<(), OrdinaryActivityError> {
    if amount.scale() > 4 {
        return Err(OrdinaryActivityError::InvalidAmount(format!(
            "{field} has more than 4 decimal places"
        )));
    }
    Ok(())
}

fn validate_amount(amount: Decimal, field: &str) -> Result<(), OrdinaryActivityError> {
    if amount.is_zero() {
        return Err(OrdinaryActivityError::InvalidAmount(format!(
            "{field} must not be zero"
        )));
    }
    if amount.scale() > 4 {
        return Err(OrdinaryActivityError::InvalidAmount(format!(
            "{field} has more than 4 decimal places"
        )));
    }
    Ok(())
}

fn decode_stored_result(
    result_json: String,
) -> Result<RequestOutcome<OrdinaryActivityResult>, OrdinaryActivityError> {
    let mut result: OrdinaryActivityResult =
        serde_json::from_str(&result_json).map_err(|error| {
            OrdinaryActivityError::InvalidInput(format!("stored result is unreadable: {error}"))
        })?;
    result.replayed = true;
    Ok(RequestOutcome::Replayed(result))
}

/// Outcome of a scope check, kept explicit so callers can report a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeVerdict {
    Eligible,
    OutOfScope,
}

impl ScopeVerdict {
    pub fn is_eligible(self) -> bool {
        self == ScopeVerdict::Eligible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wealthfolio_core::errors::{DatabaseError, Error as CoreError};

    #[test]
    fn typed_core_duplicate_maps_to_the_business_duplicate() {
        let core: CoreError =
            wealthfolio_core::activities::ActivityError::duplicate(Some("act-1")).into();
        let error = OrdinaryActivityError::from(core);
        assert_eq!(error.code(), "duplicate_activity");
        assert_eq!(error.existing_activity_id(), Some("act-1"));
        assert!(!error.retryable());
        let structured = error.structured_value();
        assert_eq!(structured["data"]["existingActivityId"], "act-1");
        assert_eq!(structured["retryable"], false);
        assert_eq!(structured["requestId"], serde_json::Value::Null);
    }

    #[test]
    fn only_the_activity_idempotency_unique_violation_is_a_duplicate() {
        let duplicate: CoreError = CoreError::Database(DatabaseError::UniqueViolation(
            "UNIQUE constraint failed: activities.idempotency_key".to_string(),
        ));
        assert_eq!(
            OrdinaryActivityError::from(duplicate).code(),
            "duplicate_activity"
        );

        let other: CoreError = CoreError::Database(DatabaseError::UniqueViolation(
            "UNIQUE constraint failed: accounts.id".to_string(),
        ));
        assert_eq!(OrdinaryActivityError::from(other).code(), "storage_error");

        let internal: CoreError = CoreError::Database(DatabaseError::Internal(
            "UNIQUE constraint failed: activities.idempotency_key".to_string(),
        ));
        assert_eq!(
            OrdinaryActivityError::from(internal).code(),
            "duplicate_activity"
        );
    }

    #[test]
    fn legacy_entry_and_rest_text_keep_the_old_wrapping() {
        let with_id = OrdinaryActivityError::DuplicateActivity {
            existing_activity_id: Some("act-1".to_string()),
        };
        assert_eq!(
            with_id.legacy_entry_message(),
            "storage error: Activity error: Invalid data: Duplicate activity detected. A matching activity already exists (id: act-1)."
        );
        assert_eq!(
            with_id.legacy_rest_message(),
            "storage_error: storage error: Activity error: Invalid data: Duplicate activity detected. A matching activity already exists (id: act-1)."
        );
        let without_id = OrdinaryActivityError::DuplicateActivity {
            existing_activity_id: None,
        };
        assert_eq!(
            without_id.legacy_entry_message(),
            "storage error: Activity error: Invalid data: Duplicate activity detected. A matching activity already exists."
        );
        assert_eq!(with_id.structured_value()["code"], "duplicate_activity");
    }

    #[test]
    fn business_duplicate_text_does_not_claim_current_field_equality() {
        let error = OrdinaryActivityError::DuplicateActivity {
            existing_activity_id: Some("act-1".to_string()),
        };
        assert!(!error.to_string().contains("matching activity"));
        assert!(!error.retryable());
    }
}
