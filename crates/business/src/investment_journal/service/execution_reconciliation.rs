//! Append-only reconciliation receipts; facts and original evidence links stay intact.
use super::links::load_execution;
use super::*;
use crate::investment_journal::{
    sources::current_reference, ExecutionStatus, InvestmentExecution, SourceReferenceState,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionOwnershipResolution {
    AdjustOwnership,
    NoOwnershipChange,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionReconciliationDraft {
    pub execution_id: String,
    pub resolution: ExecutionOwnershipResolution,
    #[serde(default)]
    pub resources: Vec<FundResource>,
    #[serde(default)]
    pub entries: Vec<AllocationDelta>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconcileExecution {
    pub draft: ExecutionReconciliationDraft,
    pub reason: String,
    pub expected_preview_hash: String,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReconciliationPreview {
    pub schema_version: u32,
    pub source: ExecutionSourceAnchor,
    pub funding: AllocationPreview,
    pub resolution: ExecutionOwnershipResolution,
    pub entries: Vec<AllocationDelta>,
    pub balances_after: Vec<crate::investment_journal::PurposeResourceBalance>,
    pub preview_hash: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSourceAnchor {
    pub execution: InvestmentExecution,
    pub source_state: SourceReferenceState,
    pub current_source_version: Option<String>,
    pub source_anchor_hash: String,
    pub projection_pending: bool,
    pub source_change: Option<Value>,
    pub active_links: Vec<Value>,
}
#[derive(Clone)]
pub(super) struct PreparedExecutionAck {
    pub draft: ExecutionReconciliationDraft,
    pub expected_preview_hash: String,
}

impl InvestmentJournalService {
    pub async fn preview_execution_reconciliation(
        &self,
        draft: &ExecutionReconciliationDraft,
        caller: &JournalCaller,
    ) -> JournalResult<ExecutionReconciliationPreview> {
        caller.require_read()?;
        validate_draft(draft)?;
        let mut resources = draft.resources.clone();
        resources.extend(draft.entries.iter().map(|e| e.resource.clone()));
        let (day, _) = self.business_day()?;
        let funding = if resources.is_empty() {
            let mut conn = get_connection(self.store.pool())?;
            build_preview(&mut conn, vec![], day)?
        } else {
            self.preview(resources, day).await?
        };
        let observations: Vec<_> = funding
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            if !observations.is_empty() {
                self.resources.validate(conn, &observations)?;
            }
            let current = build_preview(conn, observations, day)?;
            if current.preview_hash != funding.preview_hash {
                return Err(JournalError::StateConflict);
            }
            build_reconciliation_preview(conn, draft, current)
        })
    }
    pub async fn reconcile_execution(
        &self,
        input: ReconcileExecution,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        nonblank(&input.reason, "reason")?;
        validate_draft(&input.draft)?;
        let record = self.request(
            "investment_execution_reconcile",
            &input.request_key,
            &input,
            caller,
        )?;
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let preview = self
            .preview_execution_reconciliation(&input.draft, caller)
            .await?;
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        let ack = PreparedExecutionAck {
            draft: input.draft.clone(),
            expected_preview_hash: input.expected_preview_hash,
        };
        self.apply_allocation_internal(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: input.reason,
                entries: input.draft.entries,
                resources: input.draft.resources,
                expected_preview_hash: preview.funding.preview_hash,
                request_key: input.request_key,
            },
            Some(record),
            Some(ack),
            caller,
        )
        .await
    }
}
fn validate_draft(draft: &ExecutionReconciliationDraft) -> JournalResult<()> {
    nonblank(&draft.execution_id, "executionId")?;
    match draft.resolution {
        ExecutionOwnershipResolution::NoOwnershipChange if !draft.entries.is_empty() => {
            Err(invalid("NO_OWNERSHIP_CHANGE requires zero entries"))
        }
        ExecutionOwnershipResolution::AdjustOwnership if draft.entries.is_empty() => {
            Err(invalid("ADJUST_OWNERSHIP requires explicit entries"))
        }
        _ => Ok(()),
    }
}
fn build_reconciliation_preview(
    conn: &mut SqliteConnection,
    draft: &ExecutionReconciliationDraft,
    funding: AllocationPreview,
) -> JournalResult<ExecutionReconciliationPreview> {
    let source = source_anchor(conn, &draft.execution_id)?;
    if source.projection_pending || source.source_state == SourceReferenceState::Unavailable {
        return Err(JournalError::StateConflict);
    }
    let balances = allocation_rows(conn)?;
    let actual = funding
        .resources
        .iter()
        .map(|r| (r.observation.resource.clone(), r.balance.actual))
        .collect();
    let after = apply_deltas(&balances, &draft.entries, &actual)?;
    let balances_after = after
        .into_iter()
        .filter(|((_, r), _)| {
            funding
                .resources
                .iter()
                .any(|s| &s.observation.resource == r)
        })
        .map(
            |((purpose_id, resource), amount)| crate::investment_journal::PurposeResourceBalance {
                purpose_id,
                resource,
                amount,
            },
        )
        .collect();
    let mut preview = ExecutionReconciliationPreview {
        schema_version: 1,
        source,
        funding,
        resolution: draft.resolution,
        entries: draft.entries.clone(),
        balances_after,
        preview_hash: String::new(),
    };
    preview.preview_hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"source":preview.source,"funding":preview.funding.preview_hash,"resolution":preview.resolution,"entries":preview.entries}),
    ));
    Ok(preview)
}
pub(super) fn validate_ack(
    conn: &mut SqliteConnection,
    ack: &PreparedExecutionAck,
    funding: &AllocationPreview,
    entries: &[AllocationDelta],
) -> JournalResult<Value> {
    if serde_json::to_value(entries).map_err(json_error)?
        != serde_json::to_value(&ack.draft.entries).map_err(json_error)?
    {
        return Err(JournalError::StateConflict);
    }
    let preview = build_reconciliation_preview(conn, &ack.draft, funding.clone())?;
    if preview.preview_hash != ack.expected_preview_hash {
        return Err(JournalError::StateConflict);
    }
    Ok(
        json!({"schemaVersion":1,"executionId":preview.source.execution.id,"sourceKind":preview.source.execution.source_kind,"sourceId":preview.source.execution.source_id,"sourceVersion":preview.source.current_source_version,"sourceState":preview.source.source_state,"sourceAnchorHash":preview.source.source_anchor_hash,"resolution":preview.resolution,"reviewedExecution":preview.source.execution,"links":preview.source.active_links,"resources":funding.resources.iter().map(|r|&r.observation.resource).collect::<Vec<_>>()}),
    )
}
pub(super) fn source_anchor(
    conn: &mut SqliteConnection,
    id: &str,
) -> JournalResult<ExecutionSourceAnchor> {
    let execution = load_execution(conn, id)?;
    let (current_source_version, mut source_state) = current_reference(
        conn,
        execution.source_kind.as_str(),
        &execution.source_id,
        None,
    )?;
    if source_state == SourceReferenceState::Valid
        && matches!(
            execution.status,
            ExecutionStatus::Invalid | ExecutionStatus::Deleted
        )
    {
        source_state = SourceReferenceState::Invalid;
    }
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    let pending=diesel::sql_query("SELECT COUNT(*) AS count FROM investment_projection_state WHERE source_kind=?1 AND source_id=?2 AND processed_generation<generation")
        .bind::<Text,_>(execution.source_kind.as_str()).bind::<Text,_>(&execution.source_id).get_result::<Count>(conn)?.count>0;
    let links=diesel::sql_query("SELECT json_object('id',id,'revisionId',decision_revision_id,'sourceVersion',source_version,'snapshot',json(snapshot_json)) AS body FROM investment_execution_links WHERE execution_id=?1 AND unlinked_at IS NULL ORDER BY id")
        .bind::<Text,_>(id).load::<JsonRow>(conn)?.into_iter().map(|r|serde_json::from_str(&r.body).map_err(json_error)).collect::<JournalResult<Vec<_>>>()?;
    let change=diesel::sql_query("SELECT source_snapshot_json AS body FROM investment_projection_state WHERE source_kind=?1 AND source_id=?2").bind::<Text,_>(execution.source_kind.as_str()).bind::<Text,_>(&execution.source_id).get_result::<JsonRow>(conn).optional()?.map(|r|serde_json::from_str(&r.body).map_err(json_error)).transpose()?;
    let source_anchor_hash = anchor_hash(
        &current_source_version,
        &source_state,
        &serde_json::to_value(&execution).map_err(json_error)?,
    );
    Ok(ExecutionSourceAnchor {
        execution,
        source_state,
        current_source_version,
        source_anchor_hash,
        projection_pending: pending,
        source_change: change,
        active_links: links,
    })
}
fn anchor_hash(
    version: &Option<String>,
    state: &SourceReferenceState,
    execution: &Value,
) -> String {
    crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"version":version,"state":state,"effect":effect(execution)}),
    ))
}
fn effect(execution: &Value) -> Value {
    let mut value = json!({});
    for key in [
        "status",
        "accountId",
        "assetId",
        "side",
        "quantity",
        "netCash",
        "settlementCurrency",
        "activityCurrency",
        "fee",
        "tax",
        "cashBasis",
    ] {
        value[key] = execution[key].clone();
    }
    value
}
pub(super) fn original_link_anchor_hash(link: &Value) -> String {
    let execution = &link["snapshot"]["execution"];
    anchor_hash(
        &execution["sourceVersion"].as_str().map(str::to_string),
        &SourceReferenceState::Valid,
        execution,
    )
}
pub(super) fn latest_ack(conn: &mut SqliteConnection, id: &str) -> JournalResult<Option<Value>> {
    let row=diesel::sql_query("SELECT json_extract(detail_json,'$.executionAcknowledgement') AS body FROM fund_allocation_operations WHERE json_extract(detail_json,'$.executionAcknowledgement.executionId')=?1 ORDER BY rowid DESC LIMIT 1")
        .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?;
    row.map(|r| serde_json::from_str(&r.body).map_err(json_error))
        .transpose()
}
pub(super) fn acknowledged(
    conn: &mut SqliteConnection,
    source: &ExecutionSourceAnchor,
) -> JournalResult<bool> {
    Ok(latest_ack(conn, &source.execution.id)?
        .is_some_and(|ack| ack["sourceAnchorHash"].as_str() == Some(&source.source_anchor_hash)))
}
