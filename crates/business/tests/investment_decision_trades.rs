//! Confirmed-version record/evidence/ownership atomicity on synthetic native services.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::{json, Value};
use support::{journal_caller, manual_cash, Harness};
use wealthfolio_business::investment_journal::*;

async fn confirmed(h: &Harness) -> (InvestmentJournalService, DecisionExecutionTarget, i32) {
    h.initialize_manual().await;
    h.seed_purpose("purpose");
    let service = h.journal().with_context_reader(h.context.clone());
    let mut c = journal_caller();
    c.can_confirm = true;
    let p = service
        .preview_allocations(vec![manual_cash()], &c)
        .await
        .unwrap();
    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Opening ownership".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "purpose".into(),
                    resource: manual_cash(),
                    delta: ExactDecimal::parse("500").unwrap(),
                }],
                resources: vec![manual_cash()],
                expected_preview_hash: p.preview_hash,
                request_key: "assign".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let body:DecisionBody=serde_json::from_value(json!({"title":"记录确认版本","action":"BUY","assetId":"stock","plannedQuantity":"10","plannedAmount":"101.5","currency":"CNY","thesis":"用户提供的判断","purposeIds":["purpose"],"options":[{"action":"等待","description":"暂不买入"}],"declaredAt":"1999-01-01"})).unwrap();
    let saved = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "decision".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let target = DecisionDraftTarget {
        decision_id: saved["decisionId"].as_str().unwrap().into(),
        draft_revision_id: saved["draftRevisionId"].as_str().unwrap().into(),
        expected_version: 1,
    };
    let p = service
        .preview_decision_confirmation(target, &c)
        .await
        .unwrap();
    let done = service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: "ledger".into(),
                target: p.target,
                expected_content_hash: p.expected_content_hash,
                expected_context_hash: p.expected_context_hash,
                context: p.context,
                request_key: "confirm".into(),
            },
            &c,
        )
        .await
        .unwrap();
    (
        service,
        DecisionExecutionTarget {
            decision_id: done["decisionId"].as_str().unwrap().into(),
            revision_id: done["currentRevisionId"].as_str().unwrap().into(),
            deviation_reason: None,
        },
        done["version"].as_i64().unwrap() as i32,
    )
}
fn shares() -> Vec<TradePurposeShare> {
    vec![TradePurposeShare {
        purpose_id: "purpose".into(),
        quantity: ExactDecimal::parse("10").unwrap(),
        net_cash: ExactDecimal::parse("-101.5").unwrap(),
        top_ups: vec![],
    }]
}
async fn owned(h: &Harness, target: DecisionExecutionTarget) -> RecordDecisionManualTrade {
    RecordDecisionManualTrade {
        decision: target,
        trade: h.trade().await,
        ownership: DecisionTradeOwnership::Apply { shares: shares() },
        reason: "Record completed trade".into(),
        expected_preview_hash: String::new(),
    }
}
fn count(h: &Harness, table: &str) -> i64 {
    h.access
        .connect_rusqlite()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
async fn amendment(
    service: &InvestmentJournalService,
    target: &DecisionExecutionTarget,
    version: i32,
) -> Value {
    let body:DecisionBody=serde_json::from_value(json!({"title":"尚未确认的新判断","action":"SELL","assetId":"stock","thesis":"另一个草稿理由","options":[{"action":"持有","description":"继续持有"}]})).unwrap();
    service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: Some(target.decision_id.clone()),
                draft_revision_id: None,
                expected_version: Some(version),
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "amend".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn current_confirmed_revision_survives_draft_and_full_trade_does_not_auto_close() {
    let h = Harness::new().await;
    let (service, target, version) = confirmed(&h).await;
    let c = journal_caller();
    let mut input = owned(&h, target.clone()).await;
    let first = service
        .preview_decision_manual_trade(&input, &c)
        .await
        .unwrap();
    assert_eq!(first.decision["revisionNumber"], 1);
    assert_eq!(first.decision["progress"]["remainingQuantity"], "10");
    amendment(&service, &target, version).await;
    let second = service
        .preview_decision_manual_trade(&input, &c)
        .await
        .unwrap();
    assert_eq!(
        first.preview_hash, second.preview_hash,
        "an unconfirmed draft cannot replace or invalidate v1"
    );
    input.expected_preview_hash = first.preview_hash;
    let result = service
        .record_decision_manual_trade(input.clone(), &c)
        .await
        .unwrap();
    assert_eq!(result["ownershipStatus"], "APPLIED");
    assert_eq!(result["decisionRevisionId"], target.revision_id);
    assert!(result["executionLinkId"].is_string());
    let detail = service.get_decision(&target.decision_id, &c).unwrap();
    assert_eq!(detail["executionState"], "OPEN");
    assert!(detail["draftRevisionId"].is_string());
    assert_eq!(detail["currentRevisionId"], target.revision_id);
    let links = service
        .decision_execution_links(&target.decision_id, &c)
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["revisionNumber"], 1);
    assert_eq!(links[0]["temporalClassification"], "UNDETERMINED");
    assert_eq!(
        service
            .record_decision_manual_trade(input, &c)
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(count(&h, "investment_execution_links"), 1);
    assert_eq!(count(&h, "fund_allocation_entries"), 3);
    let balances = service
        .preview_allocations(
            vec![
                manual_cash(),
                FundResource::Security {
                    account_id: "manual".into(),
                    asset_id: "stock".into(),
                },
            ],
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        balances
            .resources
            .iter()
            .find(|r| r.observation.resource == manual_cash())
            .unwrap()
            .purposes["purpose"]
            .to_string(),
        "398.5"
    );
    let mut another = owned(&h, target.clone()).await;
    another.trade.request_key = "after".into();
    let next = service
        .preview_decision_manual_trade(&another, &c)
        .await
        .unwrap();
    assert_eq!(next.decision["progress"]["remainingQuantity"], "0");
    assert_eq!(next.decision["progress"]["remainingCashAmount"], "0");
}

#[tokio::test]
async fn switching_confirmed_revision_invalidates_preview_without_writing_facts() {
    let h = Harness::new().await;
    let (service, target, version) = confirmed(&h).await;
    let c = journal_caller();
    let mut input = owned(&h, target.clone()).await;
    input.expected_preview_hash = service
        .preview_decision_manual_trade(&input, &c)
        .await
        .unwrap()
        .preview_hash;
    let amended = amendment(&service, &target, version).await;
    let mut confirmer = c.clone();
    confirmer.can_confirm = true;
    let p = service
        .preview_decision_confirmation(
            DecisionDraftTarget {
                decision_id: target.decision_id.clone(),
                draft_revision_id: amended["draftRevisionId"].as_str().unwrap().into(),
                expected_version: amended["version"].as_i64().unwrap() as i32,
            },
            &confirmer,
        )
        .await
        .unwrap();
    service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: "ledger".into(),
                target: p.target,
                expected_content_hash: p.expected_content_hash,
                expected_context_hash: p.expected_context_hash,
                context: p.context,
                request_key: "confirm-amendment".into(),
            },
            &confirmer,
        )
        .await
        .unwrap();
    let before = count(&h, "business_requests");
    assert_eq!(
        service
            .record_decision_manual_trade(input, &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    assert_eq!(count(&h, "business_requests"), before);
    assert_eq!(count(&h, "investment_execution_links"), 0);
    assert_eq!(count(&h, "investment_executions"), 0);
    assert_eq!(count(&h, "fund_allocation_entries"), 1);
}

#[tokio::test]
async fn unknown_charges_allow_atomic_fact_and_evidence_without_guessing_ownership() {
    let h = Harness::new().await;
    let (service, target, _) = confirmed(&h).await;
    let c = journal_caller();
    let mut input = owned(&h, target.clone()).await;
    input.trade.fee = None;
    assert_eq!(
        service
            .preview_decision_manual_trade(&input, &c)
            .await
            .unwrap_err()
            .code(),
        "reconciliation_required"
    );
    input.ownership = DecisionTradeOwnership::ReconcileLater;
    let preview = service
        .preview_decision_manual_trade(&input, &c)
        .await
        .unwrap();
    assert_eq!(preview.trade.fee, None);
    assert!(preview.ownership.is_none());
    input.expected_preview_hash = preview.preview_hash;
    let result = service
        .record_decision_manual_trade(input.clone(), &c)
        .await
        .unwrap();
    assert_eq!(result["ownershipStatus"], "RECONCILIATION_REQUIRED");
    assert_eq!(result["allocationOperationId"], Value::Null);
    assert_eq!(result["decisionRevisionId"], target.revision_id);
    assert_eq!(count(&h, "fund_allocation_entries"), 1);
    assert_eq!(count(&h, "investment_execution_links"), 1);
    let execution = h
        .access
        .connect_rusqlite()
        .unwrap()
        .query_row("SELECT status FROM investment_executions", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    assert_eq!(execution, "RECONCILE");
    assert_eq!(
        service
            .record_decision_manual_trade(input.clone(), &c)
            .await
            .unwrap()["replayed"],
        true
    );
    let mut next = owned(&h, target).await;
    next.trade.request_key = "inspect-unknown-cash-progress".into();
    next.ownership = DecisionTradeOwnership::ReconcileLater;
    let progress = service
        .preview_decision_manual_trade(&next, &c)
        .await
        .unwrap()
        .decision["progress"]
        .clone();
    assert_eq!(progress["remainingQuantity"], "0");
    assert_eq!(
        progress["remainingCashAmount"],
        Value::Null,
        "unknown manual charges do not become a known zero fee in plan progress"
    );
    input.ownership = DecisionTradeOwnership::Apply { shares: shares() };
    assert_eq!(
        service
            .record_decision_manual_trade(input, &c)
            .await
            .unwrap_err()
            .code(),
        "request_conflict"
    );
}

#[tokio::test]
async fn audit_failure_rolls_back_trade_link_projection_request_and_ownership_together() {
    let h = Harness::new().await;
    let (service, target, _) = confirmed(&h).await;
    let c = journal_caller();
    let mut input = owned(&h, target).await;
    input.expected_preview_hash = service
        .preview_decision_manual_trade(&input, &c)
        .await
        .unwrap()
        .preview_hash;
    let db = h.access.connect_rusqlite().unwrap();
    let original: String = db
        .query_row(
            "SELECT cash_balances FROM holdings_snapshots WHERE account_id='manual'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let before = count(&h, "business_requests");
    db.execute_batch("CREATE TRIGGER fail_decision_trade_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    assert!(service
        .record_decision_manual_trade(input.clone(), &c)
        .await
        .is_err());
    assert_eq!(count(&h, "business_requests"), before);
    assert_eq!(count(&h, "investment_executions"), 0);
    assert_eq!(count(&h, "investment_execution_links"), 0);
    assert_eq!(count(&h, "fund_allocation_entries"), 1);
    assert_eq!(
        db.query_row(
            "SELECT cash_balances FROM holdings_snapshots WHERE account_id='manual'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        original
    );
    db.execute("DROP TRIGGER fail_decision_trade_audit", [])
        .unwrap();
    service
        .record_decision_manual_trade(input, &c)
        .await
        .unwrap();
    assert_eq!(count(&h, "investment_execution_links"), 1);
}

fn ledger_input(h: &Harness, target: DecisionExecutionTarget) -> RecordDecisionLedgerTrade {
    RecordDecisionLedgerTrade {
        decision: target,
        trade: RecordLedgerTrade {
            account_id: "transactions".into(),
            asset_id: "stock".into(),
            side: "BUY".into(),
            activity_date: h.day.clone(),
            quantity: ExactDecimal::parse("10").unwrap(),
            unit_price: Some(ExactDecimal::parse("10").unwrap()),
            amount: Some(ExactDecimal::parse("101.5").unwrap()),
            fee: Some(ExactDecimal::parse("1").unwrap()),
            tax: Some(ExactDecimal::parse("0.5").unwrap()),
            currency: "CNY".into(),
            fx_rate: None,
            notes: None,
            idempotency_key: None,
            request_key: "ledger-bound".into(),
        },
        ownership: DecisionTradeOwnership::Apply {
            shares: vec![TradePurposeShare {
                purpose_id: "purpose".into(),
                quantity: ExactDecimal::parse("10").unwrap(),
                net_cash: ExactDecimal::parse("-101.5").unwrap(),
                top_ups: vec![OwnershipTopUp {
                    resource: FundResource::Cash {
                        account_id: "transactions".into(),
                        currency: "CNY".into(),
                    },
                    amount: ExactDecimal::parse("101.5").unwrap(),
                }],
            }],
        },
        reason: "Already executed ledger trade".into(),
        expected_preview_hash: String::new(),
    }
}
fn gui() -> JournalCaller {
    let mut c = journal_caller();
    c.channel = wealthfolio_business::BusinessChannel::Gui;
    c.actor_kind = JournalActorKind::GuiSession;
    c.fact_permissions.activities_write = true;
    c
}

#[tokio::test]
async fn gui_ledger_trade_and_confirmed_revision_and_ownership_commit_atomically() {
    let h = Harness::new().await;
    let (service, target, _) = confirmed(&h).await;
    let c = gui();
    let mut input = ledger_input(&h, target.clone());
    input.expected_preview_hash = service
        .preview_decision_ledger_trade(&input, &c)
        .await
        .unwrap()
        .preview_hash;
    assert_eq!(
        service
            .record_decision_ledger_trade(input.clone(), &journal_caller())
            .await
            .unwrap_err()
            .code(),
        "scope_denied",
        "ordinary program holdings permission cannot create ledger BUY/SELL"
    );
    let result = service
        .record_decision_ledger_trade(input.clone(), &c)
        .await
        .unwrap();
    assert_eq!(result["decisionRevisionId"], target.revision_id);
    assert_eq!(result["ownershipStatus"], "APPLIED");
    assert_eq!(count(&h, "investment_execution_links"), 1);
    assert_eq!(count(&h, "fund_allocation_entries"), 4);
    assert_eq!(
        service
            .record_decision_ledger_trade(input, &c)
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(count(&h, "investment_execution_links"), 1);
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT amount FROM activities WHERE id=?1",
            [result["activityId"].as_str().unwrap()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "101.5"
    );
    assert_eq!(
        service.get_decision(&target.decision_id, &c).unwrap()["executionState"],
        "OPEN"
    );
}

#[tokio::test]
async fn missing_ledger_final_cash_stays_missing_and_needs_reconciliation() {
    let h = Harness::new().await;
    let (service, target, _) = confirmed(&h).await;
    let c = gui();
    let mut input = ledger_input(&h, target);
    input.trade.amount = None;
    input.trade.fee = None;
    input.trade.tax = None;
    input.ownership = DecisionTradeOwnership::ReconcileLater;
    let p = service
        .preview_decision_ledger_trade(&input, &c)
        .await
        .unwrap();
    assert!(!p.trade.automatic_allocation_eligible);
    assert!(p.trade.issues.iter().any(|i| i == "MissingFinalCash"));
    input.expected_preview_hash = p.preview_hash;
    let result = service
        .record_decision_ledger_trade(input, &c)
        .await
        .unwrap();
    assert_eq!(result["ownershipStatus"], "RECONCILIATION_REQUIRED");
    assert_eq!(result["trade"]["netCash"], Value::Null);
    assert_eq!(result["trade"]["settlementCurrency"], Value::Null);
    assert_eq!(
        result["reconciliationResources"].as_array().unwrap().len(),
        1,
        "the known security remains visible while the cash unit is unknown"
    );
    assert_eq!(count(&h, "fund_allocation_entries"), 1);
    assert_eq!(count(&h, "investment_execution_links"), 1);
    let db = h.access.connect_rusqlite().unwrap();
    let row: (Option<String>, i32) = db
        .query_row(
            "SELECT amount,needs_review FROM activities WHERE id=?1",
            [result["activityId"].as_str().unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (None, 1));
    assert_eq!(
        db.query_row("SELECT status FROM investment_executions", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "RECONCILE"
    );
}

#[tokio::test]
async fn read_progress_keeps_confirmation_units_and_rejects_new_split_basis() {
    let h = Harness::new().await;
    let (service, target, _) = confirmed(&h).await;
    let c = journal_caller();
    let state = service
        .investment_journal_account_state("manual", &c)
        .await
        .unwrap();
    assert_eq!(state["accountId"], "manual");
    assert_eq!(state["hasSnapshot"], true);
    let initial = service
        .decision_execution_progress(&target.revision_id, &c)
        .unwrap();
    assert_eq!(initial["quantityUnitStatus"], "VERIFIED_UNCHANGED");
    assert_eq!(initial["remainingQuantity"], "10");

    let mut input = owned(&h, target.clone()).await;
    input.expected_preview_hash = service
        .preview_decision_manual_trade(&input, &c)
        .await
        .unwrap()
        .preview_hash;
    service
        .record_decision_manual_trade(input, &c)
        .await
        .unwrap();
    let linked = service
        .decision_execution_progress(&target.revision_id, &c)
        .unwrap();
    assert_eq!(linked["quantityForPlannedAsset"], "10");
    assert_eq!(linked["settlementCashForPlanCurrency"], "101.5");

    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        r#"UPDATE assets SET metadata='{"contractMultiplier":"unverified"}' WHERE id='stock'"#,
        [],
    )
    .unwrap();
    let malformed = service
        .decision_execution_progress(&target.revision_id, &c)
        .unwrap();
    assert_eq!(malformed["quantityUnitStatus"], "UNVERIFIED_CONTEXT");
    assert_eq!(malformed["quantityForPlannedAsset"], Value::Null);
    assert_eq!(
        malformed["settlementCashForPlanCurrency"],
        linked["settlementCashForPlanCurrency"]
    );
    db.execute(
        r#"UPDATE assets SET metadata='{"contractMultiplier":"1"}' WHERE id='stock'"#,
        [],
    )
    .unwrap();
    assert_eq!(
        service
            .decision_execution_progress(&target.revision_id, &c)
            .unwrap()["quantityForPlannedAsset"],
        "10"
    );

    let date = format!("{}T12:00:00Z", state["asOfDate"].as_str().unwrap());
    h.access.connect_rusqlite().unwrap().execute(
        "INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,currency,quantity,created_at,updated_at) VALUES('synthetic-split','transactions','stock','SPLIT','POSTED',?1,'CNY','2',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
        [date],
    ).unwrap();
    let changed = service
        .decision_execution_progress(&target.revision_id, &c)
        .unwrap();
    assert_eq!(changed["quantityUnitStatus"], "UNVERIFIED_CONTEXT");
    assert_eq!(changed["quantityUnitDate"], initial["quantityUnitDate"]);
    assert_eq!(changed["remainingQuantity"], Value::Null);
    assert_eq!(
        changed["settlementCashForPlanCurrency"],
        linked["settlementCashForPlanCurrency"]
    );
    assert_eq!(count(&h, "investment_execution_links"), 1);
}

#[tokio::test]
async fn cross_account_progress_overflow_keeps_verified_cash_and_open_state() {
    let h = Harness::new().await;
    let (service, decision, _) = confirmed(&h).await;
    let caller = journal_caller();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('other-manual','Other','SECURITIES','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'HOLDINGS')", []).unwrap();
    let other = h.investments.get_state("other-manual").await.unwrap();
    h.investments
        .patch_snapshot(
            wealthfolio_business::investments::PatchHoldingsSnapshot {
                account_id: "other-manual".into(),
                snapshot_date: h.day.clone(),
                expected_state_version: other["stateVersion"].as_str().unwrap().into(),
                positions: vec![],
                cash_balances: std::collections::BTreeMap::from([("CNY".into(), "1000".into())]),
                notes: "Synthetic opening balance".into(),
                request_key: "other-opening".into(),
            },
            wealthfolio_business::BusinessChannel::Gui,
            "session",
            true,
        )
        .await
        .unwrap();
    let large_quantity = "40000000000000000000000000000";
    let accounts = ["manual", "other-manual"];
    for account in accounts {
        let state = h.investments.get_state(account).await.unwrap();
        let mut input = h.trade().await;
        input.account_id = account.into();
        input.expected_state_version = state["stateVersion"].as_str().unwrap().into();
        input.quantity = large_quantity.into();
        input.price = "0.0000000000000000000000000001".into();
        input.fee = Some("0".into());
        input.tax = Some("0".into());
        input.request_key = format!("large-{account}");
        h.investments
            .record_manual_trade(
                input,
                wealthfolio_business::BusinessChannel::Gui,
                "session",
                true,
            )
            .await
            .unwrap();
    }
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    assert_eq!(projector.refresh_batch(64).await.unwrap().remaining, 0);
    let executions = projector.executions().unwrap();
    for (i, account) in accounts.into_iter().enumerate() {
        let execution = executions
            .iter()
            .find(|row| row.account_id == account)
            .unwrap();
        assert_eq!(execution.quantity.unwrap().to_string(), large_quantity);
        assert_eq!(execution.net_cash.unwrap().to_string(), "-4");
        let target = ExecutionLinkTarget {
            execution_id: execution.id.clone(),
            decision_revision_id: decision.revision_id.clone(),
            deviation_reason: None,
        };
        let preview = service
            .preview_execution_link(target.clone(), &caller)
            .unwrap();
        service
            .link_existing_execution(
                LinkExistingExecution {
                    target,
                    expected_preview_hash: preview.preview_hash,
                    request_key: format!("link-large-{i}"),
                },
                &caller,
            )
            .await
            .unwrap();
        let progress = service
            .decision_execution_progress(&decision.revision_id, &caller)
            .unwrap();
        assert_eq!(progress["quantityUnitStatus"], "VERIFIED_UNCHANGED");
        if i == 0 {
            assert_eq!(progress["quantityForPlannedAsset"], large_quantity);
        } else {
            assert_eq!(progress["quantityForPlannedAsset"], Value::Null);
            assert_eq!(progress["remainingQuantity"], Value::Null);
            assert_eq!(progress["settlementCashForPlanCurrency"], "8");
            assert_eq!(progress["remainingCashAmount"], "93.5");
        }
    }
    assert_eq!(
        service
            .get_decision(&decision.decision_id, &caller)
            .unwrap()["executionState"],
        "OPEN"
    );
}
