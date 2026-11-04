//! Unified queue and durable reconciliation receipts on migrated synthetic ledgers.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::{json, Value};
use support::{journal_caller, Harness};
use wealthfolio_business::{investment_journal::*, BusinessChannel};

fn cash() -> FundResource {
    FundResource::Cash {
        account_id: "transactions".into(),
        currency: "CNY".into(),
    }
}
fn gui() -> JournalCaller {
    let mut c = journal_caller();
    c.channel = BusinessChannel::Gui;
    c.actor_kind = JournalActorKind::GuiSession;
    c.fact_permissions.activities_write = true;
    c
}
async fn index(h: &Harness) {
    let p = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    for kind in [
        InvestmentSourceKind::Activity,
        InvestmentSourceKind::ManualTrade,
        InvestmentSourceKind::Snapshot,
    ] {
        let mut cursor = None;
        loop {
            let r = p.backfill_batch(kind.clone(), cursor, 1000).await.unwrap();
            if r.exhausted {
                break;
            }
            cursor = r.last_source_id;
        }
    }
    assert_eq!(p.refresh_batch(1000).await.unwrap().remaining, 0);
}
fn body() -> DecisionBody {
    serde_json::from_value(json!({"title":"Queue decision","action":"BUY","assetId":"stock","plannedQuantity":"10","plannedAmount":"101.5","currency":"CNY","thesis":"Provided rationale","options":[{"action":"Wait","description":"Keep cash"}]})).unwrap()
}
async fn pending(h: &Harness) -> (InvestmentJournalService, String, String) {
    let service = h.journal().with_context_reader(h.context.clone());
    h.seed_purpose("purpose");
    let mut c = gui();
    c.can_confirm = true;
    let p = service.preview_allocations(vec![cash()], &c).await.unwrap();
    service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Allocate,
                reason: "Initial ownership".into(),
                entries: vec![AllocationDelta {
                    purpose_id: "purpose".into(),
                    resource: cash(),
                    delta: ExactDecimal::parse("950").unwrap(),
                }],
                resources: vec![cash()],
                expected_preview_hash: p.preview_hash,
                request_key: "assign".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let r = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body: body(),
                check_schedule: CheckScheduleChange::Keep,
                request_key: "draft".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let p = service
        .preview_decision_confirmation(
            DecisionDraftTarget {
                decision_id: r["decisionId"].as_str().unwrap().into(),
                draft_revision_id: r["draftRevisionId"].as_str().unwrap().into(),
                expected_version: 1,
            },
            &c,
        )
        .await
        .unwrap();
    let r = service
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
    let mut input = RecordDecisionLedgerTrade {
        decision: DecisionExecutionTarget {
            decision_id: r["decisionId"].as_str().unwrap().into(),
            revision_id: r["currentRevisionId"].as_str().unwrap().into(),
            deviation_reason: None,
        },
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
            request_key: "fact-only".into(),
        },
        ownership: DecisionTradeOwnership::ReconcileLater,
        reason: "Leave ownership for explicit reconciliation".into(),
        expected_preview_hash: String::new(),
    };
    input.expected_preview_hash = service
        .preview_decision_ledger_trade(&input, &c)
        .await
        .unwrap()
        .preview_hash;
    let r = service
        .record_decision_ledger_trade(input, &c)
        .await
        .unwrap();
    index(h).await;
    (
        service,
        r["executionId"].as_str().unwrap().into(),
        r["activityId"].as_str().unwrap().into(),
    )
}
fn ack_draft(execution: &str) -> ExecutionReconciliationDraft {
    ExecutionReconciliationDraft {
        execution_id: execution.into(),
        resolution: ExecutionOwnershipResolution::NoOwnershipChange,
        resources: vec![cash()],
        entries: vec![],
    }
}
async fn acknowledge(
    service: &InvestmentJournalService,
    draft: ExecutionReconciliationDraft,
    key: &str,
) -> Value {
    let mut c = journal_caller();
    c.fact_permissions = JournalFactPermissions::default();
    let p = service
        .preview_execution_reconciliation(&draft, &c)
        .await
        .unwrap();
    service
        .reconcile_execution(
            ReconcileExecution {
                draft,
                reason: "Review source; retain explicit ownership choice".into(),
                expected_preview_hash: p.preview_hash,
                request_key: key.into(),
            },
            &c,
        )
        .await
        .unwrap()
}
async fn queue(service: &InvestmentJournalService) -> InvestmentWorkQueue {
    service
        .investment_work_queue(InvestmentWorkQuery::default(), &journal_caller())
        .await
        .unwrap()
}
fn has(q: &InvestmentWorkQueue, kind: &str) -> bool {
    q.items.iter().any(|i| i["kind"] == kind)
}

#[tokio::test]
async fn zero_entry_receipt_closes_pending_ownership_but_never_a_remaining_excess() {
    let h = Harness::new().await;
    let (service, execution, _) = pending(&h).await;
    let before = queue(&service).await;
    assert!(before.complete);
    assert!(has(&before, "EXECUTION_OWNERSHIP_PENDING"));
    assert!(has(&before, "PURPOSE_EXCESS"));
    let mut draft = ack_draft(&execution);
    draft.resources.clear();
    let r = acknowledge(&service, draft, "ack").await;
    assert_eq!(
        r["executionAcknowledgement"]["resolution"],
        "NO_OWNERSHIP_CHANGE"
    );
    let after = queue(&service).await;
    assert!(!has(&after, "EXECUTION_OWNERSHIP_PENDING"));
    assert!(has(&after, "PURPOSE_EXCESS"));
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM investment_execution_links WHERE unlinked_at IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn source_edit_and_deletion_reenter_then_ack_without_overwriting_original_link() {
    let h = Harness::new().await;
    let (service, execution, activity) = pending(&h).await;
    acknowledge(&service, ack_draft(&execution), "first").await;
    let db = h.access.connect_rusqlite().unwrap();
    let original: String = db
        .query_row(
            "SELECT snapshot_json FROM investment_execution_links",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "UPDATE activities SET amount='102.5' WHERE id=?1",
        [&activity],
    )
    .unwrap();
    let p = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    p.refresh_batch(1000).await.unwrap();
    let edited = queue(&service).await;
    assert!(has(&edited, "EXECUTION_SOURCE_CHANGED"));
    assert!(has(&edited, "EXECUTION_OWNERSHIP_PENDING"));
    let preview = service
        .preview_execution_reconciliation(&ack_draft(&execution), &journal_caller())
        .await
        .unwrap();
    assert!(preview.source.source_change.is_some());
    acknowledge(&service, ack_draft(&execution), "edited").await;
    assert!(!has(&queue(&service).await, "EXECUTION_SOURCE_CHANGED"));
    db.execute("DELETE FROM activities WHERE id=?1", [&activity])
        .unwrap();
    p.refresh_batch(1000).await.unwrap();
    assert!(has(&queue(&service).await, "EXECUTION_SOURCE_INVALIDATED"));
    acknowledge(&service, ack_draft(&execution), "deleted").await;
    let result = queue(&service).await;
    assert!(!has(&result, "EXECUTION_SOURCE_INVALIDATED"));
    assert!(!has(&result, "PURPOSE_EXCESS"));
    assert_eq!(
        db.query_row(
            "SELECT snapshot_json FROM investment_execution_links",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        original
    );
}

#[tokio::test]
async fn explicit_adjustment_acknowledgement_is_atomic_replayable_and_requires_fresh_preview() {
    let h = Harness::new().await;
    let (service, execution, activity) = pending(&h).await;
    let mut draft = ack_draft(&execution);
    draft.resolution = ExecutionOwnershipResolution::AdjustOwnership;
    draft.entries = vec![
        AllocationDelta {
            purpose_id: "purpose".into(),
            resource: cash(),
            delta: ExactDecimal::parse("-101.5").unwrap(),
        },
        AllocationDelta {
            purpose_id: "purpose".into(),
            resource: FundResource::Security {
                account_id: "transactions".into(),
                asset_id: "stock".into(),
            },
            delta: ExactDecimal::parse("10").unwrap(),
        },
    ];
    let p = service
        .preview_execution_reconciliation(&draft, &journal_caller())
        .await
        .unwrap();
    let input = ReconcileExecution {
        draft,
        reason: "Apply explicit current ownership, never replay trade conversion".into(),
        expected_preview_hash: p.preview_hash,
        request_key: "adjust".into(),
    };
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_receipt_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    assert!(service
        .reconcile_execution(input.clone(), &journal_caller())
        .await
        .is_err());
    assert!(has(&queue(&service).await, "EXECUTION_OWNERSHIP_PENDING"));
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM fund_allocation_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.execute("DROP TRIGGER fail_receipt_audit", []).unwrap();
    service
        .reconcile_execution(input.clone(), &journal_caller())
        .await
        .unwrap();
    assert_eq!(
        service
            .reconcile_execution(input, &journal_caller())
            .await
            .unwrap()["replayed"],
        true
    );
    let resolved = queue(&service).await;
    assert!(!has(&resolved, "EXECUTION_OWNERSHIP_PENDING"));
    assert!(!has(&resolved, "PURPOSE_EXCESS"));
    let stale = ack_draft(&execution);
    let p = service
        .preview_execution_reconciliation(&stale, &journal_caller())
        .await
        .unwrap();
    db.execute(
        "UPDATE activities SET amount='103.5' WHERE id=?1",
        [&activity],
    )
    .unwrap();
    assert_eq!(
        service
            .reconcile_execution(
                ReconcileExecution {
                    draft: stale,
                    reason: "Old preview".into(),
                    expected_preview_hash: p.preview_hash,
                    request_key: "stale".into()
                },
                &journal_caller()
            )
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
}

#[tokio::test]
async fn history_index_completeness_and_enable_boundary_are_separate_from_ingestion_time() {
    let h = Harness::new().await;
    let service = h.journal();
    let db = h.access.connect_rusqlite().unwrap();
    let old = (chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap()
        - chrono::Duration::days(10))
    .to_string();
    for (id, date) in [("old", &old), ("current", &h.day)] {
        db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES(?1,'transactions','stock','BUY','POSTED',?2,'1','10','10','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,date)).unwrap();
    }
    let p = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    p.backfill_batch(InvestmentSourceKind::Activity, Some("zzzz".into()), 1000)
        .await
        .unwrap();
    assert!(
        !queue(&service).await.historical_backfill_complete,
        "a skipped range cannot prove full backfill"
    );
    index(&h).await;
    let inactive = queue(&service).await;
    assert!(inactive.complete);
    assert!(inactive.enabled_on.is_none());
    assert!(!has(&inactive, "UNLINKED_EXECUTION"));
    let history = service
        .investment_work_queue(
            InvestmentWorkQuery {
                include_historical_executions: true,
                ..Default::default()
            },
            &journal_caller(),
        )
        .await
        .unwrap();
    assert_eq!(
        history
            .items
            .iter()
            .filter(|i| i["kind"] == "UNLINKED_EXECUTION")
            .count(),
        2
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
    service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body: body(),
                check_schedule: CheckScheduleChange::Keep,
                request_key: "enable".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
    let active = queue(&service).await;
    let unlinked: Vec<_> = active
        .items
        .iter()
        .filter(|i| i["kind"] == "UNLINKED_EXECUTION")
        .collect();
    assert_eq!(unlinked.len(), 1);
    assert_eq!(unlinked[0]["execution"]["sourceId"], "current");

    let old_day = old.parse().unwrap();
    for (include_historical_executions, expected) in [(false, 0), (true, 1)] {
        let filtered = service
            .investment_work_queue(
                InvestmentWorkQuery {
                    include_historical_executions,
                    execution_date_from: Some(old_day),
                    execution_date_to: Some(old_day),
                },
                &journal_caller(),
            )
            .await
            .unwrap();
        assert_eq!(
            filtered
                .items
                .iter()
                .filter(|item| item["kind"] == "UNLINKED_EXECUTION")
                .count(),
            expected
        );
    }
}

#[tokio::test]
async fn unified_due_items_only_follow_the_current_draft_and_current_confirmed_reminder() {
    let h = Harness::new().await;
    let (service, _, _) = pending(&h).await;
    let c = journal_caller();
    let root = service
        .list_decisions(DecisionQuery::default(), &c)
        .unwrap()
        .remove(0);
    let id = root["decisionId"].as_str().unwrap().to_string();
    let due = chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
    let scheduled = service
        .mutate_decision(
            MutateDecision {
                decision_id: id.clone(),
                expected_version: root["version"].as_i64().unwrap() as i32,
                mutation: DecisionMutation::SetReviewDate { date: due },
                request_key: "schedule-review".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let draft = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: Some(id.clone()),
                draft_revision_id: None,
                expected_version: Some(scheduled["version"].as_i64().unwrap() as i32),
                body: body(),
                check_schedule: CheckScheduleChange::Set { date: due },
                request_key: "amend-due".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let first = queue(&service).await;
    assert_eq!(
        first
            .items
            .iter()
            .filter(|i| i["kind"] == "DRAFT_CHECK_DUE")
            .count(),
        1
    );
    assert_eq!(
        first
            .items
            .iter()
            .filter(|i| i["kind"] == "REVIEW_DUE")
            .count(),
        1
    );
    let saved = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: Some(id.clone()),
                draft_revision_id: Some(draft["draftRevisionId"].as_str().unwrap().into()),
                expected_version: Some(draft["version"].as_i64().unwrap() as i32),
                body: body(),
                check_schedule: CheckScheduleChange::Keep,
                request_key: "keep-overdue".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert!(has(&queue(&service).await, "DRAFT_CHECK_DUE"));
    service
        .mutate_decision(
            MutateDecision {
                decision_id: id,
                expected_version: saved["version"].as_i64().unwrap() as i32,
                mutation: DecisionMutation::Archive,
                request_key: "archive-due".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let archived = queue(&service).await;
    assert!(!has(&archived, "DRAFT_CHECK_DUE"));
    assert!(!has(&archived, "REVIEW_DUE"));
    assert!(
        has(&archived, "PURPOSE_EXCESS"),
        "archiving an opinion cannot dismiss a resource deficit"
    );
}

#[tokio::test]
async fn default_execution_boundary_uses_the_original_enable_timezone() {
    use std::sync::{Arc, RwLock};
    let h = Harness::new().await;
    let service = h.journal();
    service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body: body(),
                check_schedule: CheckScheduleChange::Keep,
                request_key: "enable-zone".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
    let prior = (chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap()
        - chrono::Duration::days(1))
    .to_string();
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('zone','transactions','stock','BUY','POSTED',?1,'1','10','10','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[format!("{prior}T18:00:00Z")]).unwrap();
    index(&h).await;
    let changed = InvestmentJournalService::new(
        h.store.clone(),
        "ledger".into(),
        h.reader.clone(),
        Arc::new(RwLock::new("America/New_York".into())),
    );
    let result = queue(&changed).await;
    assert_eq!(result.boundary_timezone.as_deref(), Some("Asia/Shanghai"));
    assert!(result
        .items
        .iter()
        .any(|i| i["kind"] == "UNLINKED_EXECUTION" && i["execution"]["sourceId"] == "zone"));
}

#[tokio::test]
async fn malformed_execution_dates_are_reported_without_hiding_the_whole_queue() {
    let h = Harness::new().await;
    let service = h.journal();
    service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body: body(),
                check_schedule: CheckScheduleChange::Set {
                    date: chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
                },
                request_key: "date-quality".into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap();
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('bad-date','transactions','stock','BUY','POSTED','bad-date','1','10','10','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[]).unwrap();
    index(&h).await;
    let result = queue(&service).await;
    assert!(has(&result, "EXECUTION_DATE_UNAVAILABLE"));
    assert!(has(&result, "DRAFT_CHECK_DUE"));
    assert!(!result.complete);
}

#[tokio::test]
async fn a_valid_date_prefix_does_not_hide_malformed_source_timestamp_bytes() {
    let h = Harness::new().await;
    let service = h.journal();
    let c = journal_caller();
    service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body: body(),
                check_schedule: CheckScheduleChange::Set {
                    date: chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
                },
                request_key: "enable-date-quality".into(),
            },
            &c,
        )
        .await
        .unwrap();
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('bad-suffix','transactions','stock','BUY','POSTED',?1,'1','10','10','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[format!("{}malformed",h.day)]).unwrap();
    index(&h).await;
    let result = queue(&service).await;
    assert!(has(&result, "EXECUTION_DATE_UNAVAILABLE"));
    assert!(has(&result, "DRAFT_CHECK_DUE"));
    assert!(!result.complete);
}
