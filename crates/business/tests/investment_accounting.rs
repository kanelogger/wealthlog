#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::{collections::BTreeMap, sync::Arc};
use wealthfolio_business::{investment_journal::*, investments::*, BusinessChannel};
use wealthfolio_storage_sqlite::db::{get_connection, DbAccess};
#[path = "support/investment_harness.rs"]
mod support;
use support::*;

#[tokio::test]
async fn authoritative_resources_fold_current_ledger_without_waiting_for_valuation() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('buy','transactions','stock','BUY','POSTED',?1,'10','10','101.5','1','0.5','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let cash = FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    };
    let rows = h
        .reader
        .observe(
            &[
                cash.clone(),
                FundResource::Security {
                    account_id: "transactions".into(),
                    asset_id: "stock".into(),
                },
            ],
            h.day.parse().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rows[0].actual.unwrap().to_string(), "898.5");
    assert_eq!(rows[1].actual.unwrap().to_string(), "10");
    let mut conn = get_connection(h.store.pool()).unwrap();
    h.reader.validate(&mut conn, &rows).unwrap();
    db.execute("UPDATE activities SET amount='102' WHERE id='buy'", [])
        .unwrap();
    assert!(matches!(
        h.reader.validate(&mut conn, &rows),
        Err(JournalError::StateConflict)
    ));
    db.execute("UPDATE activities SET amount=NULL WHERE id='buy'", [])
        .unwrap();
    let unknown = h
        .reader
        .observe(&[cash], h.day.parse().unwrap())
        .await
        .unwrap();
    assert!(unknown[0].actual.is_none());
    assert!(unknown[0].issues.iter().any(|i| i == "MissingFinalCash"));
}

#[tokio::test]
async fn manual_preview_and_original_commit_share_preparation_and_preserve_unknowns() {
    let h = Harness::new().await;
    let empty = h
        .reader
        .observe(
            &[FundResource::Cash {
                account_id: "manual".into(),
                currency: "CNY".into(),
            }],
            h.day.parse().unwrap(),
        )
        .await
        .unwrap();
    assert!(empty[0].actual.is_none());
    h.initialize_manual().await;
    let mut input = h.trade().await;
    let before = h.investments.get_state("manual").await.unwrap();
    let preview = h.investments.preview_manual_trade(&input).await.unwrap();
    assert!(preview.automatic_allocation_eligible);
    assert_eq!(preview.net_cash.unwrap().to_string(), "-101.5");
    assert_eq!(
        h.investments.get_state("manual").await.unwrap(),
        before,
        "preview must not write a snapshot"
    );
    input.fee = None;
    let unknown = h.investments.preview_manual_trade(&input).await.unwrap();
    assert!(!unknown.automatic_allocation_eligible);
    assert!(unknown.fee.is_none());
    input.fee = Some("1".into());
    input.observed_cash_balance = Some("850".into());
    let calibrated = h.investments.preview_manual_trade(&input).await.unwrap();
    assert!(!calibrated.automatic_allocation_eligible);
    assert_eq!(calibrated.cash_adjustment.unwrap().to_string(), "-48.5");
    assert_eq!(calibrated.net_cash.unwrap().to_string(), "-101.5");
    input.observed_cash_balance = None;
    let committed = h
        .investments
        .record_manual_trade(input.clone(), BusinessChannel::Cli, "token", true)
        .await
        .unwrap();
    assert_eq!(committed["cashBalance"], "898.5");
    assert_eq!(committed["positionQuantity"], "10");
    let replay = h
        .investments
        .record_manual_trade(input, BusinessChannel::Cli, "token", true)
        .await
        .unwrap();
    assert_eq!(replay["requestId"], committed["requestId"]);
    assert_eq!(replay["replayed"], true);
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM activities WHERE account_id='manual'",
            [],
            |r| r.get::<_, i64>(0)
        )
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
        0,
        "legacy fact writes do not enable the journal"
    );
}

#[tokio::test]
async fn late_historical_split_is_detected_only_for_the_resource_it_changes() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    let state = h.investments.get_state("manual").await.unwrap();
    h.investments
        .patch_snapshot(
            PatchHoldingsSnapshot {
                account_id: "manual".into(),
                snapshot_date: h.day.clone(),
                expected_state_version: state["stateVersion"].as_str().unwrap().into(),
                positions: vec![PositionPatch {
                    asset_id: "stock".into(),
                    quantity: "10".into(),
                    average_cost: "10".into(),
                    currency: "CNY".into(),
                }],
                cash_balances: BTreeMap::new(),
                notes: "Observed shares".into(),
                request_key: "shares".into(),
            },
            BusinessChannel::Gui,
            "session",
            true,
        )
        .await
        .unwrap();
    let today: chrono::NaiveDate = h.day.parse().unwrap();
    let earlier = today - chrono::Duration::days(2);
    let yesterday = today - chrono::Duration::days(1);
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE holdings_snapshots SET snapshot_date=?1 WHERE account_id='manual'",
        [earlier.to_string()],
    )
    .unwrap();
    let resources = [
        FundResource::Cash {
            account_id: "manual".into(),
            currency: "CNY".into(),
        },
        FundResource::Security {
            account_id: "manual".into(),
            asset_id: "stock".into(),
        },
    ];
    let baseline = h.reader.observe(&resources, today).await.unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,currency,created_at,updated_at) VALUES('late-split','manual','stock','SPLIT','POSTED',?1,'2','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[yesterday.to_string()]).unwrap();
    let current = h.reader.observe(&resources, today).await.unwrap();
    assert_eq!(baseline[1].actual.unwrap().to_string(), "10");
    assert_eq!(current[1].actual.unwrap().to_string(), "20");
    assert!(compare_fund_observation(
        &baseline[0],
        &current[0],
        ExactDecimal::parse("500").unwrap()
    )
    .unwrap()
    .is_empty());
    let tasks = compare_fund_observation(
        &baseline[1],
        &current[1],
        ExactDecimal::parse("10").unwrap(),
    )
    .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].kind, FundReconciliationKind::CorporateAction);
    assert!(
        compare_fund_observation(&current[1], &current[1], ExactDecimal::parse("10").unwrap())
            .unwrap()
            .is_empty()
    );
    // An old split already included in the observation's units is not applied
    // again and creates no new source-confirmation item.
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,currency,created_at,updated_at) VALUES('included-old-split','manual','stock','SPLIT','POSTED',?1,'3','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[(earlier-chrono::Duration::days(4)).to_string()]).unwrap();
    let included = h.reader.observe(&resources, today).await.unwrap();
    assert_eq!(included[1].actual.unwrap().to_string(), "20");
    assert!(compare_fund_observation(
        &current[1],
        &included[1],
        ExactDecimal::parse("10").unwrap()
    )
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn execution_projection_backfills_restarts_and_keeps_invalidated_history() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('buy','transactions','stock','BUY','POSTED',?1,'10','10','101.5','1','0.5','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    // Model pre-module history: backfill is the only source of these markers.
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,currency,created_at,updated_at) VALUES('void-buy','transactions','stock','BUY','VOID',?1,'1','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    db.execute("DELETE FROM investment_projection_state", [])
        .unwrap();
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    let first = projector
        .backfill_batch(InvestmentSourceKind::Activity, None, 1)
        .await
        .unwrap();
    assert_eq!(first.scanned, 1);
    assert!(!first.exhausted);
    let resumed = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    let second = resumed
        .backfill_batch(InvestmentSourceKind::Activity, first.last_source_id, 10)
        .await
        .unwrap();
    assert_eq!(second.scanned, 1);
    assert!(second.exhausted);
    resumed
        .backfill_batch(InvestmentSourceKind::Activity, None, 10)
        .await
        .unwrap();
    db.execute(
        "UPDATE investment_projection_state SET refresh_target_generation=generation",
        [],
    )
    .unwrap();
    let progress = resumed.refresh_batch(100).await.unwrap();
    assert_eq!(progress.remaining, 0);
    let indexed = resumed.executions().unwrap();
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].source_id, "buy");
    assert_eq!(indexed[0].status, ExecutionStatus::Valid);
    assert_eq!(indexed[0].net_cash.unwrap().to_string(), "-101.5");
    assert_eq!(indexed[0].gross_amount.unwrap().to_string(), "100");
    assert_eq!(resumed.refresh_batch(100).await.unwrap().processed, 0);
    let original_id = indexed[0].id.clone();
    let original_snapshot = indexed[0].snapshot.clone();
    db.execute(
        "UPDATE activities SET activity_type='DIVIDEND' WHERE id='buy'",
        [],
    )
    .unwrap();
    resumed.refresh_batch(100).await.unwrap();
    let invalid = resumed.executions().unwrap();
    assert_eq!(invalid[0].id, original_id);
    assert_eq!(invalid[0].status, ExecutionStatus::Invalid);
    assert_eq!(invalid[0].snapshot, original_snapshot);
    db.execute("DELETE FROM activities WHERE id='buy'", [])
        .unwrap();
    resumed.refresh_batch(100).await.unwrap();
    assert_eq!(
        resumed.executions().unwrap()[0].status,
        ExecutionStatus::Deleted
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
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

#[tokio::test]
async fn projection_only_indexes_completed_manual_trade_whitelist_and_keeps_unknown_fees() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    let mut trade = h.trade().await;
    trade.fee = None;
    let committed = h
        .investments
        .record_manual_trade(trade, BusinessChannel::Cli, "token", true)
        .await
        .unwrap();
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    for kind in [
        InvestmentSourceKind::Activity,
        InvestmentSourceKind::ManualTrade,
        InvestmentSourceKind::Snapshot,
    ] {
        projector.backfill_batch(kind, None, 100).await.unwrap();
    }
    projector.refresh_batch(100).await.unwrap();
    let rows = projector.executions().unwrap();
    assert_eq!(rows.len(), 1, "opening snapshot patches are not trades");
    assert_eq!(rows[0].source_id, committed["requestId"].as_str().unwrap());
    assert_eq!(rows[0].source_kind, InvestmentSourceKind::ManualTrade);
    assert_eq!(rows[0].status, ExecutionStatus::Reconcile);
    assert!(rows[0].fee.is_none());
    assert_eq!(rows[0].net_cash.unwrap().to_string(), "-100.5");
    assert!(rows[0].snapshot["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i == "FeeUnknown"));
    assert_eq!(projector.refresh_batch(100).await.unwrap().processed, 0);
    let db = h.access.connect_rusqlite().unwrap();
    let request_id = committed["requestId"].as_str().unwrap();
    db.execute(
        "UPDATE business_requests SET state='failed' WHERE id=?1",
        [request_id],
    )
    .unwrap();
    projector.refresh_batch(100).await.unwrap();
    assert_eq!(
        projector.executions().unwrap()[0].status,
        ExecutionStatus::Invalid
    );
    db.execute("DELETE FROM business_requests WHERE id=?1", [request_id])
        .unwrap();
    projector.refresh_batch(100).await.unwrap();
    assert_eq!(
        projector.executions().unwrap()[0].status,
        ExecutionStatus::Deleted
    );
}

struct RacingProjectionReader {
    inner: Arc<AccountingResourceReader>,
    access: DbAccess,
    changed: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait]
impl InvestmentExecutionReader for RacingProjectionReader {
    async fn prepare_execution(
        &self,
        source: &ProjectionSource,
    ) -> JournalResult<PreparedExecutionProjection> {
        let prepared = self.inner.prepare_execution(source).await?;
        if source.id == "buy" && !self.changed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            self.access
                .connect_rusqlite()
                .unwrap()
                .execute("UPDATE activities SET amount='102' WHERE id='buy'", [])
                .unwrap();
        }
        Ok(prepared)
    }
    fn validate_execution(
        &self,
        conn: &mut diesel::SqliteConnection,
        prepared: &PreparedExecutionProjection,
    ) -> JournalResult<()> {
        self.inner.validate_execution(conn, prepared)
    }
}

#[tokio::test]
async fn source_change_during_projection_remains_pending_until_retried() {
    let h = Harness::new().await;
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('buy','transactions','stock','BUY','POSTED',?1,'10','10','101.5','1','0.5','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let racing = Arc::new(RacingProjectionReader {
        inner: h.reader.clone(),
        access: h.access.clone(),
        changed: std::sync::atomic::AtomicBool::new(false),
    });
    let projector = InvestmentProjectionService::new(h.store.clone(), racing);
    let first = projector.refresh_batch(100).await.unwrap();
    assert_eq!(first.conflicted, 1);
    assert_eq!(first.remaining, 1);
    assert!(projector.executions().unwrap().is_empty());
    let resumed = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    assert_eq!(resumed.refresh_batch(100).await.unwrap().remaining, 0);
    assert_eq!(
        resumed.executions().unwrap()[0]
            .net_cash
            .unwrap()
            .to_string(),
        "-102"
    );
}

#[tokio::test]
async fn related_account_facts_reproject_old_executions_but_quotes_do_not() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('sell','transactions','stock','SELL','POSTED',?1,'5','12','60','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    projector.refresh_batch(100).await.unwrap();
    let original = projector
        .executions()
        .unwrap()
        .into_iter()
        .find(|e| e.source_id == "sell")
        .unwrap();
    assert_eq!(original.status, ExecutionStatus::Reconcile);
    let yesterday = h.day.parse::<chrono::NaiveDate>().unwrap() - chrono::Duration::days(1);
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('late-buy','transactions','stock','BUY','POSTED',?1,'10','10','100','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[yesterday.to_string()]).unwrap();
    projector.refresh_batch(100).await.unwrap();
    let current = projector
        .executions()
        .unwrap()
        .into_iter()
        .find(|e| e.source_id == "sell")
        .unwrap();
    assert_eq!(
        current.source_version, original.source_version,
        "dependent reprojection does not rewrite the source version"
    );
    assert_eq!(current.status, ExecutionStatus::Valid);
    db.execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('price','stock',?1,'MANUAL','999','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    assert_eq!(projector.refresh_batch(100).await.unwrap().processed, 0);
    db.execute(
        "UPDATE accounts SET is_archived=1 WHERE id='transactions'",
        [],
    )
    .unwrap();
    let changed = projector.refresh_batch(100).await.unwrap();
    assert_eq!(changed.processed, 2);
    assert_eq!(
        projector
            .executions()
            .unwrap()
            .into_iter()
            .find(|e| e.source_id == "sell")
            .unwrap()
            .status,
        ExecutionStatus::Reconcile
    );
}

#[tokio::test]
async fn manual_trade_and_multiple_purpose_legs_commit_once_with_net_fees() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("a");
    h.seed_purpose("b");
    let journal = h.journal();
    let caller = journal_caller();
    let trade = h.trade().await;
    let shares = vec![
        funded_share("a", "6", "-60.9", "60.9"),
        funded_share("b", "4", "-40.6", "40.6"),
    ];
    let preview = journal
        .preview_manual_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(preview.ownership.charges[0].fee, Some(exact("0.6")));
    assert_eq!(preview.ownership.charges[0].tax, Some(exact("0.3")));
    assert_eq!(preview.ownership.charges[1].fee, Some(exact("0.4")));
    let input = RecordManualTradeWithOwnership {
        trade,
        shares,
        reason: "Confirmed completed trade and exact top-ups".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let result = journal
        .record_manual_trade_with_ownership(input.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(result["trade"]["cashBalance"], "898.5");
    assert_eq!(result["ownershipStatus"], "APPLIED");
    let replay = journal
        .record_manual_trade_with_ownership(input, &caller)
        .await
        .unwrap();
    assert_eq!(
        replay["allocationOperationId"],
        result["allocationOperationId"]
    );
    assert_eq!(replay["replayed"], true);
    let status = journal
        .preview_allocations(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap();
    assert_eq!(status.resources[0].balance.allocated, ExactDecimal::ZERO);
    assert_eq!(status.resources[0].balance.actual, Some(exact("898.5")));
    assert_eq!(status.resources[1].balance.allocated, exact("10"));
    assert_eq!(status.resources[1].purposes["a"], exact("6"));
    assert!(journal
        .fund_reconciliation(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap()
        .is_empty());
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    projector.refresh_batch(100).await.unwrap();
    assert_eq!(projector.executions().unwrap().len(), 1);
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        6
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_operations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.enabled_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    assert_eq!(journal.rebuild_allocations(&caller).await.unwrap(), 4);
    assert!(journal
        .fund_reconciliation(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn compound_audit_failure_rolls_back_snapshot_execution_entries_request_and_enablement() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let caller = journal_caller();
    let trade = h.trade().await;
    let before = h.investments.get_state("manual").await.unwrap();
    let shares = vec![funded_share("a", "10", "-101.5", "101.5")];
    let preview = journal
        .preview_manual_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    let input = RecordManualTradeWithOwnership {
        trade,
        shares,
        reason: "Atomic failure test".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER reject_compound_audit BEFORE INSERT ON business_audit WHEN NEW.operation='investment_manual_trade_record' BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;").unwrap();
    assert!(journal
        .record_manual_trade_with_ownership(input.clone(), &caller)
        .await
        .is_err());
    assert_eq!(h.investments.get_state("manual").await.unwrap(), before);
    for table in [
        "fund_allocation_entries",
        "fund_allocations",
        "fund_allocation_operations",
        "investment_executions",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "{table} did not roll back"
        );
    }
    assert_eq!(db.query_row("SELECT COUNT(*) FROM business_requests WHERE operation='investment_manual_trade_record'",[],|r| r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute("DROP TRIGGER reject_compound_audit", [])
        .unwrap();
    journal
        .record_manual_trade_with_ownership(input, &caller)
        .await
        .unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[tokio::test]
async fn compound_requires_fact_permission_and_frozen_topups_and_rejects_unknown_charges() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let caller = journal_caller();
    let mut trade = h.trade().await;
    let shares = vec![funded_share("a", "10", "-101.5", "101.5")];
    let preview = journal
        .preview_manual_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    let input = RecordManualTradeWithOwnership {
        trade: trade.clone(),
        shares: shares.clone(),
        reason: "Explicit allocation".into(),
        expected_preview_hash: preview.preview_hash,
    };
    let mut denied = caller.clone();
    denied.fact_permissions.holdings_write = false;
    denied.channel = BusinessChannel::Mcp;
    assert!(matches!(
        journal
            .record_manual_trade_with_ownership(input.clone(), &denied)
            .await,
        Err(JournalError::PermissionDenied)
    ));
    let mut changed = input;
    changed.shares[0].top_ups[0].amount = exact("102");
    assert!(matches!(
        journal
            .record_manual_trade_with_ownership(changed, &caller)
            .await,
        Err(JournalError::StateConflict)
    ));
    let mut unfunded = shares;
    unfunded[0].top_ups.clear();
    assert!(matches!(
        journal
            .preview_manual_trade_ownership(&trade, unfunded, &caller)
            .await,
        Err(JournalError::NegativeAllocation)
    ));
    trade.fee = None;
    let unknown = vec![funded_share("a", "10", "-100.5", "100.5")];
    assert!(matches!(
        journal
            .preview_manual_trade_ownership(&trade, unknown, &caller)
            .await,
        Err(JournalError::ReconciliationRequired)
    ));
    assert_eq!(
        h.investments.get_state("manual").await.unwrap()["cashBalances"]["CNY"],
        "1000"
    );
    // The original fact-only route remains available and retains fee unknown.
    h.investments
        .record_manual_trade(trade, BusinessChannel::Cli, "token", true)
        .await
        .unwrap();
}

#[tokio::test]
async fn compound_advances_its_snapshot_baseline_without_clearing_unconfirmed_calibration() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let caller = journal_caller();
    let preview = journal
        .preview_allocations(vec![manual_cash()], &caller)
        .await
        .unwrap();
    journal
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Opening purpose cash".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "a".into(),
                    resource: manual_cash(),
                    delta: exact("100"),
                }],
                resources: vec![manual_cash()],
                expected_preview_hash: preview.preview_hash,
                request_key: "allocate".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    let state = h.investments.get_state("manual").await.unwrap();
    h.investments
        .patch_snapshot(
            PatchHoldingsSnapshot {
                account_id: "manual".into(),
                snapshot_date: h.day.clone(),
                expected_state_version: state["stateVersion"].as_str().unwrap().into(),
                positions: vec![],
                cash_balances: BTreeMap::from([("CNY".into(), "900".into())]),
                notes: "Independent observed correction".into(),
                request_key: "calibration".into(),
            },
            BusinessChannel::Gui,
            "session",
            true,
        )
        .await
        .unwrap();
    let trade = h.trade().await;
    let shares = vec![funded_share("a", "10", "-101.5", "1.5")];
    let preview = journal
        .preview_manual_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    assert_eq!(preview.pending_reconciliation.len(), 1);
    journal
        .record_manual_trade_with_ownership(
            RecordManualTradeWithOwnership {
                trade,
                shares,
                reason: "Trade does not confirm the earlier correction".into(),
                expected_preview_hash: preview.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let pending = journal
        .fund_reconciliation(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap();
    assert_eq!(
        pending.len(),
        1,
        "known trade must not silently acknowledge an earlier source change"
    );
    let current = journal
        .preview_allocations(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap();
    journal
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Explicitly confirm earlier correction".into(),
                entries: vec![],
                resources: vec![manual_cash(), manual_security()],
                expected_preview_hash: current.preview_hash,
                request_key: "confirm-calibration".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    assert!(journal
        .fund_reconciliation(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn referenced_manual_request_tampering_is_visible_even_when_observed_cash_is_unchanged() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    h.seed_purpose("a");
    let journal = h.journal();
    let caller = journal_caller();
    let trade = h.trade().await;
    let shares = vec![funded_share("a", "10", "-101.5", "101.5")];
    let preview = journal
        .preview_manual_trade_ownership(&trade, shares.clone(), &caller)
        .await
        .unwrap();
    let result = journal
        .record_manual_trade_with_ownership(
            RecordManualTradeWithOwnership {
                trade,
                shares,
                reason: "Bind completed request".into(),
                expected_preview_hash: preview.preview_hash,
            },
            &caller,
        )
        .await
        .unwrap();
    let before = h.investments.get_state("manual").await.unwrap();
    h.access.connect_rusqlite().unwrap().execute("UPDATE business_requests SET request_snapshot=json_set(request_snapshot,'$.trade.fee','2') WHERE id=?1",[result["requestId"].as_str().unwrap()]).unwrap();
    assert_eq!(h.investments.get_state("manual").await.unwrap(), before);
    let pending = journal
        .fund_reconciliation(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap();
    assert_eq!(
        pending
            .iter()
            .filter(
                |i| i.operation_id.is_some() && i.kind == FundReconciliationKind::SourceInvalidated
            )
            .count(),
        2
    );
    let current = journal
        .preview_allocations(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap();
    journal
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Acknowledge invalidated provenance without inventing a cash movement"
                    .into(),
                entries: vec![],
                resources: vec![manual_cash(), manual_security()],
                expected_preview_hash: current.preview_hash,
                request_key: "source-ack".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    assert!(journal
        .fund_reconciliation(vec![manual_cash(), manual_security()], &caller)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn cash_dividends_need_reconciliation_while_ordinary_spending_only_creates_real_excess() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let j = h.journal();
    let c = journal_caller();
    let resource = FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    };
    let p = j
        .preview_allocations(vec![resource.clone()], &c)
        .await
        .unwrap();
    j.apply_allocation(
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Allocate,
            reason: "Cash purpose".into(),
            entries: vec![AllocationDelta {
                purpose_id: "a".into(),
                resource: resource.clone(),
                delta: exact("100"),
            }],
            resources: vec![resource.clone()],
            expected_preview_hash: p.preview_hash,
            request_key: "allocate".into(),
        },
        &c,
    )
    .await
    .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('dividend','transactions','stock','DIVIDEND','POSTED',?1,'20','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    assert!(j
        .fund_reconciliation(vec![resource.clone()], &c)
        .await
        .unwrap()
        .iter()
        .any(|i| i.kind == FundReconciliationKind::CorporateAction));
    let p = j
        .preview_allocations(vec![resource.clone()], &c)
        .await
        .unwrap();
    j.apply_allocation(
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Reconcile,
            reason: "Confirm recorded dividend source".into(),
            entries: vec![],
            resources: vec![resource.clone()],
            expected_preview_hash: p.preview_hash,
            request_key: "ack-dividend".into(),
        },
        &c,
    )
    .await
    .unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('expense','transactions','WITHDRAWAL','POSTED',?1,'1000','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let items = j.fund_reconciliation(vec![resource], &c).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, FundReconciliationKind::PurposeExcess);
    assert_eq!(items[0].excess, Some(exact("80")));
}

#[tokio::test]
async fn unsupported_security_resources_are_not_exposed_as_available_journal_assets() {
    let h = Harness::new().await;
    h.seed_purpose("a");
    let j = h.journal();
    let c = journal_caller();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO assets(id,kind,is_active,quote_mode,quote_ccy,instrument_type,created_at,updated_at) VALUES('coin','INVESTMENT',1,'MARKET','CNY','CRYPTO',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,currency,created_at,updated_at) VALUES('coin-buy','transactions','coin','BUY','POSTED',?1,'1','100','100','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let resource = FundResource::Security {
        account_id: "transactions".into(),
        asset_id: "coin".into(),
    };
    let p = j
        .preview_allocations(vec![resource.clone()], &c)
        .await
        .unwrap();
    assert!(p.resources[0].balance.actual.is_none());
    assert!(matches!(
        j.apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Unsupported asset".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "a".into(),
                    resource: resource.clone(),
                    delta: exact("1")
                }],
                resources: vec![resource],
                expected_preview_hash: p.preview_hash,
                request_key: "out-of-scope".into()
            },
            &c
        )
        .await,
        Err(JournalError::ResourceUnavailable)
    ));
    h.initialize_manual().await;
    h.investments
        .record_manual_trade(h.trade().await, BusinessChannel::Gui, "session", true)
        .await
        .unwrap();
    let resource = manual_security();
    let known = j
        .preview_allocations(vec![resource.clone()], &c)
        .await
        .unwrap();
    assert_eq!(known.resources[0].balance.actual, Some(exact("10")));
    let updated = db.execute(
        "UPDATE snapshot_positions SET contract_multiplier='2'
         WHERE asset_id='stock' AND snapshot_id IN (SELECT id FROM holdings_snapshots WHERE account_id='manual')",
        [],
    ).unwrap();
    assert!(updated > 0);
    let incompatible = j.preview_allocations(vec![resource], &c).await.unwrap();
    assert!(incompatible.resources[0].balance.actual.is_none());
    assert!(incompatible.resources[0]
        .observation
        .issues
        .iter()
        .any(|issue| issue == "UnsupportedSecurityUnitBasis"));
}

#[tokio::test]
async fn historical_execution_backfill_resumes_in_batches_without_enabling_or_replaying_funds() {
    let sample_size = std::env::var("WEALTHLOG_HISTORY_TEST_ROWS")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(256);
    assert!((1..=4096).contains(&sample_size));
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    // Represent history recorded before M1 installed its source triggers.
    db.execute_batch("DROP TRIGGER investment_source_activities_insert; BEGIN;")
        .unwrap();
    let yesterday =
        (h.day.parse::<chrono::NaiveDate>().unwrap() - chrono::Duration::days(1)).to_string();
    for i in 0..sample_size {
        db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES(?1,'transactions','stock','BUY','POSTED',?2,'1','1','1','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[format!("history-{i:04}"),yesterday.clone()]).unwrap();
    }
    db.execute_batch("COMMIT; DELETE FROM investment_projection_state;")
        .unwrap();
    let p = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    let start = std::time::Instant::now();
    let mut total = 0;
    let mut complete = false;
    let mut slices = 0;
    for _ in 0..(sample_size.div_ceil(64) + 8) {
        // New service instances resume the persisted cursor and generations.
        let resumed = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
        let progress = resumed.maintenance_batch(64).await.unwrap();
        slices += 1;
        total += progress.refreshed.processed;
        if sample_size > 256 && slices % 4 == 0 {
            eprintln!("Indexed {total}/{sample_size} historical executions");
        }
        if progress.historical_complete && progress.refreshed.remaining == 0 {
            complete = true;
            break;
        }
    }
    eprintln!(
        "{sample_size} historical executions projected in {:?} across {slices} slices",
        start.elapsed()
    );
    assert!(complete);
    assert_eq!(total, sample_size);
    let rows = p.executions().unwrap();
    assert_eq!(rows.len(), sample_size);
    assert_eq!(
        rows.iter()
            .map(|row| &row.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        sample_size
    );
    assert_eq!(
        p.maintenance_batch(64).await.unwrap().refreshed.processed,
        0
    );
    for table in [
        "fund_allocation_entries",
        "fund_allocation_operations",
        "business_requests",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
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
    let service = h.journal();
    let caller = journal_caller();
    let default_before = service
        .investment_work_queue(Default::default(), &caller)
        .await
        .unwrap();
    assert!(default_before.complete);
    assert!(default_before.items.is_empty());
    let historical = service
        .investment_work_queue(
            InvestmentWorkQuery {
                include_historical_executions: true,
                ..Default::default()
            },
            &caller,
        )
        .await
        .unwrap();
    assert!(historical.complete);
    assert_eq!(
        historical
            .items
            .iter()
            .filter(|item| item["kind"] == "UNLINKED_EXECUTION")
            .count(),
        sample_size
    );
    assert!(service
        .investment_work_queue(Default::default(), &caller)
        .await
        .unwrap()
        .enabled_on
        .is_none());
    service.save_purpose(serde_json::from_value(serde_json::json!({
        "name":"Synthetic purpose","category":"CUSTOM","currency":"CNY","requestKey":"history-enable"
    })).unwrap(), &caller).await.unwrap();
    let default_after = service
        .investment_work_queue(Default::default(), &caller)
        .await
        .unwrap();
    assert!(default_after.complete);
    assert!(default_after.enabled_on.is_some());
    assert!(default_after.items.is_empty());
}

#[tokio::test]
async fn malformed_source_values_are_unavailable_instead_of_legacy_zero_fallbacks() {
    let h = Harness::new().await;
    let today = h.day.parse().unwrap();
    let resource = FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    };
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('broken-expense','transactions','WITHDRAWAL','POSTED',?1,'not-a-number','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let current = h.reader.observe(&[resource], today).await.unwrap();
    assert!(current[0].actual.is_none());
    assert!(current[0]
        .issues
        .iter()
        .any(|i| i == "MalformedActivityData"));
    h.initialize_manual().await;
    db.execute("UPDATE holdings_snapshots SET cash_balances='{\"CNY\":\"not-a-number\"}' WHERE account_id='manual'",[]).unwrap();
    let current = h.reader.observe(&[manual_cash()], today).await.unwrap();
    assert!(current[0].actual.is_none());
    assert!(current[0]
        .issues
        .iter()
        .any(|i| i == "MalformedSnapshotData"));
}

#[tokio::test]
async fn overflowing_manual_cash_carry_returns_unavailable_without_panicking() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    let today: chrono::NaiveDate = h.day.parse().unwrap();
    let yesterday = (today - chrono::Duration::days(1)).to_string();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("UPDATE holdings_snapshots SET snapshot_date=?1,cash_balances='{\"CNY\":\"79228162514264337593543950335\"}' WHERE account_id='manual'",[yesterday]).unwrap();
    db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('huge-transfer','manual','TRANSFER_IN','POSTED',?1,'79228162514264337593543950335','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let current = h.reader.observe(&[manual_cash()], today).await.unwrap();
    assert!(current[0].actual.is_none());
    assert!(current[0]
        .issues
        .iter()
        .any(|i| i == "SnapshotCalculationUnavailable"));
}

#[tokio::test]
async fn failed_split_unit_carry_does_not_report_the_unchanged_old_quantity_as_current() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    let state = h.investments.get_state("manual").await.unwrap();
    h.investments
        .patch_snapshot(
            PatchHoldingsSnapshot {
                account_id: "manual".into(),
                snapshot_date: h.day.clone(),
                expected_state_version: state["stateVersion"].as_str().unwrap().into(),
                positions: vec![PositionPatch {
                    asset_id: "stock".into(),
                    quantity: "79228162514264337593543950335".into(),
                    average_cost: "0".into(),
                    currency: "CNY".into(),
                }],
                cash_balances: BTreeMap::new(),
                notes: "Synthetic decimal-range observation".into(),
                request_key: "large-units".into(),
            },
            BusinessChannel::Gui,
            "session",
            true,
        )
        .await
        .unwrap();
    let today: chrono::NaiveDate = h.day.parse().unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE holdings_snapshots SET snapshot_date=?1 WHERE account_id='manual'",
        [(today - chrono::Duration::days(1)).to_string()],
    )
    .unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,amount,currency,created_at,updated_at) VALUES('split','manual','stock','SPLIT','POSTED',?1,'2','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let resources = h
        .reader
        .observe(&[manual_cash(), manual_security()], today)
        .await
        .unwrap();
    assert_eq!(resources[0].actual, Some(exact("1000")));
    assert!(resources[1].actual.is_none());
    assert!(resources[1]
        .issues
        .iter()
        .any(|i| i == "MalformedSnapshotData"));
}
