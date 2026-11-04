use super::*;
use crate::investment_journal::{
    apportion_transfer,
    sources::{cash_transfer_from_rows, load_cash_transfer, CashTransferSource},
    CashTransferSnapshot, ConfirmedSource, PurposeResourceBalance, SourceEffect, TransferShare,
};
use crate::investments::CreateCashTransfer;
use wealthfolio_core::{
    activities::Activity,
    portfolio::{coordinator::raw_activity, economic_events::ActivityEconomicsResolver},
};
use wealthfolio_storage_sqlite::{activities::ActivityDB, schema::activities};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferPurposeShare {
    pub purpose_id: String,
    pub source_amount: ExactDecimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCashTransferWithOwnership {
    pub transfer: CreateCashTransfer,
    pub shares: Vec<TransferPurposeShare>,
    pub reason: String,
    pub expected_preview_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyExistingTransferOwnership {
    pub activity_id: String,
    pub shares: Vec<TransferPurposeShare>,
    pub reason: String,
    pub expected_preview_hash: String,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashTransferOwnershipPreview {
    pub schema_version: u32,
    pub mode: String,
    pub transfer: CashTransferSnapshot,
    pub funding: AllocationPreview,
    pub shares: Vec<TransferShare>,
    pub entries: Vec<AllocationDelta>,
    pub balances_after: Vec<PurposeResourceBalance>,
    pub purposes: Vec<Value>,
    pub pending_reconciliation: Vec<FundReconciliationItem>,
    pub preview_hash: String,
}

impl InvestmentJournalService {
    pub async fn preview_new_cash_transfer_ownership(
        &self,
        input: &CreateCashTransfer,
        shares: Vec<TransferPurposeShare>,
        caller: &JournalCaller,
    ) -> JournalResult<CashTransferOwnershipPreview> {
        caller.require_read()?;
        nonblank(&input.request_key, "requestKey")?;
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?;
        let prepared = bookkeeping.prepare_cash_transfer(input).await?;
        if !prepared.cash_reconciliation_accounts.is_empty() {
            return Err(JournalError::ReconciliationRequired);
        }
        let rows = transfer_rows(
            prepared.activities,
            &self.ledger_id,
            &input.request_key,
            caller,
        );
        let source = cash_transfer_from_rows(
            rows.clone(),
            rows.first().map(|r| r.id.as_str()).unwrap_or_default(),
            false,
        )?;
        self.cash_transfer_preview(source, shares, true, caller)
            .await
    }

    pub async fn preview_existing_transfer_ownership(
        &self,
        activity_id: &str,
        shares: Vec<TransferPurposeShare>,
        caller: &JournalCaller,
    ) -> JournalResult<CashTransferOwnershipPreview> {
        caller.require_read()?;
        let bookkeeping = self
            .bookkeeping
            .as_ref()
            .ok_or_else(|| invalid("bookkeeping service is not configured"))?;
        if bookkeeping.get_existing_transfer(activity_id)?.is_none() {
            return Err(invalid("source is not a valid internal transfer pair"));
        }
        let source = {
            let mut conn = get_connection(self.store.pool())?;
            load_cash_transfer(&mut conn, activity_id)?
        };
        self.cash_transfer_preview(source, shares, false, caller)
            .await
    }

    async fn cash_transfer_preview(
        &self,
        source: CashTransferSource,
        shares: Vec<TransferPurposeShare>,
        new: bool,
        caller: &JournalCaller,
    ) -> JournalResult<CashTransferOwnershipPreview> {
        caller.require_read()?;
        let (day, _) = self.business_day()?;
        let funding = self
            .preview(
                vec![
                    source.view.source_resource.clone(),
                    source.view.destination_resource.clone(),
                ],
                day,
            )
            .await?;
        let observations: Vec<_> = funding
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            self.resources.validate(conn, &observations)?;
            if !new && load_cash_transfer(conn, &source.out.id)?.version != source.version {
                return Err(JournalError::StateConflict);
            }
            build_transfer_preview(conn, &source, funding, &shares, new)
        })
    }

    pub async fn create_cash_transfer_with_ownership(
        &self,
        input: CreateCashTransferWithOwnership,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        if !caller.fact_permissions.transfers_write {
            return Err(JournalError::PermissionDenied);
        }
        nonblank(&input.reason, "reason")?;
        let record = self.request(
            "investment_cash_transfer_create",
            &input.transfer.request_key,
            &input,
            caller,
        )?;
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
        let prepared = bookkeeping.prepare_cash_transfer(&input.transfer).await?;
        if !prepared.cash_reconciliation_accounts.is_empty() {
            return Err(JournalError::ReconciliationRequired);
        }
        let rows = transfer_rows(
            prepared.activities.clone(),
            &self.ledger_id,
            &input.transfer.request_key,
            caller,
        );
        let proposed = cash_transfer_from_rows(
            rows.clone(),
            rows.first().map(|r| r.id.as_str()).unwrap_or_default(),
            false,
        )?;
        let preview = self
            .cash_transfer_preview(proposed, input.shares.clone(), true, caller)
            .await?;
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        let reader = self.resources.clone();
        let ledger = self.ledger_id.clone();
        let caller = caller.clone();
        let (day, zone) = self.business_day()?;
        let (result,committed)=transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok((result,vec![])); }
            let observations:Vec<_>=preview.funding.resources.iter().map(|r| r.observation.clone()).collect();
            reader.validate(conn,&observations)?; prepared.validate(conn)?;
            let source=cash_transfer_from_rows(rows.clone(),&preview.transfer.out_activity_id,false)?;
            let funding=build_preview(conn,observations,day)?;
            let current=build_transfer_preview(conn,&source,funding,&input.shares,true)?;
            if current.preview_hash!=input.expected_preview_hash { return Err(JournalError::StateConflict); }
            let balances=allocation_rows(conn)?; let baselines=confirmed_observations(conn)?;
            let saved=persist_activity_rows(conn,rows)?;
            let actual=load_cash_transfer(conn,&source.out.id)?;
            let request_id=uuid::Uuid::new_v4().to_string(); let operation_id=uuid::Uuid::new_v4().to_string();
            let mut result=json!({"transfer":actual.view,"activityIds":saved.iter().map(|r| &r.id).collect::<Vec<_>>(),"allocationOperationId":operation_id,"ownershipStatus":"APPLIED"});
            let row=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            let version=reader.committed_state_version(conn,day)?;
            let before:Vec<_>=current.funding.resources.iter().map(|r| r.observation.clone()).collect();
            let mut after=transfer_observations_after(&before,&actual,true,&version)?;
            carry_unresolved_sources(&baselines,&before,&balances,&mut after)?;
            write_transfer_operation(conn,&actual,&current,&after,&before,&balances,&operation_id,&request_id,&input.reason,&caller)?;
            audit_completed_request(conn,&record,&caller,&ledger,&operation_id,&request_id,&row.created_at,day,&zone)?;
            Ok((result,saved.into_iter().map(Activity::from).collect::<Vec<_>>()))
        }).await?;
        if result["replayed"] != true {
            bookkeeping.notify_compound_activities_committed(&committed);
        }
        Ok(result)
    }

    pub async fn apply_existing_transfer_ownership(
        &self,
        input: ApplyExistingTransferOwnership,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        nonblank(&input.reason, "reason")?;
        let record = self.request(
            "investment_existing_transfer_apply",
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
            .preview_existing_transfer_ownership(&input.activity_id, input.shares.clone(), caller)
            .await?;
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        let reader = self.resources.clone();
        let ledger = self.ledger_id.clone();
        let caller = caller.clone();
        let (day, zone) = self.business_day()?;
        transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok(result); }
            let observations:Vec<_>=preview.funding.resources.iter().map(|r| r.observation.clone()).collect();
            reader.validate(conn,&observations)?;
            let source=load_cash_transfer(conn,&input.activity_id)?;
            let funding=build_preview(conn,observations,day)?;
            let current=build_transfer_preview(conn,&source,funding,&input.shares,false)?;
            if current.preview_hash!=input.expected_preview_hash { return Err(JournalError::StateConflict); }
            let balances=allocation_rows(conn)?; let baselines=confirmed_observations(conn)?;
            let request_id=uuid::Uuid::new_v4().to_string(); let operation_id=uuid::Uuid::new_v4().to_string();
            let mut result=json!({"transfer":source.view,"allocationOperationId":operation_id,"ownershipStatus":"APPLIED"});
            let row=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            let version=reader.committed_state_version(conn,day)?;
            let before:Vec<_>=current.funding.resources.iter().map(|r| r.observation.clone()).collect();
            let mut after=transfer_observations_after(&before,&source,false,&version)?;
            carry_unresolved_sources(&baselines,&before,&balances,&mut after)?;
            write_transfer_operation(conn,&source,&current,&after,&before,&balances,&operation_id,&request_id,&input.reason,&caller)?;
            audit_completed_request(conn,&record,&caller,&ledger,&operation_id,&request_id,&row.created_at,day,&zone)?;
            Ok(result)
        }).await
    }
}

fn transfer_rows(
    prepared: Vec<wealthfolio_core::activities::NewActivity>,
    ledger: &str,
    key: &str,
    caller: &JournalCaller,
) -> Vec<ActivityDB> {
    let identity = crate::requests::hash_json(&json!([
        ledger,
        caller.credential_id.as_deref().unwrap_or(&caller.actor_id),
        key,
        "cash-transfer"
    ]));
    prepared
        .into_iter()
        .enumerate()
        .map(|(i, mut activity)| {
            activity.id = Some(format!("investment-transfer-{identity}-{i}"));
            ActivityDB::from(activity)
        })
        .collect()
}

fn build_transfer_preview(
    conn: &mut SqliteConnection,
    source: &CashTransferSource,
    funding: AllocationPreview,
    shares: &[TransferPurposeShare],
    new: bool,
) -> JournalResult<CashTransferOwnershipPreview> {
    ensure_transfer_not_applied(conn, &source.out.id, &source.incoming.id)?;
    let weights: Vec<_> = shares
        .iter()
        .map(|s| (s.purpose_id.clone(), s.source_amount))
        .collect();
    let dest = match &source.view.destination_resource {
        FundResource::Cash { currency, .. } => currency,
        _ => return Err(invalid("cash resource required")),
    };
    let apportioned = apportion_transfer(
        source.view.source_amount,
        source.view.destination_amount,
        dest,
        source.view.destination_precision,
        &weights,
    )?;
    let mut entries = Vec::new();
    for share in &apportioned {
        entries.push(AllocationDelta {
            purpose_id: share.purpose_id.clone(),
            resource: source.view.source_resource.clone(),
            delta: ExactDecimal::new(-share.source_amount.value()),
        });
        if share.destination_amount != ExactDecimal::ZERO {
            entries.push(AllocationDelta {
                purpose_id: share.purpose_id.clone(),
                resource: source.view.destination_resource.clone(),
                delta: share.destination_amount,
            });
        }
    }
    let observations: Vec<_> = funding
        .resources
        .iter()
        .map(|r| r.observation.clone())
        .collect();
    let after = transfer_observations_after(&observations, source, new, "")?;
    let actual = after
        .iter()
        .map(|o| (o.resource.clone(), o.actual))
        .collect();
    let balances = allocation_rows(conn)?;
    let final_balances = apply_deltas(&balances, &entries, &actual)?;
    let ids: BTreeSet<_> = shares.iter().map(|s| s.purpose_id.as_str()).collect();
    let purposes: Vec<_> = purpose_rows(conn)?
        .into_iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| ids.contains(id)))
        .collect();
    if purposes.len() != ids.len() || purposes.iter().any(|p| p["isArchived"] == true) {
        return Err(invalid("transfer purposes must exist and be active"));
    }
    let changed: BTreeSet<_> = entries
        .iter()
        .map(|e| (e.purpose_id.clone(), e.resource.clone()))
        .collect();
    let balances_after: Vec<_> = final_balances
        .into_iter()
        .filter(|(k, _)| changed.contains(k))
        .map(|((purpose_id, resource), amount)| PurposeResourceBalance {
            purpose_id,
            resource,
            amount,
        })
        .collect();
    let baselines = confirmed_observations(conn)?;
    let mut pending = crate::investment_journal::sources::reference_reconciliation(
        conn,
        &funding.source_references,
    )?;
    for row in &funding.resources {
        if let Some(b) = baselines.get(&row.observation.resource) {
            pending.extend(compare_fund_observation(
                b,
                &row.observation,
                row.balance.allocated,
            )?);
        }
    }
    let mut transfer = source.view.clone();
    if new {
        transfer.current_source_version = None;
    }
    let mode = if new { "NEW" } else { "EXISTING" }.to_string();
    let preview_hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"mode":mode,"transfer":transfer,"funding":funding.preview_hash,"shares":apportioned,"purposes":purposes,"pending":pending}),
    ));
    Ok(CashTransferOwnershipPreview {
        schema_version: 1,
        mode,
        transfer,
        funding,
        shares: apportioned,
        entries,
        balances_after,
        purposes,
        pending_reconciliation: pending,
        preview_hash,
    })
}

fn transfer_observations_after(
    before: &[ResourceObservation],
    source: &CashTransferSource,
    new: bool,
    version: &str,
) -> JournalResult<Vec<ResourceObservation>> {
    before
        .iter()
        .map(|old| {
            let mut next = old.clone();
            if !version.is_empty() {
                next.state_version = version.into();
            }
            if new {
                let row = if old.resource == source.view.source_resource {
                    &source.out
                } else if old.resource == source.view.destination_resource {
                    &source.incoming
                } else {
                    return Err(invalid("transfer observation belongs to another resource"));
                };
                let activity = Activity::from(row.clone());
                let amount =
                    ActivityEconomicsResolver::resolve_cash(&activity, rust_decimal::Decimal::ONE)
                        .signed_cash_effect
                        .ok_or(JournalError::ResourceUnavailable)?;
                next.actual = old
                    .actual
                    .map(|b| b.checked_add(ExactDecimal::new(amount)))
                    .transpose()?;
                let raw = raw_activity(&activity);
                let version = crate::requests::hash_json(&crate::requests::canonicalize(
                    serde_json::to_value(raw).map_err(json_error)?,
                ));
                next.sources.push(ConfirmedSource {
                    kind: InvestmentSourceKind::Activity,
                    id: activity.id,
                    version,
                    applied: true,
                    effect: SourceEffect::CashTransfer,
                });
                next.sources.sort_by(|a, b| a.id.cmp(&b.id));
            }
            Ok(next)
        })
        .collect()
}

fn ensure_transfer_not_applied(
    conn: &mut SqliteConnection,
    out: &str,
    incoming: &str,
) -> JournalResult<()> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    let count=diesel::sql_query("SELECT COUNT(*) AS count FROM fund_allocation_operations WHERE source_kind='TRANSFER' AND first_application_key IS NOT NULL AND (source_id IN (?1,?2) OR source_counterparty_id IN (?1,?2))")
        .bind::<Text,_>(out).bind::<Text,_>(incoming).get_result::<Count>(conn)?.count;
    if count != 0 {
        Err(JournalError::AlreadyApplied)
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn write_transfer_operation(
    conn: &mut SqliteConnection,
    source: &CashTransferSource,
    preview: &CashTransferOwnershipPreview,
    after: &[ResourceObservation],
    before: &[ResourceObservation],
    balances: &BTreeMap<(String, FundResource), ExactDecimal>,
    operation_id: &str,
    request_id: &str,
    reason: &str,
    caller: &JournalCaller,
) -> JournalResult<()> {
    ensure_transfer_not_applied(conn, &source.out.id, &source.incoming.id)?;
    let confirmation = json!({"schemaVersion":1,"resources":after.iter().map(|o| json!({"resource":o.resource,"sources":o.sources})).collect::<Vec<_>>()});
    let operation = json!({"id":operation_id,"kind":"TRANSFER","requestId":request_id,"reason":reason,"sourceKind":"TRANSFER","sourceId":source.out.id,"sourceCounterpartyId":source.incoming.id,"sourceVersion":source.version,
        "firstApplicationKey":format!("TRANSFER:{}:{}",source.out.id,source.incoming.id),"correctionOf":null,
        "observation":{"schemaVersion":1,"resources":after,"before":before},"confirmation":confirmation,"detail":{"schemaVersion":1,"transfer":source.view,"shares":preview.shares}});
    insert_compound_operation(conn, &operation, caller)?;
    save_allocation_entries(conn, operation_id, balances, preview.entries.iter())
}

pub(super) fn persist_activity_rows(
    conn: &mut SqliteConnection,
    rows: Vec<ActivityDB>,
) -> JournalResult<Vec<ActivityDB>> {
    let mut saved = Vec::new();
    for row in rows {
        wealthfolio_storage_sqlite::activities::assert_final_cash_floor(&row)?;
        let inserted = diesel::insert_into(activities::table)
            .values(&row)
            .get_result::<ActivityDB>(conn)
            .map_err(|error| {
                let error: wealthfolio_core::Error =
                    wealthfolio_storage_sqlite::errors::StorageError::from(error).into();
                match crate::OrdinaryActivityError::from(error) {
                    crate::OrdinaryActivityError::DuplicateActivity { .. } => {
                        JournalError::DuplicateActivity
                    }
                    crate::OrdinaryActivityError::Storage(error) => JournalError::Storage(error),
                    other => invalid(other.to_string()),
                }
            })?;
        saved.push(inserted);
    }
    Ok(saved)
}
