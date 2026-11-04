//! Native principle history and independently authorized publication.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::{json, Value};
use support::{journal_caller, Harness};
use wealthfolio_business::investment_journal::*;
fn body() -> PrincipleBody {
    serde_json::from_value(json!({"name":"先核实现金","content":"先核实当前可用资源，再决定是否调整用途。","scenarioTags":[" 波动 ","核对","波动"],"contentOrigin":"ASSISTED_DRAFT"})).unwrap()
}
fn save(body: PrincipleBody, key: &str) -> SavePrincipleDraft {
    SavePrincipleDraft {
        principle_id: None,
        expected_draft_revision_id: None,
        expected_version: None,
        body,
        request_key: key.into(),
    }
}
fn target(value: &Value) -> PrincipleDraftTarget {
    PrincipleDraftTarget {
        principle_id: value["principleId"].as_str().unwrap().into(),
        draft_revision_id: value["draftRevisionId"].as_str().unwrap().into(),
        expected_version: value["version"].as_i64().unwrap() as i32,
    }
}
fn publication(p: PrinciplePublicationPreview, key: &str) -> PublishPrinciple {
    PublishPrinciple {
        ledger_id: p.ledger_id,
        target: p.target,
        expected_content_hash: p.expected_content_hash,
        expected_source_context_hash: p.expected_source_context_hash,
        source_context: p.source_context,
        request_key: key.into(),
    }
}
fn confirmer() -> JournalCaller {
    let mut c = journal_caller();
    c.can_confirm = true;
    c
}

#[tokio::test]
async fn principle_drafts_append_and_only_independent_confirmation_can_publish() {
    let h = Harness::new().await;
    let service = h.journal();
    let c = journal_caller();
    let first = service
        .save_principle_draft(save(body(), "first"), &c)
        .await
        .unwrap();
    let old_target = target(&first);
    let old_preview = service
        .preview_principle_publication(old_target.clone(), &c)
        .unwrap();
    let mut input = save(body(), "edit");
    input.principle_id = Some(old_target.principle_id.clone());
    input.expected_draft_revision_id = Some(old_target.draft_revision_id.clone());
    input.expected_version = Some(old_target.expected_version);
    input.body.content = "核对资源并保留未确定的信息。".into();
    let revised = service.save_principle_draft(input, &c).await.unwrap();
    assert_ne!(revised["draftRevisionId"], first["draftRevisionId"]);
    assert_eq!(revised["revisionNumber"], 2);
    assert_eq!(
        service
            .publish_principle(publication(old_preview, "old-publish"), &confirmer())
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let shown = service
        .preview_principle_publication(target(&revised), &c)
        .unwrap();
    let input = publication(shown, "publish");
    assert_eq!(
        service
            .publish_principle(input.clone(), &c)
            .await
            .unwrap_err()
            .code(),
        "scope_denied"
    );
    let done = service
        .publish_principle(input.clone(), &confirmer())
        .await
        .unwrap();
    assert_eq!(
        service
            .publish_principle(input, &confirmer())
            .await
            .unwrap()["replayed"],
        true
    );
    let record = service.get_principle(&old_target.principle_id, &c).unwrap();
    assert_eq!(record["currentRevisionId"], done["currentRevisionId"]);
    assert_eq!(record["draftRevisionId"], Value::Null);
    assert_eq!(record["revisions"][0]["status"], "DISCARDED");
    assert_eq!(record["revisions"][0]["body"]["content"], body().content);
    assert_eq!(record["revisions"][1]["status"], "EFFECTIVE");
    assert_eq!(record["revisions"][1]["publishedActorKind"], "PROGRAM");
    assert_eq!(record["revisions"][1]["contentOrigin"], "ASSISTED_DRAFT");
    assert_eq!(
        record["revisions"][1]["body"]["scenarioTags"],
        json!(["核对", "波动"])
    );
    let db = h.access.connect_rusqlite().unwrap();
    assert!(db
        .execute(
            "UPDATE investment_principle_revisions SET content='overwrite'",
            []
        )
        .is_err());
}

#[tokio::test]
async fn amendment_requires_its_reason_and_draft_never_replaces_the_active_rule() {
    let h = Harness::new().await;
    let service = h.journal();
    let c = confirmer();
    let first = service
        .save_principle_draft(save(body(), "first"), &c)
        .await
        .unwrap();
    let p = service
        .preview_principle_publication(target(&first), &c)
        .unwrap();
    let published = service
        .publish_principle(publication(p, "publish"), &c)
        .await
        .unwrap();
    let mut amend = save(body(), "amend");
    amend.principle_id = Some(first["principleId"].as_str().unwrap().into());
    amend.expected_version = Some(published["version"].as_i64().unwrap() as i32);
    amend.body.content = "新的例外处理。".into();
    let draft = service.save_principle_draft(amend, &c).await.unwrap();
    assert_eq!(
        service
            .get_principle(first["principleId"].as_str().unwrap(), &c)
            .unwrap()["currentRevisionId"],
        published["currentRevisionId"]
    );
    assert_eq!(
        service
            .preview_principle_publication(target(&draft), &c)
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    let discarded = service
        .discard_principle_draft(
            DiscardPrincipleDraft {
                target: target(&draft),
                request_key: "discard".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let record = service
        .get_principle(first["principleId"].as_str().unwrap(), &c)
        .unwrap();
    assert_eq!(record["currentRevisionId"], published["currentRevisionId"]);
    assert_eq!(record["isArchived"], false);
    service
        .archive_principle(
            ArchivePrinciple {
                principle_id: first["principleId"].as_str().unwrap().into(),
                expected_version: discarded["version"].as_i64().unwrap() as i32,
                is_archived: true,
                request_key: "archive".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert!(service
        .list_principles(PrincipleQuery::default(), &c)
        .unwrap()
        .is_empty());
    assert_eq!(
        service
            .list_principles(
                PrincipleQuery {
                    include_archived: true,
                    ..Default::default()
                },
                &c
            )
            .unwrap()
            .len(),
        1
    );
}

async fn review_source(h: &Harness) -> String {
    let service = h.journal().with_context_reader(h.context.clone());
    let c = confirmer();
    let body:DecisionBody=serde_json::from_value(json!({"action":"HOLD","thesis":"用户的原判断","options":[{"action":"等待","description":"继续研究"}]})).unwrap();
    let d = service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: None,
                draft_revision_id: None,
                expected_version: None,
                body,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "source-decision".into(),
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
                expected_version: 1,
            },
            &c,
        )
        .await
        .unwrap();
    let d = service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: "ledger".into(),
                target: p.target,
                expected_content_hash: p.expected_content_hash,
                expected_context_hash: p.expected_context_hash,
                context: p.context,
                request_key: "source-confirm".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let body: ReviewBody =
        serde_json::from_value(json!({"judgment":"原始复盘草稿","executionAssessment":"尚未提交"}))
            .unwrap();
    service
        .save_review_draft(
            SaveReviewDraft {
                review_id: None,
                root_id: None,
                expected_version: None,
                decision_revision_id: d["currentRevisionId"].as_str().unwrap().into(),
                observation_cutoff: chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap(),
                body,
                request_key: "review-source-draft".into(),
            },
            &c,
        )
        .await
        .unwrap()["reviewId"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn publishing_from_a_review_draft_freezes_its_actual_state_without_submitting_it() {
    let h = Harness::new().await;
    let review = review_source(&h).await;
    let service = h.journal();
    let c = confirmer();
    let mut b = body();
    b.source_review_id = Some(review.clone());
    let d = service
        .save_principle_draft(save(b, "from-review"), &c)
        .await
        .unwrap();
    let first = service
        .preview_principle_publication(target(&d), &c)
        .unwrap();
    assert_eq!(
        first.source_context["sourceReview"]["statusAtCapture"],
        "DRAFT"
    );
    let db = h.access.connect_rusqlite().unwrap();
    edit_review(&service, &review, "修改后的复盘草稿", "edit-source-1").await;
    assert_eq!(
        service
            .publish_principle(publication(first, "stale-source"), &c)
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let current = service
        .preview_principle_publication(target(&d), &c)
        .unwrap();
    let displayed = current.source_context.clone();
    service
        .publish_principle(publication(current, "publish-source"), &c)
        .await
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM investment_reviews WHERE id=?1",
            [&review],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "DRAFT"
    );
    edit_review(&service, &review, "后来继续编辑", "edit-source-2").await;
    let record = service
        .get_principle(d["principleId"].as_str().unwrap(), &c)
        .unwrap();
    assert_eq!(record["revisions"][0]["sourceContext"], displayed);
    assert_eq!(displayed["sourceReview"]["judgment"], "修改后的复盘草稿");
}

#[tokio::test]
async fn publication_audit_failure_rolls_back_state_pointer_and_request() {
    let h = Harness::new().await;
    let service = h.journal();
    let c = confirmer();
    let d = service
        .save_principle_draft(save(body(), "first"), &c)
        .await
        .unwrap();
    let p = service
        .preview_principle_publication(target(&d), &c)
        .unwrap();
    let input = publication(p, "publish");
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_principle_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    assert!(service.publish_principle(input.clone(), &c).await.is_err());
    let record = service
        .get_principle(d["principleId"].as_str().unwrap(), &c)
        .unwrap();
    assert_eq!(record["currentRevisionId"], Value::Null);
    assert_eq!(record["revisions"][0]["status"], "DRAFT");
    db.execute("DROP TRIGGER fail_principle_audit", []).unwrap();
    service.publish_principle(input, &c).await.unwrap();
}

async fn edit_review(service: &InvestmentJournalService, id: &str, judgment: &str, key: &str) {
    let c = journal_caller();
    let old = service.get_review(id, &c).unwrap();
    let mut body: ReviewBody = serde_json::from_value(old["body"].clone()).unwrap();
    body.judgment = judgment.into();
    service
        .save_review_draft(
            SaveReviewDraft {
                review_id: Some(id.into()),
                root_id: Some(old["rootId"].as_str().unwrap().into()),
                expected_version: Some(old["version"].as_i64().unwrap() as i32),
                decision_revision_id: old["decisionRevisionId"].as_str().unwrap().into(),
                observation_cutoff: chrono::NaiveDate::parse_from_str(
                    old["observationCutoff"].as_str().unwrap(),
                    "%Y-%m-%d",
                )
                .unwrap(),
                body,
                request_key: key.into(),
            },
            &c,
        )
        .await
        .unwrap();
}
