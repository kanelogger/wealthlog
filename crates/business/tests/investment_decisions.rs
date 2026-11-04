//! M2 state/authorization proofs against migrated synthetic SQLite ledgers.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;

use async_trait::async_trait;
use chrono::NaiveDate;
use diesel::SqliteConnection;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, RwLock,
};
use support::{journal_caller, Harness};
use wealthfolio_business::investment_journal::*;

struct Context {
    version: RwLock<String>,
    value: RwLock<String>,
    ticks: AtomicUsize,
}
impl Context {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            version: RwLock::new("v1".into()),
            value: RwLock::new("9007199254740993.01".into()),
            ticks: AtomicUsize::new(0),
        })
    }
}
#[async_trait]
impl InvestmentContextReader for Context {
    async fn capture(
        &self,
        query: &InvestmentContextQuery,
        day: NaiveDate,
    ) -> JournalResult<InvestmentContextCapture> {
        let tick = self.ticks.fetch_add(1, Ordering::SeqCst);
        let block = InvestmentContextBlock {
            scope: json!({"accounts":"ALL_UNARCHIVED"}),
            as_of_date: day,
            source_data_as_of: Some(day.to_string()),
            currency: "CNY".into(),
            quality: ContextQuality::Partial,
            stale: false,
            issues: vec![InvestmentContextIssue {
                code: "MISSING_FX".into(),
                source_id: Some("asset".into()),
                currency: Some("USD".into()),
            }],
            data: json!({"total":null,"validChild":self.value.read().unwrap().clone(),"calculatedAt":format!("refresh-{tick}")}),
        };
        Ok(InvestmentContextCapture {
            schema_version: 1,
            response: InvestmentContextResponse {
                schema_version: 1,
                query: query.clone(),
                as_of_date: day,
                currency: "CNY".into(),
                funding: block.clone(),
                recent_cash_flow: block.clone(),
                household: block,
            },
            source_version: self.version.read().unwrap().clone(),
        })
    }
    fn validate(
        &self,
        _: &mut SqliteConnection,
        capture: &InvestmentContextCapture,
    ) -> JournalResult<()> {
        if capture.source_version != *self.version.read().unwrap() {
            return Err(JournalError::StateConflict);
        }
        Ok(())
    }
}
fn body() -> DecisionBody {
    serde_json::from_value(json!({"title":"研究后的决定","action":"BUY","assetId":"stock","plannedAmount":"100.01","plannedQuantity":"2","currency":"CNY","thesis":"用户提供的理由","declaredAt":"2000-01-01","scenarioTags":[" 波动 ","研究","波动"],"options":[{"action":"继续等待","description":"保留现金","notSelectedReason":"用户暂未提供进一步细节"}],"contentOrigin":"ASSISTED_DRAFT"})).unwrap()
}
fn save(body: DecisionBody, key: &str) -> SaveDecisionDraft {
    SaveDecisionDraft {
        decision_id: None,
        draft_revision_id: None,
        expected_version: None,
        body,
        check_schedule: CheckScheduleChange::Keep,
        request_key: key.into(),
    }
}
fn target(result: &Value) -> DecisionDraftTarget {
    DecisionDraftTarget {
        decision_id: result["decisionId"].as_str().unwrap().into(),
        draft_revision_id: result["draftRevisionId"].as_str().unwrap().into(),
        expected_version: result["version"].as_i64().unwrap() as i32,
    }
}
fn confirmation(p: DecisionConfirmationPreview, key: &str) -> ConfirmDecisionDraft {
    ConfirmDecisionDraft {
        ledger_id: p.ledger_id,
        target: p.target,
        expected_content_hash: p.expected_content_hash,
        expected_context_hash: p.expected_context_hash,
        context: p.context,
        request_key: key.into(),
    }
}
fn count(h: &Harness, table: &str) -> i64 {
    h.access
        .connect_rusqlite()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn confirming() -> JournalCaller {
    let mut c = journal_caller();
    c.can_confirm = true;
    c
}

#[tokio::test]
async fn ordinary_write_cannot_confirm_and_declared_origin_cannot_spoof_host_identity() {
    let h = Harness::new().await;
    let context = Context::new();
    let service = h.journal().with_context_reader(context);
    let c = journal_caller();
    assert!(service
        .list_decisions(DecisionQuery::default(), &c)
        .unwrap()
        .is_empty());
    assert_eq!(count(&h, "business_requests"), 0);
    let mut draft = body();
    let mut other_option = draft.options[0].clone();
    other_option.action = "分批买入".into();
    other_option.not_selected_reason = None;
    draft.options.push(other_option);
    let r = service
        .save_decision_draft(save(draft, "draft"), &c)
        .await
        .unwrap();
    let p = service
        .preview_decision_confirmation(target(&r), &c)
        .await
        .unwrap();
    let input = confirmation(p, "confirm");
    assert_eq!(
        service
            .confirm_decision_draft(input.clone(), &c)
            .await
            .unwrap_err()
            .code(),
        "scope_denied"
    );
    let done = service
        .confirm_decision_draft(input.clone(), &confirming())
        .await
        .unwrap();
    assert_eq!(done["revisionNumber"], 1);
    assert_eq!(done["version"], 2);
    let detail = service
        .get_decision(r["decisionId"].as_str().unwrap(), &c)
        .unwrap();
    let rev = &detail["revisions"][0];
    assert_eq!(detail["lifecycle"], "CONFIRMED");
    assert_eq!(detail["executionState"], "OPEN");
    assert_eq!(rev["createdActorKind"], "PROGRAM");
    assert_eq!(rev["confirmedActorKind"], "PROGRAM");
    assert_eq!(rev["confirmedActorId"], "agent");
    assert_eq!(rev["body"]["declaredAt"], "2000-01-01");
    assert!(!rev["createdAt"].as_str().unwrap().starts_with("2000"));
    assert!(!rev["confirmedAt"].as_str().unwrap().starts_with("2000"));
    assert_eq!(rev["confirmedCredentialId"], "token");
    assert_eq!(rev["contentOrigin"], "ASSISTED_DRAFT");
    assert_eq!(rev["body"]["scenarioTags"], json!(["波动", "研究"]));
    assert_eq!(
        rev["body"]["options"][0]["notSelectedReason"],
        "用户暂未提供进一步细节"
    );
    assert_eq!(rev["body"]["options"][1]["notSelectedReason"], Value::Null);
    assert_eq!(rev["body"]["informationValue"], Value::Null);
    assert_eq!(rev["body"]["waitingCost"], Value::Null);
    assert_eq!(
        rev["context"]["response"]["household"]["data"]["validChild"],
        "9007199254740993.01"
    );
    assert_eq!(
        rev["context"]["response"]["household"]["quality"],
        "PARTIAL"
    );
    assert_eq!(
        rev["context"]["response"]["household"]["data"]["calculatedAt"], "refresh-0",
        "freeze exactly the displayed capture, not confirmation's refresh"
    );
    assert_eq!(
        service
            .confirm_decision_draft(input, &confirming())
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(count(&h, "investment_decision_revisions"), 1);
    assert_eq!(count(&h, "business_audit"), 2);
    let db = h.access.connect_rusqlite().unwrap();
    for sql in [
        "UPDATE investment_decision_revisions SET thesis='overwrite'",
        "DELETE FROM investment_decision_revisions",
        "UPDATE investment_decision_options SET not_selected_reason='invented'",
        "DELETE FROM investment_decision_options",
    ] {
        assert!(db.execute(sql, []).is_err(), "{sql}");
    }
    let mut bad = serde_json::to_value(save(body(), "spoof")).unwrap();
    bad["state"] = json!("CONFIRMED");
    assert!(serde_json::from_value::<SaveDecisionDraft>(bad).is_err());
    let mut bad = serde_json::to_value(ConfirmDecisionDraft {
        ledger_id: "ledger".into(),
        target: target(&r),
        expected_content_hash: "x".into(),
        expected_context_hash: "y".into(),
        context: Context::new()
            .capture(
                &InvestmentContextQuery {
                    purpose_ids: vec![],
                    account_ids: vec![],
                },
                NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            )
            .await
            .unwrap(),
        request_key: "spoof".into(),
    })
    .unwrap();
    bad["body"] = json!({"thesis":"replacement"});
    assert!(serde_json::from_value::<ConfirmDecisionDraft>(bad).is_err());
}

#[tokio::test]
async fn due_check_survives_save_amendment_and_has_explicit_exit_paths() {
    let h = Harness::new().await;
    let service = h.journal().with_context_reader(Context::new());
    let c = confirming();
    let mut input = save(body(), "initial");
    input.check_schedule = CheckScheduleChange::Set {
        date: NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
    };
    let r = service.save_decision_draft(input, &c).await.unwrap();
    let mut t = target(&r);
    assert_eq!(service.decision_draft_checks(&c).unwrap().len(), 1);
    let mut edit = save(body(), "save-again");
    edit.decision_id = Some(t.decision_id.clone());
    edit.draft_revision_id = Some(t.draft_revision_id.clone());
    edit.expected_version = Some(t.expected_version);
    edit.body.thesis = "修订理由".into();
    let r = service.save_decision_draft(edit, &c).await.unwrap();
    t = target(&r);
    assert_eq!(service.decision_draft_checks(&c).unwrap().len(), 1);
    for (schedule, key, due) in [
        (
            CheckScheduleChange::Set {
                date: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
            },
            "postpone",
            0,
        ),
        (CheckScheduleChange::Cancel, "cancel-check", 0),
        (
            CheckScheduleChange::Set {
                date: NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
            },
            "check-again",
            1,
        ),
    ] {
        let mut edit = save(body(), key);
        edit.decision_id = Some(t.decision_id.clone());
        edit.draft_revision_id = Some(t.draft_revision_id.clone());
        edit.expected_version = Some(t.expected_version);
        edit.check_schedule = schedule;
        t = target(&service.save_decision_draft(edit, &c).await.unwrap());
        assert_eq!(service.decision_draft_checks(&c).unwrap().len(), due);
    }
    let p = service
        .preview_decision_confirmation(t.clone(), &c)
        .await
        .unwrap();
    let done = service
        .confirm_decision_draft(confirmation(p, "confirm"), &c)
        .await
        .unwrap();
    assert!(service.decision_draft_checks(&c).unwrap().is_empty());
    assert_eq!(
        service.get_decision(&t.decision_id, &c).unwrap()["nextReviewAt"],
        Value::Null
    );
    let mut amendment = save(body(), "amend");
    amendment.decision_id = Some(t.decision_id.clone());
    amendment.expected_version = Some(done["version"].as_i64().unwrap() as i32);
    amendment.check_schedule = CheckScheduleChange::Set {
        date: NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
    };
    let amended = service.save_decision_draft(amendment, &c).await.unwrap();
    t = target(&amended);
    assert_eq!(amended["revisionNumber"], 2);
    let detail = service.get_decision(&t.decision_id, &c).unwrap();
    assert_eq!(detail["currentRevisionId"], done["currentRevisionId"]);
    assert_eq!(detail["executionState"], "OPEN");
    assert_eq!(service.decision_draft_checks(&c).unwrap().len(), 1);
    let mutate = |mutation, key: &str, version| MutateDecision {
        decision_id: t.decision_id.clone(),
        expected_version: version,
        mutation,
        request_key: key.into(),
    };
    let archived = service
        .mutate_decision(
            mutate(DecisionMutation::Archive, "archive", t.expected_version),
            &c,
        )
        .await
        .unwrap();
    assert!(service.decision_draft_checks(&c).unwrap().is_empty());
    let restored = service
        .mutate_decision(
            mutate(
                DecisionMutation::Restore,
                "restore",
                archived["version"].as_i64().unwrap() as i32,
            ),
            &c,
        )
        .await
        .unwrap();
    assert_eq!(service.decision_draft_checks(&c).unwrap().len(), 1);
    let discard = service
        .mutate_decision(
            mutate(
                DecisionMutation::DiscardDraft {
                    draft_revision_id: t.draft_revision_id.clone(),
                },
                "discard",
                restored["version"].as_i64().unwrap() as i32,
            ),
            &c,
        )
        .await
        .unwrap();
    assert!(service.decision_draft_checks(&c).unwrap().is_empty());
    let detail = service.get_decision(&t.decision_id, &c).unwrap();
    assert_eq!(detail["currentRevisionId"], done["currentRevisionId"]);
    assert_eq!(detail["draftRevisionId"], Value::Null);
    assert_eq!(detail["revisions"][1]["status"], "DISCARDED");
    let closed = service
        .mutate_decision(
            mutate(
                DecisionMutation::Close {
                    reason: "明确取消剩余成交".into(),
                },
                "close",
                discard["version"].as_i64().unwrap() as i32,
            ),
            &c,
        )
        .await
        .unwrap();
    service
        .mutate_decision(
            mutate(
                DecisionMutation::SetReviewDate {
                    date: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
                },
                "review",
                closed["version"].as_i64().unwrap() as i32,
            ),
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        service.get_decision(&t.decision_id, &c).unwrap()["closeReason"],
        "明确取消剩余成交"
    );
}

#[tokio::test]
async fn stale_content_context_concurrent_save_and_audit_failure_cannot_partially_confirm() {
    let h = Harness::new().await;
    let context = Context::new();
    let service = h.journal().with_context_reader(context.clone());
    let c = confirming();
    let r = service
        .save_decision_draft(save(body(), "draft"), &c)
        .await
        .unwrap();
    let t = target(&r);
    let p = service
        .preview_decision_confirmation(t.clone(), &c)
        .await
        .unwrap();
    let original = confirmation(p, "confirm");
    let mut bad = original.clone();
    bad.expected_content_hash = "fake".into();
    assert_eq!(
        service
            .confirm_decision_draft(bad, &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let mut bad = original.clone();
    bad.context.response.household.data["validChild"] = json!("1");
    assert_eq!(
        service
            .confirm_decision_draft(bad, &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    *context.value.write().unwrap() = "2".into();
    *context.version.write().unwrap() = "v2".into();
    assert_eq!(
        service
            .confirm_decision_draft(original, &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let p = service
        .preview_decision_confirmation(t.clone(), &c)
        .await
        .unwrap();
    let input = confirmation(p, "confirm-current");
    h.access.connect_rusqlite().unwrap().execute_batch("CREATE TRIGGER fail_decision_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(service
        .confirm_decision_draft(input.clone(), &c)
        .await
        .is_err());
    let detail = service.get_decision(&t.decision_id, &c).unwrap();
    assert_eq!(detail["lifecycle"], "DRAFT");
    assert_eq!(detail["revisions"][0]["status"], "DRAFT");
    assert_eq!(count(&h, "business_requests"), 1);
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_decision_audit")
        .unwrap();
    let mut edit = save(body(), "edit");
    edit.decision_id = Some(t.decision_id);
    edit.draft_revision_id = Some(t.draft_revision_id);
    edit.expected_version = Some(t.expected_version);
    let mut edit2 = edit.clone();
    edit2.request_key = "concurrent".into();
    let (one, two) = tokio::join!(
        service.save_decision_draft(edit, &c),
        service.save_decision_draft(edit2, &c)
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert_eq!(one.err().or(two.err()).unwrap().code(), "state_conflict");
    assert_eq!(
        service
            .confirm_decision_draft(input, &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    assert_eq!(count(&h, "investment_decision_revisions"), 1);
}

#[tokio::test]
async fn field_validation_loss_scenarios_and_exact_tags_preserve_unknowns() {
    let h = Harness::new().await;
    h.seed_purpose("purpose");
    let service = h.journal().with_context_reader(Context::new());
    let c = confirming();
    let mut b = body();
    b.thesis.clear();
    b.options.clear();
    let r = service
        .save_decision_draft(save(b, "incomplete"), &c)
        .await
        .unwrap();
    assert_eq!(
        service
            .preview_decision_confirmation(target(&r), &c)
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    let mut b = body();
    b.purpose_ids = vec!["purpose".into()];
    b.options[0].not_selected_reason = None;
    b.evidence=vec![serde_json::from_value(json!({"informationKind":"HYPOTHESIS","content":"用户自定情景","purposeId":"purpose","assumedLossAmount":"20.05","lossCurrency":"CNY","lossBasis":"仅作为比较假设"})).unwrap()];
    let r = service
        .save_decision_draft(save(b.clone(), "scenario"), &c)
        .await
        .unwrap();
    let p = service
        .preview_decision_confirmation(target(&r), &c)
        .await
        .unwrap();
    assert_eq!(p.body.options[0].not_selected_reason, None);
    assert_eq!(p.context.response.query.purpose_ids, vec!["purpose"]);
    b.evidence[0].loss_currency = Some("USD".into());
    assert_eq!(
        service
            .save_decision_draft(save(b.clone(), "wrong-currency"), &c)
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    b.evidence[0].loss_currency = Some("CNY".into());
    b.evidence[0].probability_lower_bps = Some(5000);
    assert!(service
        .save_decision_draft(save(b.clone(), "partial-prob"), &c)
        .await
        .is_err());
    b.evidence[0].probability_lower_bps = None;
    b.evidence.push(b.evidence[0].clone());
    assert!(service
        .save_decision_draft(save(b, "duplicate-scenario"), &c)
        .await
        .is_err());
    let query = |tags: Vec<&str>| DecisionQuery {
        scenario_tags: tags.into_iter().map(str::to_string).collect(),
        ..Default::default()
    };
    assert_eq!(
        service
            .list_decisions(query(vec!["波动", "研究"]), &c)
            .unwrap()
            .len(),
        2
    );
    assert!(service
        .list_decisions(query(vec!["动"]), &c)
        .unwrap()
        .is_empty());
    assert!(service
        .list_decisions(query(vec!["波动", "缺失"]), &c)
        .unwrap()
        .is_empty());
    assert!(service.list_decisions(query(vec![" "]), &c).is_err());
    assert_eq!(
        service.decision_scenario_tags(&c).unwrap(),
        vec!["波动", "研究"]
    );
    assert!(serde_json::from_value::<SaveDecisionDraft>(
        json!({"body":{"status":"CONFIRMED"},"requestKey":"x"})
    )
    .is_err());
}

#[tokio::test]
async fn existing_execution_link_and_unlink_leave_facts_and_ownership_unchanged() {
    let h = Harness::new().await;
    let service = h.journal().with_context_reader(Context::new());
    let c = confirming();
    let r = service
        .save_decision_draft(save(body(), "draft"), &c)
        .await
        .unwrap();
    let p = service
        .preview_decision_confirmation(target(&r), &c)
        .await
        .unwrap();
    let done = service
        .confirm_decision_draft(confirmation(p, "confirm"), &c)
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES('buy','transactions','stock','BUY','POSTED',?1,'10','10','101.5','1','0.5','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&h.day]).unwrap();
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    projector.refresh_batch(100).await.unwrap();
    let execution = projector
        .executions()
        .unwrap()
        .into_iter()
        .find(|e| e.source_id == "buy")
        .unwrap();
    let target = ExecutionLinkTarget {
        execution_id: execution.id.clone(),
        decision_revision_id: done["currentRevisionId"].as_str().unwrap().into(),
        deviation_reason: None,
    };
    let preview = service.preview_execution_link(target.clone(), &c).unwrap();
    assert_eq!(preview.temporal_classification, "UNDETERMINED");
    let activities_before = count(&h, "activities");
    let input = LinkExistingExecution {
        target: target.clone(),
        expected_preview_hash: preview.preview_hash,
        request_key: "link".into(),
    };
    let linked = service
        .link_existing_execution(input.clone(), &c)
        .await
        .unwrap();
    assert_eq!(linked["ownershipChanged"], false);
    assert_eq!(linked["factsChanged"], false);
    assert_eq!(
        service.link_existing_execution(input, &c).await.unwrap()["replayed"],
        true
    );
    assert_eq!(
        service
            .preview_execution_link(target.clone(), &c)
            .unwrap_err()
            .code(),
        "already_applied"
    );
    assert_eq!(count(&h, "activities"), activities_before);
    assert_eq!(count(&h, "fund_allocation_operations"), 0);
    assert_eq!(count(&h, "fund_allocation_entries"), 0);
    let links = service
        .decision_execution_links(r["decisionId"].as_str().unwrap(), &c)
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["snapshot"]["execution"]["netCash"], "-101.5");
    // A new unconfirmed draft does not alter the linked v1 or its evidence snapshot.
    let mut amendment = save(body(), "amend");
    amendment.decision_id = Some(r["decisionId"].as_str().unwrap().into());
    amendment.expected_version = Some(done["version"].as_i64().unwrap() as i32);
    amendment.body.action = Some(DecisionAction::Sell);
    service.save_decision_draft(amendment, &c).await.unwrap();
    assert_eq!(
        service
            .decision_execution_links(r["decisionId"].as_str().unwrap(), &c)
            .unwrap()[0]["decisionRevisionId"],
        done["currentRevisionId"]
    );
    let unlink = UnlinkExecution {
        link_id: linked["linkId"].as_str().unwrap().into(),
        expected_linked_at: linked["linkedAt"].as_str().unwrap().into(),
        reason: "解除证据关联，用途需单独核对".into(),
        request_key: "unlink".into(),
    };
    service.unlink_execution(unlink.clone(), &c).await.unwrap();
    assert_eq!(
        service.unlink_execution(unlink, &c).await.unwrap()["replayed"],
        true
    );
    assert_eq!(count(&h, "fund_allocation_entries"), 0);
    assert_eq!(count(&h, "activities"), activities_before);
    let new_preview = service.preview_execution_link(target.clone(), &c).unwrap();
    // Original source edits invalidate a previously shown association preview, even before refresh.
    db.execute("UPDATE activities SET amount='102' WHERE id='buy'", [])
        .unwrap();
    assert_eq!(
        service
            .link_existing_execution(
                LinkExistingExecution {
                    target,
                    expected_preview_hash: new_preview.preview_hash,
                    request_key: "stale-link".into()
                },
                &c
            )
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let links = service
        .decision_execution_links(r["decisionId"].as_str().unwrap(), &c)
        .unwrap();
    assert_eq!(links.len(), 1);
    assert!(links[0]["unlinkedAt"].is_string());
    assert_eq!(links[0]["snapshot"]["execution"]["netCash"], "-101.5");
}

#[tokio::test]
async fn failed_first_draft_leaves_no_enable_marker_or_partial_children() {
    let h = Harness::new().await;
    let service = h.journal();
    let c = journal_caller();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_decision_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    let input = save(body(), "first");
    assert!(service
        .save_decision_draft(input.clone(), &c)
        .await
        .is_err());
    for table in [
        "investment_decisions",
        "investment_decision_revisions",
        "investment_decision_options",
        "business_requests",
    ] {
        assert_eq!(count(&h, table), 0, "{table}");
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
    db.execute("DROP TRIGGER fail_decision_audit", []).unwrap();
    service.save_decision_draft(input, &c).await.unwrap();
    assert_eq!(count(&h, "investment_decision_options"), 1);
    // No context provider is configured: drafts still work and no invented complete context is returned.
    assert_eq!(
        service
            .get_investment_context(
                InvestmentContextQuery {
                    purpose_ids: vec![],
                    account_ids: vec![]
                },
                &c
            )
            .await
            .unwrap_err()
            .code(),
        "resource_unavailable"
    );
}
