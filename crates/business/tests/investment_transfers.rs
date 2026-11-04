#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::collections::BTreeMap;
use wealthfolio_business::{investment_journal::*, investments::*, BusinessChannel};
#[path = "support/investment_harness.rs"]
mod support;
use support::*;

fn source_cash() -> FundResource {
    FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    }
}
fn destination_cash() -> FundResource {
    FundResource::Cash {
        account_id: "bank".into(),
        currency: "USD".into(),
    }
}
fn seed_bank(h: &Harness) {
    h.access.connect_rusqlite().unwrap().execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('bank','Bank','CASH','USD',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');").unwrap();
}
fn transfer_input(h: &Harness, key: &str) -> CreateCashTransfer {
    CreateCashTransfer {
        from_account_id: "transactions".into(),
        to_account_id: "bank".into(),
        activity_date: h.day.clone(),
        source_currency: "CNY".into(),
        destination_currency: "USD".into(),
        source_amount: "300".into(),
        destination_amount: Some("41.00".into()),
        notes: Some("Already completed bank transfer".into()),
        request_key: key.into(),
        idempotency_key: None,
    }
}
fn shares() -> Vec<TransferPurposeShare> {
    vec![
        TransferPurposeShare {
            purpose_id: "a".into(),
            source_amount: exact("100"),
        },
        TransferPurposeShare {
            purpose_id: "b".into(),
            source_amount: exact("100"),
        },
    ]
}
async fn allocate(h: &Harness, journal: &InvestmentJournalService, id: &str, amount: &str) {
    h.seed_purpose(id);
    let caller = journal_caller();
    let preview = journal
        .preview_allocations(vec![source_cash()], &caller)
        .await
        .unwrap();
    journal
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Observed opening ownership".into(),
                entries: vec![AllocationDelta {
                    purpose_id: id.into(),
                    resource: source_cash(),
                    delta: exact(amount),
                }],
                resources: vec![source_cash()],
                expected_preview_hash: preview.preview_hash,
                request_key: format!("allocate-{id}"),
            },
            &caller,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn new_cash_pair_and_partial_multi_purpose_migration_are_atomic_and_conserve_recorded_legs() {
    let h = Harness::new().await;
    seed_bank(&h);
    let journal = h.journal();
    let caller = journal_caller();
    allocate(&h, &journal, "a", "300").await;
    allocate(&h, &journal, "b", "200").await;
    let input = transfer_input(&h, "cash");
    let preview = journal
        .preview_new_cash_transfer_ownership(&input, shares(), &caller)
        .await
        .unwrap();
    let second = journal
        .preview_new_cash_transfer_ownership(&input, shares(), &caller)
        .await
        .unwrap();
    assert_eq!(
        preview.preview_hash, second.preview_hash,
        "preparation refresh times are not source changes"
    );
    assert_eq!(preview.shares[0].destination_amount, exact("13.67"));
    assert_eq!(preview.shares[1].destination_amount, exact("13.66"));
    let command = CreateCashTransferWithOwnership {
        transfer: input,
        shares: shares(),
        reason: "Migrate only the explicitly selected 200 CNY".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let result = journal
        .create_cash_transfer_with_ownership(command.clone(), &caller)
        .await
        .unwrap();
    let replay = journal
        .create_cash_transfer_with_ownership(command, &caller)
        .await
        .unwrap();
    assert_eq!(result["activityIds"], replay["activityIds"]);
    assert_eq!(replay["replayed"], true);
    let status = journal
        .preview_allocations(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap();
    let source = status
        .resources
        .iter()
        .find(|r| r.observation.resource == source_cash())
        .unwrap();
    let target = status
        .resources
        .iter()
        .find(|r| r.observation.resource == destination_cash())
        .unwrap();
    assert_eq!(source.balance.actual, Some(exact("700")));
    assert_eq!(source.balance.allocated, exact("300"));
    assert_eq!(target.balance.actual, Some(exact("41")));
    assert_eq!(target.balance.allocated, exact("27.33"));
    assert_eq!(
        target.balance.unallocated,
        Some(exact("13.67")),
        "unmigrated transfer money stays unallocated"
    );
    assert!(journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap()
        .is_empty());
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM activities WHERE source_group_id IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM fund_allocation_operations WHERE operation_kind='TRANSFER'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let out = result["transfer"]["outActivityId"].as_str().unwrap();
    assert!(matches!(
        journal
            .preview_existing_transfer_ownership(out, shares(), &caller)
            .await,
        Err(JournalError::AlreadyApplied)
    ));
}

#[tokio::test]
async fn existing_transfer_application_changes_only_ownership_and_locks_both_source_legs() {
    let h = Harness::new().await;
    seed_bank(&h);
    let journal = h.journal();
    let mut caller = journal_caller();
    allocate(&h, &journal, "a", "300").await;
    allocate(&h, &journal, "b", "200").await;
    let original = h
        .investments
        .create_transfer(
            transfer_input(&h, "original"),
            BusinessChannel::Cli,
            "token",
            true,
        )
        .await
        .unwrap();
    let out = original["activityIds"][0].as_str().unwrap();
    let incoming = original["activityIds"][1].as_str().unwrap();
    let before_out = h.investments.get_state("transactions").await.unwrap();
    let before_in = h.investments.get_state("bank").await.unwrap();
    caller.fact_permissions.transfers_write = false;
    let preview = journal
        .preview_existing_transfer_ownership(incoming, shares(), &caller)
        .await
        .unwrap();
    let command = ApplyExistingTransferOwnership {
        activity_id: incoming.into(),
        shares: shares(),
        reason: "Only migrate ownership of the posted transfer".into(),
        expected_preview_hash: preview.preview_hash,
        request_key: "apply-existing".into(),
    };
    journal
        .apply_existing_transfer_ownership(command.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(
        journal
            .apply_existing_transfer_ownership(command, &caller)
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(
        h.investments.get_state("transactions").await.unwrap(),
        before_out
    );
    assert_eq!(h.investments.get_state("bank").await.unwrap(), before_in);
    assert!(matches!(
        journal
            .preview_existing_transfer_ownership(out, shares(), &caller)
            .await,
        Err(JournalError::AlreadyApplied)
    ));
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO business_requests VALUES('second','ledger','token','test','second','h','{}','completed','{}',NULL,'now','now');").unwrap();
    let error=db.execute("INSERT INTO fund_allocation_operations(id,operation_kind,request_id,reason,source_kind,source_id,source_counterparty_id,source_version,first_application_key,observation_json,source_confirmation_json,detail_json,created_at,created_channel,created_actor_kind,created_actor_id) VALUES('second','TRANSFER','second','reused leg','TRANSFER','different-out',?1,'v','different-key','{\"schemaVersion\":1}','{\"schemaVersion\":1}','{\"schemaVersion\":1}','now','cli','PROGRAM','token')",[incoming]);
    assert!(
        error.is_err(),
        "database guard must reject reuse with a different counterpart or role"
    );
}

#[tokio::test]
async fn failed_transfer_audit_rolls_back_both_legs_and_ownership() {
    let h = Harness::new().await;
    seed_bank(&h);
    let journal = h.journal();
    let caller = journal_caller();
    allocate(&h, &journal, "a", "300").await;
    allocate(&h, &journal, "b", "200").await;
    let transfer = transfer_input(&h, "atomic");
    let preview = journal
        .preview_new_cash_transfer_ownership(&transfer, shares(), &caller)
        .await
        .unwrap();
    let input = CreateCashTransferWithOwnership {
        transfer,
        shares: shares(),
        reason: "Atomic transfer".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let before = h.investments.get_state("transactions").await.unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER reject_transfer_audit BEFORE INSERT ON business_audit WHEN NEW.operation='investment_cash_transfer_create' BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;").unwrap();
    assert!(journal
        .create_cash_transfer_with_ownership(input.clone(), &caller)
        .await
        .is_err());
    assert_eq!(
        h.investments.get_state("transactions").await.unwrap(),
        before
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM activities WHERE source_group_id IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM fund_allocation_operations WHERE operation_kind='TRANSFER'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute("DROP TRIGGER reject_transfer_audit", [])
        .unwrap();
    journal
        .create_cash_transfer_with_ownership(input, &caller)
        .await
        .unwrap();
}

#[tokio::test]
async fn partial_migration_keeps_remaining_excess_and_manual_included_cash_needs_reconciliation() {
    let h = Harness::new().await;
    seed_bank(&h);
    let journal = h.journal();
    let caller = journal_caller();
    allocate(&h, &journal, "a", "1000").await;
    let transfer = transfer_input(&h, "partial");
    let share = vec![TransferPurposeShare {
        purpose_id: "a".into(),
        source_amount: exact("100"),
    }];
    let preview = journal
        .preview_new_cash_transfer_ownership(&transfer, share.clone(), &caller)
        .await
        .unwrap();
    journal
        .create_cash_transfer_with_ownership(
            CreateCashTransferWithOwnership {
                transfer,
                shares: share,
                reason: "Explicit partial application".into(),
                expected_preview_hash: preview.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let items = journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap();
    assert!(
        items
            .iter()
            .any(|i| i.kind == FundReconciliationKind::PurposeExcess
                && i.excess == Some(exact("200")))
    );
    h.initialize_manual().await;
    let manual_transfer = CreateCashTransfer {
        from_account_id: "manual".into(),
        to_account_id: "bank".into(),
        source_amount: "100".into(),
        destination_amount: Some("10".into()),
        ..transfer_input(&h, "manual-cash")
    };
    assert!(matches!(
        journal
            .preview_new_cash_transfer_ownership(
                &manual_transfer,
                vec![TransferPurposeShare {
                    purpose_id: "a".into(),
                    source_amount: exact("100")
                }],
                &caller
            )
            .await,
        Err(JournalError::ReconciliationRequired)
    ));
    assert_eq!(
        h.investments.get_state("manual").await.unwrap()["cashBalances"],
        serde_json::json!(BTreeMap::from([("CNY", "1000")]))
    );
}

async fn reconcile(journal: &InvestmentJournalService, resources: Vec<FundResource>, key: &str) {
    let caller = journal_caller();
    let current = journal
        .preview_allocations(resources.clone(), &caller)
        .await
        .unwrap();
    journal
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Confirm only these current resource sources".into(),
                entries: vec![],
                resources,
                expected_preview_hash: current.preview_hash,
                request_key: key.into(),
            },
            &caller,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn referenced_pair_changes_survive_equal_totals_and_are_acknowledged_per_resource() {
    let h = Harness::new().await;
    seed_bank(&h);
    let journal = h.journal();
    let caller = journal_caller();
    allocate(&h, &journal, "a", "300").await;
    allocate(&h, &journal, "b", "200").await;
    let transfer = transfer_input(&h, "source-version");
    let preview = journal
        .preview_new_cash_transfer_ownership(&transfer, shares(), &caller)
        .await
        .unwrap();
    let result = journal
        .create_cash_transfer_with_ownership(
            CreateCashTransferWithOwnership {
                transfer,
                shares: shares(),
                reason: "Source-bound application".into(),
                expected_preview_hash: preview.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let out = result["transfer"]["outActivityId"].as_str().unwrap();
    let incoming = result["transfer"]["inActivityId"].as_str().unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("UPDATE activities SET amount='310' WHERE id=?1", [out])
        .unwrap();
    db.execute("UPDATE activities SET amount='42' WHERE id=?1", [incoming])
        .unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('offset-source','transactions','DEPOSIT','POSTED',?1,'10','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('offset-target','bank','WITHDRAWAL','POSTED',?1,'1','USD',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let current = journal
        .preview_allocations(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap();
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == source_cash())
            .unwrap()
            .balance
            .actual,
        Some(exact("700"))
    );
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == destination_cash())
            .unwrap()
            .balance
            .actual,
        Some(exact("41"))
    );
    let referenced = journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap()
        .into_iter()
        .filter(|i| i.operation_id.is_some() && i.kind == FundReconciliationKind::TransferChanged)
        .collect::<Vec<_>>();
    assert_eq!(
        referenced.len(),
        2,
        "equal balances cannot confirm changed pair amounts"
    );
    reconcile(&journal, vec![source_cash()], "source-ack").await;
    let referenced = journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap()
        .into_iter()
        .filter(|i| i.operation_id.is_some())
        .collect::<Vec<_>>();
    assert_eq!(referenced.len(), 1);
    assert_eq!(referenced[0].resource, destination_cash());
    reconcile(&journal, vec![destination_cash()], "target-ack").await;
    assert!(journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap()
        .is_empty());
    db.execute("DELETE FROM activities WHERE id=?1", [incoming])
        .unwrap();
    assert!(journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap()
        .iter()
        .any(|i| i.kind == FundReconciliationKind::TransferChanged));
    reconcile(
        &journal,
        vec![source_cash(), destination_cash()],
        "deleted-pair-ack",
    )
    .await;
    let remaining = journal
        .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].kind, FundReconciliationKind::PurposeExcess);
    db.execute("UPDATE activities SET amount='311' WHERE id=?1", [out])
        .unwrap();
    assert!(
        journal
            .fund_reconciliation(vec![source_cash(), destination_cash()], &caller)
            .await
            .unwrap()
            .iter()
            .any(|i| i.kind == FundReconciliationKind::TransferChanged),
        "a later source revision must re-enter after a prior acknowledgement"
    );
}

#[tokio::test]
async fn reversing_transfer_ownership_does_not_unlock_a_second_first_application() {
    let h = Harness::new().await;
    seed_bank(&h);
    let journal = h.journal();
    let caller = journal_caller();
    allocate(&h, &journal, "a", "300").await;
    allocate(&h, &journal, "b", "200").await;
    let transfer = transfer_input(&h, "reversed-source");
    let p = journal
        .preview_new_cash_transfer_ownership(&transfer, shares(), &caller)
        .await
        .unwrap();
    let result = journal
        .create_cash_transfer_with_ownership(
            CreateCashTransferWithOwnership {
                transfer,
                shares: shares(),
                reason: "Initial application".into(),
                expected_preview_hash: p.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let id = result["allocationOperationId"].as_str().unwrap();
    let p = journal.preview_fund_reversal(id, &caller).await.unwrap();
    journal
        .reverse_fund_operation(
            ReverseFundOperation {
                operation_id: id.into(),
                reason: "Ownership reversal only".into(),
                expected_preview_hash: p.preview_hash,
                request_key: "reverse-transfer".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    let current = journal
        .preview_allocations(vec![source_cash(), destination_cash()], &caller)
        .await
        .unwrap();
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == source_cash())
            .unwrap()
            .balance
            .actual,
        Some(exact("700"))
    );
    assert_eq!(
        current
            .resources
            .iter()
            .find(|r| r.observation.resource == source_cash())
            .unwrap()
            .balance
            .allocated,
        exact("500")
    );
    assert!(matches!(
        journal
            .preview_existing_transfer_ownership(
                result["transfer"]["inActivityId"].as_str().unwrap(),
                shares(),
                &caller
            )
            .await,
        Err(JournalError::AlreadyApplied)
    ));
}
