//! Storage for the unified business interface.
//!
//! Everything the plan puts next to the core accounting tables lives here:
//! activity versions, pending entries, request records and the business audit
//! trail. The repository deliberately works on one writer transaction at a
//! time; callers never compose several `exec_tx` calls into something they
//! believe is atomic.

use std::sync::Arc;

use chrono::Utc;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use wealthfolio_core::activities::{Activity, ActivityUpdate, NewActivity};
use wealthfolio_core::errors::{DatabaseError, Error, Result};
use wealthfolio_storage_sqlite::activities::ActivityDB;
use wealthfolio_storage_sqlite::db::{get_connection, DbWriteTx, WriteHandle};
use wealthfolio_storage_sqlite::errors::StorageError;
use wealthfolio_storage_sqlite::schema::{
    activities, activity_taxonomy_assignments, business_audit, business_requests,
    pending_activities, spending_activity_events, spending_activity_splits, spending_events,
    taxonomy_categories,
};

pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

/// Renders the stored ordinary result from the committed rows.
pub type OrdinaryResultBuilder = Box<dyn FnOnce(&Activity, i32, &str) -> serde_json::Value + Send>;

/// Renders the stored confirm result from the committed rows.
pub type PendingResultBuilder = Box<dyn FnOnce(&Activity, &PendingRow) -> serde_json::Value + Send>;

/// Column tuple for the scoped ordinary query.
pub type OrdinaryRowTuple = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    i32,
);

// ---------------------------------------------------------------------------
// Activity versions
// ---------------------------------------------------------------------------

/// The current version of one activity row.
pub fn activity_version(conn: &mut SqliteConnection, activity_id: &str) -> Result<i32> {
    activities::table
        .select(activities::version)
        .find(activity_id)
        .first::<i32>(conn)
        .map_err(|error| match error {
            diesel::result::Error::NotFound => Error::Database(DatabaseError::QueryFailed(
                format!("activity {activity_id} was not found"),
            )),
            other => StorageError::from(other).into(),
        })
}

/// Raise `version` to `expected + 1` only while it still equals `expected`.
///
/// Returns `false` when another writer moved the row first. The caller decides
/// whether that is a conflict (new business path) or a best-effort miss (old
/// path that must not fail).
pub fn bump_activity_version(
    conn: &mut SqliteConnection,
    activity_id: &str,
    expected: i32,
) -> Result<bool> {
    let updated =
        diesel::sql_query("UPDATE activities SET version = ?1 WHERE id = ?2 AND version = ?3")
            .bind::<diesel::sql_types::Integer, _>(expected.saturating_add(1))
            .bind::<diesel::sql_types::Text, _>(activity_id)
            .bind::<diesel::sql_types::Integer, _>(expected)
            .execute(conn)
            .map_err(StorageError::from)?;
    Ok(updated == 1)
}

// ---------------------------------------------------------------------------
// Pending entries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingStatus {
    Pending,
    Committed,
    Rejected,
}

impl PendingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PendingStatus::Pending => "pending",
            PendingStatus::Committed => "committed",
            PendingStatus::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(PendingStatus::Pending),
            "committed" => Some(PendingStatus::Committed),
            "rejected" => Some(PendingStatus::Rejected),
            _ => None,
        }
    }
}

/// One stored pending entry. Missing fields stay missing on purpose.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = pending_activities)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct PendingRow {
    pub id: String,
    pub status: String,
    pub account_id: Option<String>,
    pub activity_type: String,
    pub activity_date: Option<String>,
    pub currency: Option<String>,
    pub amount: Option<String>,
    pub fee: Option<String>,
    pub tax: Option<String>,
    pub notes: Option<String>,
    pub asset_symbol: Option<String>,
    pub created_by: String,
    pub confirmed_by: Option<String>,
    pub source: Option<String>,
    pub version: i32,
    pub committed_activity_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = pending_activities)]
pub struct NewPendingRow {
    pub id: String,
    pub status: String,
    pub account_id: Option<String>,
    pub activity_type: String,
    pub activity_date: Option<String>,
    pub currency: Option<String>,
    pub amount: Option<String>,
    pub fee: Option<String>,
    pub tax: Option<String>,
    pub notes: Option<String>,
    pub asset_symbol: Option<String>,
    pub created_by: String,
    pub confirmed_by: Option<String>,
    pub source: Option<String>,
    pub version: i32,
    pub committed_activity_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Business request row.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = business_requests)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct RequestRow {
    pub id: String,
    pub ledger_id: String,
    pub subject: String,
    pub operation: String,
    pub request_key: String,
    pub request_hash: String,
    pub request_snapshot: String,
    pub state: String,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = business_requests)]
pub struct NewRequestRow {
    pub id: String,
    pub ledger_id: String,
    pub subject: String,
    pub operation: String,
    pub request_key: String,
    pub request_hash: String,
    pub request_snapshot: String,
    pub state: String,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl RequestRow {
    pub fn from_new(new: NewRequestRow) -> Self {
        Self {
            id: new.id,
            ledger_id: new.ledger_id,
            subject: new.subject,
            operation: new.operation,
            request_key: new.request_key,
            request_hash: new.request_hash,
            request_snapshot: new.request_snapshot,
            state: new.state,
            result_json: new.result_json,
            error_message: new.error_message,
            created_at: new.created_at,
            updated_at: new.updated_at,
        }
    }
}

/// Business audit row.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name = business_audit)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct AuditRow {
    pub id: String,
    pub ledger_id: String,
    pub channel: String,
    pub actor: String,
    pub authenticated: i32,
    pub operation: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub request_id: Option<String>,
    pub outcome: String,
    pub detail_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = business_audit)]
pub struct NewAuditRow {
    pub id: String,
    pub ledger_id: String,
    pub channel: String,
    pub actor: String,
    pub authenticated: i32,
    pub operation: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub request_id: Option<String>,
    pub outcome: String,
    pub detail_json: Option<String>,
    pub created_at: String,
}

/// Business store over the serial writer.
///
/// Reads use the pool; every write goes through [`WriteHandle::exec_tx`] so a
/// use case can put an activity write, its category/event follow-ups, the
/// request result and the audit row in one SQLite transaction.
#[derive(Clone)]
pub struct BusinessStore {
    pool: Arc<DbPool>,
    writer: WriteHandle,
    // Background index bookkeeping must not interrupt a multi-read financial capture.
    investment_read_window: Arc<tokio::sync::RwLock<()>>,
}

impl BusinessStore {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self {
            pool,
            writer,
            investment_read_window: Arc::new(tokio::sync::RwLock::new(())),
        }
    }

    pub(crate) fn investment_read_window(&self) -> &tokio::sync::RwLock<()> {
        &self.investment_read_window
    }

    pub fn writer(&self) -> &WriteHandle {
        &self.writer
    }

    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }

    /// Run one business transaction. The closure sees the same transaction
    /// handle the activity repository uses, so an activity write and its
    /// follow-ups share a commit.
    pub async fn transaction<T, F>(&self, job: F) -> Result<T>
    where
        F: FnOnce(&mut DbWriteTx<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.writer.exec_tx(job).await
    }

    pub fn pending(&self, id: &str) -> Result<PendingRow> {
        let mut conn = get_connection(&self.pool)?;
        pending_activities::table
            .find(id)
            .first(&mut conn)
            .map_err(|error| match error {
                diesel::result::Error::NotFound => Error::Database(DatabaseError::QueryFailed(
                    format!("pending activity {id} was not found"),
                )),
                other => StorageError::from(other).into(),
            })
    }

    /// Find an existing pending entry created by the same credential for the
    /// same upstream message. This is the "full source identity" dedupe; when
    /// no message id exists, only request replay protects against duplicates.
    pub fn find_pending_by_source_message(
        &self,
        channel: &str,
        message_id: &str,
    ) -> Result<Option<PendingRow>> {
        let mut conn = get_connection(&self.pool)?;
        #[derive(QueryableByName)]
        struct IdRow {
            #[diesel(sql_type = diesel::sql_types::Text)]
            id: String,
        }
        let rows = diesel::sql_query(
            "SELECT id FROM pending_activities \
             WHERE json_extract(source, '$.channel') = ?1 \
               AND json_extract(source, '$.messageId') = ?2 \
             LIMIT 1",
        )
        .bind::<diesel::sql_types::Text, _>(channel)
        .bind::<diesel::sql_types::Text, _>(message_id)
        .load::<IdRow>(&mut conn)
        .map_err(StorageError::from)?;
        match rows.into_iter().next() {
            Some(row) => self.pending(&row.id).map(Some),
            None => Ok(None),
        }
    }

    pub fn list_pending(&self, include_resolved: bool) -> Result<Vec<PendingRow>> {
        let mut conn = get_connection(&self.pool)?;
        let mut query = pending_activities::table.into_boxed();
        if !include_resolved {
            query = query.filter(pending_activities::status.eq("pending"));
        }
        query
            .order(pending_activities::created_at.desc())
            .load(&mut conn)
            .map_err(|error| StorageError::from(error).into())
    }

    pub fn request(
        &self,
        ledger_id: &str,
        subject: &str,
        operation: &str,
        request_key: &str,
    ) -> Result<Option<RequestRow>> {
        let mut conn = get_connection(&self.pool)?;
        let rows = business_requests::table
            .filter(business_requests::ledger_id.eq(ledger_id))
            .filter(business_requests::subject.eq(subject))
            .filter(business_requests::operation.eq(operation))
            .filter(business_requests::request_key.eq(request_key))
            .limit(1)
            .load::<RequestRow>(&mut conn)
            .map_err(StorageError::from)?;
        Ok(rows.into_iter().next())
    }

    /// Every stored audit row, newest first. Reads exist for the GUI history
    /// and tests; writes only happen through [`Self::transaction`].
    pub fn audit_rows(&self, limit: i64) -> Result<Vec<AuditRow>> {
        let mut conn = get_connection(&self.pool)?;
        business_audit::table
            .order(business_audit::created_at.desc())
            .limit(limit)
            .load(&mut conn)
            .map_err(|error| StorageError::from(error).into())
    }
}

/// Category/event follow-ups that must share the activity's transaction.
#[derive(Debug, Clone, Default)]
pub struct ActivityExtras {
    /// Classification patch: `None` preserves, `Some(None)` removes every
    /// assignment, `Some(Some(..))` replaces the taxonomy's assignment.
    pub assignment: Option<Option<(String, String)>>,
    /// Spending event tag to set; `None` leaves the tag untouched on update.
    pub event_id: Option<String>,
    /// Whether an update should clear an existing event tag.
    pub clear_event: bool,
}

/// What the unified activity commit should do.
pub enum OrdinaryCommitKind {
    Create(NewActivity),
    Update {
        update: ActivityUpdate,
        expected_version: i32,
    },
}

/// Everything written by one ordinary create/update, atomically.
pub struct OrdinaryCommitPlan {
    pub ledger_id: String,
    pub subject: String,
    pub operation: String,
    pub request_key: String,
    pub request_hash: String,
    pub request_snapshot: String,
    pub kind: OrdinaryCommitKind,
    pub extras: ActivityExtras,
    pub audit: NewAuditRow,
    /// Renders the stored request result once the activity id/version exist.
    pub build_result: OrdinaryResultBuilder,
}

#[derive(Debug)]
pub enum OrdinaryCommitOutcome {
    Committed {
        activity: Box<Activity>,
        version: i32,
        request_id: String,
    },
    Replayed(String),
    RequestConflict,
    VersionConflict {
        expected: i32,
        actual: i32,
    },
}

/// Complete source identity for pending-entry dedupe. Both `message_id` and
/// `entry_id` must be present; a message holding several entries uses a
/// distinct `entry_id` per row.
#[derive(Debug, Clone)]
pub struct PendingSourceIdentity {
    pub channel: String,
    pub message_id: String,
    pub source_account: Option<String>,
    pub source_session: Option<String>,
    pub entry_id: String,
}

pub struct PendingRequestParts {
    pub ledger_id: String,
    pub subject: String,
    pub operation: String,
    pub request_key: String,
    pub request_hash: String,
    pub request_snapshot: String,
}

pub struct PendingCreatePlan {
    pub row: NewPendingRow,
    pub source_identity: Option<PendingSourceIdentity>,
    pub request: Option<PendingRequestParts>,
    pub audit: NewAuditRow,
}

#[derive(Debug)]
pub enum PendingCreateOutcome {
    Created(PendingRow, Option<String>),
    Existing(PendingRow, Option<String>),
    Replayed(String, String),
    RequestConflict,
}

impl BusinessStore {
    /// Commit one ordinary activity and every business record in one SQLite
    /// transaction: activity, category/split/event follow-ups, request result
    /// and success audit. Any failure rolls the whole thing back.
    pub async fn commit_ordinary(
        &self,
        plan: OrdinaryCommitPlan,
    ) -> std::result::Result<OrdinaryCommitOutcome, crate::service::OrdinaryActivityError> {
        let OrdinaryCommitPlan {
            ledger_id,
            subject,
            operation,
            request_key,
            request_hash,
            request_snapshot,
            kind,
            extras,
            mut audit,
            build_result,
        } = plan;
        self.writer
            .exec_tx(move |tx| -> std::result::Result<OrdinaryCommitOutcome, Error> {
                // Replay/conflict read first: no writes yet, so returning an
                // outcome cannot leave a partial request row behind.
                let existing_request = business_requests::table
                    .filter(business_requests::ledger_id.eq(&ledger_id))
                    .filter(business_requests::subject.eq(&subject))
                    .filter(business_requests::operation.eq(&operation))
                    .filter(business_requests::request_key.eq(&request_key))
                    .first::<RequestRow>(tx.conn())
                    .optional()
                    .map_err(StorageError::from)?;
                let existing_request_id = if let Some(row) = existing_request {
                    if row.request_hash != request_hash {
                        return Ok(OrdinaryCommitOutcome::RequestConflict);
                    }
                    if row.state == "completed" {
                        return Ok(OrdinaryCommitOutcome::Replayed(
                            row.result_json.unwrap_or_default(),
                        ));
                    }
                    Some(row.id)
                } else {
                    None
                };

                // Version precondition for updates is read-only here; the
                // conditional UPDATE below is the authoritative check.
                let mut existing_for_update = None;
                if let OrdinaryCommitKind::Update {
                    update,
                    expected_version,
                } = &kind
                {
                    let existing = activities::table
                        .find(&update.id)
                        .first::<ActivityDB>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?;
                    let Some(existing) = existing else {
                        return Err(Error::Database(DatabaseError::QueryFailed(format!(
                            "activity {} was not found",
                            update.id
                        ))));
                    };
                    if existing.version != *expected_version {
                        return Ok(OrdinaryCommitOutcome::VersionConflict {
                            expected: *expected_version,
                            actual: existing.version,
                        });
                    }
                    existing_for_update = Some(existing);
                }

                let request_id = existing_request_id.unwrap_or_else(|| new_id("req"));
                let (activity, version) = match kind {
                    OrdinaryCommitKind::Create(prepared) => {
                        let mut db: ActivityDB = prepared.into();
                        if db.id.trim().is_empty() {
                            db.id = Uuid::new_v4().to_string();
                        }
                        wealthfolio_storage_sqlite::activities::assert_final_cash_floor(&db)?;
                        let inserted = diesel::insert_into(activities::table)
                            .values(&db)
                            .get_result::<ActivityDB>(tx.conn())
                            .map_err(StorageError::from)?;
                        let activity = Activity::from(inserted);
                        let version = activity_version(tx.conn(), &activity.id)?;
                        (activity, version)
                    }
                    OrdinaryCommitKind::Update {
                        update,
                        expected_version,
                    } => {
                        let existing = existing_for_update.expect("version checked above");
                        let fx_rate_patch = update.fx_rate;
                        let mut patch: ActivityDB = update.into();
                        patch.fx_rate = apply_decimal_patch(existing.fx_rate.clone(), fx_rate_patch);
                        if patch.metadata.is_none() {
                            patch.metadata = existing.metadata.clone();
                        }
                        patch.created_at = existing.created_at.clone();
                        patch.source_system = existing.source_system.clone();
                        patch.source_record_id = existing.source_record_id.clone();
                        patch.source_group_id = existing.source_group_id.clone();
                        patch.idempotency_key = existing.idempotency_key.clone();
                        patch.import_run_id = existing.import_run_id.clone();
                        patch.activity_type_override = existing.activity_type_override.clone();
                        patch.source_type = existing.source_type.clone();
                        patch.updated_at = now();
                        // Broker rows keep their base type; a direction/type
                        // correction lands in the override.
                        wealthfolio_storage_sqlite::activities::preserve_broker_base_type(
                            &mut patch,
                            &existing.activity_type,
                        );
                        wealthfolio_storage_sqlite::activities::assert_final_cash_floor(&patch)?;
                        let invalidates_splits =
                            wealthfolio_storage_sqlite::activities::activity_update_invalidates_spending_splits(
                                &existing, &patch,
                            );
                        let updated = diesel::update(
                            activities::table
                                .filter(activities::id.eq(&patch.id))
                                .filter(activities::version.eq(expected_version)),
                        )
                        .set(&patch)
                        .get_result::<ActivityDB>(tx.conn())
                        .optional()
                        .map_err(StorageError::from)?;
                        let Some(updated) = updated else {
                            return Ok(OrdinaryCommitOutcome::VersionConflict {
                                expected: expected_version,
                                actual: activity_version(tx.conn(), &patch.id).unwrap_or(expected_version),
                            });
                        };
                        if invalidates_splits {
                            clear_splits_tx(tx, &updated.id)?;
                        }
                        let activity = Activity::from(updated);
                        let version = activity_version(tx.conn(), &activity.id)?;
                        (activity, version)
                    }
                };

                apply_extras_tx(tx, &activity.id, &extras)?;

                let result = build_result(&activity, version, &request_id);
                let result_json = serde_json::to_string(&result).unwrap_or_default();
                upsert_completed_request_tx(
                    tx,
                    &ledger_id,
                    &subject,
                    &operation,
                    &request_key,
                    &request_hash,
                    &request_snapshot,
                    &request_id,
                    &result_json,
                )?;
                audit.entity_id = Some(activity.id.clone());
                audit.request_id = Some(request_id.clone());
                insert_audit_tx(tx, &audit)?;
                Ok(OrdinaryCommitOutcome::Committed {
                    activity: Box::new(activity),
                    version,
                    request_id,
                })
            })
            .await
            .map_err(crate::service::OrdinaryActivityError::from)
    }

    /// Insert a pending entry with its request record and audit in one
    /// transaction. Replays return the stored pending entry; a complete source
    /// identity returns the existing entry instead of creating a duplicate.
    pub async fn create_pending_atomic(
        &self,
        mut plan: PendingCreatePlan,
    ) -> std::result::Result<PendingCreateOutcome, crate::service::OrdinaryActivityError> {
        self.writer
            .exec_tx(
                move |tx| -> std::result::Result<PendingCreateOutcome, Error> {
                    let mut request_id = None;
                    if let Some(request) = &plan.request {
                        let existing = business_requests::table
                            .filter(business_requests::ledger_id.eq(&request.ledger_id))
                            .filter(business_requests::subject.eq(&request.subject))
                            .filter(business_requests::operation.eq(&request.operation))
                            .filter(business_requests::request_key.eq(&request.request_key))
                            .first::<RequestRow>(tx.conn())
                            .optional()
                            .map_err(StorageError::from)?;
                        if let Some(row) = existing {
                            if row.request_hash != request.request_hash {
                                return Ok(PendingCreateOutcome::RequestConflict);
                            }
                            if row.state == "completed" {
                                return Ok(PendingCreateOutcome::Replayed(
                                    row.result_json.unwrap_or_default(),
                                    row.id,
                                ));
                            }
                            request_id = Some(row.id);
                        }
                    }
                    if request_id.is_none() && plan.request.is_some() {
                        request_id = Some(new_id("req"));
                    }

                    if let Some(identity) = &plan.source_identity {
                        let existing = find_pending_by_source_identity_tx(tx, identity)?;
                        if let Some(row) = existing {
                            let mut result = crate::pending::PendingActivity::from(row.clone());
                            result.request_id = request_id.clone();
                            let result_json = serde_json::to_string(&result).unwrap_or_default();
                            complete_pending_request_tx(
                                tx,
                                plan.request.as_ref(),
                                request_id.as_deref(),
                                &result_json,
                            )?;
                            plan.audit.entity_id = Some(row.id.clone());
                            plan.audit.request_id = request_id.clone();
                            insert_audit_tx(tx, &plan.audit)?;
                            return Ok(PendingCreateOutcome::Existing(row, request_id));
                        }
                    }

                    insert_pending_tx(tx, &plan.row)?;
                    let row = pending_activities::table
                        .find(&plan.row.id)
                        .first::<PendingRow>(tx.conn())
                        .map_err(StorageError::from)?;
                    let mut result = crate::pending::PendingActivity::from(row.clone());
                    result.request_id = request_id.clone();
                    let result_json = serde_json::to_string(&result).unwrap_or_default();
                    complete_pending_request_tx(
                        tx,
                        plan.request.as_ref(),
                        request_id.as_deref(),
                        &result_json,
                    )?;
                    plan.audit.entity_id = Some(row.id.clone());
                    plan.audit.request_id = request_id.clone();
                    insert_audit_tx(tx, &plan.audit)?;
                    Ok(PendingCreateOutcome::Created(row, request_id))
                },
            )
            .await
            .map_err(crate::service::OrdinaryActivityError::from)
    }

    /// Confirm a pending entry atomically: insert the posted activity, flip the
    /// pending status with its version precondition, complete the request and
    /// write the audit row in one transaction. `Ok(None)` means the entry moved
    /// concurrently.
    #[allow(clippy::too_many_arguments)]
    pub async fn commit_pending_confirmation(
        &self,
        prepared: NewActivity,
        pending_id: String,
        expected_version: i32,
        confirmed_by: String,
        request_id: String,
        request_hash: String,
        request_snapshot: String,
        audit: NewAuditRow,
        build_result: PendingResultBuilder,
    ) -> Result<Option<(PendingRow, Activity)>> {
        let mut db: ActivityDB = prepared.into();
        if db.id.trim().is_empty() {
            db.id = Uuid::new_v4().to_string();
        }
        self.writer
            .exec_tx(move |tx| {
                let pending = pending_activities::table
                    .filter(pending_activities::id.eq(&pending_id))
                    .filter(pending_activities::status.eq("pending"))
                    .filter(pending_activities::version.eq(expected_version))
                    .first::<PendingRow>(tx.conn())
                    .optional()
                    .map_err(StorageError::from)?;
                let Some(mut pending) = pending else {
                    return Ok(None);
                };
                wealthfolio_storage_sqlite::activities::assert_final_cash_floor(&db)?;
                let inserted = diesel::insert_into(activities::table)
                    .values(&db)
                    .get_result::<ActivityDB>(tx.conn())
                    .map_err(StorageError::from)?;
                let activity = Activity::from(inserted);
                let timestamp = now();
                let updated = diesel::update(
                    pending_activities::table
                        .filter(pending_activities::id.eq(&pending_id))
                        .filter(pending_activities::version.eq(expected_version))
                        .filter(pending_activities::status.eq("pending")),
                )
                .set((
                    pending_activities::status.eq("committed"),
                    pending_activities::confirmed_by.eq(&confirmed_by),
                    pending_activities::committed_activity_id.eq(&activity.id),
                    pending_activities::updated_at.eq(&timestamp),
                    pending_activities::version.eq(expected_version.saturating_add(1)),
                ))
                .execute(tx.conn())
                .map_err(StorageError::from)?;
                if updated != 1 {
                    return Ok(None);
                }
                pending.status = "committed".to_string();
                pending.confirmed_by = Some(confirmed_by);
                pending.committed_activity_id = Some(activity.id.clone());
                pending.updated_at = timestamp;
                pending.version = expected_version.saturating_add(1);
                let result = build_result(&activity, &pending);
                let result_json = serde_json::to_string(&result).unwrap_or_default();
                upsert_completed_request_tx(
                    tx,
                    &audit.ledger_id,
                    &audit.actor,
                    "pending_activity_confirm",
                    &format!("pending-confirm:{}", pending.id),
                    &request_hash,
                    &request_snapshot,
                    &request_id,
                    &result_json,
                )?;
                insert_audit_tx(tx, &audit)?;
                Ok(Some((pending, activity)))
            })
            .await
    }
}

fn apply_decimal_patch(existing: Option<String>, patch: Option<Option<Decimal>>) -> Option<String> {
    match patch {
        None => existing,
        Some(None) => None,
        Some(Some(value)) => Some(value.to_string()),
    }
}

fn clear_splits_tx(tx: &mut DbWriteTx<'_>, activity_id: &str) -> Result<()> {
    diesel::delete(
        spending_activity_splits::table
            .filter(spending_activity_splits::activity_id.eq(activity_id)),
    )
    .execute(tx.conn())
    .map_err(StorageError::from)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn upsert_completed_request_tx(
    tx: &mut DbWriteTx<'_>,
    ledger_id: &str,
    subject: &str,
    operation: &str,
    request_key: &str,
    request_hash: &str,
    request_snapshot: &str,
    request_id: &str,
    result_json: &str,
) -> Result<()> {
    let existing = business_requests::table
        .filter(business_requests::ledger_id.eq(ledger_id))
        .filter(business_requests::subject.eq(subject))
        .filter(business_requests::operation.eq(operation))
        .filter(business_requests::request_key.eq(request_key))
        .first::<RequestRow>(tx.conn())
        .optional()
        .map_err(StorageError::from)?;
    if let Some(row) = existing {
        diesel::update(business_requests::table.find(&row.id))
            .set((
                business_requests::state.eq("completed"),
                business_requests::result_json.eq(Some(result_json.to_string())),
                business_requests::error_message.eq::<Option<String>>(None),
                business_requests::updated_at.eq(now()),
            ))
            .execute(tx.conn())
            .map_err(StorageError::from)?;
        return Ok(());
    }
    let row = NewRequestRow {
        id: request_id.to_string(),
        ledger_id: ledger_id.to_string(),
        subject: subject.to_string(),
        operation: operation.to_string(),
        request_key: request_key.to_string(),
        request_hash: request_hash.to_string(),
        request_snapshot: request_snapshot.to_string(),
        state: "completed".to_string(),
        result_json: Some(result_json.to_string()),
        error_message: None,
        created_at: now(),
        updated_at: now(),
    };
    insert_request_tx(tx, &row)
}

fn complete_pending_request_tx(
    tx: &mut DbWriteTx<'_>,
    request: Option<&PendingRequestParts>,
    request_id: Option<&str>,
    result_json: &str,
) -> Result<()> {
    let (Some(request), Some(request_id)) = (request, request_id) else {
        return Ok(());
    };
    upsert_completed_request_tx(
        tx,
        &request.ledger_id,
        &request.subject,
        &request.operation,
        &request.request_key,
        &request.request_hash,
        &request.request_snapshot,
        request_id,
        result_json,
    )
}

fn find_pending_by_source_identity_tx(
    tx: &mut DbWriteTx<'_>,
    identity: &PendingSourceIdentity,
) -> Result<Option<PendingRow>> {
    #[derive(QueryableByName)]
    struct IdRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        id: String,
    }
    let rows = diesel::sql_query(
        "SELECT id FROM pending_activities \
         WHERE json_extract(source, '$.channel') = ?1 \
           AND json_extract(source, '$.messageId') = ?2 \
           AND COALESCE(json_extract(source, '$.sourceAccount'), '') = ?3 \
           AND COALESCE(json_extract(source, '$.sourceSession'), '') = ?4 \
           AND json_extract(source, '$.entryId') = ?5 \
         LIMIT 1",
    )
    .bind::<diesel::sql_types::Text, _>(&identity.channel)
    .bind::<diesel::sql_types::Text, _>(&identity.message_id)
    .bind::<diesel::sql_types::Text, _>(identity.source_account.clone().unwrap_or_default())
    .bind::<diesel::sql_types::Text, _>(identity.source_session.clone().unwrap_or_default())
    .bind::<diesel::sql_types::Text, _>(&identity.entry_id)
    .load::<IdRow>(tx.conn())
    .map_err(StorageError::from)?;
    match rows.into_iter().next() {
        Some(row) => {
            let pending = pending_activities::table
                .find(&row.id)
                .first::<PendingRow>(tx.conn())
                .optional()
                .map_err(StorageError::from)?;
            Ok(pending)
        }
        None => Ok(None),
    }
}

fn apply_extras_tx(
    tx: &mut DbWriteTx<'_>,
    activity_id: &str,
    extras: &ActivityExtras,
) -> Result<()> {
    if let Some(cleared) = &extras.assignment {
        if cleared.is_none() {
            // An explicit clear removes every assignment and its split lines,
            // matching the single-select field the user emptied.
            diesel::delete(
                activity_taxonomy_assignments::table
                    .filter(activity_taxonomy_assignments::activity_id.eq(activity_id)),
            )
            .execute(tx.conn())
            .map_err(StorageError::from)?;
            clear_splits_tx(tx, activity_id)?;
            return apply_event_tx(tx, activity_id, extras);
        }
    }
    if let Some(Some((taxonomy_id, category_id))) = &extras.assignment {
        // Re-validate inside the transaction: an invalid category must roll
        // back the activity insert, not leave a half-written row.
        let category_exists: i64 = taxonomy_categories::table
            .filter(taxonomy_categories::id.eq(category_id))
            .filter(taxonomy_categories::taxonomy_id.eq(taxonomy_id))
            .count()
            .get_result(tx.conn())
            .map_err(StorageError::from)?;
        if category_exists == 0 {
            return Err(Error::Validation(
                wealthfolio_core::errors::ValidationError::InvalidInput(format!(
                    "category {category_id} does not belong to taxonomy {taxonomy_id}"
                )),
            ));
        }
        diesel::delete(
            activity_taxonomy_assignments::table
                .filter(activity_taxonomy_assignments::activity_id.eq(activity_id))
                .filter(activity_taxonomy_assignments::taxonomy_id.eq(taxonomy_id)),
        )
        .execute(tx.conn())
        .map_err(StorageError::from)?;
        // Single-select assignment replaces split lines.
        diesel::delete(
            spending_activity_splits::table
                .filter(spending_activity_splits::activity_id.eq(activity_id)),
        )
        .execute(tx.conn())
        .map_err(StorageError::from)?;
        let now = now();
        diesel::insert_into(activity_taxonomy_assignments::table)
            .values((
                activity_taxonomy_assignments::id.eq(format!("{activity_id}:{taxonomy_id}")),
                activity_taxonomy_assignments::activity_id.eq(activity_id),
                activity_taxonomy_assignments::taxonomy_id.eq(taxonomy_id),
                activity_taxonomy_assignments::category_id.eq(category_id),
                activity_taxonomy_assignments::weight.eq(10_000_i32),
                activity_taxonomy_assignments::source.eq("manual"),
                activity_taxonomy_assignments::created_at.eq(now.clone()),
                activity_taxonomy_assignments::updated_at.eq(now),
            ))
            .execute(tx.conn())
            .map_err(StorageError::from)?;
    }
    apply_event_tx(tx, activity_id, extras)
}

fn apply_event_tx(
    tx: &mut DbWriteTx<'_>,
    activity_id: &str,
    extras: &ActivityExtras,
) -> Result<()> {
    if let Some(event_id) = &extras.event_id {
        let event_exists: i64 = spending_events::table
            .filter(spending_events::id.eq(event_id))
            .count()
            .get_result(tx.conn())
            .map_err(StorageError::from)?;
        if event_exists == 0 {
            return Err(Error::Validation(
                wealthfolio_core::errors::ValidationError::InvalidInput(format!(
                    "spending event {event_id} was not found"
                )),
            ));
        }
        let timestamp = now();
        diesel::insert_into(spending_activity_events::table)
            .values((
                spending_activity_events::activity_id.eq(activity_id),
                spending_activity_events::event_id.eq(event_id),
                spending_activity_events::created_at.eq(timestamp.clone()),
                spending_activity_events::updated_at.eq(timestamp.clone()),
            ))
            .on_conflict(spending_activity_events::activity_id)
            .do_update()
            .set((
                spending_activity_events::event_id.eq(event_id),
                spending_activity_events::updated_at.eq(timestamp),
            ))
            .execute(tx.conn())
            .map_err(StorageError::from)?;
    } else if extras.clear_event {
        diesel::delete(
            spending_activity_events::table
                .filter(spending_activity_events::activity_id.eq(activity_id)),
        )
        .execute(tx.conn())
        .map_err(StorageError::from)?;
    }
    Ok(())
}

// Helpers shared by the store transaction bodies.

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339()
}

pub(crate) fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4())
}

pub(crate) fn insert_pending_tx(tx: &mut DbWriteTx<'_>, row: &NewPendingRow) -> Result<()> {
    diesel::insert_into(pending_activities::table)
        .values(row)
        .execute(tx.conn())
        .map_err(StorageError::from)?;
    Ok(())
}

pub(crate) fn update_pending_tx(
    tx: &mut DbWriteTx<'_>,
    row: &PendingRow,
    expected_version: i32,
) -> Result<bool> {
    let updated = diesel::update(
        pending_activities::table
            .filter(pending_activities::id.eq(&row.id))
            .filter(pending_activities::version.eq(expected_version)),
    )
    .set((
        pending_activities::status.eq(&row.status),
        pending_activities::account_id.eq(&row.account_id),
        pending_activities::activity_type.eq(&row.activity_type),
        pending_activities::activity_date.eq(&row.activity_date),
        pending_activities::currency.eq(&row.currency),
        pending_activities::amount.eq(&row.amount),
        pending_activities::fee.eq(&row.fee),
        pending_activities::tax.eq(&row.tax),
        pending_activities::notes.eq(&row.notes),
        pending_activities::asset_symbol.eq(&row.asset_symbol),
        pending_activities::confirmed_by.eq(&row.confirmed_by),
        pending_activities::committed_activity_id.eq(&row.committed_activity_id),
        pending_activities::updated_at.eq(&row.updated_at),
        pending_activities::version.eq(expected_version.saturating_add(1)),
    ))
    .execute(tx.conn())
    .map_err(StorageError::from)?;
    Ok(updated == 1)
}

pub(crate) fn insert_request_tx(tx: &mut DbWriteTx<'_>, row: &NewRequestRow) -> Result<()> {
    diesel::insert_into(business_requests::table)
        .values(row)
        .execute(tx.conn())
        .map_err(StorageError::from)?;
    Ok(())
}

pub(crate) fn insert_audit_tx(tx: &mut DbWriteTx<'_>, row: &NewAuditRow) -> Result<()> {
    diesel::insert_into(business_audit::table)
        .values(row)
        .execute(tx.conn())
        .map_err(StorageError::from)?;
    Ok(())
}
