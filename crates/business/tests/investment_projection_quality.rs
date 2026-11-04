//! Dirty native projections are distinct from old-but-valid market observations.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use chrono::{Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use support::{journal_caller, Harness};
use wealthfolio_business::{investment_journal::*, BusinessChannel};
use wealthfolio_core::portfolio::{
    net_worth::NetWorthServiceTrait,
    projection::{
        ActivityIssue, ActivityIssueKind, MarkerScope, ProjectionStoreTrait, RunCompletion,
        WindowRows,
    },
};
use wealthfolio_storage_sqlite::portfolio::{
    projection::ProjectionStore, snapshot::SnapshotRepository,
};

async fn capture(h: &Harness, account: &str) -> InvestmentContextCapture {
    h.journal()
        .with_context_reader(h.context.clone())
        .get_investment_context(
            InvestmentContextQuery {
                purpose_ids: vec![],
                account_ids: vec![account.into()],
            },
            &journal_caller(),
        )
        .await
        .unwrap()
}
fn item<'a>(read: &'a InvestmentContextCapture, id: &str) -> &'a Value {
    read.response.household.data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap()
}
async fn publish_cash_projection(h: &Harness, amount: &str) {
    publish_cash_projection_with_issues(h, amount, Vec::new()).await;
}
async fn publish_cash_projection_with_issues(
    h: &Harness,
    amount: &str,
    issues: Vec<ActivityIssue>,
) {
    let day = NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap();
    let repository = SnapshotRepository::new(h.store.pool().clone(), h.store.writer().clone());
    let mut snapshot = repository
        .get_latest_snapshot_before_date("transactions", day)
        .unwrap()
        .unwrap();
    let value = Decimal::from_str_exact(amount).unwrap();
    snapshot.cash_balances.insert("CNY".into(), value);
    snapshot.net_contribution = value;
    snapshot.net_contribution_base = value;
    snapshot.cash_total_account_currency = value;
    snapshot.cash_total_base_currency = value;
    snapshot.calculated_at = Utc::now().naive_utc();
    let store = ProjectionStore::new(h.store.pool().clone(), h.store.writer().clone());
    let consumed = store
        .pending_markers()
        .unwrap()
        .into_iter()
        .filter(|marker| marker.scope == MarkerScope::Account("transactions".into()))
        .collect();
    store
        .write_window(vec![WindowRows {
            account_id: "transactions".into(),
            start: day,
            end: Some(day),
            snapshots: Some(vec![snapshot]),
            valuations: vec![],
        }])
        .await
        .unwrap();
    store
        .complete_run(RunCompletion {
            consumed,
            activity_issues: vec![("transactions".into(), issues)],
            ..Default::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn same_day_calculated_snapshot_stays_unknown_until_its_fact_marker_is_consumed() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("UPDATE accounts SET is_archived=1 WHERE id='manual';")
        .unwrap();
    db.execute("INSERT INTO holdings_snapshots(id,account_id,snapshot_date,currency,positions,cash_balances,cost_basis,net_contribution,calculated_at,net_contribution_base,cash_total_account_currency,cash_total_base_currency,source) VALUES('calculated','transactions',?1,'CNY','{}','{\"CNY\":\"1000\"}','0','1000','2100-01-01T00:00:00','1000','1000','1000','CALCULATED')",[&h.day]).unwrap();
    // Explicitly establish this synthetic starting projection as complete.
    db.execute("UPDATE projection_state SET dirty_from=NULL", [])
        .unwrap();
    assert_eq!(
        capture(&h, "transactions").await.response.household.data["netWorth"],
        "1000"
    );
    db.execute("UPDATE activities SET amount='1200' WHERE id='deposit'", [])
        .unwrap();
    let pending = capture(&h, "transactions").await;
    assert_eq!(pending.response.household.data["netWorth"], Value::Null);
    assert_eq!(
        item(&pending, "CASH:transactions:CNY")["value"],
        Value::Null
    );
    let row = &pending.response.funding.data["accountValuations"]["accounts"][0]["resources"][0];
    assert_eq!(row["nativeAmount"], Value::Null);
    assert_eq!(row["valueBase"], Value::Null);
    assert!(row["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["code"] == "PROJECTION_NOT_CURRENT"));
    assert_eq!(
        pending.response.funding.data["allocation"]["resources"][0]["balance"]["actual"], "1200",
        "native resource observation refolds facts independently of the cached snapshot"
    );
    assert_eq!(
        h.net_worth
            .get_net_worth(NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap())
            .await
            .unwrap()
            .net_worth
            .to_string(),
        "1000",
        "legacy cached amount remains compatible"
    );
    publish_cash_projection(&h, "1200").await;
    let fresh = capture(&h, "transactions").await;
    assert_eq!(fresh.response.household.data["netWorth"], "1200");
    assert_eq!(
        fresh.response.funding.data["accountValuations"]["accounts"][0]["resources"][0]
            ["nativeAmount"],
        "1200"
    );
    // A force/rebuild cycle with identical facts and values changes clocks and
    // marker versions only. It must not manufacture a confirmation conflict.
    let store = ProjectionStore::new(h.store.pool().clone(), h.store.writer().clone());
    store
        .invalidate(
            MarkerScope::Account("transactions".into()),
            NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap(),
        )
        .await
        .unwrap();
    publish_cash_projection(&h, "1200").await;
    assert_eq!(
        capture(&h, "transactions").await.source_version,
        fresh.source_version
    );
}

async fn manual_with_stored_value() -> Harness {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.investments
        .record_manual_trade(
            h.trade().await,
            BusinessChannel::Gui,
            "synthetic-session",
            true,
        )
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("UPDATE accounts SET is_archived=1 WHERE id='transactions';")
        .unwrap();
    let quote_day = Utc::now().date_naive().to_string();
    db.execute("DELETE FROM quotes WHERE asset_id='stock'", [])
        .unwrap();
    db.execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('market-proof','stock',?1,'MANUAL','10','CNY',CURRENT_TIMESTAMP,?2)",(&quote_day,format!("{quote_day}T00:00:00Z"))).unwrap();
    let day = h.day.parse().unwrap();
    let mut facts = h
        .facts
        .load_for_investment_journal(&["manual".into()], "CNY", "Asia/Shanghai", day)
        .unwrap();
    // Production reads sparse prices per valuation window, separately from the
    // accounting fact loader (which only brings split-adjacent prices).
    let quotes = h
        .facts
        .quotes
        .get_sparse_quotes_in_range(
            &std::collections::HashSet::from(["stock".into()]),
            quote_day.parse().unwrap(),
            day,
        )
        .unwrap();
    facts.raw.quotes.extend(quotes.into_iter().map(|quote| {
        wealthfolio_portfolio_engine::model::RawQuote {
            asset_id: quote.asset_id,
            day: quote.timestamp.date_naive(),
            close: quote.close,
            currency: quote.currency,
            source: quote.data_source,
        }
    }));
    let engine = wealthfolio_portfolio_engine::Engine::new(facts.raw).unwrap();
    let projected = engine.project().unwrap();
    let values = engine.value(&projected);
    let series = values
        .get(&wealthfolio_portfolio_engine::model::AccountId::new(
            "manual",
        ))
        .unwrap();
    let row = wealthfolio_core::portfolio::coordinator::valuation_rows(series, "manual", "CNY")
        .into_iter()
        .find(|row| row.valuation_date == day)
        .unwrap();
    assert!(row
        .quote_inputs
        .as_ref()
        .is_some_and(|inputs| inputs.len() == 1));
    ProjectionStore::new(h.store.pool().clone(), h.store.writer().clone())
        .write_window(vec![WindowRows {
            account_id: "manual".into(),
            start: day,
            end: Some(day),
            snapshots: None,
            valuations: vec![row],
        }])
        .await
        .unwrap();
    db.execute("UPDATE projection_state SET dirty_from=NULL", [])
        .unwrap();
    drop(db);
    h
}

#[tokio::test]
async fn price_invalidation_hides_stored_investments_and_retains_current_cash_and_live_revaluation()
{
    let h = manual_with_stored_value().await;
    assert_eq!(
        capture(&h, "manual").await.response.household.data["netWorth"],
        "998.5"
    );
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("UPDATE quotes SET close='12' WHERE asset_id='stock'", [])
        .unwrap();
    let pending = capture(&h, "manual").await;
    assert_eq!(item(&pending, "INVESTMENTS:manual")["value"], Value::Null);
    assert_eq!(
        item(&pending, "INVESTMENTS:manual")["substituteValue"],
        Value::Null
    );
    assert_eq!(item(&pending, "CASH:manual")["value"], "898.5");
    assert_eq!(
        pending.response.funding.data["accountValuations"]["investmentMarketValueBase"], "120",
        "live prices do not require a quantity refold"
    );
    assert_eq!(
        pending.response.funding.data["accountValuations"]["cashBalanceBase"],
        "898.5"
    );
    let day = NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap();
    assert_eq!(
        h.net_worth
            .get_net_worth(day)
            .await
            .unwrap()
            .net_worth
            .to_string(),
        "998.5"
    );
}

#[tokio::test]
async fn future_dirty_days_do_not_poison_prior_values_and_undated_invalidations_fail_closed() {
    let h = manual_with_stored_value().await;
    let db = h.access.connect_rusqlite().unwrap();
    let future =
        (NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap() + Duration::days(2)).to_string();
    db.execute(
        "UPDATE projection_state SET dirty_from=?1 WHERE scope='manual'",
        [future],
    )
    .unwrap();
    assert_eq!(
        capture(&h, "manual").await.response.household.data["netWorth"],
        "998.5"
    );
    db.execute(
        "UPDATE projection_state SET dirty_from='bad-day' WHERE scope='manual'",
        [],
    )
    .unwrap();
    let pending = capture(&h, "manual").await;
    assert_eq!(item(&pending, "CASH:manual")["value"], Value::Null);
    assert_eq!(
        pending.response.funding.data["accountValuations"]["cashBalanceBase"], "898.5",
        "observed manual snapshots remain authoritative while their stored valuation waits"
    );
}

#[tokio::test]
async fn missing_final_cash_is_not_a_verified_cached_value_or_substitute() {
    let h = Harness::new().await;
    let day: NaiveDate = h.day.parse().unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("UPDATE accounts SET is_archived=1 WHERE id='manual';")
        .unwrap();
    db.execute(r#"INSERT INTO holdings_snapshots(id,account_id,snapshot_date,currency,positions,cash_balances,cost_basis,net_contribution,calculated_at,net_contribution_base,cash_total_account_currency,cash_total_base_currency,source) VALUES('calculated','transactions',?1,'CNY','{}','{"CNY":"1000"}','0','1000','2100-01-01T00:00:00','1000','1000','1000','CALCULATED')"#,[&h.day]).unwrap();
    let future = format!("{}T18:30:00Z", day);
    let included = format!("{}T15:30:00Z", day);
    for (id, stamp) in [("future-missing", &future), ("included-missing", &included)] {
        db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,created_at,updated_at) VALUES(?1,'transactions','WITHDRAWAL','POSTED',?2,'CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,stamp)).unwrap();
    }
    db.execute("INSERT INTO app_settings(setting_key,setting_value) VALUES('timezone','Asia/Shanghai') ON CONFLICT(setting_key) DO UPDATE SET setting_value=excluded.setting_value",[]).unwrap();
    let issue = |id: &str| ActivityIssue {
        activity_id: id.into(),
        kind: ActivityIssueKind::MissingAmount,
        message: "Synthetic kernel diagnostic".into(),
    };
    publish_cash_projection_with_issues(
        &h,
        "1000",
        vec![issue("future-missing"), issue("included-missing")],
    )
    .await;
    db.execute("UPDATE projection_state SET dirty_from=NULL", [])
        .unwrap();
    let incomplete = capture(&h, "transactions").await;
    assert_eq!(incomplete.response.household.data["netWorth"], Value::Null);
    assert_eq!(
        item(&incomplete, "CASH:transactions:CNY")["value"],
        Value::Null
    );
    assert_eq!(
        item(&incomplete, "CASH:transactions:CNY")["substituteValue"],
        Value::Null
    );
    assert!(incomplete
        .response
        .household
        .issues
        .iter()
        .any(|item| item.code == "MISSING_FINAL_CASH"));
    let valuation = &incomplete.response.funding.data["accountValuations"]["accounts"][0];
    assert_eq!(valuation["cashBalanceBase"], Value::Null);
    assert_eq!(valuation["investmentMarketValueBase"], "0");
    assert_eq!(valuation["resources"][0]["nativeAmount"], Value::Null);
    assert!(incomplete
        .response
        .funding
        .issues
        .iter()
        .any(|item| item.code == "MISSING_FINAL_CASH"));
    assert_eq!(
        h.net_worth
            .get_net_worth(day)
            .await
            .unwrap()
            .net_worth
            .to_string(),
        "1000"
    );

    db.execute(
        "UPDATE activities SET amount='125' WHERE id='included-missing'",
        [],
    )
    .unwrap();
    publish_cash_projection_with_issues(&h, "875", vec![issue("future-missing")]).await;
    let recovered = capture(&h, "transactions").await;
    assert_eq!(recovered.response.household.data["netWorth"], "875");
    assert_eq!(
        recovered.response.funding.data["accountValuations"]["accounts"][0]["cashBalanceBase"],
        "875"
    );
    assert!(!recovered
        .response
        .household
        .issues
        .iter()
        .any(|item| item.code == "MISSING_FINAL_CASH"));
}

#[tokio::test]
async fn cached_investments_require_the_original_quote_not_an_alternative() {
    let h = manual_with_stored_value().await;
    let db = h.access.connect_rusqlite().unwrap();
    let saved_inputs: String = db
        .query_row(
            "SELECT quote_inputs FROM daily_account_valuation WHERE account_id='manual'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let inputs: Value = serde_json::from_str(&saved_inputs).unwrap();
    assert_eq!(inputs[0]["source"], "MANUAL");
    let before = capture(&h, "manual").await;
    assert_eq!(item(&before, "INVESTMENTS:manual")["value"], "100");
    assert_eq!(
        item(&before, "INVESTMENTS:manual")["valuationDate"],
        inputs[0]["day"]
    );

    db.execute(
        "UPDATE daily_account_valuation SET quote_inputs=NULL WHERE account_id='manual'",
        [],
    )
    .unwrap();
    let legacy_cache = capture(&h, "manual").await;
    assert_eq!(
        item(&legacy_cache, "INVESTMENTS:manual")["value"],
        Value::Null
    );
    assert_eq!(
        item(&legacy_cache, "INVESTMENTS:manual")["substituteValue"],
        Value::Null
    );
    assert_eq!(
        item(&legacy_cache, "INVESTMENTS:manual")["valuationDate"],
        Value::Null
    );
    assert!(legacy_cache
        .response
        .household
        .issues
        .iter()
        .any(|issue| issue.code == "STORED_QUOTE_UNVERIFIED"));
    assert_eq!(item(&legacy_cache, "CASH:manual")["value"], "898.5");
    db.execute(
        "UPDATE daily_account_valuation SET quote_inputs=?1 WHERE account_id='manual'",
        [&saved_inputs],
    )
    .unwrap();

    let day = inputs[0]["day"].as_str().unwrap();
    db.execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('alternate','stock',?1,'BROKER','12','CNY',CURRENT_TIMESTAMP,?2)",(day,format!("{day}T00:00:00Z"))).unwrap();
    db.execute("DELETE FROM quotes WHERE id='market-proof'", [])
        .unwrap();
    // Even if freshness markers were already consumed, the original row is
    // gone and the valid alternate price cannot certify the cached 100.
    db.execute("UPDATE projection_state SET dirty_from=NULL", [])
        .unwrap();
    let replaced = capture(&h, "manual").await;
    assert_eq!(item(&replaced, "INVESTMENTS:manual")["value"], Value::Null);
    assert!(replaced
        .response
        .household
        .issues
        .iter()
        .any(|issue| issue.code == "INVALID_QUOTE_SOURCE"));
    assert_eq!(
        replaced.response.funding.data["accountValuations"]["investmentMarketValueBase"],
        "120"
    );
    assert_eq!(
        h.net_worth
            .get_net_worth(h.day.parse().unwrap())
            .await
            .unwrap()
            .net_worth
            .to_string(),
        "998.5"
    );
}
