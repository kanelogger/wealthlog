//! Record facts and evidence against one exact current confirmed revision.
use super::super::links::{current_decision_binding, insert_prepared_link};
use super::*;
use crate::investment_journal::DecisionExecutionTarget;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum DecisionTradeOwnership {
    Apply { shares: Vec<TradePurposeShare> },
    ReconcileLater,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordDecisionManualTrade {
    pub decision: DecisionExecutionTarget,
    pub trade: RecordManualTrade,
    pub ownership: DecisionTradeOwnership,
    pub reason: String,
    pub expected_preview_hash: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTradePreview {
    pub schema_version: u32,
    pub decision: Value,
    pub ownership_mode: String,
    pub trade: TradeImpactPreview,
    pub funding: Option<AllocationPreview>,
    pub ownership: Option<TradeOwnershipPreview>,
    pub pending_reconciliation: Vec<FundReconciliationItem>,
    pub preview_hash: String,
    #[serde(skip)]
    pub(super) applied: Option<InvestmentTradeOwnershipPreview>,
}
#[derive(Clone)]
pub(super) struct DecisionTradeBinding {
    pub target: DecisionExecutionTarget,
    pub expected_preview_hash: String,
}

impl InvestmentJournalService {
    pub async fn preview_decision_manual_trade(
        &self,
        input: &RecordDecisionManualTrade,
        caller: &JournalCaller,
    ) -> JournalResult<DecisionTradePreview> {
        caller.require_read()?;
        match &input.ownership {
            DecisionTradeOwnership::Apply { shares } => {
                let preview = self
                    .preview_manual_trade_ownership(&input.trade, shares.clone(), caller)
                    .await?;
                let observations: Vec<_> = preview
                    .funding
                    .resources
                    .iter()
                    .map(|r| r.observation.clone())
                    .collect();
                let mut conn = get_connection(self.store.pool())?;
                conn.transaction::<_, JournalError, _>(|conn| {
                    self.resources.validate(conn, &observations)?;
                    if build_preview(conn, observations, preview.funding.as_of_date)?.preview_hash
                        != preview.funding.preview_hash
                    {
                        return Err(JournalError::StateConflict);
                    }
                    bound_owned_preview(conn, &input.decision, preview)
                })
            }
            DecisionTradeOwnership::ReconcileLater => {
                let bookkeeping = self
                    .bookkeeping
                    .as_ref()
                    .ok_or_else(|| invalid("bookkeeping service is not configured"))?;
                let trade = bookkeeping.preview_manual_trade(&input.trade).await?;
                let mut conn = get_connection(self.store.pool())?;
                bound_fact_preview(&mut conn, &input.decision, trade)
            }
        }
    }
    pub async fn record_decision_manual_trade(
        &self,
        input: RecordDecisionManualTrade,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        if !caller.fact_permissions.holdings_write {
            return Err(JournalError::PermissionDenied);
        }
        nonblank(&input.reason, "reason")?;
        let record = self.request(
            "investment_manual_trade_record",
            &input.trade.request_key,
            &input,
            caller,
        )?;
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let preview = self.preview_decision_manual_trade(&input, caller).await?;
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        if let DecisionTradeOwnership::Apply { shares } = input.ownership {
            let inner = preview.applied.ok_or(JournalError::StateConflict)?;
            let binding = DecisionTradeBinding {
                target: input.decision,
                expected_preview_hash: input.expected_preview_hash,
            };
            return self
                .record_manual_trade_internal(
                    RecordManualTradeWithOwnership {
                        trade: input.trade,
                        shares,
                        reason: input.reason,
                        expected_preview_hash: inner.preview_hash,
                    },
                    Some(record),
                    Some(binding),
                    caller,
                )
                .await;
        }
        self.record_manual_fact_with_decision(input, preview, record, caller)
            .await
    }
    async fn record_manual_fact_with_decision(
        &self,
        input: RecordDecisionManualTrade,
        preview: DecisionTradePreview,
        record: RequestRecord,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?
            .clone();
        let prepared = bookkeeping.prepare_manual_trade(&input.trade).await?;
        let current_trade = bookkeeping.preview_manual_trade(&input.trade).await?;
        if current_trade.facts_hash != preview.trade.facts_hash {
            return Err(JournalError::StateConflict);
        }
        let (day, zone) = self.business_day()?;
        let ledger = self.ledger_id.clone();
        let caller = caller.clone();
        let notification = prepared.snapshot.clone();
        let result=transact(&self.store,move |conn|{
            if let Some(result)=replay(conn,&record)? {return Ok(result);}
            prepared.validate(conn)?;
            let current=bound_fact_preview(conn,&input.decision,current_trade)?;
            if current.preview_hash!=input.expected_preview_hash {return Err(JournalError::StateConflict);}
            let request_id=uuid::Uuid::new_v4().to_string();let link_id=uuid::Uuid::new_v4().to_string();
            let execution_id=InvestmentExecution::stable_id(&InvestmentSourceKind::ManualTrade,&request_id);
            SnapshotRepository::save_snapshot_tx(conn,prepared.snapshot.clone())?;
            let mut trade_result=prepared.result.clone();trade_result["requestId"]=json!(request_id);trade_result["replayed"]=json!(false);
            let resources=fact_reconciliation_resources(&current.trade,&input.trade.account_id,&input.trade.asset_id);
            let mut result=json!({"trade":trade_result,"executionId":execution_id,"executionLinkId":link_id,"decisionRevisionId":input.decision.revision_id,"allocationOperationId":null,"ownershipStatus":"RECONCILIATION_REQUIRED","reconciliationResources":resources});
            let row=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            let execution=project_manual_request(&row)?;
            if !matches!(execution.status,ExecutionStatus::Valid|ExecutionStatus::Reconcile){return Err(JournalError::StateConflict);}
            save_execution(conn,&execution)?;
            insert_prepared_link(conn,&link_id,&execution,&current.decision,&caller,&zone)?;
            diesel::sql_query("UPDATE investment_projection_state SET processed_generation=generation,refresh_target_generation=NULL,resource_impact_json=?1 WHERE source_kind='MANUAL_TRADE' AND source_id=?2")
                .bind::<Text,_>(json!({"schemaVersion":1,"changes":current.trade.changes,"issues":current.trade.issues}).to_string()).bind::<Text,_>(&request_id).execute(conn)?;
            audit_completed_request(conn,&record,&caller,&ledger,&execution.id,&request_id,&row.created_at,day,&zone)?;
            Ok(result)
        }).await?;
        if result["replayed"] != true {
            bookkeeping.notify_compound_snapshot_committed(&notification);
        }
        Ok(result)
    }
}

pub(super) fn bound_owned_preview(
    conn: &mut SqliteConnection,
    target: &DecisionExecutionTarget,
    inner: InvestmentTradeOwnershipPreview,
) -> JournalResult<DecisionTradePreview> {
    let decision = current_decision_binding(conn, target)?;
    let hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"decision":decision,"mode":"APPLY","trade":inner.trade.facts_hash,"ownership":inner.preview_hash}),
    ));
    Ok(DecisionTradePreview {
        schema_version: 1,
        decision,
        ownership_mode: "APPLY".into(),
        trade: inner.trade.clone(),
        funding: Some(inner.funding.clone()),
        ownership: Some(inner.ownership.clone()),
        pending_reconciliation: inner.pending_reconciliation.clone(),
        preview_hash: hash,
        applied: Some(inner),
    })
}
fn bound_fact_preview(
    conn: &mut SqliteConnection,
    target: &DecisionExecutionTarget,
    trade: TradeImpactPreview,
) -> JournalResult<DecisionTradePreview> {
    let decision = current_decision_binding(conn, target)?;
    let hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"decision":decision,"mode":"RECONCILE_LATER","trade":trade.facts_hash}),
    ));
    Ok(DecisionTradePreview {
        schema_version: 1,
        decision,
        ownership_mode: "RECONCILE_LATER".into(),
        trade,
        funding: None,
        ownership: None,
        pending_reconciliation: vec![],
        preview_hash: hash,
        applied: None,
    })
}
pub(super) fn validate_owned_binding(
    conn: &mut SqliteConnection,
    binding: &Option<DecisionTradeBinding>,
    inner: &InvestmentTradeOwnershipPreview,
) -> JournalResult<Option<Value>> {
    binding
        .as_ref()
        .map(|binding| {
            let preview = bound_owned_preview(conn, &binding.target, inner.clone())?;
            if preview.preview_hash != binding.expected_preview_hash {
                return Err(JournalError::StateConflict);
            }
            Ok(preview.decision)
        })
        .transpose()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordDecisionLedgerTrade {
    pub decision: DecisionExecutionTarget,
    pub trade: RecordLedgerTrade,
    pub ownership: DecisionTradeOwnership,
    pub reason: String,
    pub expected_preview_hash: String,
}
impl InvestmentJournalService {
    pub async fn preview_decision_ledger_trade(
        &self,
        input: &RecordDecisionLedgerTrade,
        caller: &JournalCaller,
    ) -> JournalResult<DecisionTradePreview> {
        caller.require_read()?;
        match &input.ownership {
            DecisionTradeOwnership::Apply { shares } => {
                let inner = self
                    .preview_ledger_trade_ownership(&input.trade, shares.clone(), caller)
                    .await?;
                let observations: Vec<_> = inner
                    .funding
                    .resources
                    .iter()
                    .map(|r| r.observation.clone())
                    .collect();
                let mut conn = get_connection(self.store.pool())?;
                conn.transaction::<_, JournalError, _>(|conn| {
                    self.resources.validate(conn, &observations)?;
                    if build_preview(conn, observations, inner.funding.as_of_date)?.preview_hash
                        != inner.funding.preview_hash
                    {
                        return Err(JournalError::StateConflict);
                    }
                    bound_owned_preview(conn, &input.decision, inner)
                })
            }
            DecisionTradeOwnership::ReconcileLater => {
                let (_, row, facts) = self.prepare_decision_ledger_fact(input, caller).await?;
                let trade = super::ledger_trades::ledger_impact(&facts, &row)?;
                let mut conn = get_connection(self.store.pool())?;
                self.resources.validate_ledger_facts(&mut conn, &facts)?;
                bound_fact_preview(&mut conn, &input.decision, trade)
            }
        }
    }
    pub async fn record_decision_ledger_trade(
        &self,
        input: RecordDecisionLedgerTrade,
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
        let record = self.request(
            "investment_ledger_trade_record",
            &input.trade.request_key,
            &input,
            caller,
        )?;
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let preview = self.preview_decision_ledger_trade(&input, caller).await?;
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        if let DecisionTradeOwnership::Apply { shares } = input.ownership {
            let inner = preview.applied.ok_or(JournalError::StateConflict)?;
            let binding = DecisionTradeBinding {
                target: input.decision,
                expected_preview_hash: input.expected_preview_hash,
            };
            return self
                .record_ledger_trade_internal(
                    RecordLedgerTradeWithOwnership {
                        trade: input.trade,
                        shares,
                        reason: input.reason,
                        expected_preview_hash: inner.preview_hash,
                    },
                    Some(record),
                    Some(binding),
                    caller,
                )
                .await;
        }
        self.record_ledger_fact_with_decision(input, record, caller)
            .await
    }
    async fn prepare_decision_ledger_fact(
        &self,
        input: &RecordDecisionLedgerTrade,
        caller: &JournalCaller,
    ) -> JournalResult<(
        crate::investments::PreparedLedgerTrade,
        wealthfolio_storage_sqlite::activities::ActivityDB,
        crate::investment_journal::PreparedLedgerFacts,
    )> {
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?;
        let id = super::ledger_trades::ledger_activity_id(
            &self.ledger_id,
            &input.trade.request_key,
            caller,
        )?;
        let prepared = bookkeeping
            .prepare_ledger_trade_fact(&input.trade, id)
            .await?;
        let row =
            wealthfolio_storage_sqlite::activities::ActivityDB::from(prepared.activity.clone());
        let (day, _) = self.business_day()?;
        let facts = self
            .resources
            .prepare_ledger_facts(std::slice::from_ref(&row), day)
            .await?;
        Ok((prepared, row, facts))
    }
    async fn record_ledger_fact_with_decision(
        &self,
        input: RecordDecisionLedgerTrade,
        record: RequestRecord,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        let (prepared, row, facts) = self.prepare_decision_ledger_fact(&input, caller).await?;
        let (day, zone) = self.business_day()?;
        let ledger = self.ledger_id.clone();
        let caller = caller.clone();
        let reader = self.resources.clone();
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?
            .clone();
        let (result,saved)=transact(&self.store,move |conn|{
            if let Some(result)=replay(conn,&record)? {return Ok((result,vec![]));}
            prepared.validate(conn)?;reader.validate_ledger_facts(conn,&facts)?;
            let trade=super::ledger_trades::ledger_impact(&facts,&row)?;
            let current=bound_fact_preview(conn,&input.decision,trade)?;
            if current.preview_hash!=input.expected_preview_hash {return Err(JournalError::StateConflict);}
            let saved=super::cash_transfers::persist_activity_rows(conn,vec![row])?;
            let row=saved.first().ok_or_else(||invalid("ledger trade was not saved"))?;
            let mut execution=super::ledger_trades::execution_from_ledger(row,&current.trade,&facts)?;
            if !current.trade.automatic_allocation_eligible {execution.status=ExecutionStatus::Reconcile;}
            save_execution(conn,&execution)?;
            let request_id=uuid::Uuid::new_v4().to_string();let link_id=uuid::Uuid::new_v4().to_string();
            let mut result=json!({"activityId":row.id,"executionId":execution.id,"executionLinkId":link_id,"decisionRevisionId":input.decision.revision_id,"trade":current.trade,"ownershipStatus":"RECONCILIATION_REQUIRED","allocationOperationId":null,"reconciliationResources":fact_reconciliation_resources(&current.trade,&input.trade.account_id,&input.trade.asset_id)});
            let request=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            insert_prepared_link(conn,&link_id,&execution,&current.decision,&caller,&zone)?;
            diesel::sql_query("UPDATE investment_projection_state SET processed_generation=generation,refresh_target_generation=NULL,resource_impact_json=?1 WHERE source_kind='ACTIVITY' AND source_id=?2")
                .bind::<Text,_>(json!({"schemaVersion":1,"changes":current.trade.changes,"issues":current.trade.issues}).to_string()).bind::<Text,_>(&row.id).execute(conn)?;
            audit_completed_request(conn,&record,&caller,&ledger,&execution.id,&request_id,&request.created_at,day,&zone)?;
            Ok((result,saved.into_iter().map(wealthfolio_core::activities::Activity::from).collect::<Vec<_>>()))
        }).await?;
        if result["replayed"] != true {
            bookkeeping.notify_compound_activities_committed(&saved);
        }
        Ok(result)
    }
}

fn fact_reconciliation_resources(
    trade: &TradeImpactPreview,
    account: &str,
    asset: &str,
) -> Vec<FundResource> {
    let mut resources: BTreeSet<_> = trade.changes.iter().map(|c| c.resource.clone()).collect();
    resources.insert(FundResource::Security {
        account_id: account.into(),
        asset_id: asset.into(),
    });
    if let Some(currency) = &trade.settlement_currency {
        resources.insert(FundResource::Cash {
            account_id: account.into(),
            currency: currency.clone(),
        });
    }
    resources.into_iter().collect()
}
