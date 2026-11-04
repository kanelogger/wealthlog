#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
use wealthfolio_business::{investment_journal::*, BusinessChannel};
#[path = "support/investment_harness.rs"]
mod support;
use support::*;

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
fn desktop() -> JournalCaller {
    let mut c = journal_caller();
    c.channel = BusinessChannel::Gui;
    c.actor_kind = JournalActorKind::GuiSession;
    c.fact_permissions.activities_write = true;
    c
}
fn count(h: &Harness, table: &str) -> i64 {
    h.access
        .connect_rusqlite()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
async fn recorded_trade(h: &Harness, j: &InvestmentJournalService) -> serde_json::Value {
    h.seed_purpose("a");
    let c = desktop();
    let p = j.preview_allocations(vec![cash()], &c).await.unwrap();
    j.apply_allocation(
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Allocate,
            reason: "Opening ownership".into(),
            entries: vec![AllocationDelta {
                purpose_id: "a".into(),
                resource: cash(),
                delta: exact("600"),
            }],
            resources: vec![cash()],
            expected_preview_hash: p.preview_hash,
            request_key: "opening-purpose".into(),
        },
        &c,
    )
    .await
    .unwrap();
    let trade = RecordLedgerTrade {
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
        notes: None,
        idempotency_key: None,
        request_key: "trade".into(),
    };
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
            reason: "Atomic source allocation".into(),
            expected_preview_hash: p.preview_hash,
        },
        &c,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn reversal_restores_ownership_without_reversing_trade_and_only_applies_once() {
    let h = Harness::new().await;
    let j = h.journal();
    let trade = recorded_trade(&h, &j).await;
    let id = trade["allocationOperationId"].as_str().unwrap();
    let native = h.investments.get_state("transactions").await.unwrap();
    let mut c = journal_caller();
    c.fact_permissions = JournalFactPermissions::default();
    let p = j.preview_fund_reversal(id, &c).await.unwrap();
    assert_eq!(p.entries.len(), 2);
    let command = ReverseFundOperation {
        operation_id: id.into(),
        reason: "Undo ownership only".into(),
        expected_preview_hash: p.preview_hash,
        request_key: "reverse".into(),
    };
    assert_eq!(
        j.reverse_fund_operation(command.clone(), &c).await.unwrap()["factsChanged"],
        false
    );
    assert_eq!(
        j.reverse_fund_operation(command, &c).await.unwrap()["replayed"],
        true
    );
    assert_eq!(
        h.investments.get_state("transactions").await.unwrap(),
        native
    );
    let current = j
        .preview_allocations(vec![cash(), stock()], &c)
        .await
        .unwrap();
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == cash())
            .unwrap()
            .balance
            .allocated,
        exact("600")
    );
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == stock())
            .unwrap()
            .balance
            .allocated,
        exact("0")
    );
    assert!(matches!(
        j.preview_fund_reversal(id, &c).await,
        Err(JournalError::AlreadyApplied)
    ));
    assert_eq!(count(&h, "activities"), 2);
    assert_eq!(count(&h, "investment_executions"), 1);
    j.rebuild_allocations(&c).await.unwrap();
    assert!(j
        .fund_reconciliation(vec![cash(), stock()], &c)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn correction_freezes_source_versions_and_acknowledges_with_its_adjustment() {
    let h = Harness::new().await;
    let j = h.journal();
    let c = journal_caller();
    let trade = recorded_trade(&h, &j).await;
    let id = trade["allocationOperationId"].as_str().unwrap();
    let activity = trade["activityId"].as_str().unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE activities SET amount='102.5',unit_price='10.1' WHERE id=?1",
        [activity],
    )
    .unwrap();
    let native = h.investments.get_state("transactions").await.unwrap();
    let entries = vec![AllocationDelta {
        purpose_id: "a".into(),
        resource: cash(),
        delta: exact("-1"),
    }];
    let p = j
        .preview_fund_correction(id, entries.clone(), &c)
        .await
        .unwrap();
    assert!(!p.pending_reconciliation.is_empty());
    let command = CorrectFundOperation {
        operation_id: id.into(),
        entries,
        reason: "Align with amended final cash".into(),
        expected_preview_hash: p.preview_hash,
        request_key: "correct".into(),
    };
    j.correct_fund_operation(command.clone(), &c).await.unwrap();
    assert_eq!(
        j.correct_fund_operation(command, &c).await.unwrap()["replayed"],
        true
    );
    assert_eq!(
        h.investments.get_state("transactions").await.unwrap(),
        native
    );
    assert!(j
        .fund_reconciliation(vec![cash(), stock()], &c)
        .await
        .unwrap()
        .is_empty());
    let current = j.preview_allocations(vec![cash()], &c).await.unwrap();
    assert_eq!(current.resources[0].balance.actual, Some(exact("897.5")));
    assert_eq!(current.resources[0].balance.allocated, exact("497.5"));
    assert_eq!(current.resources[0].balance.unallocated, Some(exact("400")));
    db.execute(
        "UPDATE activities SET amount='103.5',unit_price='10.2' WHERE id=?1",
        [activity],
    )
    .unwrap();
    assert!(j
        .fund_reconciliation(vec![cash(), stock()], &c)
        .await
        .unwrap()
        .iter()
        .any(|i| i.operation_id.is_some()));
}

#[tokio::test]
async fn stale_correction_and_failed_reversal_audit_cannot_partially_change_balances() {
    let h = Harness::new().await;
    let j = h.journal();
    let c = journal_caller();
    let trade = recorded_trade(&h, &j).await;
    let id = trade["allocationOperationId"].as_str().unwrap();
    let activity = trade["activityId"].as_str().unwrap();
    let entries = vec![AllocationDelta {
        purpose_id: "a".into(),
        resource: cash(),
        delta: exact("-1"),
    }];
    let p = j
        .preview_fund_correction(id, entries.clone(), &c)
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE activities SET amount='102.5',unit_price='10.1' WHERE id=?1",
        [activity],
    )
    .unwrap();
    let operations = count(&h, "fund_allocation_operations");
    assert!(matches!(
        j.correct_fund_operation(
            CorrectFundOperation {
                operation_id: id.into(),
                entries,
                reason: "Stale source".into(),
                expected_preview_hash: p.preview_hash,
                request_key: "stale".into()
            },
            &c
        )
        .await,
        Err(JournalError::StateConflict)
    ));
    assert_eq!(count(&h, "fund_allocation_operations"), operations);
    let p = j.preview_fund_reversal(id, &c).await.unwrap();
    let command = ReverseFundOperation {
        operation_id: id.into(),
        reason: "Atomic reversal".into(),
        expected_preview_hash: p.preview_hash,
        request_key: "atomic-reverse".into(),
    };
    let entries = count(&h, "fund_allocation_entries");
    db.execute_batch("CREATE TRIGGER reject_reversal_audit BEFORE INSERT ON business_audit WHEN NEW.operation='investment_fund_reverse' BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;").unwrap();
    assert!(j.reverse_fund_operation(command.clone(), &c).await.is_err());
    assert_eq!(count(&h, "fund_allocation_entries"), entries);
    assert_eq!(count(&h, "fund_allocation_operations"), operations);
    db.execute("DROP TRIGGER reject_reversal_audit", [])
        .unwrap();
    j.reverse_fund_operation(command, &c).await.unwrap();
}
