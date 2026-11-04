//! Detect lossy legacy fallbacks before treating a native balance as known.
//! This validates source bytes; it never computes an alternative balance.
use super::*;
use wealthfolio_core::{
    activities::{effective_activity_type, Activity},
    portfolio::snapshot::Position,
    utils::time_utils::activity_date_in_tz,
};
use wealthfolio_storage_sqlite::{
    portfolio::snapshot::AccountStateSnapshotDB,
    schema::{holdings_snapshots, snapshot_positions},
};

fn decimal_valid(value: &str) -> bool {
    rust_decimal::Decimal::from_str_exact(value).is_ok()
        || (value.contains(['e', 'E']) && rust_decimal::Decimal::from_scientific(value).is_ok())
}

pub(super) fn bad_activities(
    conn: &mut SqliteConnection,
    raw: &engine::model::RawFacts,
    rejected: &BTreeSet<String>,
) -> JournalResult<Vec<ActivityDB>> {
    let accounts: Vec<_> = raw.accounts.iter().map(|a| a.id.as_str()).collect();
    let ids: BTreeSet<_> = raw.activities.iter().map(|a| a.id.as_str()).collect();
    let rows = activities::table
        .filter(activities::account_id.eq_any(accounts))
        .load::<ActivityDB>(conn)?;
    Ok(rows
        .into_iter()
        .filter(|r| r.status == "POSTED" && ids.contains(r.id.as_str()))
        .filter(|r| {
            let kind =
                effective_activity_type(&r.activity_type, r.activity_type_override.as_deref());
            rejected.contains(&r.id)
                || matches!(
                    engine::model::ActivityKind::parse(kind),
                    None | Some(engine::model::ActivityKind::Unknown)
                )
                || !valid_stored_timestamp(&r.activity_date)
                || [
                    &r.quantity,
                    &r.unit_price,
                    &r.amount,
                    &r.fee,
                    &r.tax,
                    &r.fx_rate,
                ]
                .into_iter()
                .flatten()
                .any(|v| !decimal_valid(v))
        })
        .collect())
}

pub(super) fn relevant_bad_rows<'a>(
    rows: &'a [ActivityDB],
    resource: &FundResource,
    holding: bool,
    snapshot_date: Option<NaiveDate>,
    day: NaiveDate,
    timezone: &str,
) -> JournalResult<Vec<&'a ActivityDB>> {
    let timezone = parse_user_timezone(timezone)?;
    Ok(rows
        .iter()
        .filter(|row| {
            let kind =
                effective_activity_type(&row.activity_type, row.activity_type_override.as_deref());
            let related = match resource {
                FundResource::Cash { .. } => row.account_id == resource.account_id(),
                FundResource::Security { asset_id, .. } => {
                    row.asset_id.as_deref() == Some(asset_id)
                        && (row.account_id == resource.account_id() || kind == "SPLIT")
                }
            };
            if !related || !holding {
                return related;
            }
            let carried = match resource {
                FundResource::Cash { .. } => matches!(kind, "TRANSFER_IN" | "TRANSFER_OUT"),
                FundResource::Security { .. } => kind == "SPLIT",
            };
            if !carried {
                return false;
            }
            let Some(observed) = snapshot_date else {
                return false;
            };
            if !valid_stored_timestamp(&row.activity_date) {
                return observed < day;
            }
            let date = activity_date_in_tz(Activity::from((**row).clone()).activity_date, timezone);
            observed < date && date <= day
        })
        .collect())
}

pub(super) fn snapshot_valid(
    conn: &mut SqliteConnection,
    id: &str,
    resource: &FundResource,
    factor: Option<rust_decimal::Decimal>,
) -> JournalResult<bool> {
    let row = holdings_snapshots::table
        .find(id)
        .first::<AccountStateSnapshotDB>(conn)?;
    match resource {
        FundResource::Cash { .. } => Ok(serde_json::from_str::<
            BTreeMap<String, rust_decimal::Decimal>,
        >(&row.cash_balances)
        .is_ok()),
        FundResource::Security { asset_id, .. } => {
            type Values = (
                String,
                String,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
            );
            let stored = snapshot_positions::table
                .filter(snapshot_positions::snapshot_id.eq(id))
                .select((
                    snapshot_positions::asset_id,
                    snapshot_positions::quantity,
                    snapshot_positions::average_cost,
                    snapshot_positions::total_cost_basis,
                    snapshot_positions::contract_multiplier,
                    snapshot_positions::cost_basis_account,
                    snapshot_positions::cost_basis_base,
                ))
                .load::<Values>(conn)?;
            let Some(factor) = factor.filter(|f| *f > rust_decimal::Decimal::ZERO) else {
                return Ok(false);
            };
            let carry_safe = |quantity: rust_decimal::Decimal, cost: rust_decimal::Decimal| {
                quantity.checked_mul(factor).is_some() && cost.checked_div(factor).is_some()
            };
            if stored.is_empty() {
                return Ok(
                    serde_json::from_str::<BTreeMap<String, Position>>(&row.positions).is_ok_and(
                        |positions| {
                            positions
                                .get(asset_id)
                                .is_none_or(|p| carry_safe(p.quantity, p.average_cost))
                        },
                    ),
                );
            }
            Ok(stored.iter().filter(|p| p.0 == *asset_id).all(|p| {
                [&p.1, &p.2, &p.3, &p.4]
                    .into_iter()
                    .all(|v| decimal_valid(v))
                    && [&p.5, &p.6].into_iter().flatten().all(|v| decimal_valid(v))
                    && rust_decimal::Decimal::from_str_exact(&p.1)
                        .or_else(|_| rust_decimal::Decimal::from_scientific(&p.1))
                        .ok()
                        .zip(
                            rust_decimal::Decimal::from_str_exact(&p.2)
                                .or_else(|_| rust_decimal::Decimal::from_scientific(&p.2))
                                .ok(),
                        )
                        .is_some_and(|(q, c)| carry_safe(q, c))
            }))
        }
    }
}
