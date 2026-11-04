//! Atomic fact + execution + ownership use cases. All writes use the same
//! BusinessStore writer transaction, including request replay and final audit.

use super::*;
use crate::investment_journal::{
    accounting::{project_manual_request, snapshot_source},
    compare_fund_observation, preview_trade_ownership,
    projection::save_execution,
    ExecutionStatus, FundReconciliationItem, FundReconciliationKind, InvestmentExecution,
    InvestmentSourceKind, TradeImpactPreview, TradeOwnershipPreview, TradePurposeShare,
};
use crate::investments::RecordManualTrade;
use wealthfolio_core::portfolio::snapshot::AccountStateSnapshot;
use wealthfolio_storage_sqlite::portfolio::snapshot::SnapshotRepository;

mod cash_transfers;
pub use cash_transfers::*;
mod ledger_trades;
pub use ledger_trades::*;
mod decision_trades;
pub use decision_trades::*;

mod corrections;
pub use corrections::*;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordManualTradeWithOwnership {
    pub trade: RecordManualTrade,
    pub shares: Vec<TradePurposeShare>,
    pub reason: String,
    pub expected_preview_hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvestmentTradeOwnershipPreview {
    pub schema_version: u32,
    pub trade: TradeImpactPreview,
    pub funding: AllocationPreview,
    pub ownership: TradeOwnershipPreview,
    pub purposes: Vec<Value>,
    pub pending_reconciliation: Vec<FundReconciliationItem>,
    pub preview_hash: String,
}
pub type ManualTradeOwnershipPreview = InvestmentTradeOwnershipPreview;
pub type LedgerTradeOwnershipPreview = InvestmentTradeOwnershipPreview;

impl InvestmentJournalService {
    pub async fn preview_manual_trade_ownership(
        &self,
        trade: &RecordManualTrade,
        shares: Vec<TradePurposeShare>,
        caller: &JournalCaller,
    ) -> JournalResult<ManualTradeOwnershipPreview> {
        caller.require_read()?;
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?;
        let trade_preview = bookkeeping.preview_manual_trade(trade).await?;
        let resources = trade_resources(&trade_preview)?;
        let (day, _) = self.business_day()?;
        let funding = self.preview(resources, day).await?;
        let observations: Vec<_> = funding
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            self.resources.validate(conn, &observations)?;
            build_trade_preview(conn, trade_preview, funding, &shares)
        })
    }

    pub async fn record_manual_trade_with_ownership(
        &self,
        input: RecordManualTradeWithOwnership,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        self.record_manual_trade_internal(input, None, None, caller)
            .await
    }

    async fn record_manual_trade_internal(
        &self,
        input: RecordManualTradeWithOwnership,
        request_override: Option<RequestRecord>,
        binding: Option<decision_trades::DecisionTradeBinding>,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        if !caller.fact_permissions.holdings_write {
            return Err(JournalError::PermissionDenied);
        }
        nonblank(&input.reason, "reason")?;
        let record = match request_override {
            Some(record) => record,
            None => self.request(
                "investment_manual_trade_record",
                &input.trade.request_key,
                &input,
                caller,
            )?,
        };
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?
            .clone();
        let prepared = bookkeeping.prepare_manual_trade(&input.trade).await?;
        let preview = self
            .preview_manual_trade_ownership(&input.trade, input.shares.clone(), caller)
            .await?;
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        let (day, timezone) = self.business_day()?;
        let reader = self.resources.clone();
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let snapshot_for_notification = prepared.snapshot.clone();
        let result=transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok(result); }
            let observations:Vec<_>=preview.funding.resources.iter().map(|r| r.observation.clone()).collect();
            reader.validate(conn,&observations)?;
            prepared.validate(conn)?;
            let funding=build_preview(conn,observations,day)?;
            let current=build_trade_preview(conn,preview.trade,funding,&input.shares)?;
            if current.preview_hash!=input.expected_preview_hash { return Err(JournalError::StateConflict); }
            let decision=decision_trades::validate_owned_binding(conn,&binding,&current)?;
            let link_id=decision.as_ref().map(|_|uuid::Uuid::new_v4().to_string());
            let request_id=uuid::Uuid::new_v4().to_string();
            let operation_id=uuid::Uuid::new_v4().to_string();
            let execution_id=InvestmentExecution::stable_id(&InvestmentSourceKind::ManualTrade,&request_id);
            SnapshotRepository::save_snapshot_tx(conn,prepared.snapshot.clone())?;
            let mut trade_result=prepared.result.clone();
            trade_result["requestId"]=json!(request_id);
            trade_result["replayed"]=json!(false);
            let mut result=json!({"trade":trade_result,"executionId":execution_id,"allocationOperationId":operation_id,"ownershipStatus":"APPLIED"});
            if let Some(decision)=&decision {result["executionLinkId"]=json!(link_id);result["decisionRevisionId"]=decision["id"].clone();}
            let row=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            let execution=project_manual_request(&row)?;
            if execution.status!=ExecutionStatus::Valid { return Err(JournalError::ReconciliationRequired); }
            save_execution(conn,&execution)?;
            // The same projector can resume other pending facts later; this
            // exact new execution is already indexed in the atomic transaction.
            diesel::sql_query("UPDATE investment_projection_state SET processed_generation=generation,refresh_target_generation=NULL,resource_impact_json=?1 WHERE source_kind='MANUAL_TRADE' AND source_id=?2")
                .bind::<Text,_>(json!({"schemaVersion":1,"changes":current.trade.changes,"issues":[]}).to_string())
                .bind::<Text,_>(&request_id).execute(conn)?;
            let after_version=reader.committed_state_version(conn,day)?;
            let before:Vec<_>=current.funding.resources.iter().map(|r| r.observation.clone()).collect();
            let mut after=manual_observations_after(&before,&prepared.snapshot,&after_version);
            let baselines=confirmed_observations(conn)?;
            let balances=allocation_rows(conn)?;
            carry_unresolved_sources(&baselines,&before,&balances,&mut after)?;
            let confirmation=json!({"schemaVersion":1,"resources":after.iter().map(|o| json!({"resource":o.resource,"sources":o.sources})).collect::<Vec<_>>()});
            let operation=json!({"id":operation_id,"kind":"TRADE","requestId":request_id,"reason":input.reason,
                "sourceKind":execution.source_kind,"sourceId":execution.source_id,"sourceVersion":execution.source_version,
                "firstApplicationKey":format!("TRADE:MANUAL_TRADE:{}",execution.source_id),"correctionOf":null,
                "observation":{"schemaVersion":1,"resources":after,"before":before},"confirmation":confirmation,
                "detail":{"schemaVersion":1,"trade":current.trade,"ownership":current.ownership}});
            insert_compound_operation(conn,&operation,&caller)?;
            save_ownership_legs(conn,&operation_id,&balances,&current.ownership)?;
            if let (Some(decision),Some(link_id))=(&decision,&link_id) {super::links::insert_prepared_link(conn,link_id,&execution,decision,&caller,&timezone)?;}
            // Deliberately last: an audit or enablement failure rolls back the
            // fact, projection, request and every ownership leg together.
            audit_completed_request(conn,&record,&caller,&ledger,&execution_id,&request_id,&row.created_at,day,&timezone)?;
            Ok(result)
        }).await?;
        if result["replayed"] != true {
            bookkeeping.notify_compound_snapshot_committed(&snapshot_for_notification);
        }
        Ok(result)
    }
}

fn trade_resources(trade: &TradeImpactPreview) -> JournalResult<Vec<FundResource>> {
    let security = trade
        .changes
        .iter()
        .find(|c| matches!(c.resource, FundResource::Security { .. }))
        .ok_or(JournalError::ReconciliationRequired)?;
    let cash = FundResource::Cash {
        account_id: security.resource.account_id().into(),
        currency: trade
            .settlement_currency
            .clone()
            .ok_or(JournalError::ResourceUnavailable)?,
    };
    Ok(vec![cash, security.resource.clone()])
}

fn build_trade_preview(
    conn: &mut SqliteConnection,
    trade: TradeImpactPreview,
    funding: AllocationPreview,
    shares: &[TradePurposeShare],
) -> JournalResult<ManualTradeOwnershipPreview> {
    let observations: Vec<_> = funding
        .resources
        .iter()
        .map(|r| r.observation.clone())
        .collect();
    let balances = allocation_rows(conn)?;
    let ownership = preview_trade_ownership(&trade, shares, &balances, &observations)?;
    let ids: BTreeSet<_> = shares.iter().map(|s| s.purpose_id.as_str()).collect();
    let purposes: Vec<_> = purpose_rows(conn)?
        .into_iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| ids.contains(id)))
        .collect();
    if purposes.len() != ids.len() || purposes.iter().any(|p| p["isArchived"] == true) {
        return Err(invalid("trade purposes must exist and be active"));
    }
    let baselines = confirmed_observations(conn)?;
    let mut pending = crate::investment_journal::sources::reference_reconciliation(
        conn,
        &funding.source_references,
    )?;
    for row in &funding.resources {
        if let Some(baseline) = baselines.get(&row.observation.resource) {
            pending.extend(compare_fund_observation(
                baseline,
                &row.observation,
                row.balance.allocated,
            )?);
        }
    }
    let preview_hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,
        "trade":trade.facts_hash,"funding":funding.preview_hash,"ownership":ownership,"purposes":purposes,"pending":pending}),
    ));
    Ok(InvestmentTradeOwnershipPreview {
        schema_version: 1,
        trade,
        funding,
        ownership,
        purposes,
        pending_reconciliation: pending,
        preview_hash,
    })
}

fn manual_observations_after(
    before: &[ResourceObservation],
    snapshot: &AccountStateSnapshot,
    version: &str,
) -> Vec<ResourceObservation> {
    before
        .iter()
        .map(|old| {
            let mut next = old.clone();
            let amount = match &old.resource {
                FundResource::Cash { currency, .. } => snapshot
                    .cash_balances
                    .get(currency)
                    .copied()
                    .unwrap_or_default(),
                FundResource::Security { asset_id, .. } => snapshot
                    .positions
                    .get(asset_id)
                    .map(|p| p.quantity)
                    .unwrap_or_default(),
            };
            next.actual = Some(ExactDecimal::new(amount));
            next.state_version = version.into();
            // The new observed snapshot includes today's carried events. Keep
            // their identities, with the same applicability the snapshot reader uses.
            next.sources
                .retain(|s| s.kind != InvestmentSourceKind::Snapshot);
            for source in &mut next.sources {
                source.applied = false;
            }
            next.sources.push(snapshot_source(
                &snapshot.id,
                snapshot.snapshot_date,
                &old.resource,
                next.actual,
            ));
            next.sources.sort_by(|a, b| a.id.cmp(&b.id));
            next
        })
        .collect()
}

fn carry_unresolved_sources(
    baselines: &BTreeMap<FundResource, ResourceObservation>,
    before: &[ResourceObservation],
    balances: &BTreeMap<(String, FundResource), ExactDecimal>,
    after: &mut [ResourceObservation],
) -> JournalResult<()> {
    for next in after {
        if let (Some(baseline), Some(current)) = (
            baselines.get(&next.resource),
            before.iter().find(|o| o.resource == next.resource),
        ) {
            let allocated = balances
                .iter()
                .filter(|((_, r), _)| *r == next.resource)
                .try_fold(ExactDecimal::ZERO, |sum, (_, v)| sum.checked_add(*v))?;
            next.unresolved_sources = compare_fund_observation(baseline, current, allocated)?
                .into_iter()
                .filter(|i| {
                    !matches!(
                        i.kind,
                        FundReconciliationKind::PurposeExcess
                            | FundReconciliationKind::ResourceUnavailable
                    )
                })
                .collect();
        }
    }
    Ok(())
}

fn insert_compound_operation(
    conn: &mut SqliteConnection,
    operation: &Value,
    caller: &JournalCaller,
) -> JournalResult<()> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    let first_key = operation["firstApplicationKey"].as_str();
    if let Some(key) = first_key {
        let count=diesel::sql_query("SELECT COUNT(*) AS count FROM fund_allocation_operations WHERE first_application_key=?1").bind::<Text,_>(key).get_result::<Count>(conn)?.count;
        if count != 0 {
            return Err(JournalError::AlreadyApplied);
        }
    }
    diesel::sql_query(INSERT_COMPOUND_OPERATION)
        .bind::<Text, _>(operation.to_string())
        .bind::<Text, _>(now())
        .bind::<Text, _>(caller.channel.as_str())
        .bind::<Text, _>(caller.actor_kind.as_str())
        .bind::<Text, _>(&caller.actor_id)
        .bind::<Nullable<Text>, _>(&caller.credential_id)
        .execute(conn)?;
    Ok(())
}

fn save_ownership_legs(
    conn: &mut SqliteConnection,
    operation: &str,
    before: &BTreeMap<(String, FundResource), ExactDecimal>,
    ownership: &TradeOwnershipPreview,
) -> JournalResult<()> {
    save_allocation_entries(
        conn,
        operation,
        before,
        ownership
            .top_up_entries
            .iter()
            .chain(&ownership.conversion_entries),
    )
}

fn save_allocation_entries<'a>(
    conn: &mut SqliteConnection,
    operation: &str,
    before: &BTreeMap<(String, FundResource), ExactDecimal>,
    entries: impl Iterator<Item = &'a AllocationDelta>,
) -> JournalResult<()> {
    let mut running = before.clone();
    for entry in entries {
        let key = (entry.purpose_id.clone(), entry.resource.clone());
        let next = running
            .get(&key)
            .copied()
            .unwrap_or_default()
            .checked_add(entry.delta)?;
        if next < ExactDecimal::ZERO {
            return Err(JournalError::NegativeAllocation);
        }
        save_entry(conn, operation, entry, next)?;
        running.insert(key, next);
    }
    Ok(())
}

const INSERT_COMPOUND_OPERATION: &str = r#"
INSERT INTO fund_allocation_operations(id,operation_kind,request_id,reason,source_kind,source_id,source_counterparty_id,source_version,first_application_key,correction_of,observation_json,source_confirmation_json,detail_json,created_at,created_channel,created_actor_kind,created_actor_id,created_credential_id)
VALUES(json_extract(?1,'$.id'),json_extract(?1,'$.kind'),json_extract(?1,'$.requestId'),json_extract(?1,'$.reason'),json_extract(?1,'$.sourceKind'),json_extract(?1,'$.sourceId'),json_extract(?1,'$.sourceCounterpartyId'),json_extract(?1,'$.sourceVersion'),json_extract(?1,'$.firstApplicationKey'),json_extract(?1,'$.correctionOf'),json_extract(?1,'$.observation'),json_extract(?1,'$.confirmation'),json_extract(?1,'$.detail'),?2,?3,?4,?5,?6)
"#;
