use super::cash_transfers::persist_activity_rows;
use super::*;
use crate::investment_journal::{
    preview_ledger_trade, sources::activity_source_version, ConfirmedSource, PreparedLedgerFacts,
    SourceEffect,
};
use wealthfolio_core::{activities::Activity, portfolio::coordinator::raw_activity};
use wealthfolio_storage_sqlite::activities::ActivityDB;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordLedgerTrade {
    pub account_id: String,
    pub asset_id: String,
    pub side: String,
    pub activity_date: String,
    pub quantity: ExactDecimal,
    pub unit_price: Option<ExactDecimal>,
    pub amount: Option<ExactDecimal>,
    pub fee: Option<ExactDecimal>,
    pub tax: Option<ExactDecimal>,
    pub currency: String,
    pub fx_rate: Option<ExactDecimal>,
    pub notes: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordLedgerTradeWithOwnership {
    pub trade: RecordLedgerTrade,
    pub shares: Vec<TradePurposeShare>,
    pub reason: String,
    pub expected_preview_hash: String,
}

impl InvestmentJournalService {
    pub async fn preview_ledger_trade_ownership(
        &self,
        input: &RecordLedgerTrade,
        shares: Vec<TradePurposeShare>,
        caller: &JournalCaller,
    ) -> JournalResult<LedgerTradeOwnershipPreview> {
        caller.require_read()?;
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?;
        let id = ledger_activity_id(&self.ledger_id, &input.request_key, caller)?;
        let prepared = bookkeeping.prepare_ledger_trade(input, id).await?;
        let row = ActivityDB::from(prepared.activity);
        let (day, _) = self.business_day()?;
        let facts = self
            .resources
            .prepare_ledger_facts(std::slice::from_ref(&row), day)
            .await?;
        let trade = ledger_impact(&facts, &row)?;
        let funding = self.preview(trade_resources(&trade)?, day).await?;
        let observations: Vec<_> = funding
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            self.resources.validate(conn, &observations)?;
            self.resources.validate_ledger_facts(conn, &facts)?;
            build_trade_preview(conn, trade, funding, &shares)
        })
    }

    pub async fn record_ledger_trade_with_ownership(
        &self,
        input: RecordLedgerTradeWithOwnership,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        self.record_ledger_trade_internal(input, None, None, caller)
            .await
    }

    pub(super) async fn record_ledger_trade_internal(
        &self,
        input: RecordLedgerTradeWithOwnership,
        request_override: Option<RequestRecord>,
        binding: Option<super::decision_trades::DecisionTradeBinding>,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        if !caller.fact_permissions.activities_write
            || !matches!(caller.actor_kind, JournalActorKind::GuiSession)
            || caller.channel != BusinessChannel::Gui
        {
            return Err(JournalError::PermissionDenied);
        }
        nonblank(&input.reason, "reason")?;
        let record = match request_override {
            Some(record) => record,
            None => self.request(
                "investment_ledger_trade_record",
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
        let id = ledger_activity_id(&self.ledger_id, &input.trade.request_key, caller)?;
        let prepared = bookkeeping.prepare_ledger_trade(&input.trade, id).await?;
        let row = ActivityDB::from(prepared.activity.clone());
        let (day, zone) = self.business_day()?;
        let facts = self
            .resources
            .prepare_ledger_facts(std::slice::from_ref(&row), day)
            .await?;
        let trade = ledger_impact(&facts, &row)?;
        let funding = self.preview(trade_resources(&trade)?, day).await?;
        let preview = {
            let observations: Vec<_> = funding
                .resources
                .iter()
                .map(|r| r.observation.clone())
                .collect();
            let mut conn = get_connection(self.store.pool())?;
            self.resources.validate(&mut conn, &observations)?;
            self.resources.validate_ledger_facts(&mut conn, &facts)?;
            build_trade_preview(&mut conn, trade, funding, &input.shares)?
        };
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        let reader = self.resources.clone();
        let ledger = self.ledger_id.clone();
        let caller = caller.clone();
        let (result,saved)=transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok((result,vec![])); }
            reader.validate_ledger_facts(conn,&facts)?; prepared.validate(conn)?;
            let observations:Vec<_>=preview.funding.resources.iter().map(|r| r.observation.clone()).collect();
            reader.validate(conn,&observations)?;
            let funding=build_preview(conn,observations,day)?;
            let trade=ledger_impact(&facts,&row)?;
            let current=build_trade_preview(conn,trade,funding,&input.shares)?;
            if current.preview_hash!=input.expected_preview_hash { return Err(JournalError::StateConflict); }
            let decision=super::decision_trades::validate_owned_binding(conn,&binding,&current)?;
            let link_id=decision.as_ref().map(|_|uuid::Uuid::new_v4().to_string());
            let balances=allocation_rows(conn)?; let baselines=confirmed_observations(conn)?;
            let saved=persist_activity_rows(conn,vec![row])?;
            let actual=saved.first().ok_or_else(|| invalid("ledger trade was not saved"))?;
            let request_id=uuid::Uuid::new_v4().to_string(); let operation_id=uuid::Uuid::new_v4().to_string();
            let source_version=activity_source_version(actual)?;
            let execution=execution_from_ledger(actual,&current.trade,&facts)?;
            save_execution(conn,&execution)?;
            let mut result=json!({"activityId":actual.id,"executionId":execution.id,"allocationOperationId":operation_id,"ownershipStatus":"APPLIED","trade":current.trade});
            if let Some(decision)=&decision {result["executionLinkId"]=json!(link_id);result["decisionRevisionId"]=decision["id"].clone();}
            let request=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            diesel::sql_query("UPDATE investment_projection_state SET processed_generation=generation,refresh_target_generation=NULL,resource_impact_json=?1 WHERE source_kind='ACTIVITY' AND source_id=?2")
                .bind::<Text,_>(json!({"schemaVersion":1,"changes":current.trade.changes,"issues":current.trade.issues}).to_string()).bind::<Text,_>(&actual.id).execute(conn)?;
            let version=reader.committed_state_version(conn,day)?;
            let before:Vec<_>=current.funding.resources.iter().map(|r| r.observation.clone()).collect();
            let mut after=ledger_observations_after(&before,actual,&current.trade,&version)?;
            carry_unresolved_sources(&baselines,&before,&balances,&mut after)?;
            let confirmation=json!({"schemaVersion":1,"resources":after.iter().map(|o| json!({"resource":o.resource,"sources":o.sources})).collect::<Vec<_>>()});
            let operation=json!({"id":operation_id,"kind":"TRADE","requestId":request_id,"reason":input.reason,"sourceKind":"ACTIVITY","sourceId":actual.id,"sourceVersion":source_version,
                "firstApplicationKey":format!("TRADE:ACTIVITY:{}",actual.id),"observation":{"schemaVersion":1,"resources":after,"before":before},"confirmation":confirmation,
                "detail":{"schemaVersion":1,"trade":current.trade,"ownership":current.ownership}});
            insert_compound_operation(conn,&operation,&caller)?;
            save_ownership_legs(conn,&operation_id,&balances,&current.ownership)?;
            if let (Some(decision),Some(link_id))=(&decision,&link_id) {super::super::links::insert_prepared_link(conn,link_id,&execution,decision,&caller,&zone)?;}
            audit_completed_request(conn,&record,&caller,&ledger,&execution.id,&request_id,&request.created_at,day,&zone)?;
            Ok((result,saved.into_iter().map(Activity::from).collect::<Vec<_>>()))
        }).await?;
        if result["replayed"] != true {
            bookkeeping.notify_compound_activities_committed(&saved);
        }
        Ok(result)
    }
}

pub(super) fn ledger_activity_id(
    ledger: &str,
    key: &str,
    caller: &JournalCaller,
) -> JournalResult<String> {
    nonblank(key, "requestKey")?;
    Ok(format!(
        "investment-trade-{}",
        crate::requests::hash_json(&json!([
            ledger,
            caller.credential_id.as_deref().unwrap_or(&caller.actor_id),
            key
        ]))
    ))
}

pub(super) fn ledger_impact(
    facts: &PreparedLedgerFacts,
    row: &ActivityDB,
) -> JournalResult<TradeImpactPreview> {
    let raw = raw_activity(&Activity::from(row.clone()));
    let mut preview = preview_ledger_trade(facts.raw.clone(), raw)?;
    let mut semantic = serde_json::to_value(row).map_err(json_error)?;
    if let Some(object) = semantic.as_object_mut() {
        for name in [
            "created_at",
            "updated_at",
            "createdAt",
            "updatedAt",
            "version",
        ] {
            object.remove(name);
        }
    }
    // The generated reservation time may refresh between preview and submit.
    // Native effects are recomputed; cost/lot changes never become eligibility.
    preview.facts_hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"facts":facts.state_version,"activity":semantic,"changes":preview.changes,"eligible":preview.automatic_allocation_eligible,"netCash":preview.net_cash,"settlementCurrency":preview.settlement_currency}),
    ));
    Ok(preview)
}

fn ledger_observations_after(
    before: &[ResourceObservation],
    row: &ActivityDB,
    trade: &TradeImpactPreview,
    version: &str,
) -> JournalResult<Vec<ResourceObservation>> {
    let raw = raw_activity(&Activity::from(row.clone()));
    let source_version = crate::requests::hash_json(&crate::requests::canonicalize(
        serde_json::to_value(raw).map_err(json_error)?,
    ));
    before
        .iter()
        .map(|old| {
            let mut next = old.clone();
            let delta = trade
                .changes
                .iter()
                .find(|c| c.resource == old.resource)
                .map(|c| c.delta)
                .unwrap_or_default();
            next.actual = old.actual.map(|b| b.checked_add(delta)).transpose()?;
            next.state_version = version.into();
            next.sources.push(ConfirmedSource {
                kind: InvestmentSourceKind::Activity,
                id: row.id.clone(),
                version: source_version.clone(),
                applied: true,
                effect: SourceEffect::Trade,
            });
            next.sources.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(next)
        })
        .collect()
}

pub(super) fn execution_from_ledger(
    row: &ActivityDB,
    preview: &TradeImpactPreview,
    facts: &PreparedLedgerFacts,
) -> JournalResult<InvestmentExecution> {
    let engine = wealthfolio_portfolio_engine::Engine::new(facts.raw.clone())
        .map_err(|e| invalid(e.to_string()))?;
    let gross = if preview.fee.is_some() && preview.tax.is_some() {
        engine
            .ledger()
            .events
            .iter()
            .find(|e| e.source.as_str() == row.id)
            .and_then(|e| e.cash.as_ref())
            .and_then(|c| c.gross)
            .map(|v| ExactDecimal::new(v.abs()))
    } else {
        None
    };
    let time_precision = crate::investment_journal::accounting::activity_time_precision(row);
    let raw = raw_activity(&Activity::from(row.clone()));
    Ok(InvestmentExecution {
        id: InvestmentExecution::stable_id(&InvestmentSourceKind::Activity, &row.id),
        source_kind: InvestmentSourceKind::Activity,
        source_id: row.id.clone(),
        source_version: activity_source_version(row)?,
        status: ExecutionStatus::Valid,
        account_id: row.account_id.clone(),
        asset_id: row.asset_id.clone(),
        side: row.activity_type.clone(),
        quantity: raw.quantity.map(ExactDecimal::new),
        gross_amount: gross,
        activity_currency: preview.activity_currency.clone(),
        net_cash: preview.net_cash,
        settlement_currency: preview.settlement_currency.clone(),
        fee: preview.fee,
        tax: preview.tax,
        cash_basis: "LEDGER_FINAL_CASH".into(),
        occurred_at: row.activity_date.clone(),
        time_precision,
        snapshot: json!({"schemaVersion":1,"source":row,"preview":preview}),
    })
}
