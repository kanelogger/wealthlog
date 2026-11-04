//! Evidence links never apply, undo or otherwise modify purpose ownership.
use super::*;
use crate::investment_journal::{
    sources::current_reference, InvestmentExecution, SourceReferenceState,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionLinkTarget {
    pub execution_id: String,
    pub decision_revision_id: String,
    pub deviation_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkExistingExecution {
    pub target: ExecutionLinkTarget,
    pub expected_preview_hash: String,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnlinkExecution {
    pub link_id: String,
    pub expected_linked_at: String,
    pub reason: String,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionLinkPreview {
    pub schema_version: u32,
    pub target: ExecutionLinkTarget,
    pub source_version: String,
    pub temporal_classification: String,
    pub snapshot: Value,
    pub preview_hash: String,
}

impl InvestmentJournalService {
    pub fn decision_execution_progress(
        &self,
        revision: &str,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            let row = diesel::sql_query(
                "SELECT body_json AS body FROM investment_decision_revisions WHERE id=?1 AND status='CONFIRMED'",
            )
            .bind::<Text, _>(revision)
            .get_result::<JsonRow>(conn)?;
            execution_progress(conn, revision, &serde_json::from_str(&row.body).map_err(json_error)?)
        })
    }
    pub fn preview_execution_link(
        &self,
        target: ExecutionLinkTarget,
        caller: &JournalCaller,
    ) -> JournalResult<ExecutionLinkPreview> {
        caller.require_read()?;
        let (_, zone) = self.business_day()?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| build_link_preview(conn, target, &zone))
    }
    pub async fn link_existing_execution(
        &self,
        input: LinkExistingExecution,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        let record = self.request(
            "investment_execution_link",
            &input.request_key,
            &input,
            caller,
        )?;
        let (day, timezone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? {return Ok(result);}
            let preview=build_link_preview(conn,input.target,&timezone)?;
            if preview.preview_hash!=input.expected_preview_hash {return Err(JournalError::StateConflict);}
            let id=uuid::Uuid::new_v4().to_string();let at=now();
            let data=json!({"id":id,"execution":preview.target.execution_id,"revision":preview.target.decision_revision_id,"at":at,"sourceVersion":preview.source_version,"timing":preview.temporal_classification,"deviation":preview.target.deviation_reason,"snapshot":preview.snapshot,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id}).to_string();
            diesel::sql_query(INSERT_LINK).bind::<Text,_>(data).execute(conn)?;
            complete(conn,&record,&caller,&ledger,&id,json!({"linkId":id,"linkedAt":at,"factsChanged":false,"ownershipChanged":false}),day,&timezone)
        }).await
    }
    pub async fn unlink_execution(
        &self,
        input: UnlinkExecution,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        nonblank(&input.reason, "unlink reason")?;
        let record = self.request(
            "investment_execution_unlink",
            &input.request_key,
            &input,
            caller,
        )?;
        let (day, timezone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? {return Ok(result);}
            let at=now();let data=json!({"at":at,"reason":input.reason,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id}).to_string();
            let changed=diesel::sql_query("UPDATE investment_execution_links SET unlinked_at=json_extract(?1,'$.at'),unlink_reason=json_extract(?1,'$.reason'),unlinked_channel=json_extract(?1,'$.channel'),unlinked_actor_kind=json_extract(?1,'$.actorKind'),unlinked_actor_id=json_extract(?1,'$.actorId'),unlinked_credential_id=json_extract(?1,'$.credentialId') WHERE id=?2 AND linked_at=?3 AND unlinked_at IS NULL")
                .bind::<Text,_>(data).bind::<Text,_>(&input.link_id).bind::<Text,_>(&input.expected_linked_at).execute(conn)?;
            if changed!=1 {return Err(JournalError::StateConflict);}
            complete(conn,&record,&caller,&ledger,&input.link_id,json!({"linkId":input.link_id,"unlinkedAt":at,"factsChanged":false,"ownershipChanged":false}),day,&timezone)
        }).await
    }
    pub fn decision_execution_links(
        &self,
        decision_id: &str,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        let rows=diesel::sql_query("SELECT json_object('id',l.id,'executionId',l.execution_id,'decisionRevisionId',l.decision_revision_id,'revisionNumber',r.revision_number,'sourceVersion',l.source_version,'temporalClassification',l.temporal_classification,'deviationReason',l.deviation_reason,'snapshot',json(l.snapshot_json),'linkedAt',l.linked_at,'unlinkedAt',l.unlinked_at,'unlinkReason',l.unlink_reason,'createdChannel',l.created_channel,'createdActorKind',l.created_actor_kind,'createdActorId',l.created_actor_id,'createdCredentialId',l.created_credential_id) AS body FROM investment_execution_links l JOIN investment_decision_revisions r ON r.id=l.decision_revision_id WHERE r.decision_id=?1 ORDER BY l.linked_at,l.id")
            .bind::<Text,_>(decision_id).load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(json_error))
            .collect()
    }
}

pub(super) fn load_execution(
    conn: &mut SqliteConnection,
    id: &str,
) -> JournalResult<InvestmentExecution> {
    let row=diesel::sql_query(r#"SELECT json_object('id',id,'sourceKind',source_kind,'sourceId',source_id,'sourceVersion',source_version,'status',status,'accountId',account_id,'assetId',asset_id,'side',side,'quantity',quantity,'grossAmount',gross_amount,'activityCurrency',activity_currency,'netCash',net_cash,'settlementCurrency',settlement_currency,'fee',fee,'tax',tax,'cashBasis',cash_basis,'occurredAt',occurred_at,'timePrecision',time_precision,'snapshot',json(snapshot_json)) AS body FROM investment_executions WHERE id=?1"#)
        .bind::<Text,_>(id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    serde_json::from_str(&row.body).map_err(json_error)
}
fn build_link_preview(
    conn: &mut SqliteConnection,
    target: ExecutionLinkTarget,
    zone: &str,
) -> JournalResult<ExecutionLinkPreview> {
    let execution = load_execution(conn, &target.execution_id)?;
    if !matches!(
        execution.status,
        super::super::ExecutionStatus::Valid | super::super::ExecutionStatus::Reconcile
    ) {
        return Err(JournalError::StateConflict);
    }
    let kind = execution.source_kind.as_str();
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    let pending=diesel::sql_query("SELECT COUNT(*) AS count FROM investment_projection_state WHERE source_kind=?1 AND source_id=?2 AND (generation<>processed_generation OR is_deleted=1)")
        .bind::<Text,_>(kind).bind::<Text,_>(&execution.source_id).get_result::<Count>(conn)?.count;
    let (version, state) = current_reference(conn, kind, &execution.source_id, None)?;
    if pending != 0
        || state != SourceReferenceState::Valid
        || version.as_deref() != Some(&execution.source_version)
    {
        return Err(JournalError::StateConflict);
    }
    let active=diesel::sql_query("SELECT COUNT(*) AS count FROM investment_execution_links WHERE execution_id=?1 AND unlinked_at IS NULL")
        .bind::<Text,_>(&execution.id).get_result::<Count>(conn)?.count;
    if active != 0 {
        return Err(JournalError::AlreadyApplied);
    }
    let row=diesel::sql_query("SELECT json_object('id',id,'decisionId',decision_id,'revisionNumber',revision_number,'body',json(body_json),'confirmedAt',confirmed_at) AS body FROM investment_decision_revisions WHERE id=?1 AND status='CONFIRMED'")
        .bind::<Text,_>(&target.decision_revision_id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    let revision: Value = serde_json::from_str(&row.body).map_err(json_error)?;
    let timing = temporal_classification(
        &execution.occurred_at,
        &execution.time_precision,
        revision["confirmedAt"].as_str().unwrap_or_default(),
        zone,
    )?;
    let snapshot = json!({"schemaVersion":1,"execution":execution,"decision":revision,"ownershipChanged":false});
    let mut preview = ExecutionLinkPreview {
        schema_version: 1,
        target,
        source_version: execution.source_version,
        temporal_classification: timing,
        snapshot,
        preview_hash: String::new(),
    };
    preview.preview_hash = crate::requests::hash_json(&crate::requests::canonicalize(
        serde_json::to_value(&preview).map_err(json_error)?,
    ));
    Ok(preview)
}
fn temporal_classification(
    occurred: &str,
    precision: &str,
    confirmed: &str,
    zone: &str,
) -> JournalResult<String> {
    use chrono::DateTime;
    let Ok(confirmed) = DateTime::parse_from_rfc3339(confirmed) else {
        return Ok("UNDETERMINED".into());
    };
    if precision == "DATETIME" {
        if occurred.len() <= 10 {
            return Ok("UNDETERMINED".into());
        }
        return Ok(
            match wealthfolio_storage_sqlite::activities::parse_db_datetime(occurred) {
                Some(at) if at < confirmed => "BEFORE_CONFIRMATION",
                Some(at) if at > confirmed => "AFTER_CONFIRMATION",
                _ => "UNDETERMINED",
            }
            .into(),
        );
    }
    let Ok(date) = NaiveDate::parse_from_str(occurred.get(..10).unwrap_or_default(), "%Y-%m-%d")
    else {
        return Ok("UNDETERMINED".into());
    };
    let confirmation_day = confirmed
        .with_timezone(&parse_user_timezone(zone)?)
        .date_naive();
    Ok(if date < confirmation_day {
        "BEFORE_CONFIRMATION"
    } else if date > confirmation_day {
        "AFTER_CONFIRMATION"
    } else {
        "UNDETERMINED"
    }
    .into())
}
const INSERT_LINK: &str = r#"INSERT INTO investment_execution_links(id,execution_id,decision_revision_id,linked_at,source_version,temporal_classification,deviation_reason,snapshot_json,created_channel,created_actor_kind,created_actor_id,created_credential_id)
 VALUES(json_extract(?1,'$.id'),json_extract(?1,'$.execution'),json_extract(?1,'$.revision'),json_extract(?1,'$.at'),json_extract(?1,'$.sourceVersion'),json_extract(?1,'$.timing'),json_extract(?1,'$.deviation'),json_extract(?1,'$.snapshot'),json_extract(?1,'$.channel'),json_extract(?1,'$.actorKind'),json_extract(?1,'$.actorId'),json_extract(?1,'$.credentialId'))"#;

/// The new-trade path always targets the current confirmed revision. An
/// unconfirmed amendment is deliberately outside this target's identity.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionExecutionTarget {
    pub decision_id: String,
    pub revision_id: String,
    pub deviation_reason: Option<String>,
}

pub(super) fn current_decision_binding(
    conn: &mut SqliteConnection,
    target: &DecisionExecutionTarget,
) -> JournalResult<Value> {
    if let Some(reason) = &target.deviation_reason {
        nonblank(reason, "deviationReason")?;
    }
    let row=diesel::sql_query("SELECT json_object('id',r.id,'decisionId',d.id,'revisionNumber',r.revision_number,'body',json(r.body_json),'confirmedAt',r.confirmed_at,'executionState',d.execution_state) AS body FROM investment_decisions d JOIN investment_decision_revisions r ON r.id=d.current_revision_id WHERE d.id=?1 AND r.id=?2 AND d.lifecycle='CONFIRMED' AND r.status='CONFIRMED'")
        .bind::<Text,_>(&target.decision_id).bind::<Text,_>(&target.revision_id).get_result::<JsonRow>(conn).optional()?.ok_or(JournalError::StateConflict)?;
    let mut decision: Value = serde_json::from_str(&row.body).map_err(json_error)?;
    decision["progress"] = execution_progress(conn, &target.revision_id, &decision["body"])?;
    decision["deviationReason"] = json!(target.deviation_reason);
    Ok(decision)
}

pub(super) fn insert_prepared_link(
    conn: &mut SqliteConnection,
    id: &str,
    execution: &InvestmentExecution,
    decision: &Value,
    caller: &JournalCaller,
    zone: &str,
) -> JournalResult<()> {
    let revision = decision["id"]
        .as_str()
        .ok_or_else(|| invalid("confirmed decision has no revision id"))?;
    let timing = temporal_classification(
        &execution.occurred_at,
        &execution.time_precision,
        decision["confirmedAt"].as_str().unwrap_or_default(),
        zone,
    )?;
    let mut version = decision.clone();
    if let Some(body) = version.as_object_mut() {
        body.remove("progress");
    }
    let snapshot = json!({"schemaVersion":1,"execution":execution,"decision":version,"ownershipChangedByLink":false});
    let data=json!({"id":id,"execution":execution.id,"revision":revision,"at":now(),"sourceVersion":execution.source_version,"timing":timing,"deviation":decision["deviationReason"],"snapshot":snapshot,"channel":caller.channel.as_str(),"actorKind":caller.actor_kind.as_str(),"actorId":caller.actor_id,"credentialId":caller.credential_id}).to_string();
    diesel::sql_query(INSERT_LINK)
        .bind::<Text, _>(data)
        .execute(conn)?;
    Ok(())
}

fn execution_progress(
    conn: &mut SqliteConnection,
    revision: &str,
    body: &Value,
) -> JournalResult<Value> {
    let rows=diesel::sql_query("SELECT json_object('execution',e.id,'indexedVersion',e.source_version,'linkedVersion',l.source_version,'pending',EXISTS(SELECT 1 FROM investment_projection_state p WHERE p.source_kind=e.source_kind AND p.source_id=e.source_id AND (p.generation<>p.processed_generation OR p.is_deleted=1))) AS body FROM investment_execution_links l JOIN investment_executions e ON e.id=l.execution_id WHERE l.decision_revision_id=?1 AND l.unlinked_at IS NULL ORDER BY l.linked_at,l.id")
        .bind::<Text,_>(revision).load::<JsonRow>(conn)?;
    let plan_side = body["action"].as_str();
    let plan_asset = body["assetId"].as_str();
    let plan_currency = body["currency"].as_str();
    let trade_plan = matches!(plan_side, Some("BUY" | "SELL"));
    let mut quantity = (trade_plan && plan_asset.is_some()).then_some(ExactDecimal::ZERO);
    let mut cash = (trade_plan && plan_currency.is_some()).then_some(ExactDecimal::ZERO);
    let mut executions = Vec::new();
    let mut unit_dates = Vec::new();
    for row in rows {
        let anchor: Value = serde_json::from_str(&row.body).map_err(json_error)?;
        let execution = load_execution(conn, anchor["execution"].as_str().unwrap_or_default())?;
        let (version, state) = current_reference(
            conn,
            execution.source_kind.as_str(),
            &execution.source_id,
            None,
        )?;
        let source = super::execution_reconciliation::source_anchor(conn, &execution.id)?;
        let acknowledged = super::execution_reconciliation::acknowledged(conn, &source)?;
        let reliable = state == SourceReferenceState::Valid
            && matches!(
                execution.status,
                super::super::ExecutionStatus::Valid | super::super::ExecutionStatus::Reconcile
            )
            && version.as_deref() == Some(&execution.source_version)
            && anchor["pending"] == 0
            && (anchor["linkedVersion"] == anchor["indexedVersion"] || acknowledged);
        let matches_plan = Some(execution.side.as_str()) == plan_side
            && plan_asset.is_none_or(|id| execution.asset_id.as_deref() == Some(id));
        if matches_plan {
            if acknowledged
                && matches!(
                    source.source_state,
                    SourceReferenceState::Deleted | SourceReferenceState::Invalid
                )
            {
                executions.push(json!({"execution":execution,"matchesPlan":true,"sourceVerified":false,"sourceAcknowledged":true,"effective":false}));
                continue;
            }
            quantity = if reliable {
                quantity
                    .zip(execution.quantity)
                    .and_then(|(a, b)| a.value().checked_add(b.value()))
                    .map(ExactDecimal::new)
            } else {
                None
            };
            unit_dates.push((
                execution.occurred_at.clone(),
                execution.time_precision.clone(),
            ));
            // A manual request's arithmetic defaults do not prove its cash
            // consumption when fees/tax are unknown. Reliable ledger final
            // cash remains usable independently of its charge breakdown.
            let charges_known = execution.source_kind
                != super::super::InvestmentSourceKind::ManualTrade
                || (execution.fee.is_some() && execution.tax.is_some());
            cash = if reliable
                && charges_known
                && execution.settlement_currency.as_deref() == plan_currency
            {
                cash.zip(execution.net_cash)
                    .and_then(|(a, b)| a.value().checked_add(b.value().abs()))
                    .map(ExactDecimal::new)
            } else {
                None
            };
        }
        executions.push(
            json!({"execution":execution,"matchesPlan":matches_plan,"sourceVerified":reliable}),
        );
    }
    let planned_quantity = body["plannedQuantity"]
        .as_str()
        .map(ExactDecimal::parse)
        .transpose()?;
    let (unit_date, unit_status) = quantity_unit_status(conn, revision, plan_asset, &unit_dates)?;
    if unit_status != "VERIFIED_UNCHANGED" {
        quantity = None;
    }
    let planned_amount = body["plannedAmount"]
        .as_str()
        .map(ExactDecimal::parse)
        .transpose()?;
    let remaining_quantity = if plan_asset.is_some() {
        planned_quantity
            .zip(quantity)
            .and_then(|(a, b)| a.value().checked_sub(b.value()))
            .map(ExactDecimal::new)
    } else {
        None
    };
    let remaining_amount = planned_amount
        .zip(cash)
        .and_then(|(a, b)| a.value().checked_sub(b.value()))
        .map(ExactDecimal::new);
    Ok(
        json!({"linkedExecutions":executions,"quantityForPlannedAsset":if plan_asset.is_some(){quantity}else{None},"settlementCashForPlanCurrency":cash,"remainingQuantity":remaining_quantity,"remainingCashAmount":remaining_amount,"quantityUnitDate":unit_date,"quantityUnitStatus":unit_status,"amountBasis":"ABSOLUTE_SETTLEMENT_CASH_IN_PLAN_CURRENCY","automaticallyClosesExecution":false}),
    )
}

/// A plan has no implicit unit conversion. Until a corporate-action mapping
/// can be established, report unknown rather than summing different share units.
fn quantity_unit_status(
    conn: &mut SqliteConnection,
    revision: &str,
    asset: Option<&str>,
    dates: &[(String, String)],
) -> JournalResult<(Option<NaiveDate>, &'static str)> {
    let Some(asset_id) = asset else {
        return Ok((None, "NO_PLANNED_ASSET"));
    };
    let raw = diesel::sql_query(
        "SELECT context_snapshot_json AS body FROM investment_decision_revisions WHERE id=?1",
    )
    .bind::<Text, _>(revision)
    .get_result::<JsonRow>(conn)?
    .body;
    let context: Value = serde_json::from_str(&raw).map_err(json_error)?;
    let day = context["response"]["asOfDate"]
        .as_str()
        .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok());
    let Some(day) = day else {
        return Ok((None, "UNVERIFIED_CONTEXT"));
    };
    let references = &context["response"]["funding"]["data"]["decisionReferences"];
    let Some(zone_name) = references["unitTimezone"].as_str() else {
        return Ok((Some(day), "UNVERIFIED_CONTEXT"));
    };
    let zone = parse_user_timezone(zone_name)?;
    let original = references["assets"]
        .as_array()
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset["id"].as_str() == Some(asset_id))
        })
        .map(|asset| &asset["unitBasis"]);
    let current = asset_unit_basis(conn, asset_id, day, zone_name)?;
    if original
        .is_none_or(|original| original["status"] != "VERIFIED_EQUITY" || *original != current)
    {
        return Ok((Some(day), "UNVERIFIED_CONTEXT"));
    }
    use wealthfolio_storage_sqlite::schema::activities;
    let mut low = day;
    let mut high = day;
    for (occurred, precision) in dates {
        let actual = if precision == "DATE" {
            NaiveDate::parse_from_str(occurred.get(..10).unwrap_or_default(), "%Y-%m-%d").ok()
        } else {
            wealthfolio_storage_sqlite::activities::parse_db_datetime(occurred)
                .map(|stamp| stamp.with_timezone(&zone).date_naive())
        };
        let Some(actual) = actual else {
            return Ok((Some(day), "UNVERIFIED_EXECUTION_DATE"));
        };
        low = low.min(actual);
        high = high.max(actual);
    }
    if !dates.is_empty() {
        // Date-only records carry no UTC instant. Keep a two-day margin so a
        // ledger timezone change cannot make a boundary split disappear.
        low = low
            .checked_sub_signed(chrono::Duration::days(2))
            .unwrap_or(low);
        high = high
            .checked_add_signed(chrono::Duration::days(2))
            .unwrap_or(high);
        let rows = activities::table
            .filter(activities::asset_id.eq(asset_id))
            .filter(activities::status.eq("POSTED"))
            .load::<wealthfolio_storage_sqlite::activities::ActivityDB>(conn)?;
        for row in rows {
            if wealthfolio_core::activities::effective_activity_type(
                &row.activity_type,
                row.activity_type_override.as_deref(),
            ) != "SPLIT"
            {
                continue;
            }
            let split =
                wealthfolio_storage_sqlite::activities::parse_db_datetime(&row.activity_date)
                    .map(|stamp| stamp.with_timezone(&zone).date_naive());
            if split.is_none_or(|split| split >= low && split <= high) {
                return Ok((Some(day), "UNVERIFIED_CORPORATE_ACTION"));
            }
        }
    }
    Ok((Some(day), "VERIFIED_UNCHANGED"))
}

pub(super) fn asset_unit_basis(
    conn: &mut SqliteConnection,
    asset_id: &str,
    day: NaiveDate,
    timezone: &str,
) -> JournalResult<Value> {
    use wealthfolio_storage_sqlite::{
        assets::AssetDB,
        schema::{activities, assets},
    };
    let Some(raw) = assets::table
        .find(asset_id)
        .first::<AssetDB>(conn)
        .optional()?
    else {
        return Ok(json!({"status":"UNVERIFIED_INSTRUMENT"}));
    };
    if raw
        .metadata
        .as_ref()
        .is_some_and(|raw| serde_json::from_str::<Value>(raw).is_err())
    {
        return Ok(json!({"status":"UNVERIFIED_INSTRUMENT"}));
    }
    let native: wealthfolio_core::assets::Asset = raw.into();
    if !native.is_unit_equity() {
        return Ok(json!({"status":"UNVERIFIED_INSTRUMENT"}));
    }
    let zone = parse_user_timezone(timezone)?;
    let rows = activities::table
        .filter(activities::asset_id.eq(asset_id))
        .filter(activities::status.eq("POSTED"))
        .order(activities::id)
        .load::<wealthfolio_storage_sqlite::activities::ActivityDB>(conn)?;
    let mut sources = vec![];
    for row in rows {
        if wealthfolio_core::activities::effective_activity_type(
            &row.activity_type,
            row.activity_type_override.as_deref(),
        ) != "SPLIT"
        {
            continue;
        }
        let Some(split) =
            wealthfolio_storage_sqlite::activities::parse_db_datetime(&row.activity_date)
                .map(|stamp| stamp.with_timezone(&zone).date_naive())
        else {
            return Ok(json!({"status":"UNVERIFIED_CORPORATE_ACTION"}));
        };
        if split <= day {
            sources.push(json!({"id":row.id,"version":super::super::sources::activity_source_version(&row)?}));
        }
    }
    Ok(
        json!({"status":"VERIFIED_EQUITY","contractMultiplier":"1","asOfDate":day,"splitSourceHash":crate::requests::hash_json(&crate::requests::canonicalize(json!(sources)))}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_only_same_business_day_remains_undetermined() {
        assert_eq!(
            temporal_classification(
                "2026-11-03",
                "DATE",
                "2026-11-02T18:00:00Z",
                "Asia/Shanghai"
            )
            .unwrap(),
            "UNDETERMINED"
        );
        assert_eq!(
            temporal_classification(
                "2026-11-02",
                "DATE",
                "2026-11-02T18:00:00Z",
                "Asia/Shanghai"
            )
            .unwrap(),
            "BEFORE_CONFIRMATION"
        );
        assert_eq!(
            temporal_classification(
                "2026-11-04",
                "DATE",
                "2026-11-02T18:00:00Z",
                "Asia/Shanghai"
            )
            .unwrap(),
            "AFTER_CONFIRMATION"
        );
        assert_eq!(
            temporal_classification(
                "2026-11-03T00:00:00+08:00",
                "DATETIME",
                "2026-11-02T18:00:00Z",
                "Asia/Shanghai"
            )
            .unwrap(),
            "BEFORE_CONFIRMATION"
        );
        assert_eq!(
            temporal_classification(
                "2026-11-02T19:00:00Z",
                "DATETIME",
                "2026-11-02T18:00:00Z",
                "Asia/Shanghai"
            )
            .unwrap(),
            "AFTER_CONFIRMATION"
        );
    }
}
