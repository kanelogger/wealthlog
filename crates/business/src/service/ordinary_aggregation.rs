//! Unpaginated ordinary cash-flow aggregation using the ordinary query's eligibility.
use crate::{
    investment_journal::{ExactDecimal, JournalError, JournalResult},
    BusinessService,
};
use chrono::{DateTime, Duration, NaiveDate};
use diesel::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wealthfolio_storage_sqlite::{
    activities::ActivityDB,
    db::get_connection,
    schema::{accounts, activities},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryCurrencyAggregate {
    pub currency: String,
    pub income: Option<ExactDecimal>,
    pub expense: Option<ExactDecimal>,
    pub known_income: ExactDecimal,
    pub known_expense: ExactDecimal,
    pub income_records: usize,
    pub expense_records: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryDayCurrencyAggregate {
    pub date: NaiveDate,
    pub amounts: OrdinaryCurrencyAggregate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryAggregationIssue {
    pub code: String,
    pub activity_id: String,
    pub currency: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryAggregation {
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
    pub timezone: String,
    pub account_ids: Vec<String>,
    pub record_count: usize,
    pub currencies: Vec<OrdinaryCurrencyAggregate>,
    pub daily: Vec<OrdinaryDayCurrencyAggregate>,
    pub issues: Vec<OrdinaryAggregationIssue>,
    pub source_version: String,
}
impl BusinessService {
    pub fn aggregate_ordinary_native(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        timezone: &str,
    ) -> JournalResult<OrdinaryAggregation> {
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| aggregate_ordinary(conn, from, to, timezone))
    }
}

pub(crate) fn aggregate_ordinary(
    conn: &mut SqliteConnection,
    from: NaiveDate,
    to: NaiveDate,
    timezone: &str,
) -> JournalResult<OrdinaryAggregation> {
    if from > to {
        return Err(invalid("dateFrom must not be after dateTo"));
    }
    let zone = wealthfolio_core::utils::time_utils::parse_user_timezone(timezone)?;
    let eligible: Vec<(String, String, String, bool, bool)> = accounts::table
        .filter(accounts::is_active.eq(true))
        .filter(accounts::is_archived.eq(false))
        .filter(accounts::account_type.eq_any(wealthfolio_core::accounts::SPENDING_ACCOUNT_TYPES))
        .select((
            accounts::id,
            accounts::account_type,
            accounts::currency,
            accounts::is_active,
            accounts::is_archived,
        ))
        .order(accounts::id)
        .load(conn)?;
    // A two-day margin covers even opposite extreme stored and business UTC offsets.
    // Final membership uses the
    // ledger's business day; a legacy DATE remains that explicit calendar date.
    let lower = from
        .checked_sub_signed(Duration::days(2))
        .ok_or_else(|| invalid("dateFrom out of range"))?;
    let upper = to
        .checked_add_signed(Duration::days(3))
        .ok_or_else(|| invalid("dateTo out of range"))?;
    let rows = crate::service::ordinary_activity_scope()
        .filter(activities::activity_date.ge(lower.to_string()))
        .filter(activities::activity_date.lt(upper.to_string()))
        .order(activities::id)
        .load::<ActivityDB>(conn)?;
    let mut currencies: BTreeMap<String, OrdinaryCurrencyAggregate> = BTreeMap::new();
    let mut daily: BTreeMap<(NaiveDate, String), OrdinaryCurrencyAggregate> = BTreeMap::new();
    let mut issues = Vec::new();
    let mut count = 0;
    for row in &rows {
        let day = if row.activity_date.len() == 10 {
            NaiveDate::parse_from_str(&row.activity_date, "%Y-%m-%d").ok()
        } else {
            DateTime::parse_from_rfc3339(&row.activity_date)
                .ok()
                .map(|d| d.with_timezone(&zone).date_naive())
        };
        let Some(day) = day else {
            issues.push(OrdinaryAggregationIssue {
                code: "INVALID_DATE".into(),
                activity_id: row.id.clone(),
                currency: row.currency.clone(),
            });
            continue;
        };
        if day < from || day > to {
            continue;
        }
        count += 1;
        let income = row.activity_type == "DEPOSIT";
        let amount = row
            .amount
            .as_deref()
            .and_then(|v| {
                Decimal::from_str_exact(v)
                    .or_else(|_| Decimal::from_scientific(v))
                    .ok()
            })
            .map(|v| v.abs());
        if amount.is_none() {
            issues.push(OrdinaryAggregationIssue {
                code: if row.amount.is_none() {
                    "MISSING_AMOUNT"
                } else {
                    "INVALID_AMOUNT"
                }
                .into(),
                activity_id: row.id.clone(),
                currency: row.currency.clone(),
            });
        }
        let overall = currencies
            .entry(row.currency.clone())
            .or_insert_with(|| empty_currency(&row.currency));
        add_ordinary_amount(overall, income, amount)?;
        let per_day = daily
            .entry((day, row.currency.clone()))
            .or_insert_with(|| empty_currency(&row.currency));
        add_ordinary_amount(per_day, income, amount)?;
    }
    let mut source_rows = serde_json::to_value(&rows).map_err(|e| invalid(&e.to_string()))?;
    if let Some(rows) = source_rows.as_array_mut() {
        for row in rows {
            if let Some(fields) = row.as_object_mut() {
                for key in ["created_at", "updated_at", "createdAt", "updatedAt"] {
                    fields.remove(key);
                }
            }
        }
    }
    let source_version = crate::requests::hash_json(&crate::requests::canonicalize(
        serde_json::json!({"schemaVersion":1,"from":from,"to":to,"timezone":timezone,"accounts":eligible,"activities":source_rows}),
    ));
    Ok(OrdinaryAggregation {
        date_from: from,
        date_to: to,
        timezone: timezone.into(),
        account_ids: eligible.into_iter().map(|a| a.0).collect(),
        record_count: count,
        currencies: currencies.into_values().collect(),
        daily: daily
            .into_iter()
            .map(|((date, _), amounts)| OrdinaryDayCurrencyAggregate { date, amounts })
            .collect(),
        issues,
        source_version,
    })
}

fn empty_currency(currency: &str) -> OrdinaryCurrencyAggregate {
    OrdinaryCurrencyAggregate {
        currency: currency.into(),
        income: Some(ExactDecimal::ZERO),
        expense: Some(ExactDecimal::ZERO),
        known_income: ExactDecimal::ZERO,
        known_expense: ExactDecimal::ZERO,
        income_records: 0,
        expense_records: 0,
    }
}
fn add_ordinary_amount(
    item: &mut OrdinaryCurrencyAggregate,
    income: bool,
    amount: Option<Decimal>,
) -> JournalResult<()> {
    if income {
        item.income_records += 1;
    } else {
        item.expense_records += 1;
    }
    let Some(amount) = amount else {
        if income {
            item.income = None;
        } else {
            item.expense = None;
        }
        return Ok(());
    };
    let known = if income {
        &mut item.known_income
    } else {
        &mut item.known_expense
    };
    *known = ExactDecimal::new(
        known
            .value()
            .checked_add(amount)
            .ok_or_else(|| invalid("ordinary cash-flow total overflow"))?,
    );
    if income {
        if item.income.is_some() {
            item.income = Some(*known);
        }
    } else if item.expense.is_some() {
        item.expense = Some(*known);
    }
    Ok(())
}

fn invalid(message: &str) -> JournalError {
    JournalError::InvalidInput(message.into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests {
    use super::*;
    use wealthfolio_storage_sqlite::db::DbAccess;
    #[test]
    fn native_aggregate_covers_more_than_200_rows_with_exact_currency_totals_and_shared_scope() {
        let dir = tempfile::tempdir().unwrap();
        let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
        access.run_migrations().unwrap();
        let db = access.connect_rusqlite().unwrap();
        db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','cash','CASH','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS'),('inactive','inactive','CASH','CNY',0,0,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS'),('archived','archived','CASH','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,1,'TRANSACTIONS'),('securities','securities','SECURITIES','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');").unwrap();
        for i in 0..256 {
            db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES(?1,'cash','DEPOSIT','POSTED','2026-11-02T12:00:00Z','CNY','0.01',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[format!("income-{i:03}")]).unwrap();
        }
        for (id, account, kind, status, subtype) in [
            ("inactive", "inactive", "DEPOSIT", "POSTED", None),
            ("archived", "archived", "DEPOSIT", "POSTED", None),
            ("security", "securities", "DEPOSIT", "POSTED", None),
            ("draft", "cash", "DEPOSIT", "DRAFT", None),
            ("dividend", "cash", "DIVIDEND", "POSTED", None),
            ("subtype", "cash", "DEPOSIT", "POSTED", Some("OTHER")),
            ("transfer", "cash", "TRANSFER_IN", "POSTED", None),
        ] {
            db.execute("INSERT INTO activities(id,account_id,activity_type,status,subtype,activity_date,currency,amount,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'2026-11-02T12:00:00Z','CNY','10000',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,account,kind,status,subtype)).unwrap();
        }
        db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES('large','cash','WITHDRAWAL','POSTED','2026-11-02T12:00:00Z','CNY','9007199254740993.01',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[]).unwrap();
        drop(db);
        let mut conn = access.connect().unwrap();
        let from = NaiveDate::from_ymd_opt(2026, 10, 4).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let result = conn
            .transaction::<_, JournalError, _>(|c| aggregate_ordinary(c, from, to, "Asia/Shanghai"))
            .unwrap();
        assert_eq!(result.record_count, 257);
        assert_eq!(result.currencies.len(), 1);
        assert_eq!(result.account_ids, vec!["cash"]);
        assert_eq!(result.currencies[0].income.unwrap().to_string(), "2.56");
        assert_eq!(
            result.currencies[0].expense.unwrap().to_string(),
            "9007199254740993.01"
        );
        assert!(result.issues.is_empty());
        let page = crate::service::ordinary_activity_scope()
            .limit(200)
            .load::<ActivityDB>(&mut conn)
            .unwrap();
        assert_eq!(
            page.len(),
            200,
            "ordinary pagination remains capped independently of complete aggregation"
        );
        let encoded = serde_json::to_value(result).unwrap();
        assert_eq!(encoded["currencies"][0]["expense"], "9007199254740993.01");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod missing_tests {
    use super::*;
    use wealthfolio_storage_sqlite::db::DbAccess;
    #[test]
    fn business_day_dates_and_missing_amounts_are_not_zero_or_inferred_income() {
        let dir = tempfile::tempdir().unwrap();
        let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
        access.run_migrations().unwrap();
        let db = access.connect_rusqlite().unwrap();
        db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','cash','CASH','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');").unwrap();
        for (id, date, kind, amount) in [
            ("at-start", "2026-11-02T16:00:00Z", "DEPOSIT", Some("0.1")),
            ("before", "2026-11-02T15:59:59Z", "DEPOSIT", Some("10000")),
            ("after", "2026-11-03T16:00:00Z", "DEPOSIT", Some("10000")),
            ("date-only", "2026-11-03", "WITHDRAWAL", Some("0.3")),
            ("missing", "2026-11-03T01:00:00Z", "DEPOSIT", None),
        ] {
            db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES(?1,'cash',?2,'POSTED',?3,'CNY',?4,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,kind,date,amount)).unwrap();
        }
        let day = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let mut conn = access.connect().unwrap();
        let result = conn
            .transaction::<_, JournalError, _>(|c| aggregate_ordinary(c, day, day, "Asia/Shanghai"))
            .unwrap();
        assert_eq!(result.record_count, 3);
        assert_eq!(result.currencies[0].income, None);
        assert_eq!(result.currencies[0].known_income.to_string(), "0.1");
        assert_eq!(result.currencies[0].expense.unwrap().to_string(), "0.3");
        assert_eq!(result.issues[0].code, "MISSING_AMOUNT");
        db.execute("UPDATE activities SET amount='1' WHERE id='missing'", [])
            .unwrap();
        let changed = conn
            .transaction::<_, JournalError, _>(|c| aggregate_ordinary(c, day, day, "Asia/Shanghai"))
            .unwrap();
        assert_ne!(result.source_version, changed.source_version);
        assert!(changed.issues.is_empty());
        assert_eq!(changed.currencies[0].income.unwrap().to_string(), "1.1");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM business_requests", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
