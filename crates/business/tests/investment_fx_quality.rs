//! Native FX paths verify their selected original rows, including inverse hops.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use chrono::{Duration, Utc};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use support::{exact, journal_caller, Harness};
use wealthfolio_business::{
    investment_journal::*, investments::PatchHoldingsSnapshot, BusinessChannel,
};
use wealthfolio_core::fx::{FxService, FxServiceTrait};
use wealthfolio_storage_sqlite::fx::FxRepository;

fn fx(h: &Harness) -> FxService {
    FxService::new(Arc::new(FxRepository::new(
        h.store.pool().clone(),
        h.store.writer().clone(),
    )))
}

#[tokio::test]
async fn stored_values_check_the_kernel_source_even_when_legacy_inverse_precedence_uses_a_valid_other_row(
) {
    let h = setup().await;
    let day = Utc::now().date_naive();
    seed_rate(&h, "z-cny-eur", "CNY", "EUR", "0.1", &day.to_string());
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE quotes SET timestamp='bad-timestamp' WHERE id='quote-eur-cny'",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO daily_account_valuation(id,account_id,valuation_date,account_currency,base_currency,fx_rate_to_base,cash_balance,investment_market_value,total_value,cost_basis,net_contribution,cash_balance_base,investment_market_value_base,total_value_base) VALUES('kernel-cached','manual',?1,'CNY','CNY','1','1700','0','1700','0','1000','1700','0','1700')",[&h.day]).unwrap();
    // Simulate the old producer having completed with its compatibility time
    // fallback. A clean marker is insufficient proof of its original FX row.
    db.execute("UPDATE projection_state SET dirty_from=NULL", [])
        .unwrap();
    let fx = fx(&h);
    fx.initialize().unwrap();
    let legacy = fx
        .get_exchange_rate_observation("EUR", "CNY", Some(day))
        .unwrap();
    assert_eq!(legacy.rate.to_string(), "10");
    assert!(legacy.verified);
    let kernel = fx
        .get_projection_exchange_rate_observation("EUR", "CNY", day)
        .unwrap();
    assert_eq!(kernel.rate.to_string(), "7");
    assert!(!kernel.verified);
    assert_eq!(kernel.source_date, None);
    let read = context(&h).await;
    let stored = read.response.household.data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "CASH:manual")
        .unwrap();
    assert_eq!(stored["value"], Value::Null);
    assert_eq!(stored["substituteValue"], Value::Null);
    assert!(stored["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["code"] == "MISSING_FX"));
    assert!(
        cash(&read, "EUR")["valueBase"].is_string(),
        "the live owner can still verify its own distinct native route"
    );
}

#[tokio::test]
async fn canonical_provider_selection_does_not_borrow_a_same_price_bad_provider_as_its_source() {
    let h = Harness::new().await;
    let day = Utc::now().date_naive();
    seed_rate(&h, "eur-cny", "EUR", "CNY", "7", &day.to_string());
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('other-provider','eur-cny',?1,'YAHOO','7','EUR',CURRENT_TIMESTAMP,'bad-timestamp')",[day.to_string()]).unwrap();
    let fx = fx(&h);
    fx.initialize().unwrap();
    let canonical = fx
        .get_projection_exchange_rate_observation("EUR", "CNY", day)
        .unwrap();
    assert!(canonical.verified);
    assert_eq!(canonical.source_date, Some(day));
    assert!(
        !fx.get_exchange_rate_observation("EUR", "CNY", Some(day))
            .unwrap()
            .verified
    );
}

#[tokio::test]
async fn extreme_multi_hop_fx_returns_unavailable_without_panicking_or_certifying_rounded_zero() {
    let h = Harness::new().await;
    let day = Utc::now().date_naive();
    let max = rust_decimal::Decimal::MAX.to_string();
    seed_rate(&h, "eur-usd", "EUR", "USD", &max, &day.to_string());
    seed_rate(&h, "usd-cny", "USD", "CNY", &max, &day.to_string());
    let fx = fx(&h);
    fx.initialize().unwrap();
    assert!(fx.get_exchange_rate_for_date("EUR", "CNY", day).is_err());
    assert!(!fx.is_exchange_rate_verified("EUR", "CNY", Some(day)));
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("UPDATE quotes SET close='0.0000000000000000000000000001' WHERE id IN ('quote-eur-usd','quote-usd-cny')",[]).unwrap();
    fx.initialize().unwrap();
    assert_eq!(
        fx.get_exchange_rate_for_date("EUR", "CNY", day)
            .unwrap()
            .to_string(),
        "0"
    );
    assert!(!fx.is_exchange_rate_verified("EUR", "CNY", Some(day)));
}
fn seed_rate(h: &Harness, id: &str, from: &str, to: &str, rate: &str, day: &str) {
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,instrument_type,instrument_symbol,created_at,updated_at) VALUES(?1,'FX',?1,?1,1,'MANUAL',?3,'FX',?2,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,from,to)).unwrap();
    db.execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES(?1,?2,?3,'MANUAL',?4,?5,CURRENT_TIMESTAMP,?6)",(format!("quote-{id}"),id,day,rate,from,format!("{day}T00:00:00Z"))).unwrap();
}
async fn setup() -> Harness {
    let h = Harness::new().await;
    let rate_day = Utc::now().date_naive().to_string();
    seed_rate(&h, "eur-cny", "EUR", "CNY", "7", &rate_day);
    let past = (Utc::now().date_naive() - Duration::days(5)).to_string();
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('past-eur-cny','eur-cny',?1,'MANUAL','7','EUR',CURRENT_TIMESTAMP,?2)",(&past,format!("{past}T00:00:00Z"))).unwrap();
    h.initialize_manual().await;
    let state = h.investments.get_state("manual").await.unwrap();
    h.investments
        .patch_snapshot(
            PatchHoldingsSnapshot {
                account_id: "manual".into(),
                snapshot_date: h.day.clone(),
                expected_state_version: state["stateVersion"].as_str().unwrap().into(),
                positions: vec![],
                cash_balances: BTreeMap::from([
                    ("CNY".into(), "1000".into()),
                    ("EUR".into(), "100".into()),
                ]),
                notes: "Synthetic observed foreign cash".into(),
                request_key: "foreign-opening".into(),
            },
            BusinessChannel::Gui,
            "synthetic-session",
            true,
        )
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','Synthetic foreign cash','CASH','EUR',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');").unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES('foreign-income','cash','DEPOSIT','POSTED',?1,'EUR','10',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    for (id, account, currency, balances, total) in [
        (
            "tx-current",
            "transactions",
            "CNY",
            "{\"CNY\":\"1000\"}",
            "1000",
        ),
        ("cash-current", "cash", "EUR", "{\"EUR\":\"10\"}", "10"),
    ] {
        db.execute("INSERT INTO holdings_snapshots(id,account_id,snapshot_date,currency,positions,cash_balances,cost_basis,net_contribution,calculated_at,net_contribution_base,cash_total_account_currency,cash_total_base_currency,source) VALUES(?1,?2,?3,?4,'{}',?5,'0',?6,'2000-01-01T00:00:00',?6,?6,?6,'CALCULATED')",(id,account,&h.day,currency,balances,total)).unwrap();
    }
    h.seed_purpose("purpose");
    db.execute(
        "UPDATE projection_state SET dirty_from=NULL WHERE scope IN ('@all','transactions','cash')",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE fund_purposes SET minimum_cash='100' WHERE id='purpose'",
        [],
    )
    .unwrap();
    let service = h.journal();
    let resource = FundResource::Cash {
        account_id: "manual".into(),
        currency: "EUR".into(),
    };
    let shown = service
        .preview_allocations(vec![resource.clone()], &journal_caller())
        .await
        .unwrap();
    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Synthetic explicit foreign cash ownership".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "purpose".into(),
                    resource: resource.clone(),
                    delta: exact("50"),
                }],
                resources: vec![resource],
                expected_preview_hash: shown.preview_hash,
                request_key: "assign".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
    drop(db);
    h
}
async fn context(h: &Harness) -> InvestmentContextCapture {
    h.journal()
        .with_context_reader(h.context.clone())
        .get_investment_context(
            InvestmentContextQuery {
                purpose_ids: vec!["purpose".into()],
                account_ids: vec!["manual".into()],
            },
            &journal_caller(),
        )
        .await
        .unwrap()
}
fn cash<'a>(read: &'a InvestmentContextCapture, currency: &str) -> &'a Value {
    read.response.funding.data["accountValuations"]["accounts"][0]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "CASH" && row["currency"] == currency)
        .unwrap()
}

#[tokio::test]
async fn old_valid_fx_dates_survive_all_detail_blocks_without_turning_values_into_unknown() {
    let h = setup().await;
    let old = (Utc::now().date_naive() - Duration::days(100)).to_string();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("DELETE FROM quotes WHERE id='past-eur-cny'", [])
        .unwrap();
    db.execute(
        "UPDATE quotes SET day=?1,timestamp=?2 WHERE id='quote-eur-cny'",
        (&old, format!("{old}T00:00:00Z")),
    )
    .unwrap();
    let read = context(&h).await;
    assert_eq!(cash(&read, "EUR")["nativeAmount"], "100");
    assert_eq!(cash(&read, "EUR")["valueBase"], "700");
    assert_eq!(cash(&read, "EUR")["valuationDate"], h.day);
    assert_eq!(cash(&read, "EUR")["fxSourceDate"], old);
    assert_eq!(cash(&read, "EUR")["stale"], true);
    assert_eq!(
        read.response.funding.data["accountValuations"]["sourceDataAsOf"],
        old
    );
    assert_eq!(read.response.funding.data["risks"][0]["totalValue"], "350");
    assert_eq!(read.response.funding.data["risks"][0]["fxSourceDate"], old);
    assert_eq!(read.response.funding.data["risks"][0]["stale"], true);
    assert_eq!(
        read.response.funding.source_data_as_of.as_deref(),
        Some(old.as_str())
    );
    assert!(read.response.funding.stale);
    assert_eq!(
        read.response.recent_cash_flow.data["convertedOrdinary"]["income"],
        "70"
    );
    assert_eq!(
        read.response.recent_cash_flow.data["convertedOrdinary"]["fxSources"][0]["sourceDate"],
        old
    );
    assert_eq!(
        read.response.recent_cash_flow.data["convertedOrdinary"]["fxSources"][0]["activityDate"],
        h.day
    );
    assert_eq!(
        read.response.recent_cash_flow.source_data_as_of.as_deref(),
        Some(old.as_str())
    );
    assert!(read.response.recent_cash_flow.stale);
    assert_eq!(read.response.household.data["netWorth"], "2770");
    let foreign = read.response.household.data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "CASH:manual:EUR")
        .unwrap();
    assert_eq!(foreign["fxSourceDate"], old);
    assert_eq!(foreign["valuationDate"], h.day);
    assert_eq!(foreign["value"], "700");
    assert_eq!(
        read.response.household.source_data_as_of.as_deref(),
        Some(old.as_str())
    );
    assert!(read.response.household.stale);
}

#[tokio::test]
async fn original_fx_fallbacks_cannot_verify_foreign_totals_but_native_resources_survive() {
    let h = setup().await;
    let known = context(&h).await;
    assert_eq!(known.response.household.data["netWorth"], "2770");
    assert_eq!(cash(&known, "EUR")["valueBase"], "700");
    assert_eq!(known.response.funding.data["risks"][0]["totalValue"], "350");
    assert_eq!(
        known.response.recent_cash_flow.data["convertedOrdinary"]["income"],
        "70"
    );
    let db = h.access.connect_rusqlite().unwrap();
    let valid_day = Utc::now().date_naive().to_string();
    for (price, timestamp, day, currency) in [
        (
            "malformed",
            format!("{valid_day}T00:00:00Z"),
            valid_day.clone(),
            "EUR",
        ),
        (
            "0",
            format!("{valid_day}T00:00:00Z"),
            valid_day.clone(),
            "EUR",
        ),
        (
            "-1",
            format!("{valid_day}T00:00:00Z"),
            valid_day.clone(),
            "EUR",
        ),
        (
            "0.12345678901234567890123456789012345",
            format!("{valid_day}T00:00:00Z"),
            valid_day.clone(),
            "EUR",
        ),
        ("7", "bad-timestamp".into(), valid_day.clone(), "EUR"),
        (
            "7",
            format!("{valid_day}T00:00:00Z"),
            "2000-01-01".into(),
            "EUR",
        ),
        (
            "7",
            format!("{valid_day}T00:00:00Z"),
            valid_day.clone(),
            "JPY",
        ),
    ] {
        db.execute(
            "UPDATE quotes SET close=?1,timestamp=?2,day=?3,currency=?4 WHERE id='quote-eur-cny'",
            (price, timestamp, day, currency),
        )
        .unwrap();
        let read = context(&h).await;
        assert_eq!(
            cash(&read, "EUR")["valueBase"],
            Value::Null,
            "accepted bad FX source {price}"
        );
        assert_eq!(cash(&read, "EUR")["nativeAmount"], "100");
        assert_eq!(cash(&read, "CNY")["valueBase"], "1000");
        assert_eq!(
            read.response.funding.data["accountValuations"]["cashBalanceBase"],
            Value::Null
        );
        assert_eq!(
            read.response.funding.data["risks"][0]["totalValue"],
            Value::Null
        );
        assert_eq!(
            read.response.funding.data["risks"][0]["minimumCashStatus"],
            "UNABLE_TO_DETERMINE"
        );
        assert_eq!(
            read.response.recent_cash_flow.data["convertedOrdinary"]["income"],
            Value::Null
        );
        assert_eq!(
            read.response.recent_cash_flow.data["ordinary"]["currencies"][0]["income"],
            "10"
        );
        assert_eq!(read.response.household.data["netWorth"], Value::Null);
    }
    let saved = h.journal().with_context_reader(h.context.clone()).save_decision_draft(SaveDecisionDraft {
        decision_id:None, draft_revision_id:None, expected_version:None,
        body:serde_json::from_value(serde_json::json!({"title":"Synthetic partial thought survives unavailable FX"})).unwrap(),
        check_schedule:CheckScheduleChange::Keep, request_key:"partial-after-invalid-fx".into(),
    }, &journal_caller()).await.unwrap();
    assert!(saved["decisionId"].is_string());
    db.execute(
        "UPDATE quotes SET close='7',timestamp=?1,day=?2,currency='CNY' WHERE id='quote-eur-cny'",
        (format!("{valid_day}T00:00:00Z"), &valid_day),
    )
    .unwrap();
    assert_eq!(
        cash(&context(&h).await, "EUR")["valueBase"],
        "700",
        "provider quote-currency form also verifies"
    );
}

#[tokio::test]
async fn original_rows_are_checked_on_native_multi_hop_and_inverse_paths_only() {
    let h = Harness::new().await;
    let day = Utc::now().date_naive();
    seed_rate(&h, "eur-usd", "EUR", "USD", "2", &day.to_string());
    seed_rate(&h, "usd-cny", "USD", "CNY", "3", &day.to_string());
    seed_rate(
        &h,
        "bad-branch",
        "EUR",
        "AUD",
        "malformed",
        &day.to_string(),
    );
    let fx = fx(&h);
    fx.initialize().unwrap();
    assert_eq!(
        fx.get_exchange_rate_for_date("EUR", "CNY", day)
            .unwrap()
            .to_string(),
        "6"
    );
    assert!(fx.is_exchange_rate_verified("EUR", "CNY", Some(day)));
    assert!(fx.is_exchange_rate_verified("CNY", "EUR", Some(day)));
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE quotes SET close='3.000000000000000000000000000001' WHERE id='quote-usd-cny'",
        [],
    )
    .unwrap();
    fx.initialize().unwrap();
    assert!(!fx.is_exchange_rate_verified("EUR", "CNY", Some(day)));
    assert!(!fx.is_exchange_rate_verified("CNY", "EUR", Some(day)));
    assert!(
        fx.is_exchange_rate_verified("EUR", "USD", Some(day)),
        "an unrelated invalid source cannot poison a verified route"
    );
    assert!(
        fx.is_exchange_rate_verified("GBp", "GBP", Some(day)),
        "native deterministic minor-unit identity needs no market quote"
    );
}

#[tokio::test]
async fn future_nearest_rate_and_legacy_zero_do_not_become_verified_at_an_earlier_cutoff() {
    let h = Harness::new().await;
    let day = Utc::now().date_naive();
    seed_rate(
        &h,
        "eur-cny",
        "EUR",
        "CNY",
        "7",
        &(day + Duration::days(2)).to_string(),
    );
    let fx = fx(&h);
    fx.initialize().unwrap();
    assert_eq!(
        fx.get_exchange_rate_for_date("EUR", "CNY", day)
            .unwrap()
            .to_string(),
        "7",
        "legacy nearest policy remains available"
    );
    assert!(!fx.is_exchange_rate_verified("EUR", "CNY", Some(day)));
    assert!(fx.is_exchange_rate_verified("EUR", "CNY", Some(day + Duration::days(2))));
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE quotes SET close='malformed' WHERE id='quote-eur-cny'",
        [],
    )
    .unwrap();
    fx.initialize().unwrap();
    assert_eq!(
        fx.get_exchange_rate_for_date("EUR", "CNY", day)
            .unwrap()
            .to_string(),
        "0"
    );
    assert!(!fx.is_exchange_rate_verified("EUR", "CNY", Some(day + Duration::days(2))));
    assert!(fx.is_exchange_rate_verified("CNY", "CNY", Some(day)));
}
