use super::super::{model::invalid, AllocationDelta, ExactDecimal, JournalError, JournalResult};
use super::{
    accounting::{issue, quality, AccountingContextReader},
    InvestmentContextBlock, InvestmentContextIssue, InvestmentContextQuery,
};
use chrono::{DateTime, Duration, NaiveDate};
use diesel::{prelude::*, sql_types::Text};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use wealthfolio_storage_sqlite::{
    activities::ActivityDB,
    db::get_connection,
    schema::{accounts, activities},
};

impl AccountingContextReader {
    pub(super) fn recent_cash_flow(
        &self,
        query: &InvestmentContextQuery,
        day: NaiveDate,
        currency: &str,
        timezone: &str,
        fx_ready: bool,
    ) -> JournalResult<InvestmentContextBlock> {
        let from = day
            .checked_sub_signed(Duration::days(29))
            .ok_or_else(|| invalid("business day out of range"))?;
        let (ordinary, transfers, purpose_flows, mut issues, latest_other) = {
            let mut conn = get_connection(self.store.pool())?;
            conn.transaction::<_, JournalError, _>(|conn| {
                let ordinary = crate::service::ordinary_aggregation::aggregate_ordinary(
                    conn, from, day, timezone,
                )?;
                let mut issues: Vec<_> = ordinary
                    .issues
                    .iter()
                    .map(|p| {
                        issue(
                            &p.code,
                            Some(p.activity_id.clone()),
                            Some(p.currency.clone()),
                        )
                    })
                    .collect();
                let (transfers, transfer_date) =
                    internal_transfers(conn, from, day, timezone, &mut issues)?;
                let (purpose_flows, purpose_date) =
                    purpose_flows(conn, &query.purpose_ids, from, day, timezone)?;
                Ok((
                    ordinary,
                    transfers,
                    purpose_flows,
                    issues,
                    transfer_date.into_iter().chain(purpose_date).max(),
                ))
            })?
        };
        let mut income = Some(ExactDecimal::ZERO);
        let mut expense = Some(ExactDecimal::ZERO);
        let mut fx_sources = BTreeSet::new();
        for entry in &ordinary.daily {
            let source = &entry.amounts.currency;
            let incoming = entry
                .amounts
                .income
                .and_then(|v| {
                    self.convert_observed(v.value(), source, currency, entry.date, fx_ready)
                })
                .map(|(value, date)| {
                    if let Some(date) = date {
                        fx_sources.insert((source.clone(), entry.date, date));
                    }
                    value
                })
                .map(ExactDecimal::new);
            let outgoing = entry
                .amounts
                .expense
                .and_then(|v| {
                    self.convert_observed(v.value(), source, currency, entry.date, fx_ready)
                })
                .map(|(value, date)| {
                    if let Some(date) = date {
                        fx_sources.insert((source.clone(), entry.date, date));
                    }
                    value
                })
                .map(ExactDecimal::new);
            if (incoming.is_none() && entry.amounts.income.is_some())
                || (outgoing.is_none() && entry.amounts.expense.is_some())
            {
                issues.push(issue(
                    "MISSING_FX",
                    Some(entry.date.to_string()),
                    Some(source.clone()),
                ));
            }
            income = add(income, incoming)?;
            expense = add(expense, outgoing)?;
        }
        if ordinary.issues.iter().any(|i| i.code == "INVALID_DATE") {
            income = None;
            expense = None;
        }
        let source_date = ordinary
            .daily
            .iter()
            .map(|d| d.date)
            .chain(latest_other)
            .max()
            .into_iter()
            .chain(fx_sources.iter().map(|(_, _, date)| *date))
            .min()
            .map(|d| d.to_string());
        let stale = fx_sources
            .iter()
            .any(|(_, activity_date, fx_date)| fx_date < activity_date);
        let fx_sources:Vec<_> = fx_sources.into_iter().map(|(source,activity_date,fx_date)|json!({"sourceCurrency":source,"activityDate":activity_date,"sourceDate":fx_date})).collect();
        let data = json!({"ordinary":ordinary,"convertedOrdinary":{"currency":currency,"income":income,"expense":expense,"rateDateBasis":"ACTIVITY_BUSINESS_DAY","fxSources":fx_sources},"internalCashTransfers":transfers,"purposeFlows":purpose_flows});
        Ok(InvestmentContextBlock {
            scope: json!({"dateFrom":from,"dateTo":day,"timezone":timezone,"ordinaryAccounts":"ACTIVE_UNARCHIVED_CASH_AND_CREDIT_CARD","transfers":"PAIRED_CASH_TRANSFERS_TOUCHING_ACTIVE_UNARCHIVED_ACCOUNTS","transferDateBasis":"OUTGOING_LEG_BUSINESS_DAY","purposeIds":query.purpose_ids,"purposeFlowDateBasis":"OPERATION_BUSINESS_DAY","purposeFlowsAreOrdinaryIncome":false}),
            as_of_date: day,
            source_data_as_of: source_date,
            currency: currency.into(),
            quality: quality(true, &issues),
            stale,
            issues,
            data,
        })
    }
}
fn add(a: Option<ExactDecimal>, b: Option<ExactDecimal>) -> JournalResult<Option<ExactDecimal>> {
    a.zip(b).map(|(a, b)| a.checked_add(b)).transpose()
}
fn business_date(value: &str, timezone: &str) -> JournalResult<NaiveDate> {
    if value.len() == 10 {
        return NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| invalid("invalid source date"));
    }
    let zone = wealthfolio_core::utils::time_utils::parse_user_timezone(timezone)?;
    DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&zone).date_naive())
        .map_err(|_| invalid("invalid source datetime"))
}
fn internal_transfers(
    conn: &mut SqliteConnection,
    from: NaiveDate,
    to: NaiveDate,
    timezone: &str,
    issues: &mut Vec<InvestmentContextIssue>,
) -> JournalResult<(Vec<super::super::CashTransferSnapshot>, Option<NaiveDate>)> {
    let lower = from
        .checked_sub_signed(Duration::days(2))
        .ok_or_else(|| invalid("date out of range"))?;
    let upper = to
        .checked_add_signed(Duration::days(3))
        .ok_or_else(|| invalid("date out of range"))?;
    let eligible = accounts::table
        .filter(accounts::is_active.eq(true))
        .filter(accounts::is_archived.eq(false))
        .select(accounts::id);
    let candidates = activities::table
        .filter(activities::account_id.eq_any(eligible))
        .filter(activities::source_group_id.is_not_null())
        .filter(activities::activity_date.ge(lower.to_string()))
        .filter(activities::activity_date.lt(upper.to_string()))
        .order(activities::id)
        .load::<ActivityDB>(conn)?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    let mut latest = None;
    for row in candidates {
        let kind = wealthfolio_core::activities::effective_activity_type(
            &row.activity_type,
            row.activity_type_override.as_deref(),
        );
        if !matches!(kind, "TRANSFER_IN" | "TRANSFER_OUT") || row.asset_id.is_some() {
            continue;
        }
        let Some(group) = row.source_group_id.as_deref() else {
            continue;
        };
        if !seen.insert(group.to_string()) {
            continue;
        }
        let source = match super::super::sources::load_cash_transfer(conn, &row.id) {
            Ok(source) => source,
            Err(_) => {
                issues.push(issue("TRANSFER_PAIR_INVALID", Some(group.into()), None));
                continue;
            }
        };
        let date = match business_date(&source.view.occurred_at, timezone) {
            Ok(date) => date,
            Err(_) => {
                issues.push(issue("TRANSFER_DATE_INVALID", Some(group.into()), None));
                continue;
            }
        };
        if date < from || date > to {
            continue;
        }
        latest = latest.into_iter().chain(Some(date)).max();
        result.push(source.view);
    }
    result.sort_by(|a, b| {
        (&a.occurred_at, &a.out_activity_id).cmp(&(&b.occurred_at, &b.out_activity_id))
    });
    Ok((result, latest))
}
#[derive(QueryableByName)]
struct Row {
    #[diesel(sql_type=Text)]
    body: String,
}
fn purpose_flows(
    conn: &mut SqliteConnection,
    purposes: &[String],
    from: NaiveDate,
    to: NaiveDate,
    timezone: &str,
) -> JournalResult<(Value, Option<NaiveDate>)> {
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let mut conversions = Vec::new();
    let mut adjustments = Vec::new();
    let mut latest = None;
    let rows=diesel::sql_query("SELECT json_object('id',id,'kind',operation_kind,'createdAt',created_at,'detail',json(detail_json)) AS body FROM fund_allocation_operations ORDER BY rowid").load::<Row>(conn)?;
    for row in rows {
        let operation: Value =
            serde_json::from_str(&row.body).map_err(|e| invalid(e.to_string()))?;
        let date = business_date(
            operation["createdAt"].as_str().unwrap_or_default(),
            timezone,
        )?;
        if date < from || date > to {
            continue;
        }
        let id = operation["id"]
            .as_str()
            .ok_or_else(|| invalid("purpose operation has no id"))?;
        let kind = operation["kind"].as_str().unwrap_or_default();
        let groups: Vec<(&str, Vec<AllocationDelta>)> = if kind == "TRADE" {
            let ownership = &operation["detail"]["ownership"];
            vec![
                (
                    "TOP_UP",
                    serde_json::from_value(ownership["topUpEntries"].clone())
                        .map_err(|e| invalid(e.to_string()))?,
                ),
                (
                    "TRADE_CONVERSION",
                    serde_json::from_value(ownership["conversionEntries"].clone())
                        .map_err(|e| invalid(e.to_string()))?,
                ),
            ]
        } else {
            let entries=diesel::sql_query("SELECT json_object('purposeId',purpose_id,'resource',json(CASE WHEN resource_kind='CASH' THEN json_object('kind','CASH','accountId',account_id,'currency',currency) ELSE json_object('kind','SECURITY','accountId',account_id,'assetId',asset_id) END),'delta',COALESCE(cash_delta,quantity_delta)) AS body FROM fund_allocation_entries WHERE operation_id=?1 ORDER BY rowid")
                .bind::<Text,_>(id).load::<Row>(conn)?.into_iter().map(|r|serde_json::from_str::<AllocationDelta>(&r.body).map_err(|e|invalid(e.to_string()))).collect::<JournalResult<Vec<_>>>()?;
            vec![(kind, entries)]
        };
        for (leg, entries) in groups {
            for entry in entries
                .into_iter()
                .filter(|e| purposes.contains(&e.purpose_id))
            {
                let item = json!({"operationId":id,"operationKind":kind,"leg":leg,"occurredOn":date,"entry":entry});
                latest = latest.into_iter().chain(Some(date)).max();
                if leg == "TRADE_CONVERSION" {
                    conversions.push(item);
                } else if matches!(kind, "REVERSE" | "RECONCILE") {
                    adjustments.push(item);
                } else if entry.delta > ExactDecimal::ZERO {
                    incoming.push(item);
                } else {
                    outgoing.push(item);
                }
            }
        }
    }
    Ok((
        json!({"allocationIn":incoming,"allocationOut":outgoing,"tradeConversions":conversions,"adjustments":adjustments}),
        latest,
    ))
}
