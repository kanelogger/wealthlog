//! Formal reviews freeze native evidence and preserve the fixed-cutoff correction chain.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use chrono::NaiveDate;
use serde_json::{json, Value};
use support::{journal_caller, Harness};
use wealthfolio_business::investment_journal::*;

fn confirmer() -> JournalCaller {
    let mut c = journal_caller();
    c.can_confirm = true;
    c
}
fn service(h: &Harness) -> InvestmentJournalService {
    h.journal()
        .with_context_reader(h.context.clone())
        .with_review_facts_reader(h.context.clone())
}
async fn confirmed(
    service: &InvestmentJournalService,
    id: Option<&str>,
    principles: Vec<String>,
    key: &str,
) -> Value {
    let c = confirmer();
    let version = id.map(|id| {
        service.get_decision(id, &c).unwrap()["version"]
            .as_i64()
            .unwrap() as i32
    });
    let body:DecisionBody=serde_json::from_value(json!({"title":"记录独立判断","action":"BUY","assetId":"stock","thesis":"用户明确提供的判断","plannedQuantity":"10","options":[{"action":"等待","description":"先核实现金"}],"principleRevisionIds":principles,"scenarioTags":["核对","执行"]})).unwrap();
    let d = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: id.map(str::to_string),
                draft_revision_id: None,
                expected_version: version,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: format!("{key}-save"),
            },
            &c,
        )
        .await
        .unwrap();
    let p = service
        .preview_decision_confirmation(
            DecisionDraftTarget {
                decision_id: d["decisionId"].as_str().unwrap().into(),
                draft_revision_id: d["draftRevisionId"].as_str().unwrap().into(),
                expected_version: d["version"].as_i64().unwrap() as i32,
            },
            &c,
        )
        .await
        .unwrap();
    service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: p.ledger_id,
                target: p.target,
                expected_content_hash: p.expected_content_hash,
                expected_context_hash: p.expected_context_hash,
                context: p.context,
                request_key: format!("{key}-confirm"),
            },
            &c,
        )
        .await
        .unwrap()
}
async fn draft(
    service: &InvestmentJournalService,
    revision: &str,
    day: NaiveDate,
    key: &str,
) -> Value {
    service.save_review_draft(SaveReviewDraft{review_id:None,root_id:None,expected_version:None,decision_revision_id:revision.into(),observation_cutoff:day,body:serde_json::from_value(json!({"judgment":"判断仍待后续证据核实","executionAssessment":"仅依据已关联事实评价执行","outcomeEvidence":[{"informationKind":"UNKNOWN","content":"尚不能确认变化原因"}],"otherExplanations":"行情变化也可能与外部事件有关","openQuestions":"继续核实费用与信息来源","scenarioTags":["核对"],"contentOrigin":"ASSISTED_DRAFT"})).unwrap(),request_key:key.into()},&journal_caller()).await.unwrap()
}
fn target(d: &Value) -> ReviewDraftTarget {
    ReviewDraftTarget {
        review_id: d["reviewId"].as_str().unwrap().into(),
        expected_version: d["version"].as_i64().unwrap() as i32,
    }
}
fn submission(p: ReviewSubmissionPreview, key: &str) -> SubmitReview {
    SubmitReview {
        ledger_id: p.ledger_id,
        target: p.target,
        expected_content_hash: p.expected_content_hash,
        expected_facts_hash: p.expected_facts_hash,
        facts: p.facts,
        request_key: key.into(),
    }
}
fn day(h: &Harness) -> NaiveDate {
    NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap()
}
fn quote(h: &Harness, price: &str) {
    h.access.connect_rusqlite().unwrap().execute("INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp) VALUES('review-quote','stock',?1,'MANUAL',?2,'CNY',CURRENT_TIMESTAMP,?3)",(&h.day,price,format!("{}T00:00:00Z",h.day))).unwrap();
}

#[tokio::test]
async fn submission_requires_confirm_and_freezes_the_actual_original_context_and_native_price() {
    let h = Harness::new().await;
    quote(&h, "12.34567890123456789");
    let service = service(&h);
    let c = confirmer();
    let decision = confirmed(&service, None, vec![], "decision").await;
    let revision = decision["currentRevisionId"].as_str().unwrap();
    let original = service
        .get_decision(decision["decisionId"].as_str().unwrap(), &c)
        .unwrap()["revisions"][0]["context"]
        .clone();
    let d = draft(&service, revision, day(&h), "review").await;
    let shown = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    assert_eq!(
        shown.facts.subject_market.as_ref().unwrap()["quote"]["price"],
        "12.34567890123456789"
    );
    assert_eq!(
        shown.facts.observations["decision"]["contextAtConfirmation"],
        original
    );
    assert_eq!(
        shown.expected_facts_hash,
        service
            .preview_review_submission(target(&d), &c)
            .await
            .unwrap()
            .expected_facts_hash
    );
    let input = submission(shown, "submit");
    assert_eq!(
        service
            .submit_review(input.clone(), &journal_caller())
            .await
            .unwrap_err()
            .code(),
        "scope_denied"
    );
    let mut wrong = input.clone();
    wrong.ledger_id = "other-ledger".into();
    assert_eq!(
        service.submit_review(wrong, &c).await.unwrap_err().code(),
        "scope_denied"
    );
    let mut changed = input.clone();
    changed.facts.observations["decision"]["body"]["thesis"] = json!("未展示的替换正文");
    assert_eq!(
        service.submit_review(changed, &c).await.unwrap_err().code(),
        "state_conflict"
    );
    let mut result = service.submit_review(input.clone(), &c).await.unwrap();
    result["replayed"] = json!(true);
    assert_eq!(service.submit_review(input, &c).await.unwrap(), result);
    let frozen = service.get_review(&target(&d).review_id, &c).unwrap();
    assert_eq!(frozen["status"], "SUBMITTED");
    assert_eq!(frozen["submittedActorKind"], "PROGRAM");
    assert_eq!(
        frozen["factsSnapshot"]["observations"]["decision"]["contextAtConfirmation"],
        original
    );
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute("UPDATE quotes SET close='99' WHERE id='review-quote'", [])
        .unwrap();
    assert_eq!(
        service.get_review(&target(&d).review_id, &c).unwrap(),
        frozen
    );
    let db = h.access.connect_rusqlite().unwrap();
    assert!(db
        .execute(
            "UPDATE investment_reviews SET judgment='rewrite' WHERE id=?1",
            [&target(&d).review_id]
        )
        .is_err());
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM business_requests WHERE operation='investment_review_submit'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn changed_quotes_require_new_preview_and_malformed_native_quotes_remain_unavailable() {
    let h = Harness::new().await;
    quote(&h, "12");
    let service = service(&h);
    let c = confirmer();
    let decision = confirmed(&service, None, vec![], "decision").await;
    let d = draft(
        &service,
        decision["currentRevisionId"].as_str().unwrap(),
        day(&h),
        "review",
    )
    .await;
    let old = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute(
        "UPDATE quotes SET close='malformed' WHERE id='review-quote'",
        [],
    )
    .unwrap();
    assert_eq!(
        service
            .submit_review(submission(old, "stale"), &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let shown = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    assert_eq!(shown.facts.quality, ContextQuality::Partial);
    assert_eq!(
        shown.facts.subject_market.as_ref().unwrap()["quote"],
        Value::Null
    );
    assert!(shown
        .facts
        .issues
        .iter()
        .any(|i| i.code == "QUOTE_SOURCE_INVALID"));
    service
        .submit_review(submission(shown, "submit-missing"), &c)
        .await
        .unwrap();
    let newer = draft(
        &service,
        decision["currentRevisionId"].as_str().unwrap(),
        day(&h).pred_opt().unwrap(),
        "older-cutoff",
    )
    .await;
    assert!(
        service
            .preview_review_submission(target(&newer), &c)
            .await
            .unwrap()
            .facts
            .subject_market
            .as_ref()
            .unwrap()["quote"]
            .is_null(),
        "historical observation does not use a quote after its cutoff"
    );
    let body =
        serde_json::from_value(service.get_review(&target_review(&d), &c).unwrap()["body"].clone())
            .unwrap();
    let correction = service
        .save_review_draft(
            SaveReviewDraft {
                review_id: None,
                root_id: Some(d["rootId"].as_str().unwrap().into()),
                expected_version: None,
                decision_revision_id: decision["currentRevisionId"].as_str().unwrap().into(),
                observation_cutoff: day(&h),
                body,
                request_key: "check-known-source-units".into(),
            },
            &c,
        )
        .await
        .unwrap();
    db.execute("UPDATE quotes SET close='0' WHERE id='review-quote'", [])
        .unwrap();
    let p = service
        .preview_review_submission(target(&correction), &c)
        .await
        .unwrap();
    assert_eq!(
        p.facts.subject_market.as_ref().unwrap()["quote"]["price"],
        "0",
        "an explicit verified zero is distinct from a malformed source"
    );
    db.execute(
        "UPDATE quotes SET close='250',currency='GBp' WHERE id='review-quote'",
        [],
    )
    .unwrap();
    let p = service
        .preview_review_submission(target(&correction), &c)
        .await
        .unwrap();
    assert_eq!(
        p.facts.subject_market.as_ref().unwrap()["quote"]["price"],
        "250"
    );
    assert_eq!(
        p.facts.subject_market.as_ref().unwrap()["quote"]["currency"],
        "GBp",
        "preserve native minor quote units without applying an unrequested conversion"
    );
}

#[tokio::test]
async fn corrections_append_and_old_versions_or_early_cutoffs_do_not_clear_current_reminders() {
    let h = Harness::new().await;
    let service = service(&h);
    let c = confirmer();
    let first = confirmed(&service, None, vec![], "first").await;
    let decision = first["decisionId"].as_str().unwrap();
    let revision = first["currentRevisionId"].as_str().unwrap();
    service
        .mutate_decision(
            MutateDecision {
                decision_id: decision.into(),
                expected_version: 2,
                mutation: DecisionMutation::SetReviewDate { date: day(&h) },
                request_key: "due".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let early = draft(&service, revision, day(&h).pred_opt().unwrap(), "early").await;
    let p = service
        .preview_review_submission(target(&early), &c)
        .await
        .unwrap();
    assert_eq!(
        service
            .submit_review(submission(p, "early-submit"), &c)
            .await
            .unwrap()["reviewReminderResolved"],
        false
    );
    let old = service.get_review(&target(&early).review_id, &c).unwrap();
    let mut correction = SaveReviewDraft {
        review_id: None,
        root_id: Some(early["rootId"].as_str().unwrap().into()),
        expected_version: None,
        decision_revision_id: revision.into(),
        observation_cutoff: day(&h).pred_opt().unwrap(),
        body: serde_json::from_value(old["body"].clone()).unwrap(),
        request_key: "correction".into(),
    };
    correction.body.judgment = "修订判断并保留旧记录".into();
    let next = service
        .save_review_draft(correction.clone(), &c)
        .await
        .unwrap();
    assert_eq!(next["revisionNumber"], 2);
    correction.request_key = "parallel-fork".into();
    assert_eq!(
        service
            .save_review_draft(correction, &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let p = service
        .preview_review_submission(target(&next), &c)
        .await
        .unwrap();
    service
        .submit_review(submission(p, "correction-submit"), &c)
        .await
        .unwrap();
    assert_eq!(
        service.get_review(&target(&next).review_id, &c).unwrap()["correctsReviewId"],
        old["id"]
    );
    assert_eq!(
        service.get_review(&target(&early).review_id, &c).unwrap(),
        old
    );
    let current = confirmed(&service, Some(decision), vec![], "second").await;
    let historical = draft(&service, revision, day(&h), "v1-today").await;
    let p = service
        .preview_review_submission(target(&historical), &c)
        .await
        .unwrap();
    assert_eq!(
        service
            .submit_review(submission(p, "v1-submit"), &c)
            .await
            .unwrap()["reviewReminderResolved"],
        false
    );
    assert_eq!(
        service.get_decision(decision, &c).unwrap()["nextReviewAt"],
        h.day
    );
    let current_review = draft(
        &service,
        current["currentRevisionId"].as_str().unwrap(),
        day(&h),
        "v2-today",
    )
    .await;
    let p = service
        .preview_review_submission(target(&current_review), &c)
        .await
        .unwrap();
    assert_eq!(
        service
            .submit_review(submission(p, "v2-submit"), &c)
            .await
            .unwrap()["reviewReminderResolved"],
        true
    );
    assert_eq!(
        service.get_decision(decision, &c).unwrap()["nextReviewAt"],
        Value::Null
    );
}

#[tokio::test]
async fn late_audit_failure_rolls_back_submission_and_reminder_and_same_request_can_retry() {
    let h = Harness::new().await;
    let service = service(&h);
    let c = confirmer();
    let decision = confirmed(&service, None, vec![], "decision").await;
    service
        .mutate_decision(
            MutateDecision {
                decision_id: decision["decisionId"].as_str().unwrap().into(),
                expected_version: 2,
                mutation: DecisionMutation::SetReviewDate { date: day(&h) },
                request_key: "due".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let d = draft(
        &service,
        decision["currentRevisionId"].as_str().unwrap(),
        day(&h),
        "review",
    )
    .await;
    let p = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    let input = submission(p, "submit");
    let before = service.get_review(&target(&d).review_id, &c).unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_submit_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    assert!(service.submit_review(input.clone(), &c).await.is_err());
    assert_eq!(
        service.get_review(&target(&d).review_id, &c).unwrap(),
        before
    );
    assert_eq!(
        service
            .get_decision(decision["decisionId"].as_str().unwrap(), &c)
            .unwrap()["nextReviewAt"],
        h.day
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM business_requests WHERE operation='investment_review_submit'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER fail_submit_audit;").unwrap();
    service.submit_review(input, &c).await.unwrap();
}

#[tokio::test]
async fn review_observes_linked_execution_with_event_cutoff_and_keeps_original_link_after_unlink() {
    let h = Harness::new().await;
    h.initialize_manual().await;
    let service = service(&h);
    let c = confirmer();
    let decision = confirmed(&service, None, vec![], "decision").await;
    let mut trade = RecordDecisionManualTrade {
        reason: "按确认版本记录实际成交，资金归属稍后核对".into(),
        decision: DecisionExecutionTarget {
            decision_id: decision["decisionId"].as_str().unwrap().into(),
            revision_id: decision["currentRevisionId"].as_str().unwrap().into(),
            deviation_reason: None,
        },
        trade: h.trade().await,
        ownership: DecisionTradeOwnership::ReconcileLater,
        expected_preview_hash: String::new(),
    };
    trade.expected_preview_hash = service
        .preview_decision_manual_trade(&trade, &c)
        .await
        .unwrap()
        .preview_hash;
    service
        .record_decision_manual_trade(trade, &c)
        .await
        .unwrap();
    let older = draft(
        &service,
        decision["currentRevisionId"].as_str().unwrap(),
        day(&h).pred_opt().unwrap(),
        "older",
    )
    .await;
    let older = service
        .preview_review_submission(target(&older), &c)
        .await
        .unwrap();
    assert_eq!(
        older.facts.observations["executions"][0]["observationScope"],
        "AFTER_CUTOFF"
    );
    let d = draft(
        &service,
        decision["currentRevisionId"].as_str().unwrap(),
        day(&h),
        "today",
    )
    .await;
    let p = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    let link = p.facts.observations["executions"][0]["link"].clone();
    assert_eq!(
        p.facts.observations["executions"][0]["currentSource"]["execution"]["quantity"],
        "10"
    );
    assert_eq!(
        p.facts.observations["executions"][0]["observationScope"],
        "IN_SCOPE"
    );
    let stale = submission(p, "stale-link");
    service
        .unlink_execution(
            UnlinkExecution {
                link_id: link["id"].as_str().unwrap().into(),
                expected_linked_at: link["linkedAt"].as_str().unwrap().into(),
                reason: "重新核对归属".into(),
                request_key: "unlink".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        service.submit_review(stale, &c).await.unwrap_err().code(),
        "state_conflict"
    );
    let p = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    assert!(!p.facts.observations["executions"][0]["link"]["unlinkedAt"].is_null());
    service
        .submit_review(submission(p, "submit-history"), &c)
        .await
        .unwrap();
    assert_eq!(
        h.access
            .connect_rusqlite()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM investment_execution_links", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

async fn principle_draft(
    service: &InvestmentJournalService,
    previous: Option<&Value>,
    key: &str,
) -> Value {
    let body:PrincipleBody=serde_json::from_value(json!({"name":format!("原则 {key}"),"content":format!("原则正文 {key}"),"revisionReason":"补充核对边界","scenarioTags":["执行"],"contentOrigin":"ASSISTED_DRAFT"})).unwrap();
    service
        .save_principle_draft(
            SavePrincipleDraft {
                principle_id: previous.map(|v| v["principleId"].as_str().unwrap().into()),
                expected_draft_revision_id: None,
                expected_version: previous.map(|v| v["version"].as_i64().unwrap() as i32),
                body,
                request_key: key.into(),
            },
            &journal_caller(),
        )
        .await
        .unwrap()
}
async fn publish(service: &InvestmentJournalService, d: &Value, key: &str) -> Value {
    let p = service
        .preview_principle_publication(
            PrincipleDraftTarget {
                principle_id: d["principleId"].as_str().unwrap().into(),
                draft_revision_id: d["draftRevisionId"].as_str().unwrap().into(),
                expected_version: d["version"].as_i64().unwrap() as i32,
            },
            &journal_caller(),
        )
        .unwrap();
    service
        .publish_principle(
            PublishPrinciple {
                ledger_id: p.ledger_id,
                target: p.target,
                expected_content_hash: p.expected_content_hash,
                expected_source_context_hash: p.expected_source_context_hash,
                source_context: p.source_context,
                request_key: key.into(),
            },
            &confirmer(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn exact_principle_versions_remain_frozen_and_scenario_search_does_not_create_links() {
    let h = Harness::new().await;
    let service = service(&h);
    let c = confirmer();
    let draft_rule = principle_draft(&service, None, "rule-v1").await;
    let old_rule = draft_rule["draftRevisionId"].as_str().unwrap().to_string();
    let body: DecisionBody = serde_json::from_value(
        json!({"title":"草稿不能作为生效原则","principleRevisionIds":[old_rule]}),
    )
    .unwrap();
    assert_eq!(
        service
            .save_decision_draft(
                SaveDecisionDraft {
                    decision_id: None,
                    draft_revision_id: None,
                    expected_version: None,
                    body,
                    check_schedule: CheckScheduleChange::Keep,
                    request_key: "reject-draft-rule".into()
                },
                &c
            )
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    let first_rule = publish(&service, &draft_rule, "publish-v1").await;
    let first = confirmed(&service, None, vec![old_rule.clone()], "decision-v1").await;
    let decision = first["decisionId"].as_str().unwrap();
    let frozen = service.get_decision(decision, &c).unwrap()["revisions"][0]["context"].clone();
    assert_eq!(
        frozen["response"]["funding"]["data"]["principleVersions"][0]["body"]["content"],
        "原则正文 rule-v1"
    );
    let next_rule = principle_draft(&service, Some(&first_rule), "rule-v2").await;
    let saved=service.save_decision_draft(SaveDecisionDraft{decision_id:Some(decision.into()),draft_revision_id:None,expected_version:Some(2),body:serde_json::from_value(json!({"title":"继续引用原原则","action":"HOLD","thesis":"等待更多证据","options":[{"action":"买入"}],"principleRevisionIds":[old_rule],"scenarioTags":["核对","执行"]})).unwrap(),check_schedule:CheckScheduleChange::Keep,request_key:"amend-decision".into()},&c).await.unwrap();
    let decision_target = DecisionDraftTarget {
        decision_id: decision.into(),
        draft_revision_id: saved["draftRevisionId"].as_str().unwrap().into(),
        expected_version: 3,
    };
    let old_preview = service
        .preview_decision_confirmation(decision_target.clone(), &c)
        .await
        .unwrap();
    let current_rule = publish(&service, &next_rule, "publish-v2").await;
    assert_eq!(
        service
            .confirm_decision_draft(
                ConfirmDecisionDraft {
                    ledger_id: old_preview.ledger_id,
                    target: old_preview.target,
                    expected_content_hash: old_preview.expected_content_hash,
                    expected_context_hash: old_preview.expected_context_hash,
                    context: old_preview.context,
                    request_key: "stale-principle-state".into()
                },
                &c
            )
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let p = service
        .preview_decision_confirmation(decision_target, &c)
        .await
        .unwrap();
    assert_eq!(
        p.context.response.funding.data["principleVersions"][0]["id"],
        old_rule
    );
    assert_eq!(
        p.context.response.funding.data["principleVersions"][0]["currentRevisionIdAtCapture"],
        current_rule["currentRevisionId"]
    );
    let second = service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: p.ledger_id,
                target: p.target,
                expected_content_hash: p.expected_content_hash,
                expected_context_hash: p.expected_context_hash,
                context: p.context,
                request_key: "confirm-old-explicit-rule".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        service.get_decision(decision, &c).unwrap()["revisions"][0]["context"],
        frozen
    );
    let proposed = principle_draft(&service, Some(&current_rule), "rule-v3-draft").await;
    let review = draft(
        &service,
        second["currentRevisionId"].as_str().unwrap(),
        day(&h),
        "review",
    )
    .await;
    let detail = service.get_review(&target_review(&review), &c).unwrap();
    let mut body: ReviewBody = serde_json::from_value(detail["body"].clone()).unwrap();
    body.principles = vec![ReviewPrincipleAssessment {
        principle_revision_id: Some(old_rule.clone()),
        disposition: PrincipleDisposition::Revise,
        new_principle_revision_id: Some(proposed["draftRevisionId"].as_str().unwrap().into()),
        reason: Some("保留旧版本并提出修订".into()),
    }];
    let edited = service
        .save_review_draft(
            SaveReviewDraft {
                review_id: Some(target_review(&review)),
                root_id: None,
                expected_version: Some(1),
                decision_revision_id: second["currentRevisionId"].as_str().unwrap().into(),
                observation_cutoff: day(&h),
                body,
                request_key: "assess-rule".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let p = service
        .preview_review_submission(target(&edited), &c)
        .await
        .unwrap();
    assert!(p.facts.observations["principles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["statusAtCapture"] == "DRAFT"));
    service
        .submit_review(submission(p, "submit-rules"), &c)
        .await
        .unwrap();
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM investment_principle_revisions WHERE id=?1",
            [proposed["draftRevisionId"].as_str().unwrap()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "DRAFT"
    );
    let linked = service
        .investment_cases(
            InvestmentCaseQuery {
                principle_revision_id: Some(old_rule),
                ..Default::default()
            },
            &c,
        )
        .unwrap();
    assert_eq!(
        linked.len(),
        3,
        "two confirmed decisions plus the submitted review explicitly reference v1"
    );
    let case_review = linked.iter().find(|row| row["kind"] == "REVIEW").unwrap();
    let case_decision = linked
        .iter()
        .find(|row| row["id"] == case_review["decisionRevisionId"])
        .unwrap();
    assert_eq!(case_review["decisionTitle"], case_decision["title"]);
    assert_eq!(
        case_review["decisionRevisionNumber"],
        case_decision["revisionNumber"]
    );
    let categories = service
        .investment_cases(
            InvestmentCaseQuery {
                scenario_tags: vec!["核对".into(), "执行".into()],
                ..Default::default()
            },
            &c,
        )
        .unwrap();
    assert_eq!(categories.len(), 2);
    assert!(categories.iter().all(|v| v["kind"] == "DECISION"));
    assert!(service
        .investment_cases(
            InvestmentCaseQuery {
                scenario_tags: vec!["核".into()],
                ..Default::default()
            },
            &c
        )
        .unwrap()
        .is_empty());
    let page1 = service
        .investment_cases(
            InvestmentCaseQuery {
                limit: Some(1),
                ..Default::default()
            },
            &c,
        )
        .unwrap();
    let page2 = service
        .investment_cases(
            InvestmentCaseQuery {
                limit: Some(1),
                offset: Some(1),
                ..Default::default()
            },
            &c,
        )
        .unwrap();
    assert_ne!(page1[0]["id"], page2[0]["id"]);
    assert_eq!(
        service
            .investment_cases(
                InvestmentCaseQuery {
                    scenario_tags: vec!["核对".into()],
                    principle_revision_id: Some(
                        current_rule["currentRevisionId"].as_str().unwrap().into()
                    ),
                    ..Default::default()
                },
                &c
            )
            .unwrap()
            .len(),
        0,
        "same categories do not imply an explicit principle reference"
    );
}
fn target_review(d: &Value) -> String {
    d["reviewId"].as_str().unwrap().into()
}

#[tokio::test]
async fn concurrent_submissions_have_one_winner_and_blank_assessments_stay_drafts() {
    let h = Harness::new().await;
    let service = service(&h);
    let c = confirmer();
    let decision = confirmed(&service, None, vec![], "decision").await;
    let revision = decision["currentRevisionId"].as_str().unwrap();
    let empty = service
        .save_review_draft(
            SaveReviewDraft {
                review_id: None,
                root_id: None,
                expected_version: None,
                decision_revision_id: revision.into(),
                observation_cutoff: day(&h).pred_opt().unwrap(),
                body: serde_json::from_value(json!({})).unwrap(),
                request_key: "empty-draft".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .preview_review_submission(target(&empty), &c)
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    assert_eq!(
        service.get_review(&target_review(&empty), &c).unwrap()["status"],
        "DRAFT"
    );
    let d = draft(&service, revision, day(&h), "ready").await;
    let p = service
        .preview_review_submission(target(&d), &c)
        .await
        .unwrap();
    let first = submission(p, "submit-one");
    let mut other = first.clone();
    other.request_key = "submit-two".into();
    let (a, b) = tokio::join!(
        service.submit_review(first, &c),
        service.submit_review(other, &c)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let rejected = if let Err(e) = a { e } else { b.unwrap_err() };
    assert_eq!(rejected.code(), "state_conflict");
    let db = h.access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM business_requests WHERE operation='investment_review_submit'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM investment_reviews WHERE status='SUBMITTED'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
