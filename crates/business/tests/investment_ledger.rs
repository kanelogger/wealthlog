#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use wealthfolio_business::{investment_journal::*, BusinessChannel};
#[path = "support/investment_harness.rs"]
mod support;
use support::*;

fn caller() -> JournalCaller {
    let mut caller = journal_caller();
    caller.channel = BusinessChannel::Gui;
    caller.actor_kind = JournalActorKind::GuiSession;
    caller.actor_id = "desktop-session".into();
    caller.credential_id = None;
    caller.fact_permissions.activities_write = true;
    caller
}
fn cash() -> FundResource {
    FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    }
}
fn stock() -> FundResource {
    FundResource::Security {
        account_id: "transactions".into(),
        asset_id: "stock".into(),
    }
}
fn trade(h: &Harness, key: &str) -> RecordLedgerTrade {
    RecordLedgerTrade {
        account_id: "transactions".into(),
        asset_id: "stock".into(),
        side: "BUY".into(),
        activity_date: h.day.clone(),
        quantity: exact("10"),
        unit_price: Some(exact("10")),
        amount: Some(exact("101.5")),
        fee: Some(exact("1")),
        tax: Some(exact("0.5")),
        currency: "CNY".into(),
        fx_rate: None,
        notes: Some("Completed stock purchase".into()),
        idempotency_key: None,
        request_key: key.into(),
    }
}
fn share(quantity: &str, amount: &str) -> Vec<TradePurposeShare> {
    vec![TradePurposeShare {
        purpose_id: "a".into(),
        quantity: exact(quantity),
        net_cash: exact(amount),
        top_ups: vec![OwnershipTopUp {
            resource: cash(),
            amount: ExactDecimal::new(exact(amount).value().abs()),
        }],
    }]
}
fn count(h: &Harness, table: &str) -> i64 {
    h.access
        .connect_rusqlite()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn ledger_trade_uses_final_cash_once_and_replay_never_reapplies_ownership() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let caller = caller();
    let trade = trade(&h, "ledger");
    let shares = share("10", "-101.5");
    let preview = journal
        .preview_ledger_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    let refreshed = journal
        .preview_ledger_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(preview.preview_hash, refreshed.preview_hash);
    let input = RecordLedgerTradeWithOwnership {
        trade,
        shares,
        reason: "Known broker final cash".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let result = journal
        .record_ledger_trade_with_ownership(input.clone(), &caller)
        .await
        .unwrap();
    let replay = journal
        .record_ledger_trade_with_ownership(input, &caller)
        .await
        .unwrap();
    assert_eq!(result["activityId"], replay["activityId"]);
    assert_eq!(replay["replayed"], true);
    let current = journal
        .preview_allocations(vec![cash(), stock()], &caller)
        .await
        .unwrap();
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == cash())
            .unwrap()
            .balance
            .actual,
        Some(exact("898.5"))
    );
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == stock())
            .unwrap()
            .balance
            .allocated,
        exact("10")
    );
    assert!(journal
        .fund_reconciliation(vec![cash(), stock()], &caller)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(count(&h, "activities"), 2);
    assert_eq!(count(&h, "fund_allocation_entries"), 3);
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    let rows = projector.executions().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].time_precision, "DATE");
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE investment_projection_state SET processed_generation=0 WHERE source_id=?1",
            [result["activityId"].as_str().unwrap()],
        )
        .unwrap();
    projector.refresh_batch(100).await.unwrap();
    assert_eq!(projector.executions().unwrap()[0].time_precision, "DATE");
    assert_eq!(
        count(&h, "fund_allocation_entries"),
        3,
        "reprojection cannot convert twice"
    );
}

#[tokio::test]
async fn ledger_previews_are_read_only_for_manual_quotes_and_fx_registration() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE assets SET quote_mode='MANUAL',quote_ccy='USD' WHERE id='stock'",
            [],
        )
        .unwrap();
    let journal = h.journal();
    let caller = caller();
    let mut trade = trade(&h, "foreign");
    trade.currency = "USD".into();
    trade.fx_rate = Some(exact("7.2"));
    let assets_before = count(&h, "assets");
    let quotes_before = count(&h, "quotes");
    let shares = share("10", "-730.8");
    let preview = journal
        .preview_ledger_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(count(&h, "assets"), assets_before);
    assert_eq!(count(&h, "quotes"), quotes_before);
    assert_eq!(count(&h, "activities"), 1);
    assert_eq!(count(&h, "business_requests"), 0);
    assert_eq!(preview.ownership.charges[0].currency, "USD");
    journal
        .record_ledger_trade_with_ownership(
            RecordLedgerTradeWithOwnership {
                trade,
                shares,
                reason: "Use the supplied broker settlement rate once".into(),
                expected_preview_hash: preview.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let current = journal
        .preview_allocations(vec![cash()], &caller)
        .await
        .unwrap();
    assert_eq!(current.resources[0].balance.actual, Some(exact("269.2")));
    assert_eq!(
        count(&h, "quotes"),
        quotes_before,
        "recording a cost input does not silently create a market observation"
    );
}

#[tokio::test]
async fn ledger_audit_failure_rolls_back_activity_projection_ownership_and_enablement() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let caller = caller();
    let trade = trade(&h, "rollback");
    let shares = share("10", "-101.5");
    let preview = journal
        .preview_ledger_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    let input = RecordLedgerTradeWithOwnership {
        trade,
        shares,
        reason: "Atomic ledger mutation".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER reject_ledger_audit BEFORE INSERT ON business_audit WHEN NEW.operation='investment_ledger_trade_record' BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;").unwrap();
    assert!(journal
        .record_ledger_trade_with_ownership(input.clone(), &caller)
        .await
        .is_err());
    assert_eq!(count(&h, "activities"), 1);
    for table in [
        "investment_executions",
        "fund_allocation_operations",
        "fund_allocation_entries",
        "business_requests",
    ] {
        assert_eq!(count(&h, table), 0, "{table} did not roll back");
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute("DROP TRIGGER reject_ledger_audit", []).unwrap();
    journal
        .record_ledger_trade_with_ownership(input, &caller)
        .await
        .unwrap();
}

#[tokio::test]
async fn reliable_ledger_net_cash_allows_unknown_charges_and_cost_reordering() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let today: chrono::NaiveDate = h.day.parse().unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('old-buy','transactions','stock','BUY','POSTED',?1,'10','10','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[(today-chrono::Duration::days(3)).to_string()]).unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('sell','transactions','stock','SELL','POSTED',?1,'5','20','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let journal = h.journal();
    let caller = caller();
    let mut trade = trade(&h, "backdated");
    trade.activity_date = (today - chrono::Duration::days(4)).to_string();
    trade.quantity = exact("5");
    trade.unit_price = Some(exact("20"));
    trade.amount = Some(exact("100"));
    trade.fee = None;
    trade.tax = None;
    let shares = share("5", "-100");
    let preview = journal
        .preview_ledger_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    assert!(preview.trade.automatic_allocation_eligible);
    assert!(preview.ownership.charges[0].fee.is_none());
    journal
        .record_ledger_trade_with_ownership(
            RecordLedgerTradeWithOwnership {
                trade,
                shares,
                reason: "Backfill changes costs while cash and units match".into(),
                expected_preview_hash: preview.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let state = journal
        .preview_allocations(vec![cash(), stock()], &caller)
        .await
        .unwrap();
    assert_eq!(
        state
            .resources
            .iter()
            .find(|r| r.observation.resource == cash())
            .unwrap()
            .balance
            .actual,
        Some(exact("900"))
    );
    assert_eq!(
        state
            .resources
            .iter()
            .find(|r| r.observation.resource == stock())
            .unwrap()
            .balance
            .actual,
        Some(exact("10"))
    );
}

#[tokio::test]
async fn ledger_write_permission_cannot_be_forged_in_arguments() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let allowed = caller();
    let trade = trade(&h, "permission");
    let shares = share("10", "-101.5");
    let preview = journal
        .preview_ledger_trade_ownership(&trade, shares.clone(), &allowed)
        .await
        .unwrap();
    let input = RecordLedgerTradeWithOwnership {
        trade,
        shares,
        reason: "Permission test".into(),
        expected_preview_hash: preview.preview_hash,
    };
    assert!(matches!(
        journal
            .record_ledger_trade_with_ownership(input.clone(), &journal_caller())
            .await,
        Err(JournalError::PermissionDenied)
    ));
    let mut body = serde_json::to_value(input).unwrap();
    body["channel"] = serde_json::json!("gui");
    assert!(serde_json::from_value::<RecordLedgerTradeWithOwnership>(body).is_err());
    assert_eq!(count(&h, "activities"), 1);
}

#[tokio::test]
async fn conserving_trade_preserves_an_existing_excess_without_hiding_or_increasing_it() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let j = h.journal();
    let c = caller();
    let p = j.preview_allocations(vec![cash()], &c).await.unwrap();
    j.apply_allocation(
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Allocate,
            reason: "Opening cash ownership".into(),
            entries: vec![AllocationDelta {
                purpose_id: "a".into(),
                resource: cash(),
                delta: exact("600"),
            }],
            resources: vec![cash()],
            expected_preview_hash: p.preview_hash,
            request_key: "opening".into(),
        },
        &c,
    )
    .await
    .unwrap();
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('expense','transactions','WITHDRAWAL','POSTED',?1,'700','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let trade = trade(&h, "existing-excess");
    let shares = vec![TradePurposeShare {
        purpose_id: "a".into(),
        quantity: exact("10"),
        net_cash: exact("-101.5"),
        top_ups: vec![],
    }];
    let p = j
        .preview_ledger_trade_ownership(&trade, shares.clone(), &c)
        .await
        .unwrap();
    j.record_ledger_trade_with_ownership(
        RecordLedgerTradeWithOwnership {
            trade,
            shares,
            reason: "Record matching cash/units; leave earlier excess unresolved".into(),
            expected_preview_hash: p.preview_hash,
        },
        &c,
    )
    .await
    .unwrap();
    let current = j.preview_allocations(vec![cash()], &c).await.unwrap();
    assert_eq!(current.resources[0].balance.excess, Some(exact("300")));
    assert!(
        j.fund_reconciliation(vec![cash(), stock()], &c)
            .await
            .unwrap()
            .iter()
            .any(|i| i.kind == FundReconciliationKind::PurposeExcess
                && i.excess == Some(exact("300")))
    );
}

#[tokio::test]
async fn incoming_units_can_preserve_old_security_excess_but_cannot_turn_debt_into_allocatable_cash(
) {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let j = h.journal();
    let c = caller();
    let yesterday =
        (h.day.parse::<chrono::NaiveDate>().unwrap() - chrono::Duration::days(1)).to_string();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('old-buy','transactions','stock','BUY','POSTED',?1,'20','10','200','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[yesterday]).unwrap();
    let p = j
        .preview_allocations(vec![cash(), stock()], &c)
        .await
        .unwrap();
    j.apply_allocation(
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Allocate,
            reason: "Original purpose resources".into(),
            entries: vec![
                AllocationDelta {
                    purpose_id: "a".into(),
                    resource: cash(),
                    delta: exact("600"),
                },
                AllocationDelta {
                    purpose_id: "a".into(),
                    resource: stock(),
                    delta: exact("20"),
                },
            ],
            resources: vec![cash(), stock()],
            expected_preview_hash: p.preview_hash,
            request_key: "original".into(),
        },
        &c,
    )
    .await
    .unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('unallocated-sale','transactions','stock','SELL','POSTED',?1,'10','10','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let mut input = trade(&h, "new-buy");
    input.quantity = exact("5");
    input.amount = Some(exact("50"));
    input.fee = Some(exact("0"));
    input.tax = Some(exact("0"));
    let shares = vec![TradePurposeShare {
        purpose_id: "a".into(),
        quantity: exact("5"),
        net_cash: exact("-50"),
        top_ups: vec![],
    }];
    let p = j
        .preview_ledger_trade_ownership(&input, shares.clone(), &c)
        .await
        .unwrap();
    j.record_ledger_trade_with_ownership(
        RecordLedgerTradeWithOwnership {
            trade: input,
            shares,
            reason: "Preserve old unresolved units".into(),
            expected_preview_hash: p.preview_hash,
        },
        &c,
    )
    .await
    .unwrap();
    let current = j.preview_allocations(vec![stock()], &c).await.unwrap();
    assert_eq!(current.resources[0].balance.excess, Some(exact("10")));
    let h = Harness::new().await;
    h.seed_purpose("a");
    let j = h.journal();
    let c = caller();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('old-buy','transactions','stock','BUY','POSTED',?1,'10','10','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let p = j.preview_allocations(vec![stock()], &c).await.unwrap();
    j.apply_allocation(
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Allocate,
            reason: "Observed shares".into(),
            entries: vec![AllocationDelta {
                purpose_id: "a".into(),
                resource: stock(),
                delta: exact("10"),
            }],
            resources: vec![stock()],
            expected_preview_hash: p.preview_hash,
            request_key: "shares".into(),
        },
        &c,
    )
    .await
    .unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('withdrawal','transactions','WITHDRAWAL','POSTED',?1,'910','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let mut input = trade(&h, "sale");
    input.side = "SELL".into();
    input.amount = Some(exact("100"));
    input.fee = Some(exact("0"));
    input.tax = Some(exact("0"));
    let shares = vec![TradePurposeShare {
        purpose_id: "a".into(),
        quantity: exact("10"),
        net_cash: exact("100"),
        top_ups: vec![],
    }];
    assert!(matches!(
        j.preview_ledger_trade_ownership(&input, shares, &c).await,
        Err(JournalError::InsufficientResource)
    ));
}
