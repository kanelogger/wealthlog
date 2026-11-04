//! Actual SQLite originals, native quote service and detailed financial consumers.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::{json, Value};
use support::{journal_caller, Harness};
use wealthfolio_business::{investment_journal::*, BusinessChannel};
use wealthfolio_core::portfolio::net_worth::NetWorthServiceTrait;

async fn setup() -> Harness {
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
    db.execute_batch("DELETE FROM quotes WHERE asset_id='stock';")
        .unwrap();
    db.execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('quality-quote','stock',?1,'MANUAL','12','CNY',CURRENT_TIMESTAMP,?2)",(&h.day,format!("{}T00:00:00Z",h.day))).unwrap();
    db.execute("INSERT INTO holdings_snapshots(id,account_id,snapshot_date,currency,positions,cash_balances,cost_basis,net_contribution,calculated_at,net_contribution_base,cash_total_account_currency,cash_total_base_currency,source) VALUES('tx-current','transactions',?1,'CNY','{}','{\"CNY\":\"1000\"}','0','1000','2000-01-01T00:00:00','1000','1000','1000','CALCULATED')",[&h.day]).unwrap();
    h.seed_purpose("purpose");
    db.execute(
        "UPDATE projection_state SET dirty_from=NULL WHERE scope IN ('@all','transactions')",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE fund_purposes SET single_asset_limit_bps=5000 WHERE id='purpose'",
        [],
    )
    .unwrap();
    let service = h.journal();
    let c = journal_caller();
    let resource = FundResource::Security {
        account_id: "manual".into(),
        asset_id: "stock".into(),
    };
    let p = service
        .preview_allocations(vec![resource.clone()], &c)
        .await
        .unwrap();
    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Explicit synthetic ownership".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "purpose".into(),
                    resource: resource.clone(),
                    delta: ExactDecimal::parse("5").unwrap(),
                }],
                resources: vec![resource],
                expected_preview_hash: p.preview_hash,
                request_key: "assign".into(),
            },
            &c,
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
fn position(context: &InvestmentContextCapture) -> &Value {
    context.response.funding.data["accountValuations"]["accounts"][0]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "SECURITY")
        .unwrap()
}

#[tokio::test]
async fn malformed_original_prices_never_become_verified_zero_in_any_financial_block() {
    let h = setup().await;
    let db = h.access.connect_rusqlite().unwrap();
    for raw in ["malformed", "-1", "0.12345678901234567890123456789012345"] {
        db.execute("UPDATE quotes SET close=?1 WHERE id='quality-quote'", [raw])
            .unwrap();
        let read = context(&h).await;
        assert_eq!(position(&read)["unitValue"], Value::Null);
        assert_eq!(position(&read)["valueBase"], Value::Null);
        assert_eq!(
            read.response.funding.data["accountValuations"]["investmentMarketValueBase"],
            Value::Null
        );
        let stock = read.response.household.data["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "stock")
            .unwrap();
        assert_eq!(stock["value"], Value::Null);
        assert_eq!(stock["substituteValue"], Value::Null);
        assert_eq!(stock["valuationDate"], Value::Null);
        assert!(stock["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "INVALID_QUOTE_SOURCE"));
        assert_eq!(read.response.household.data["financialAssets"], Value::Null);
        assert_eq!(
            read.response.funding.data["risks"][0]["singleAssetLimitStatus"],
            "UNABLE_TO_DETERMINE"
        );
        assert_eq!(
            read.response.funding.data["risks"][0]["totalValue"],
            Value::Null
        );
        let allocation = read.response.funding.data["allocation"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["observation"]["resource"]["kind"] == "SECURITY")
            .unwrap();
        assert_eq!(allocation["balance"]["actual"], "10");
    }
}

#[tokio::test]
async fn invalid_source_times_do_not_use_the_refresh_clock_as_an_asset_date() {
    let h = setup().await;
    let db = h.access.connect_rusqlite().unwrap();
    for time in ["bad-timestamp", "2000-01-01T00:00:00Z"] {
        db.execute(
            "UPDATE quotes SET timestamp=?1 WHERE id='quality-quote'",
            [time],
        )
        .unwrap();
        let read = context(&h).await;
        assert_eq!(position(&read)["valuationDate"], Value::Null);
        assert_eq!(position(&read)["unitValue"], Value::Null);
        assert_eq!(read.response.household.data["netWorth"], Value::Null);
        assert_eq!(
            read.response.funding.data["risks"][0]["singleAssetLimitStatus"],
            "UNABLE_TO_DETERMINE"
        );
    }
}

#[tokio::test]
async fn explicit_verified_zero_is_distinct_from_legacy_fallback_zero_and_cash_stays_known() {
    let h = setup().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("UPDATE quotes SET close='0' WHERE id='quality-quote'", [])
        .unwrap();
    let good = context(&h).await;
    assert_eq!(position(&good)["unitValue"], "0");
    assert_eq!(position(&good)["valueBase"], "0");
    assert_eq!(good.response.funding.data["risks"][0]["totalValue"], "0");
    assert_eq!(
        good.response.funding.data["risks"][0]["quality"],
        "COMPLETE"
    );
    assert_eq!(
        good.response.funding.data["risks"][0]["singleAssetLimitStatus"], "UNABLE_TO_DETERMINE",
        "zero denominator is not a concentration pass"
    );
    let day = chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap();
    let legacy_zero = h.net_worth.get_net_worth(day).await.unwrap();
    db.execute("UPDATE quotes SET close='bad' WHERE id='quality-quote'", [])
        .unwrap();
    let bad = context(&h).await;
    assert_eq!(position(&bad)["unitValue"], Value::Null);
    assert!(bad.response.household.data["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["category"] == "cash" && item["value"].is_string()));
    let legacy_bad = h.net_worth.get_net_worth(day).await.unwrap();
    assert_eq!(
        legacy_bad.assets.total, legacy_zero.assets.total,
        "legacy numeric fallback contract remains compatible"
    );
    assert_eq!(bad.response.household.data["netWorth"], Value::Null);
    assert_eq!(
        good.response.household.data["netWorth"],
        json!(legacy_zero.net_worth.to_string())
    );
}

#[tokio::test]
async fn native_valuation_overflow_is_contained_and_does_not_block_other_blocks_or_drafts() {
    let h = setup().await;
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE quotes SET close='79228162514264337593543950335' WHERE id='quality-quote'",
            [],
        )
        .unwrap();
    let read = context(&h).await;
    assert_eq!(read.response.household.quality, ContextQuality::Unavailable);
    assert_eq!(
        read.response.recent_cash_flow.quality,
        ContextQuality::Complete
    );
    assert!(read
        .response
        .funding
        .issues
        .iter()
        .any(|issue| issue.code == "ACCOUNT_VALUATION_UNAVAILABLE"));
    assert_eq!(
        read.response.funding.data["risks"][0]["totalValue"],
        Value::Null
    );
    assert_eq!(
        read.response.funding.data["risks"][0]["singleAssetLimitStatus"],
        "UNABLE_TO_DETERMINE"
    );
    let body: DecisionBody =
        serde_json::from_value(json!({"title":"Keep partial thinking"})).unwrap();
    h.journal()
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "draft-during-overflow".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn tiny_minor_unit_prices_do_not_round_to_a_verified_zero() {
    let h = setup().await;
    h.access.connect_rusqlite().unwrap().execute("UPDATE quotes SET close='0.0000000000000000000000000001',currency='GBp' WHERE id='quality-quote'",[]).unwrap();
    let read = context(&h).await;
    assert_eq!(position(&read)["unitValue"], Value::Null);
    assert!(position(&read)["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["code"] == "ARITHMETIC_OVERFLOW"));
    assert_eq!(
        read.response.funding.data["risks"][0]["totalValue"],
        Value::Null
    );
}
