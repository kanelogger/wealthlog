//! Pending entries (待确认收支).
//!
//! A pending entry may be missing the account, currency or date. Confirming
//! one creates exactly one `POSTED` activity and records its id on the entry;
//! retries return the original result instead of creating a second activity.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use wealthfolio_core::activities::{Activity, ActivityServiceTrait, ActivityStatus, NewActivity};
use wealthfolio_spending::settings::SpendingSettingsService;

use crate::audit::{BusinessAuditEntry, BusinessChannel};
use crate::requests::{RequestOutcome, RequestRecord, RequestReplay};
use crate::store::{
    insert_audit_tx, new_id, now, update_pending_tx, BusinessStore, NewPendingRow, PendingRow,
    PendingStatus,
};

/// A pending entry as returned to clients.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivity {
    pub id: String,
    /// Business request that produced this create result (absent on reads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub status: PendingActivityStatus,
    pub account_id: Option<String>,
    pub activity_type: String,
    pub activity_date: Option<String>,
    pub currency: Option<String>,
    pub amount: Option<String>,
    pub fee: Option<String>,
    pub tax: Option<String>,
    pub notes: Option<String>,
    pub asset_symbol: Option<String>,
    /// Which credential created it; a token id, never a raw token.
    pub created_by: String,
    pub confirmed_by: Option<String>,
    pub committed_activity_id: Option<String>,
    pub version: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PendingActivityStatus {
    Pending,
    Committed,
    Rejected,
}

impl PendingActivityStatus {
    fn from_str(value: &str) -> Self {
        match value {
            "committed" => PendingActivityStatus::Committed,
            "rejected" => PendingActivityStatus::Rejected,
            _ => PendingActivityStatus::Pending,
        }
    }
}

impl From<PendingRow> for PendingActivity {
    fn from(row: PendingRow) -> Self {
        Self {
            id: row.id,
            request_id: None,
            status: PendingActivityStatus::from_str(&row.status),
            account_id: row.account_id,
            activity_type: row.activity_type,
            activity_date: row.activity_date,
            currency: row.currency,
            amount: row.amount,
            fee: row.fee,
            tax: row.tax,
            notes: row.notes,
            asset_symbol: row.asset_symbol,
            created_by: row.created_by,
            confirmed_by: row.confirmed_by,
            committed_activity_id: row.committed_activity_id,
            version: row.version,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Input for creating a pending entry. Anything the Agent does not know stays
/// absent; nothing is guessed.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingCreate {
    pub activity_type: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub activity_date: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub fee: Option<String>,
    #[serde(default)]
    pub tax: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub asset_symbol: Option<String>,
    /// Source channel, message id, entry id and original excerpt/reference.
    #[serde(default)]
    pub source: Option<serde_json::Value>,
    /// Frozen per create; a retry returns the stored entry.
    pub request_key: String,
}

/// Input for editing a pending entry before confirmation.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdate {
    pub expected_version: i32,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub activity_date: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub fee: Option<String>,
    #[serde(default)]
    pub tax: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub asset_symbol: Option<String>,
}

/// Result of confirming a pending entry.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingConfirmResult {
    pub pending: PendingActivity,
    #[schema(value_type = Option<Object>)]
    pub activity: Option<Activity>,
    pub replayed: bool,
}

/// Config that confirmation needs, independent of the transport.
pub struct PendingConfirmContext<'a> {
    pub channel: BusinessChannel,
    /// The confirming credential; recorded separately from the creator.
    pub actor: &'a str,
    pub authenticated: bool,
}

/// Pending-entry use cases.
pub struct PendingService {
    activity_service: Arc<dyn ActivityServiceTrait>,
    account_service: Arc<dyn wealthfolio_core::accounts::AccountServiceTrait>,
    store: BusinessStore,
    ledger_id: String,
    spending_settings: Arc<SpendingSettingsService>,
}

impl PendingService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        activity_service: Arc<dyn ActivityServiceTrait>,
        account_service: Arc<dyn wealthfolio_core::accounts::AccountServiceTrait>,
        store: BusinessStore,
        ledger_id: impl Into<String>,
        spending_settings: Arc<SpendingSettingsService>,
    ) -> Self {
        Self {
            activity_service,
            account_service,
            store,
            ledger_id: ledger_id.into(),
            spending_settings,
        }
    }

    /// An archived account, or one whose type is no longer suitable, stops
    /// qualifying: the entry fails explicitly instead of posting to it.
    fn ensure_account_confirmable(
        &self,
        account_id: &str,
    ) -> Result<(), crate::service::OrdinaryActivityError> {
        let account = self.account_service.get_account(account_id).map_err(|_| {
            crate::service::OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: format!("account {account_id} was not found"),
            }
        })?;
        if account.is_archived {
            return Err(crate::service::OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: "account is archived".to_string(),
            });
        }
        if !wealthfolio_core::accounts::account_supports_purpose(
            &account.account_type,
            wealthfolio_core::accounts::AccountPurpose::Spending,
        ) {
            return Err(crate::service::OrdinaryActivityError::OutOfScope {
                activity_id: String::new(),
                reason: format!(
                    "account type {} is not a spending account",
                    account.account_type
                ),
            });
        }
        Ok(())
    }

    /// Store a new pending entry. `created_by` is a PAT database id (or
    /// `manual` for GUI-created entries), so replacing a token never inherits
    /// the entry.
    ///
    /// `requestKey` makes the create replayable; a complete source identity
    /// (channel + messageId + entryId + optional source account/session)
    /// additionally prevents redelivery from creating a second entry.
    pub async fn create(
        &self,
        input: PendingCreate,
        created_by: &str,
        channel: BusinessChannel,
        authenticated: bool,
    ) -> Result<PendingActivity, crate::service::OrdinaryActivityError> {
        if input.request_key.trim().is_empty() {
            return Err(crate::service::OrdinaryActivityError::InvalidInput(
                "requestKey is required".to_string(),
            ));
        }
        let source_identity = input.source.as_ref().and_then(|source| {
            let channel = source.get("channel")?.as_str()?.trim();
            let message_id = source
                .get("messageId")
                .or_else(|| source.get("message_id"))?
                .as_str()?
                .trim();
            let entry_id = source
                .get("entryId")
                .or_else(|| source.get("entry_id"))?
                .as_str()?
                .trim();
            if channel.is_empty() || message_id.is_empty() || entry_id.is_empty() {
                return None;
            }
            Some(crate::store::PendingSourceIdentity {
                channel: channel.to_string(),
                message_id: message_id.to_string(),
                source_account: source
                    .get("sourceAccount")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                source_session: source
                    .get("sourceSession")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                entry_id: entry_id.to_string(),
            })
        });
        let row = NewPendingRow {
            id: new_id("pend"),
            status: PendingStatus::Pending.as_str().to_string(),
            account_id: input.account_id.clone(),
            activity_type: input.activity_type.clone(),
            activity_date: input.activity_date.clone(),
            currency: input.currency.clone(),
            amount: input.amount.clone(),
            fee: input.fee.clone(),
            tax: input.tax.clone(),
            notes: input.notes.clone(),
            asset_symbol: input.asset_symbol.clone(),
            created_by: created_by.to_string(),
            confirmed_by: None,
            source: input
                .source
                .as_ref()
                .and_then(|value| serde_json::to_string(value).ok()),
            version: 1,
            committed_activity_id: None,
            created_at: now(),
            updated_at: now(),
        };
        let id = row.id.clone();
        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            channel,
            created_by,
            authenticated,
            "pending_activity_create",
            "pending_activity",
        )
        .entity(id.clone());
        let record = RequestRecord::new(
            &self.ledger_id,
            created_by,
            "pending_activity_create",
            &input.request_key,
            serde_json::to_value(&input).unwrap_or(serde_json::Value::Null),
        );
        let request = crate::store::PendingRequestParts {
            ledger_id: record.ledger_id.clone(),
            subject: record.subject.clone(),
            operation: record.operation.clone(),
            request_key: record.request_key.clone(),
            request_hash: record.request_hash.clone(),
            request_snapshot: serde_json::to_string(&record.request_snapshot).unwrap_or_default(),
        };
        let plan = crate::store::PendingCreatePlan {
            row,
            source_identity,
            request: Some(request),
            audit: audit.to_row(),
        };
        match self.store.create_pending_atomic(plan).await? {
            crate::store::PendingCreateOutcome::Created(row, request_id)
            | crate::store::PendingCreateOutcome::Existing(row, request_id) => {
                let mut pending = PendingActivity::from(row);
                pending.request_id = request_id;
                Ok(pending)
            }
            crate::store::PendingCreateOutcome::Replayed(stored, request_id) => {
                let mut pending =
                    serde_json::from_str::<PendingActivity>(&stored).map_err(|error| {
                        crate::service::OrdinaryActivityError::InvalidInput(format!(
                            "stored pending result is unreadable: {error}"
                        ))
                    })?;
                pending.request_id = Some(request_id);
                Ok(pending)
            }
            crate::store::PendingCreateOutcome::RequestConflict => {
                Err(crate::service::OrdinaryActivityError::RequestConflict {
                    key: input.request_key,
                })
            }
        }
    }

    pub fn list(
        &self,
        include_resolved: bool,
    ) -> Result<Vec<PendingActivity>, crate::service::OrdinaryActivityError> {
        self.store
            .list_pending(include_resolved)
            .map(|rows| rows.into_iter().map(PendingActivity::from).collect())
            .map_err(crate::service::OrdinaryActivityError::from)
    }

    /// Edit a pending entry. Committed or rejected entries are immutable.
    pub async fn update(
        &self,
        id: &str,
        input: PendingUpdate,
        actor: &str,
        channel: BusinessChannel,
        authenticated: bool,
    ) -> Result<PendingActivity, crate::service::OrdinaryActivityError> {
        let mut row = self
            .store
            .pending(id)
            .map_err(crate::service::OrdinaryActivityError::from)?;
        if row.status != PendingStatus::Pending.as_str() {
            return Err(crate::service::OrdinaryActivityError::InvalidInput(
                format!(
                    "pending entry {id} is {} and can no longer be edited",
                    row.status
                ),
            ));
        }
        if row.version != input.expected_version {
            return Err(crate::service::OrdinaryActivityError::VersionConflict {
                expected: input.expected_version,
                actual: row.version,
            });
        }
        row.account_id = input.account_id.or(row.account_id);
        row.activity_date = input.activity_date.or(row.activity_date);
        row.currency = input.currency.or(row.currency);
        row.amount = input.amount.or(row.amount);
        row.fee = input.fee.or(row.fee);
        row.tax = input.tax.or(row.tax);
        row.notes = input.notes.or(row.notes);
        row.asset_symbol = input.asset_symbol.or(row.asset_symbol);
        row.updated_at = now();
        let expected = input.expected_version;
        let updated = row.clone();
        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            channel,
            actor,
            authenticated,
            "pending_activity_update",
            "pending_activity",
        )
        .entity(id.to_string());
        let ok = self
            .store
            .transaction(move |tx| {
                let ok = update_pending_tx(tx, &updated, expected)?;
                if !ok {
                    return Ok(false);
                }
                insert_audit_tx(tx, &audit.to_row())?;
                Ok(true)
            })
            .await
            .map_err(crate::service::OrdinaryActivityError::from)?;
        if !ok {
            return Err(crate::service::OrdinaryActivityError::Conflict(
                "pending entry changed concurrently".to_string(),
            ));
        }
        self.store
            .pending(id)
            .map(PendingActivity::from)
            .map_err(crate::service::OrdinaryActivityError::from)
    }

    /// Confirm a pending entry: it becomes one `POSTED` activity, exactly once.
    pub async fn confirm(
        &self,
        id: &str,
        expected_version: i32,
        context: PendingConfirmContext<'_>,
    ) -> Result<RequestOutcome<PendingConfirmResult>, crate::service::OrdinaryActivityError> {
        let pending = self
            .store
            .pending(id)
            .map_err(crate::service::OrdinaryActivityError::from)?;
        if pending.status == PendingStatus::Committed.as_str() {
            let activity = match pending.committed_activity_id.as_deref() {
                Some(activity_id) => self.activity_service.get_activity(activity_id).ok(),
                None => None,
            };
            return Ok(RequestOutcome::Replayed(PendingConfirmResult {
                pending: PendingActivity::from(pending),
                activity,
                replayed: true,
            }));
        }
        if pending.status != PendingStatus::Pending.as_str() {
            return Err(crate::service::OrdinaryActivityError::InvalidInput(
                format!(
                    "pending entry {id} is {} and cannot be confirmed",
                    pending.status
                ),
            ));
        }
        if pending.version != expected_version {
            return Err(crate::service::OrdinaryActivityError::VersionConflict {
                expected: expected_version,
                actual: pending.version,
            });
        }
        let account_id = pending
            .account_id
            .clone()
            .ok_or_else(|| missing("account"))?;
        // Re-check eligibility at confirmation time: the plan requires the
        // entry to fail explicitly if the account stopped qualifying.
        let settings = self.spending_settings.get().await.map_err(|error| {
            crate::service::OrdinaryActivityError::InvalidInput(error.to_string())
        })?;
        self.ensure_account_confirmable(&account_id)?;
        if !settings.enabled || !settings.account_ids.iter().any(|id| id == &account_id) {
            return Err(crate::service::OrdinaryActivityError::OutOfScope {
                activity_id: pending.id.clone(),
                reason: "account stopped qualifying for spending".to_string(),
            });
        }
        let activity_type = pending.activity_type.clone();
        if !matches!(activity_type.as_str(), "DEPOSIT" | "WITHDRAWAL") {
            return Err(crate::service::OrdinaryActivityError::InvalidInput(
                format!("{activity_type} is not an ordinary income/expense type"),
            ));
        }
        let date = pending
            .activity_date
            .clone()
            .ok_or_else(|| missing("activity date"))?;
        let currency = pending
            .currency
            .clone()
            .ok_or_else(|| missing("currency"))?;
        let amount = pending.amount.clone().ok_or_else(|| missing("amount"))?;
        let amount = amount
            .parse::<rust_decimal::Decimal>()
            .map_err(|_| crate::service::OrdinaryActivityError::InvalidAmount(amount.clone()))?;

        let request_key = format!("pending-confirm:{id}");
        let snapshot = serde_json::json!({
            "pendingId": pending.id,
            "version": expected_version,
            "accountId": account_id,
            "activityType": activity_type,
            "activityDate": date,
            "currency": currency,
            "amount": amount.to_string(),
        });
        let record = RequestRecord::new(
            &self.ledger_id,
            context.actor,
            "pending_activity_confirm",
            &request_key,
            snapshot,
        );
        let existing = self
            .store
            .request(
                &record.ledger_id,
                &record.subject,
                &record.operation,
                &record.request_key,
            )
            .map_err(crate::service::OrdinaryActivityError::from)?;
        match RequestReplay::decide(existing.as_ref(), &record.request_hash) {
            RequestReplay::Completed(result) => {
                let mut stored: PendingConfirmResult =
                    serde_json::from_str(&result).map_err(|error| {
                        crate::service::OrdinaryActivityError::InvalidInput(error.to_string())
                    })?;
                stored.replayed = true;
                return Ok(RequestOutcome::Replayed(stored));
            }
            RequestReplay::Conflict => {
                return Err(crate::service::OrdinaryActivityError::RequestConflict {
                    key: request_key,
                })
            }
            RequestReplay::Fresh | RequestReplay::Retry => {}
        }
        let request_id = existing
            .as_ref()
            .map(|row| row.id.clone())
            .unwrap_or_else(|| new_id("req"));

        let new_activity = NewActivity {
            id: None,
            account_id: account_id.clone(),
            asset: None,
            activity_type: activity_type.clone(),
            subtype: None,
            activity_date: date,
            quantity: None,
            unit_price: None,
            currency,
            fee: pending.fee.as_deref().and_then(|value| value.parse().ok()),
            tax: pending.tax.as_deref().and_then(|value| value.parse().ok()),
            amount: Some(amount.abs()),
            status: Some(ActivityStatus::Posted),
            notes: pending.notes.clone(),
            fx_rate: None,
            metadata: None,
            source_system: Some("MANUAL".to_string()),
            source_record_id: None,
            source_group_id: None,
            idempotency_key: Some(format!("pending-activity:{id}")),
            needs_review: Some(false),
        };
        let prepared = self
            .activity_service
            .prepare_activity_for_commit(new_activity)
            .await
            .map_err(crate::service::OrdinaryActivityError::from)?;

        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            context.channel,
            context.actor,
            context.authenticated,
            "pending_activity_confirm",
            "pending_activity",
        )
        .entity(id.to_string())
        .request(request_id.clone())
        .detail(serde_json::json!({
            "creator": pending.created_by,
            "previousVersion": expected_version,
        }));

        let request_hash = record.request_hash.clone();
        let request_snapshot = serde_json::to_string(&record.request_snapshot).unwrap_or_default();
        let audit_actor = context.actor.to_string();
        let committed = self
            .store
            .commit_pending_confirmation(
                prepared,
                id.to_string(),
                expected_version,
                audit_actor,
                request_id.clone(),
                request_hash.clone(),
                request_snapshot.clone(),
                audit.to_row(),
                Box::new(move |activity, pending_row| {
                    serde_json::to_value(PendingConfirmResult {
                        pending: PendingActivity::from(pending_row.clone()),
                        activity: Some(activity.clone()),
                        replayed: false,
                    })
                    .unwrap_or_else(|_| {
                        serde_json::json!({
                            "pending": PendingActivity::from(pending_row.clone()),
                            "activity": activity,
                            "replayed": false
                        })
                    })
                }),
            )
            .await
            .map_err(crate::service::OrdinaryActivityError::from)?;
        let Some((row, activity)) = committed else {
            return Err(crate::service::OrdinaryActivityError::Conflict(
                "pending entry changed concurrently".to_string(),
            ));
        };
        self.activity_service
            .notify_activity_committed(None, &activity);
        Ok(RequestOutcome::Executed(PendingConfirmResult {
            pending: PendingActivity::from(row),
            activity: Some(activity),
            replayed: false,
        }))
    }

    /// Reject a pending entry. It stays stored for review.
    pub async fn reject(
        &self,
        id: &str,
        expected_version: i32,
        actor: &str,
        channel: BusinessChannel,
        authenticated: bool,
    ) -> Result<PendingActivity, crate::service::OrdinaryActivityError> {
        let mut pending = self
            .store
            .pending(id)
            .map_err(crate::service::OrdinaryActivityError::from)?;
        if pending.status != PendingStatus::Pending.as_str() {
            return Err(crate::service::OrdinaryActivityError::InvalidInput(
                format!("pending entry {id} is {}", pending.status),
            ));
        }
        if pending.version != expected_version {
            return Err(crate::service::OrdinaryActivityError::VersionConflict {
                expected: expected_version,
                actual: pending.version,
            });
        }
        pending.status = PendingStatus::Rejected.as_str().to_string();
        pending.confirmed_by = Some(actor.to_string());
        pending.updated_at = now();
        let updated = pending.clone();
        let expected = expected_version;
        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            channel,
            actor,
            authenticated,
            "pending_activity_reject",
            "pending_activity",
        )
        .entity(id.to_string());
        let ok = self
            .store
            .transaction(move |tx| {
                let ok = update_pending_tx(tx, &updated, expected)?;
                if !ok {
                    return Ok(false);
                }
                insert_audit_tx(tx, &audit.to_row())?;
                Ok(true)
            })
            .await
            .map_err(crate::service::OrdinaryActivityError::from)?;
        if !ok {
            return Err(crate::service::OrdinaryActivityError::Conflict(
                "pending entry changed concurrently".to_string(),
            ));
        }
        Ok(PendingActivity::from(pending))
    }
}

fn missing(field: &str) -> crate::service::OrdinaryActivityError {
    crate::service::OrdinaryActivityError::InvalidInput(format!("pending entry is missing {field}"))
}
