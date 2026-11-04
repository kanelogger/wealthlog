//! Native business records in every journal table survive an encrypted snapshot.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;

use chrono::NaiveDate;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use support::{exact, journal_caller, manual_cash, Harness};
use wealthfolio_business::{investment_journal::*, BusinessStore};
use wealthfolio_storage_sqlite::db::{
    self, write_actor::spawn_writer_with_task, DatabaseOwner, DbAccess, DbEncryptionKey,
};

const TABLES: [&str; 17] = [
    "fund_purposes",
    "fund_allocation_operations",
    "fund_allocation_entries",
    "fund_allocations",
    "investment_decisions",
    "investment_decision_revisions",
    "investment_decision_options",
    "investment_decision_evidence",
    "investment_decision_funding",
    "investment_executions",
    "investment_execution_links",
    "investment_projection_state",
    "investment_reviews",
    "investment_principles",
    "investment_principle_revisions",
    "investment_decision_principles",
    "investment_review_principles",
];

// Compare every column, including exact text, frozen JSON, identities and audit
// metadata. SQL quote() preserves SQLite types and NULL without float coercion.
fn records(access: &DbAccess) -> BTreeMap<String, Vec<Vec<String>>> {
    let db = access.connect_rusqlite().unwrap();
    let mut snapshot: BTreeMap<_, _> = TABLES
        .into_iter()
        .chain([
            "accounts",
            "assets",
            "activities",
            "goals",
            "business_requests",
            "business_audit",
            "app_settings",
            "holdings_snapshots",
            "quotes",
            "__diesel_schema_migrations",
        ])
        .map(|table| {
            let mut stmt = db.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
            let names = stmt
                .column_names()
                .into_iter()
                .map(|name| format!("\"{}\"", name.replace('"', "\"\"")))
                .collect::<Vec<_>>();
            let select = names
                .iter()
                .map(|name| format!("quote({name})"))
                .collect::<Vec<_>>()
                .join(",");
            stmt = db
                .prepare(&format!(
                    "SELECT {select} FROM \"{table}\" ORDER BY {}",
                    names.join(",")
                ))
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    (0..names.len())
                        .map(|i| row.get::<_, String>(i))
                        .collect::<Result<Vec<_>, _>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (table.into(), rows)
        })
        .collect();
    let mut schema = db.prepare("SELECT type,name,quote(sql) FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name").unwrap();
    let objects = schema
        .query_map([], |row| {
            (0..3)
                .map(|i| row.get::<_, String>(i))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    snapshot.insert("schema".into(), objects);
    snapshot
}

// Replayed requests and immutable reads must work after reopening even before
// accounting providers initialize; any accidental fresh observation fails.
struct NoAccountingOnReplay;
#[async_trait::async_trait]
impl InvestmentResourceReader for NoAccountingOnReplay {
    async fn observe(
        &self,
        _: &[FundResource],
        _: NaiveDate,
    ) -> JournalResult<Vec<ResourceObservation>> {
        panic!("restored request replay must not observe current accounting")
    }
    fn validate(
        &self,
        _: &mut diesel::SqliteConnection,
        _: &[ResourceObservation],
    ) -> JournalResult<()> {
        panic!("restored request replay must not validate new facts")
    }
}

async fn publish(
    service: &InvestmentJournalService,
    caller: &JournalCaller,
    source: Option<String>,
    key: &str,
) -> Value {
    let saved = service.save_principle_draft(SavePrincipleDraft {
        principle_id: None, expected_draft_revision_id: None, expected_version: None,
        body: serde_json::from_value(json!({"name":"Synthetic rule", "content":"Verify original sources before allocating resources.", "sourceReviewId":source, "scenarioTags":["synthetic"], "contentOrigin":"USER_PROVIDED"})).unwrap(),
        request_key: format!("{key}-draft"),
    }, caller).await.unwrap();
    let shown = service
        .preview_principle_publication(
            PrincipleDraftTarget {
                principle_id: saved["principleId"].as_str().unwrap().into(),
                draft_revision_id: saved["draftRevisionId"].as_str().unwrap().into(),
                expected_version: saved["version"].as_i64().unwrap() as i32,
            },
            caller,
        )
        .unwrap();
    service
        .publish_principle(
            PublishPrinciple {
                ledger_id: shown.ledger_id,
                target: shown.target,
                expected_content_hash: shown.expected_content_hash,
                expected_source_context_hash: shown.expected_source_context_hash,
                source_context: shown.source_context,
                request_key: format!("{key}-publish"),
            },
            caller,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn populated_journal_encrypted_backup_restores_all_rows_constraints_and_request_replay() {
    let h = Harness::encrypted().await;
    h.initialize_manual().await;
    let service = h
        .journal()
        .with_context_reader(h.context.clone())
        .with_review_facts_reader(h.context.clone());
    let mut caller = journal_caller();
    caller.can_confirm = true;
    let purpose_input: SaveFundPurpose = serde_json::from_value(json!({
        "name":"Synthetic backup purpose", "category":"CUSTOM", "currency":"CNY",
        "targetAmount":"9007199254740993.123456789", "lossBudget":"20.00000000000000000000000001", "requestKey":"purpose",
    })).unwrap();
    let purpose = service
        .save_purpose(purpose_input.clone(), &caller)
        .await
        .unwrap();
    let purpose_id = purpose["purposeId"].as_str().unwrap();
    let shown = service
        .preview_allocations(vec![manual_cash()], &caller)
        .await
        .unwrap();
    let allocation = ApplyFundAllocation {
        operation_kind: AllocationOperationKind::Allocate,
        reason: "Synthetic explicit ownership".into(),
        entries: vec![AllocationDelta {
            purpose_id: purpose_id.into(),
            resource: manual_cash(),
            delta: exact("500.1234567890123456789012345"),
        }],
        resources: vec![manual_cash()],
        expected_preview_hash: shown.preview_hash,
        request_key: "allocation".into(),
    };
    service
        .apply_allocation(allocation.clone(), &caller)
        .await
        .unwrap();
    let principle = publish(&service, &caller, None, "first-rule").await;
    let principle_revision = principle["currentRevisionId"].as_str().unwrap();
    let saved = service.save_decision_draft(SaveDecisionDraft {
        decision_id:None, draft_revision_id:None, expected_version:None,
        body:serde_json::from_value(json!({
            "title":"Synthetic confirmed decision", "action":"BUY", "assetId":"stock", "plannedQuantity":"10", "plannedAmount":"101.5", "currency":"CNY", "thesis":"User supplied synthetic rationale", "purposeIds":[purpose_id], "principleRevisionIds":[principle_revision], "fundingStatus":"PLANNED",
            "options":[{"action":"WAIT","description":"Verify fees first","notSelectedReason":"Synthetic alternative reason"}],
            "evidence":[{"informationKind":"HYPOTHESIS","content":"Synthetic comparison scenario","purposeId":purpose_id,"assumedLossAmount":"10.1234567890123456789","lossCurrency":"CNY","lossBasis":"User supplied scenario, not a forecast"}],
            "funding":[{"purposeId":purpose_id,"resource":manual_cash(),"planned":"101.5"}], "scenarioTags":["synthetic"], "contentOrigin":"USER_PROVIDED",
        })).unwrap(), check_schedule:CheckScheduleChange::Keep, request_key:"decision".into(),
    }, &caller).await.unwrap();
    let shown = service
        .preview_decision_confirmation(
            DecisionDraftTarget {
                decision_id: saved["decisionId"].as_str().unwrap().into(),
                draft_revision_id: saved["draftRevisionId"].as_str().unwrap().into(),
                expected_version: saved["version"].as_i64().unwrap() as i32,
            },
            &caller,
        )
        .await
        .unwrap();
    let decision = service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: shown.ledger_id,
                target: shown.target,
                expected_content_hash: shown.expected_content_hash,
                expected_context_hash: shown.expected_context_hash,
                context: shown.context,
                request_key: "confirm".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    let decision_id = decision["decisionId"].as_str().unwrap();
    let revision_id = decision["currentRevisionId"].as_str().unwrap();
    let mut trade = RecordDecisionManualTrade {
        decision: DecisionExecutionTarget {
            decision_id: decision_id.into(),
            revision_id: revision_id.into(),
            deviation_reason: None,
        },
        trade: h.trade().await,
        ownership: DecisionTradeOwnership::Apply {
            shares: vec![TradePurposeShare {
                purpose_id: purpose_id.into(),
                quantity: exact("10"),
                net_cash: exact("-101.5"),
                top_ups: vec![],
            }],
        },
        reason: "Synthetic already-executed trade".into(),
        expected_preview_hash: String::new(),
    };
    trade.expected_preview_hash = service
        .preview_decision_manual_trade(&trade, &caller)
        .await
        .unwrap()
        .preview_hash;
    service
        .record_decision_manual_trade(trade.clone(), &caller)
        .await
        .unwrap();
    let draft = service.save_review_draft(SaveReviewDraft {
        review_id:None, root_id:None, expected_version:None, decision_revision_id:revision_id.into(), observation_cutoff:NaiveDate::parse_from_str(&h.day,"%Y-%m-%d").unwrap(),
        body:serde_json::from_value(json!({"judgment":"Synthetic review judgment", "executionAssessment":"Observe only linked facts", "otherExplanations":"No causal claim", "outcomeEvidence":[{"informationKind":"UNKNOWN","content":"Synthetic outcome remains uncertain"}], "principles":[{"principleRevisionId":principle_revision,"disposition":"KEEP","reason":"Synthetic explicit assessment"}], "scenarioTags":["synthetic"], "contentOrigin":"USER_PROVIDED"})).unwrap(), request_key:"review".into(),
    }, &caller).await.unwrap();
    let review_id = draft["reviewId"].as_str().unwrap();
    let shown = service
        .preview_review_submission(
            ReviewDraftTarget {
                review_id: review_id.into(),
                expected_version: draft["version"].as_i64().unwrap() as i32,
            },
            &caller,
        )
        .await
        .unwrap();
    service
        .submit_review(
            SubmitReview {
                ledger_id: shown.ledger_id,
                target: shown.target,
                expected_content_hash: shown.expected_content_hash,
                expected_facts_hash: shown.expected_facts_hash,
                facts: shown.facts,
                request_key: "submit-review".into(),
            },
            &caller,
        )
        .await
        .unwrap();
    let sourced_rule = publish(&service, &caller, Some(review_id.into()), "review-rule").await;
    h.access.connect_rusqlite().unwrap().execute_batch("INSERT INTO goals(id,title,target_amount,goal_type,status_lifecycle,status_health,priority,created_at,updated_at) VALUES('legacy-goal','Keep existing goal',100,'custom_save_up','active','not_applicable',1,'2026-01-01','2026-01-01');").unwrap();

    let before = records(&h.access);
    for table in TABLES {
        assert!(
            !before[table].is_empty(),
            "unpopulated journal table {table}"
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let backup = db::backup_database(&h.access, directory.path().to_str().unwrap()).unwrap();
    assert!(!std::fs::read(&backup)
        .unwrap()
        .starts_with(b"SQLite format 3\0"));
    let mut later = purpose_input;
    later.id = Some(purpose_id.into());
    later.expected_version = Some(1);
    later.name = "Changed only after the backup".into();
    later.request_key = "purpose-after-backup".into();
    service.save_purpose(later, &caller).await.unwrap();
    assert_ne!(records(&h.access), before);
    // Restore this self-contained file to a distinct empty database location.
    let restored_path = directory.path().join("restored/app.db");
    std::fs::create_dir(restored_path.parent().unwrap()).unwrap();
    std::fs::copy(&backup, &restored_path).unwrap();
    let restored = DbAccess::new(restored_path.to_str().unwrap(), h.access.key().cloned());
    assert!(restored.is_encrypted());
    for unauthorized in [
        DbAccess::plaintext(restored.path()),
        DbAccess::encrypted(restored.path(), Arc::new(DbEncryptionKey::generate())),
    ] {
        let denied = unauthorized.connect_rusqlite().and_then(|conn| {
            conn.query_row("SELECT COUNT(*) FROM fund_purposes", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| wealthfolio_core::errors::Error::Unexpected(error.to_string()))
        });
        assert!(
            denied.is_err(),
            "the encrypted schema must not be readable without the retained key"
        );
    }
    let owner = DatabaseOwner::acquire(restored.path()).unwrap();
    restored
        .run_migrations_with_backup(directory.path().to_str().unwrap(), &owner)
        .unwrap();
    assert_eq!(records(&restored), before);
    let conn = restored.connect_rusqlite().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(conn
        .execute(
            "UPDATE investment_decision_revisions SET thesis='rewrite' WHERE status='CONFIRMED'",
            []
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE investment_reviews SET judgment='rewrite' WHERE status='SUBMITTED'",
            []
        )
        .is_err());
    assert!(conn
        .execute("DELETE FROM fund_allocation_entries", [])
        .is_err());
    drop(conn);
    let pool = restored.create_pool().unwrap();
    let (writer, task) = spawn_writer_with_task(pool.as_ref().clone()).unwrap();
    let reopened = InvestmentJournalService::new(
        BusinessStore::new(pool, writer.clone()),
        "ledger".into(),
        Arc::new(NoAccountingOnReplay),
        Arc::new(RwLock::new("Asia/Shanghai".into())),
    );
    assert_eq!(
        reopened.get_decision(decision_id, &caller).unwrap(),
        service.get_decision(decision_id, &caller).unwrap()
    );
    assert_eq!(
        reopened.get_review(review_id, &caller).unwrap(),
        service.get_review(review_id, &caller).unwrap()
    );
    let rule_id = sourced_rule["principleId"].as_str().unwrap();
    assert_eq!(
        reopened.get_principle(rule_id, &caller).unwrap(),
        service.get_principle(rule_id, &caller).unwrap()
    );
    assert_eq!(
        reopened
            .apply_allocation(allocation, &caller)
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(
        reopened
            .record_decision_manual_trade(trade, &caller)
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(
        records(&restored),
        before,
        "replay may not append facts, evidence, ownership, requests or audit rows"
    );
    drop(reopened);
    writer.shutdown().await;
    task.join().await;
}
