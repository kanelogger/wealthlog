use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use chrono::NaiveDate;
use diesel::prelude::*;
use diesel::sql_types::Text;
use serde_json::{json, Value};
use wealthfolio_core::portfolio::{coordinator::FactSources, snapshot::SnapshotServiceTrait};
use wealthfolio_core::utils::time_utils::{parse_user_timezone, user_today};
use wealthfolio_portfolio_engine::{
    self as engine,
    model::{AccountId, AssetId, Currency, TrackingMode},
};
use wealthfolio_storage_sqlite::db::get_connection;
use wealthfolio_storage_sqlite::{
    activities::ActivityDB,
    schema::{activities, business_requests},
};

use super::model::invalid;
use super::{
    ConfirmedSource, ExactDecimal, ExecutionStatus, FundResource, InvestmentExecution,
    InvestmentExecutionReader, InvestmentResourceReader, InvestmentSourceKind, JournalError,
    JournalResult, PreparedExecutionProjection, ProjectionSource, ResourceObservation,
    SourceEffect,
};
use crate::BusinessStore;
mod quality;

/// Transaction-mode resources are folded from current facts. Holdings resources
/// use the existing snapshot service's split/transfer carry rules. Valuations,
/// purpose balances and frontend DTOs never enter this reader.
pub struct AccountingResourceReader {
    store: BusinessStore,
    facts: FactSources,
    snapshots: Arc<dyn SnapshotServiceTrait>,
    base_currency: Arc<RwLock<String>>,
    timezone: Arc<RwLock<String>>,
}

#[async_trait::async_trait]
impl InvestmentExecutionReader for AccountingResourceReader {
    async fn prepare_execution(
        &self,
        source: &ProjectionSource,
    ) -> JournalResult<PreparedExecutionProjection> {
        let timezone = self
            .timezone
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let day = user_today(parse_user_timezone(&timezone)?);
        let mut before = None;
        let mut resource_impact = json!({"schemaVersion":1,"sourceKind":source.kind,"sourceId":source.id,"changes":[],"issues":[]});
        let execution = if source.deleted {
            None
        } else {
            match source.kind {
                InvestmentSourceKind::Activity => {
                    let row = {
                        let mut conn = get_connection(self.store.pool())?;
                        activities::table
                            .find(&source.id)
                            .first::<ActivityDB>(&mut conn)
                            .optional()?
                    };
                    match row {
                        Some(row) => {
                            let kind = wealthfolio_core::activities::effective_activity_type(
                                &row.activity_type,
                                row.activity_type_override.as_deref(),
                            );
                            if row.status == "POSTED" && matches!(kind, "BUY" | "SELL") {
                                let mut conn = get_connection(self.store.pool())?;
                                before = Some(self.version(&mut conn, day)?);
                            }
                            self.project_activity(row, day, &timezone, &mut resource_impact)?
                        }
                        None => return Err(JournalError::StateConflict),
                    }
                }
                InvestmentSourceKind::ManualTrade => {
                    let row = {
                        let mut conn = get_connection(self.store.pool())?;
                        business_requests::table
                            .find(&source.id)
                            .first::<crate::store::RequestRow>(&mut conn)
                            .optional()?
                    };
                    match row {
                        Some(row)
                            if row.state == "completed"
                                && matches!(
                                    row.operation.as_str(),
                                    "manual_trade_record" | "investment_manual_trade_record"
                                ) =>
                        {
                            match project_manual_request(&row) {
                                Ok(execution) => Some(execution),
                                Err(e) => {
                                    resource_impact["issues"] = json!([{"code":"MalformedManualTrade","message":e.to_string()}]);
                                    None
                                }
                            }
                        }
                        Some(_) => None,
                        None => return Err(JournalError::StateConflict),
                    }
                }
                InvestmentSourceKind::Snapshot => None,
            }
        };
        if let Some(version) = &before {
            let mut conn = get_connection(self.store.pool())?;
            if self.version(&mut conn, day)? != *version {
                return Err(JournalError::StateConflict);
            }
        }
        Ok(PreparedExecutionProjection {
            as_of_date: day,
            state_version: before,
            execution,
            resource_impact,
        })
    }

    fn validate_execution(
        &self,
        conn: &mut SqliteConnection,
        prepared: &PreparedExecutionProjection,
    ) -> JournalResult<()> {
        if let Some(version) = &prepared.state_version {
            if self.version(conn, prepared.as_of_date)? != *version {
                return Err(JournalError::StateConflict);
            }
        }
        Ok(())
    }
}

impl AccountingResourceReader {
    fn project_activity(
        &self,
        row: ActivityDB,
        day: NaiveDate,
        timezone: &str,
        impact: &mut Value,
    ) -> JournalResult<Option<InvestmentExecution>> {
        let side = wealthfolio_core::activities::effective_activity_type(
            &row.activity_type,
            row.activity_type_override.as_deref(),
        )
        .to_string();
        if !matches!(side.as_str(), "BUY" | "SELL") || row.status != "POSTED" {
            return Ok(None);
        }
        let snapshot = serde_json::to_value(&row).map_err(|e| invalid(e.to_string()))?;
        let version = super::sources::activity_source_version(&row)?;
        let mut execution = InvestmentExecution {
            id: InvestmentExecution::stable_id(&InvestmentSourceKind::Activity, &row.id),
            source_kind: InvestmentSourceKind::Activity,
            source_id: row.id.clone(),
            source_version: version,
            status: ExecutionStatus::Reconcile,
            account_id: row.account_id.clone(),
            asset_id: row.asset_id.clone(),
            side,
            quantity: None,
            gross_amount: None,
            activity_currency: row.currency.clone(),
            net_cash: None,
            settlement_currency: None,
            fee: None,
            tax: None,
            cash_basis: "UNAVAILABLE".into(),
            occurred_at: row.activity_date.clone(),
            time_precision: activity_time_precision(&row),
            snapshot: json!({"schemaVersion":1,"source":snapshot,"issues":[]}),
        };
        if [&row.activity_date, &row.created_at, &row.updated_at]
            .iter()
            .any(|v| !valid_stored_timestamp(v))
        {
            execution.snapshot["issues"] = json!(["MalformedActivityDate"]);
            impact["issues"] = execution.snapshot["issues"].clone();
            return Ok(Some(execution));
        }
        let exact = |value: &Option<String>| value.as_deref().map(ExactDecimal::parse).transpose();
        for value in [
            &row.quantity,
            &row.unit_price,
            &row.amount,
            &row.fee,
            &row.tax,
            &row.fx_rate,
        ] {
            if let Err(e) = exact(value) {
                execution.snapshot["issues"] =
                    json!([{"code":"MalformedDecimal","message":e.to_string()}]);
                impact["issues"] = execution.snapshot["issues"].clone();
                return Ok(Some(execution));
            }
        }
        execution.quantity = exact(&row.quantity)?.map(|q| ExactDecimal::new(q.value().abs()));
        execution.fee = exact(&row.fee)?.map(|v| ExactDecimal::new(v.value().abs()));
        execution.tax = exact(&row.tax)?.map(|v| ExactDecimal::new(v.value().abs()));
        let base = self
            .base_currency
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let loaded = self.facts.load_for_investment_journal(
            std::slice::from_ref(&row.account_id),
            &base,
            timezone,
            day,
        )?;
        if loaded
            .unsupported_accounts
            .iter()
            .any(|(id, _)| id == &row.account_id)
            || loaded
                .invalid_snapshot_dates
                .iter()
                .any(|(id, _)| id == &row.account_id)
        {
            execution.snapshot["issues"] = json!(["UnsupportedAccountingState"]);
            impact["issues"] = execution.snapshot["issues"].clone();
            return Ok(Some(execution));
        }
        let raw = loaded
            .raw
            .activities
            .iter()
            .find(|a| a.id == row.id)
            .cloned();
        let Some(raw) = raw else {
            execution.snapshot["issues"] = json!(["SourceNotInAccountingScope"]);
            impact["issues"] = execution.snapshot["issues"].clone();
            return Ok(Some(execution));
        };
        let prepared = super::preview::preview_ledger_trade_with_gross(loaded.raw, raw);
        match prepared {
            Ok((preview, gross)) => {
                execution.gross_amount = gross;
                execution.status = if preview.automatic_allocation_eligible {
                    ExecutionStatus::Valid
                } else {
                    ExecutionStatus::Reconcile
                };
                execution.net_cash = preview.net_cash;
                execution.settlement_currency = preview.settlement_currency.clone();
                execution.cash_basis = if preview.net_cash.is_some() {
                    "LEDGER_FINAL_CASH"
                } else {
                    "MISSING_FINAL_CASH"
                }
                .into();
                *impact =
                    json!({"schemaVersion":1,"changes":preview.changes,"issues":preview.issues});
                execution.snapshot["preview"] =
                    serde_json::to_value(preview).map_err(|e| invalid(e.to_string()))?;
            }
            Err(e) => {
                execution.snapshot["issues"] =
                    json!([{"code":"AccountingPreviewUnavailable","message":e.to_string()}]);
                impact["issues"] = execution.snapshot["issues"].clone();
            }
        }
        Ok(Some(execution))
    }
}

pub(crate) fn valid_stored_timestamp(value: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(value).is_ok()
        || NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
        || ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S"]
            .iter()
            .any(|format| chrono::NaiveDateTime::parse_from_str(value, format).is_ok())
}

pub(crate) fn activity_time_precision(row: &ActivityDB) -> String {
    let declared = row
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|m| {
            m.get("investmentJournal")?
                .get("timePrecision")?
                .as_str()
                .map(str::to_string)
        });
    if let Some(value) = declared.filter(|s| matches!(s.as_str(), "DATE" | "DATETIME")) {
        return value;
    }
    // Legacy writers often encoded a calendar day as midnight. Without an
    // explicit declaration, do not turn that default into proof of ordering.
    let midnight = chrono::DateTime::parse_from_rfc3339(&row.activity_date)
        .is_ok_and(|t| t.time() == chrono::NaiveTime::MIN)
        || chrono::NaiveDateTime::parse_from_str(&row.activity_date, "%Y-%m-%d %H:%M:%S%.f")
            .is_ok_and(|t| t.time() == chrono::NaiveTime::MIN);
    if midnight || NaiveDate::parse_from_str(&row.activity_date, "%Y-%m-%d").is_ok() {
        "DATE"
    } else {
        "DATETIME"
    }
    .into()
}

pub(crate) fn project_manual_request(
    row: &crate::store::RequestRow,
) -> JournalResult<InvestmentExecution> {
    let request: Value =
        serde_json::from_str(&row.request_snapshot).map_err(|e| invalid(e.to_string()))?;
    if crate::requests::hash_json(&crate::requests::canonicalize(request.clone()))
        != row.request_hash
    {
        return Err(invalid(
            "stored manual request does not match its frozen request hash",
        ));
    }
    let result: Value = serde_json::from_str(
        row.result_json
            .as_deref()
            .ok_or_else(|| invalid("completed trade has no result"))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let (input, result) = if row.operation == "investment_manual_trade_record" {
        (request["trade"].clone(), result["trade"].clone())
    } else {
        (request.clone(), result.clone())
    };
    let input: crate::investments::RecordManualTrade =
        serde_json::from_value(input).map_err(|e| invalid(e.to_string()))?;
    let amounts = crate::investments::ManualTradeAmounts::prepare(&input)
        .map_err(|e| invalid(e.to_string()))?;
    let net = amounts
        .cash_change(&input.side)
        .map_err(|e| invalid(e.to_string()))?;
    let reported_gross = result["grossAmount"]
        .as_str()
        .ok_or_else(|| invalid("completed trade has no prepared gross amount"))?;
    if ExactDecimal::parse(reported_gross)?.value() != amounts.gross {
        return Err(invalid(
            "completed manual trade result does not match its request",
        ));
    }
    let adjustment = result["cashAdjustment"]
        .as_str()
        .map(ExactDecimal::parse)
        .transpose()?;
    let mut issues = Vec::new();
    if amounts.fee.is_none() {
        issues.push("FeeUnknown");
    }
    if amounts.tax.is_none() {
        issues.push("TaxUnknown");
    }
    if adjustment != Some(ExactDecimal::ZERO) {
        issues.push("IndependentCashCalibration");
    }
    let version = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"request":request,"result":result,"requestHash":row.request_hash}),
    ));
    Ok(InvestmentExecution {
        id: InvestmentExecution::stable_id(&InvestmentSourceKind::ManualTrade, &row.id),
        source_kind: InvestmentSourceKind::ManualTrade,
        source_id: row.id.clone(),
        source_version: version,
        status: if issues.is_empty() {
            ExecutionStatus::Valid
        } else {
            ExecutionStatus::Reconcile
        },
        account_id: input.account_id,
        asset_id: Some(input.asset_id),
        side: input.side,
        quantity: Some(ExactDecimal::new(amounts.quantity)),
        gross_amount: Some(ExactDecimal::new(amounts.gross)),
        activity_currency: input.currency.clone(),
        net_cash: Some(ExactDecimal::new(net)),
        settlement_currency: Some(input.currency),
        fee: amounts.fee.map(ExactDecimal::new),
        tax: amounts.tax.map(ExactDecimal::new),
        cash_basis: "MANUAL_PREPARED_SNAPSHOT".into(),
        occurred_at: input.activity_date,
        time_precision: "DATE".into(),
        snapshot: json!({"schemaVersion":1,"request":request,"result":result,"issues":issues,"cashAdjustment":adjustment,"recordedAt":row.created_at}),
    })
}

impl AccountingResourceReader {
    pub fn new(
        store: BusinessStore,
        facts: FactSources,
        snapshots: Arc<dyn SnapshotServiceTrait>,
        base_currency: Arc<RwLock<String>>,
        timezone: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            store,
            facts,
            snapshots,
            base_currency,
            timezone,
        }
    }

    fn version(&self, conn: &mut SqliteConnection, day: NaiveDate) -> JournalResult<String> {
        let source = source_fingerprint(conn)?;
        Ok(crate::requests::hash_json(
            &json!({"schemaVersion":1,"source":source,"day":day,"baseCurrency":self.base_currency.read().unwrap_or_else(|e| e.into_inner()).clone(),"timezone":self.timezone.read().unwrap_or_else(|e| e.into_inner()).clone()}),
        ))
    }
}

#[async_trait::async_trait]
impl InvestmentResourceReader for AccountingResourceReader {
    async fn observe(
        &self,
        resources: &[FundResource],
        day: NaiveDate,
    ) -> JournalResult<Vec<ResourceObservation>> {
        for resource in resources {
            resource.validate()?;
        }
        let ids: Vec<_> = resources
            .iter()
            .map(|r| r.account_id().to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let before = {
            let mut conn = get_connection(self.store.pool())?;
            self.version(&mut conn, day)?
        };
        let base = self
            .base_currency
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let timezone = self
            .timezone
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let asset_ids: Vec<_> = resources
            .iter()
            .filter_map(|r| match r {
                FundResource::Security { asset_id, .. } => Some(asset_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let resource_assets = if asset_ids.is_empty() {
            vec![]
        } else {
            self.facts.assets.list_by_asset_ids(&asset_ids)?
        };
        let loaded = self
            .facts
            .load_for_investment_journal(&ids, &base, &timezone, day)?;
        let raw = loaded.raw.clone();
        let engine = engine::Engine::new(loaded.raw).map_err(|e| invalid(e.to_string()))?;
        let projected = engine.project().map_err(|e| invalid(e.to_string()))?;
        let rejected = projected.rejected_activities();
        let missing_cash: BTreeSet<_> = engine
            .ledger()
            .diagnostics
            .iter()
            .filter(|d| d.code == engine::DiagnosticCode::MissingFinalCash)
            .map(|d| d.source.as_str())
            .collect();
        let selected_splits: BTreeSet<_> = engine::group_splits_with_sources(
            raw.activities
                .iter()
                .filter_map(|a| engine::SplitRow::from_raw(a, &raw.policy.timezone)),
        )
        .into_iter()
        .map(|s| s.source_id)
        .collect();
        let mut events_by_source: BTreeMap<&str, Vec<&engine::model::EconomicEvent>> =
            BTreeMap::new();
        for event in &engine.ledger().events {
            events_by_source
                .entry(event.source.as_str())
                .or_default()
                .push(event);
        }
        let mut snapshots = std::collections::HashMap::new();
        let mut snapshot_failures = BTreeSet::new();
        for id in &ids {
            if engine
                .facts()
                .accounts()
                .get(&AccountId::new(id))
                .is_some_and(|a| a.tracking == TrackingMode::Holdings)
            {
                match self
                    .snapshots
                    .get_latest_snapshots_as_of(std::slice::from_ref(id), day)
                {
                    Ok(found) => snapshots.extend(found),
                    Err(_) => {
                        snapshot_failures.insert(id.clone());
                    }
                }
            }
        }
        let mut quality_conn = get_connection(self.store.pool())?;
        let unusable: BTreeSet<_> = engine
            .normalize_diagnostics()
            .iter()
            .chain(&engine.ledger().diagnostics)
            .chain(&projected.diagnostics)
            .filter(|d| {
                d.severity == engine::Severity::Error
                    || d.code == engine::DiagnosticCode::ValueOutOfRange
            })
            .map(|d| d.source.clone())
            .collect();
        let bad_rows = quality::bad_activities(&mut quality_conn, &raw, &unusable)?;
        let mut observations = Vec::new();
        for resource in resources {
            let id = AccountId::new(resource.account_id());
            let Some(account) = engine.facts().accounts().get(&id) else {
                observations.push(ResourceObservation {
                    resource: resource.clone(),
                    actual: None,
                    observed_at: wealthfolio_core::utils::clock::now().to_rfc3339(),
                    as_of_date: day,
                    tracking_mode: "UNKNOWN".into(),
                    state_version: before.clone(),
                    sources: vec![],
                    unresolved_sources: vec![],
                    issues: vec!["MissingAccount".into()],
                });
                continue;
            };
            let mut issues = Vec::new();
            let security_supported = match resource {
                FundResource::Cash { .. } => true,
                FundResource::Security { asset_id, .. } => resource_assets
                    .iter()
                    .any(|a| a.id == *asset_id && a.is_unit_equity()),
            };
            let supported = !account.archived
                && security_supported
                && !loaded
                    .unsupported_accounts
                    .iter()
                    .any(|(a, _)| a == resource.account_id())
                && !loaded
                    .invalid_snapshot_dates
                    .iter()
                    .any(|(a, _)| a == resource.account_id());
            let holding = account.tracking == TrackingMode::Holdings;
            let snapshot = snapshots.get(resource.account_id());
            let state = projected.final_state.accounts.get(&id);
            let mut actual = if !supported {
                issues.push(
                    if account.archived {
                        "ArchivedAccount"
                    } else if !security_supported {
                        "UnsupportedSecurityResource"
                    } else {
                        "UnsupportedAccountingState"
                    }
                    .into(),
                );
                None
            } else if holding {
                snapshot.map(|s| match resource {
                    FundResource::Cash { currency, .. } => {
                        s.cash_balances.get(currency).copied().unwrap_or_default()
                    }
                    FundResource::Security { asset_id, .. } => s
                        .positions
                        .get(asset_id)
                        .map(|p| p.quantity)
                        .unwrap_or_default(),
                })
            } else {
                // A supported transactions account with no events has known
                // zero resources; a missing holdings observation is unknown.
                Some(match resource {
                    FundResource::Cash { currency, .. } => state
                        .and_then(|s| {
                            s.cash.get(
                                &Currency::parse(currency)
                                    .unwrap_or_else(|| account.currency.clone()),
                            )
                        })
                        .copied()
                        .unwrap_or_default(),
                    FundResource::Security { asset_id, .. } => state
                        .and_then(|s| s.positions.get(&AssetId::new(asset_id)))
                        .map(|p| p.quantity)
                        .unwrap_or_default(),
                })
            };
            if let FundResource::Security { asset_id, .. } = resource {
                let multiplier = if holding {
                    snapshot
                        .and_then(|row| row.positions.get(asset_id))
                        .map(|position| position.contract_multiplier)
                } else {
                    state
                        .and_then(|row| row.positions.get(&AssetId::new(asset_id)))
                        .map(|position| position.contract_multiplier)
                };
                if multiplier.is_some_and(|multiplier| multiplier != rust_decimal::Decimal::ONE) {
                    actual = None;
                    issues.push("UnsupportedSecurityUnitBasis".into());
                }
            }
            if holding && snapshot.is_none() {
                issues.push("MissingObservedSnapshot".into());
            }
            if snapshot_failures.contains(resource.account_id()) {
                actual = None;
                issues.push("SnapshotCalculationUnavailable".into());
            }
            let mut snapshot_data_valid = true;
            if let Some(snapshot) = snapshot.filter(|_| holding) {
                let factor = match resource {
                    FundResource::Cash { .. } => Some(rust_decimal::Decimal::ONE),
                    FundResource::Security { asset_id, .. } => {
                        engine.surfaces().split_quantity_factor(
                            &AssetId::new(asset_id),
                            snapshot.snapshot_date,
                            day,
                        )
                    }
                };
                snapshot_data_valid =
                    quality::snapshot_valid(&mut quality_conn, &snapshot.id, resource, factor)?;
                if !snapshot_data_valid {
                    actual = None;
                    issues.push("MalformedSnapshotData".into());
                }
            }
            let malformed = quality::relevant_bad_rows(
                &bad_rows,
                resource,
                holding,
                snapshot.map(|s| s.snapshot_date),
                day,
                raw.policy.timezone.name(),
            )?;
            if !malformed.is_empty() {
                actual = None;
                issues.push("MalformedActivityData".into());
            }
            let mut sources = Vec::new();
            for activity in &raw.activities {
                let events = events_by_source
                    .get(activity.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let applicable = match resource {
                    FundResource::Cash { currency, .. } => {
                        activity.account_id == resource.account_id()
                            && (events.iter().any(|e| {
                                e.cash.as_ref().is_some_and(|cash| match cash.booking {
                                    engine::model::Booking::ActivityCurrency => {
                                        e.currency.as_str() == currency
                                    }
                                    engine::model::Booking::AccountCurrency { .. } => {
                                        account.currency.as_str() == currency
                                    }
                                })
                            }) || missing_cash.contains(activity.id.as_str()))
                    }
                    FundResource::Security { asset_id, .. } => {
                        events.iter().any(|e| match &e.action {
                            engine::model::Action::Split { asset, .. } => {
                                asset.as_str() == asset_id
                            }
                            engine::model::Action::Trade { asset, .. }
                            | engine::model::Action::SecurityTransfer { asset, .. }
                            | engine::model::Action::OptionExpiry { asset, .. } => {
                                e.account == id && asset.as_str() == asset_id
                            }
                            _ => false,
                        })
                    }
                };
                if !applicable {
                    continue;
                }
                let mut applied = supported
                    && events.iter().any(|e| e.date <= day)
                    && !rejected.contains(&engine::model::ActivityId::new(&activity.id));
                let effect = events
                    .iter()
                    .find_map(|e| match &e.action {
                        engine::model::Action::Split { .. } => Some(SourceEffect::CorporateAction),
                        engine::model::Action::Trade { .. } => Some(
                            if matches!(
                                wealthfolio_core::activities::effective_activity_type(
                                    &activity.activity_type,
                                    activity.activity_type_override.as_deref()
                                ),
                                "BUY" | "SELL"
                            ) {
                                SourceEffect::Trade
                            } else {
                                SourceEffect::CorporateAction
                            },
                        ),
                        engine::model::Action::SecurityTransfer { .. } => {
                            Some(SourceEffect::SecurityTransfer)
                        }
                        engine::model::Action::ReturnOfCapital { .. } => {
                            Some(SourceEffect::CorporateAction)
                        }
                        _ if e.kind == engine::model::ActivityKind::Dividend => {
                            Some(SourceEffect::CorporateAction)
                        }
                        _ if matches!(
                            e.kind,
                            engine::model::ActivityKind::TransferIn
                                | engine::model::ActivityKind::TransferOut
                        ) =>
                        {
                            Some(SourceEffect::CashTransfer)
                        }
                        _ => None,
                    })
                    .unwrap_or(SourceEffect::Ordinary);
                if matches!(resource, FundResource::Cash { .. })
                    && events.iter().all(|e| e.cash.is_none())
                {
                    applied = false;
                }
                if holding {
                    // The existing reader applies only grouped splits and posted cash
                    // transfers after the observation date, never ordinary BUY/SELL.
                    applied = applied
                        && snapshot.is_some_and(|s| {
                            events
                                .iter()
                                .any(|e| e.date > s.snapshot_date && e.date <= day)
                        })
                        && matches!(
                            effect,
                            SourceEffect::CorporateAction | SourceEffect::CashTransfer
                        );
                    if effect == SourceEffect::CorporateAction {
                        applied = applied && selected_splits.contains(&activity.id);
                    }
                }
                let version = crate::requests::hash_json(&crate::requests::canonicalize(
                    serde_json::to_value(activity).map_err(|e| invalid(e.to_string()))?,
                ));
                sources.push(ConfirmedSource {
                    kind: InvestmentSourceKind::Activity,
                    id: activity.id.clone(),
                    version,
                    applied,
                    effect,
                });
            }
            for row in malformed {
                sources.retain(|s| s.kind != InvestmentSourceKind::Activity || s.id != row.id);
                sources.push(ConfirmedSource {
                    kind: InvestmentSourceKind::Activity,
                    id: row.id.clone(),
                    version: super::sources::activity_source_version(row)?,
                    applied: false,
                    effect: SourceEffect::Unknown,
                });
            }
            if holding {
                if let Some(snapshot) = snapshot {
                    // Refresh/calculation time is excluded from this version.
                    let raw_snapshot = raw.observed_snapshots.iter().find(|s| {
                        s.account_id == resource.account_id() && s.date == snapshot.snapshot_date
                    });
                    let raw_value =
                        raw_snapshot
                            .filter(|_| snapshot_data_valid)
                            .map(|s| match resource {
                                FundResource::Cash { currency, .. } => s
                                    .cash
                                    .iter()
                                    .find(|(c, _)| c == currency)
                                    .map(|(_, v)| ExactDecimal::new(*v))
                                    .unwrap_or_default(),
                                FundResource::Security { asset_id, .. } => s
                                    .positions
                                    .iter()
                                    .find(|p| p.asset_id == *asset_id)
                                    .map(|p| ExactDecimal::new(p.quantity))
                                    .unwrap_or_default(),
                            });
                    sources.push(snapshot_source(
                        &snapshot.id,
                        snapshot.snapshot_date,
                        resource,
                        raw_value,
                    ));
                }
            }
            let source_ids: BTreeSet<_> = sources.iter().map(|s| s.id.as_str()).collect();
            for diagnostic in engine
                .normalize_diagnostics()
                .iter()
                .chain(&engine.ledger().diagnostics)
                .chain(&projected.diagnostics)
            {
                if source_ids.contains(diagnostic.source.as_str())
                    && !matches!(
                        diagnostic.code,
                        engine::DiagnosticCode::FxUnavailable
                            | engine::DiagnosticCode::CarriedFxRate
                            | engine::DiagnosticCode::ConflictingFxRates
                            | engine::DiagnosticCode::MissingQuote
                            | engine::DiagnosticCode::CarriedQuote
                    )
                {
                    issues.push(format!("{:?}", diagnostic.code));
                }
            }
            if !holding
                && matches!(resource, FundResource::Cash { .. })
                && issues.iter().any(|i| i == "MissingFinalCash")
            {
                actual = None;
            }
            issues.sort();
            issues.dedup();
            sources.sort_by(|a, b| a.id.cmp(&b.id));
            observations.push(ResourceObservation {
                resource: resource.clone(),
                actual: actual.map(ExactDecimal::new),
                observed_at: wealthfolio_core::utils::clock::now().to_rfc3339(),
                as_of_date: day,
                tracking_mode: if holding { "HOLDINGS" } else { "TRANSACTIONS" }.into(),
                state_version: before.clone(),
                sources,
                unresolved_sources: vec![],
                issues,
            });
        }
        let mut conn = get_connection(self.store.pool())?;
        if self.version(&mut conn, day)? != before {
            return Err(JournalError::StateConflict);
        }
        Ok(observations)
    }

    fn validate(
        &self,
        conn: &mut SqliteConnection,
        observations: &[ResourceObservation],
    ) -> JournalResult<()> {
        if let Some(first) = observations.first() {
            let version = self.version(conn, first.as_of_date)?;
            if observations
                .iter()
                .any(|o| o.as_of_date != first.as_of_date || o.state_version != version)
            {
                return Err(JournalError::StateConflict);
            }
        }
        Ok(())
    }

    fn committed_state_version(
        &self,
        conn: &mut SqliteConnection,
        day: NaiveDate,
    ) -> JournalResult<String> {
        self.version(conn, day)
    }

    async fn prepare_ledger_facts(
        &self,
        proposed: &[ActivityDB],
        day: NaiveDate,
    ) -> JournalResult<super::PreparedLedgerFacts> {
        let before = {
            let mut conn = get_connection(self.store.pool())?;
            self.version(&mut conn, day)?
        };
        let base = self
            .base_currency
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let timezone = self
            .timezone
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let proposed: Vec<_> = proposed
            .iter()
            .cloned()
            .map(wealthfolio_core::activities::Activity::from)
            .collect();
        let loaded = self
            .facts
            .load_for_investment_preview(&proposed, &base, &timezone, day)?;
        if !loaded.unsupported_accounts.is_empty() || !loaded.invalid_snapshot_dates.is_empty() {
            return Err(JournalError::ResourceUnavailable);
        }
        let mut conn = get_connection(self.store.pool())?;
        if self.version(&mut conn, day)? != before {
            return Err(JournalError::StateConflict);
        }
        Ok(super::PreparedLedgerFacts {
            raw: loaded.raw,
            state_version: before,
            as_of_date: day,
        })
    }

    fn validate_ledger_facts(
        &self,
        conn: &mut SqliteConnection,
        prepared: &super::PreparedLedgerFacts,
    ) -> JournalResult<()> {
        if self.version(conn, prepared.as_of_date)? != prepared.state_version {
            Err(JournalError::StateConflict)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn snapshot_source(
    id: &str,
    date: NaiveDate,
    resource: &FundResource,
    value: Option<ExactDecimal>,
) -> ConfirmedSource {
    let version = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"date":date,"resource":resource,"observedValue":value}),
    ));
    ConfirmedSource {
        kind: InvestmentSourceKind::Snapshot,
        id: id.into(),
        version,
        applied: value.is_some(),
        effect: SourceEffect::Calibration,
    }
}

#[derive(QueryableByName)]
struct JsonRow {
    #[diesel(sql_type=Text)]
    body: String,
}

fn source_fingerprint(conn: &mut SqliteConnection) -> JournalResult<String> {
    // Journal generations exclude derived valuation refreshes. Hash the other
    // original fact owners too so mode, asset metadata and raw position edits
    // cannot slip through between an observation and its transaction.
    let queries=[
        "SELECT json_object('kind',source_kind,'id',source_id,'generation',generation) AS body FROM investment_projection_state ORDER BY source_kind,source_id",
        "SELECT json_object('id',id,'currency',currency,'type',account_type,'mode',tracking_mode,'active',is_active,'archived',is_archived,'meta',meta) AS body FROM accounts ORDER BY id",
        "SELECT json_object('id',id,'kind',kind,'currency',quote_ccy,'instrumentType',instrument_type,'metadata',metadata) AS body FROM assets ORDER BY id",
        "SELECT json_object('snapshot',p.snapshot_id,'asset',p.asset_id,'quantity',p.quantity,'currency',p.currency,'multiplier',p.contract_multiplier) AS body FROM snapshot_positions p JOIN holdings_snapshots s ON s.id=p.snapshot_id WHERE s.source<>'CALCULATED' ORDER BY p.snapshot_id,p.asset_id",
    ];
    let mut facts = Vec::new();
    for query in queries {
        let rows = diesel::sql_query(query).load::<JsonRow>(conn)?;
        let values: Vec<Value> = rows
            .into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(|e| invalid(e.to_string())))
            .collect::<JournalResult<_>>()?;
        facts.push(values);
    }
    Ok(crate::requests::hash_json(&json!(facts)))
}
