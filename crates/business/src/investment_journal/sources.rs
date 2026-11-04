//! Original source identities and exact, reusable current-source fingerprints.

use diesel::prelude::*;
use diesel::sql_types::{Nullable, Text};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use wealthfolio_core::{
    activities::{Activity, TransferPairResolution},
    portfolio::economic_events::ActivityEconomicsResolver,
};
use wealthfolio_storage_sqlite::{
    activities::ActivityDB,
    schema::{activities, business_requests},
};

use super::{
    model::{invalid, validate_currency},
    ExactDecimal, FundReconciliationItem, FundReconciliationKind, FundResource, JournalError,
    JournalResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceReferenceState {
    Valid,
    Invalid,
    Deleted,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsedSourceReference {
    pub operation_id: String,
    pub resource: FundResource,
    pub source_kind: String,
    pub source_id: String,
    pub source_counterparty_id: Option<String>,
    pub origin_version: Option<String>,
    pub current_version: Option<String>,
    pub state: SourceReferenceState,
}

pub(crate) fn used_source_references(
    conn: &mut SqliteConnection,
    resources: &[FundResource],
) -> JournalResult<Vec<UsedSourceReference>> {
    #[derive(QueryableByName)]
    struct Operation {
        #[diesel(sql_type=Text)]
        id: String,
        #[diesel(sql_type=Text)]
        source_kind: String,
        #[diesel(sql_type=Text)]
        source_id: String,
        #[diesel(sql_type=Nullable<Text>)]
        source_counterparty_id: Option<String>,
        #[diesel(sql_type=Nullable<Text>)]
        source_version: Option<String>,
        #[diesel(sql_type=Text)]
        observation_json: String,
    }
    let selected: BTreeSet<_> = resources.iter().cloned().collect();
    let operations=diesel::sql_query("SELECT id,source_kind,source_id,source_counterparty_id,source_version,observation_json FROM fund_allocation_operations op WHERE source_kind IS NOT NULL AND source_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM fund_allocation_operations reversal WHERE reversal.operation_kind='REVERSE' AND reversal.correction_of=op.id) ORDER BY op.rowid").load::<Operation>(conn)?;
    let mut result = Vec::new();
    for operation in operations {
        let observed: Value = serde_json::from_str(&operation.observation_json)
            .map_err(|e| invalid(e.to_string()))?;
        let mut relevant = BTreeSet::new();
        for row in observed["resources"]
            .as_array()
            .ok_or_else(|| invalid("source operation has no resource observations"))?
        {
            let resource: FundResource = serde_json::from_value(row["resource"].clone())
                .map_err(|e| invalid(e.to_string()))?;
            if selected.contains(&resource) {
                relevant.insert(resource);
            }
        }
        if relevant.is_empty() {
            continue;
        }
        let (current_version, state) = current_reference(
            conn,
            &operation.source_kind,
            &operation.source_id,
            operation.source_counterparty_id.as_deref(),
        )?;
        for resource in relevant {
            result.push(UsedSourceReference {
                operation_id: operation.id.clone(),
                resource,
                source_kind: operation.source_kind.clone(),
                source_id: operation.source_id.clone(),
                source_counterparty_id: operation.source_counterparty_id.clone(),
                origin_version: operation.source_version.clone(),
                current_version: current_version.clone(),
                state,
            });
        }
    }
    result.sort_by(|a, b| {
        a.operation_id
            .cmp(&b.operation_id)
            .then(a.resource.cmp(&b.resource))
    });
    Ok(result)
}

pub(crate) fn reference_reconciliation(
    conn: &mut SqliteConnection,
    references: &[UsedSourceReference],
) -> JournalResult<Vec<FundReconciliationItem>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type=Text)]
        body: String,
    }
    let confirmations = diesel::sql_query(
        "SELECT source_confirmation_json AS body FROM fund_allocation_operations ORDER BY rowid",
    )
    .load::<Row>(conn)?;
    let mut acknowledged = BTreeMap::new();
    for row in confirmations {
        let body: Value = serde_json::from_str(&row.body).map_err(|e| invalid(e.to_string()))?;
        if let Some(references) = body["sourceReferences"].as_array() {
            for value in references {
                let reference: UsedSourceReference =
                    serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
                acknowledged.insert(
                    (reference.operation_id.clone(), reference.resource.clone()),
                    reference,
                );
            }
        }
    }
    let mut items = Vec::new();
    for reference in references {
        let key = (reference.operation_id.clone(), reference.resource.clone());
        let changed = if let Some(ack) = acknowledged.get(&key) {
            ack.current_version != reference.current_version || ack.state != reference.state
        } else {
            reference.origin_version != reference.current_version
                || reference.state != SourceReferenceState::Valid
        };
        if !changed {
            continue;
        }
        let kind = if reference.source_kind == "TRANSFER" {
            FundReconciliationKind::TransferChanged
        } else if matches!(
            reference.state,
            SourceReferenceState::Invalid | SourceReferenceState::Deleted
        ) {
            FundReconciliationKind::SourceInvalidated
        } else {
            FundReconciliationKind::SourceChanged
        };
        let id = crate::requests::hash_json(&crate::requests::canonicalize(
            json!({"resource":reference.resource,"operation":reference.operation_id,"version":reference.current_version,"state":reference.state}),
        ));
        items.push(FundReconciliationItem {
            id,
            kind,
            resource: reference.resource.clone(),
            operation_id: Some(reference.operation_id.clone()),
            source_id: Some(reference.source_id.clone()),
            current_source_version: reference.current_version.clone(),
            excess: None,
        });
    }
    Ok(items)
}

pub(crate) fn current_reference(
    conn: &mut SqliteConnection,
    kind: &str,
    id: &str,
    counterparty: Option<&str>,
) -> JournalResult<(Option<String>, SourceReferenceState)> {
    match kind {
        "ACTIVITY" => {
            let row = activities::table
                .find(id)
                .first::<ActivityDB>(conn)
                .optional()?;
            match row {
                None => Ok((None, SourceReferenceState::Deleted)),
                Some(row) => {
                    let kind = wealthfolio_core::activities::effective_activity_type(
                        &row.activity_type,
                        row.activity_type_override.as_deref(),
                    );
                    let state = if row.status == "POSTED"
                        && matches!(kind, "BUY" | "SELL")
                        && row.asset_id.is_some()
                    {
                        SourceReferenceState::Valid
                    } else {
                        SourceReferenceState::Invalid
                    };
                    Ok((Some(activity_source_version(&row)?), state))
                }
            }
        }
        "MANUAL_TRADE" => {
            let row = business_requests::table
                .find(id)
                .first::<crate::store::RequestRow>(conn)
                .optional()?;
            let Some(row) = row else {
                return Ok((None, SourceReferenceState::Deleted));
            };
            if row.state == "completed"
                && matches!(
                    row.operation.as_str(),
                    "manual_trade_record" | "investment_manual_trade_record"
                )
            {
                if let Ok(execution) = super::accounting::project_manual_request(&row) {
                    return Ok((Some(execution.source_version), SourceReferenceState::Valid));
                }
            }
            Ok((
                Some(crate::requests::hash_json(&crate::requests::canonicalize(
                    serde_json::to_value(row).map_err(|e| invalid(e.to_string()))?,
                ))),
                SourceReferenceState::Invalid,
            ))
        }
        "TRANSFER" => {
            let Some(other) = counterparty else {
                return Ok((None, SourceReferenceState::Unavailable));
            };
            let used = activities::table
                .filter(activities::id.eq_any([id, other]))
                .load::<ActivityDB>(conn)?;
            if used.is_empty() {
                return Ok((None, SourceReferenceState::Deleted));
            }
            let groups: Vec<_> = used
                .iter()
                .filter_map(|r| r.source_group_id.as_deref())
                .collect();
            let mut members = activities::table
                .filter(activities::source_group_id.eq_any(groups))
                .load::<ActivityDB>(conn)?;
            for row in &used {
                if !members.iter().any(|r| r.id == row.id) {
                    members.push(row.clone());
                }
            }
            let version = transfer_source_version(id, other, &members)?;
            let state = if used.len() != 2 {
                SourceReferenceState::Deleted
            } else if load_cash_transfer(conn, id)
                .is_ok_and(|p| p.out.id == id && p.incoming.id == other)
            {
                SourceReferenceState::Valid
            } else {
                SourceReferenceState::Invalid
            };
            Ok((Some(version), state))
        }
        _ => Ok((None, SourceReferenceState::Unavailable)),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashTransferSnapshot {
    pub out_activity_id: String,
    pub in_activity_id: String,
    pub source_group_id: String,
    pub source_resource: FundResource,
    pub destination_resource: FundResource,
    pub source_amount: ExactDecimal,
    pub destination_amount: ExactDecimal,
    pub destination_precision: u32,
    pub occurred_at: String,
    pub current_source_version: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CashTransferSource {
    pub out: ActivityDB,
    pub incoming: ActivityDB,
    pub view: CashTransferSnapshot,
    pub version: String,
}

pub(crate) fn load_cash_transfer(
    conn: &mut SqliteConnection,
    id: &str,
) -> JournalResult<CashTransferSource> {
    let source = activities::table
        .find(id)
        .first::<ActivityDB>(conn)
        .optional()?
        .ok_or_else(|| invalid("transfer source no longer exists"))?;
    let group = source
        .source_group_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("transfer is not paired"))?;
    let members = activities::table
        .filter(activities::source_group_id.eq(group))
        .order(activities::id.asc())
        .load::<ActivityDB>(conn)?;
    cash_transfer_from_rows(members, id, true)
}

pub(crate) fn cash_transfer_from_rows(
    mut rows: Vec<ActivityDB>,
    id: &str,
    recorded: bool,
) -> JournalResult<CashTransferSource> {
    if rows.len() != 2 {
        return Err(invalid("transfer must resolve to exactly two linked legs"));
    }
    if rows
        .iter()
        .any(|r| !super::accounting::valid_stored_timestamp(&r.activity_date))
    {
        return Err(JournalError::ReconciliationRequired);
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    let domain: Vec<Activity> = rows.iter().cloned().map(Activity::from).collect();
    let resolution = TransferPairResolution::from_activities(&domain);
    let pair = resolution
        .pair_for_activity(id)
        .filter(|p| p.is_internal())
        .ok_or_else(|| invalid("source is not a valid internal transfer pair"))?;
    if !pair.transfer_out.is_posted() || !pair.transfer_in.is_posted() {
        return Err(invalid("transfer migration requires posted legs"));
    }
    if ActivityEconomicsResolver::is_security_transfer(&pair.transfer_out)
        || ActivityEconomicsResolver::is_security_transfer(&pair.transfer_in)
    {
        return Err(JournalError::ReconciliationRequired);
    }
    let out = rows
        .iter()
        .find(|r| r.id == pair.transfer_out.id)
        .cloned()
        .ok_or_else(|| invalid("source transfer leg is missing"))?;
    let incoming = rows
        .iter()
        .find(|r| r.id == pair.transfer_in.id)
        .cloned()
        .ok_or_else(|| invalid("destination transfer leg is missing"))?;
    validate_currency(&out.currency)?;
    validate_currency(&incoming.currency)?;
    let amount = |row: &ActivityDB| -> JournalResult<ExactDecimal> {
        let value = ExactDecimal::new(
            ExactDecimal::parse(
                row.amount
                    .as_deref()
                    .ok_or(JournalError::ResourceUnavailable)?,
            )?
            .value()
            .abs(),
        );
        if value == ExactDecimal::ZERO {
            return Err(invalid("cash transfer amounts must be positive"));
        }
        Ok(value)
    };
    let source_amount = amount(&out)?;
    let destination_amount = amount(&incoming)?;
    let precision = Decimal::from_str_exact(incoming.amount.as_deref().unwrap_or_default())
        .map_err(|_| invalid("destination transfer precision is invalid"))?
        .scale();
    let version = transfer_source_version(&out.id, &incoming.id, &rows)?;
    let view = CashTransferSnapshot {
        out_activity_id: out.id.clone(),
        in_activity_id: incoming.id.clone(),
        source_group_id: pair.group_id.clone(),
        source_resource: FundResource::Cash {
            account_id: out.account_id.clone(),
            currency: out.currency.clone(),
        },
        destination_resource: FundResource::Cash {
            account_id: incoming.account_id.clone(),
            currency: incoming.currency.clone(),
        },
        source_amount,
        destination_amount,
        destination_precision: precision,
        occurred_at: out.activity_date.clone(),
        current_source_version: recorded.then(|| version.clone()),
    };
    Ok(CashTransferSource {
        out,
        incoming,
        view,
        version,
    })
}

pub(crate) fn transfer_source_version(
    out: &str,
    incoming: &str,
    members: &[ActivityDB],
) -> JournalResult<String> {
    let mut members = members.to_vec();
    members.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"out":out,"in":incoming,"members":members}),
    )))
}

pub(crate) fn activity_source_version(row: &ActivityDB) -> JournalResult<String> {
    Ok(crate::requests::hash_json(&crate::requests::canonicalize(
        serde_json::to_value(row).map_err(|e| invalid(e.to_string()))?,
    )))
}
