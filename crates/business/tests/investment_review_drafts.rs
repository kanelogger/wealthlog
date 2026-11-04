//! Fixed-cutoff review drafts and their source identities.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::json;
use support::{journal_caller, Harness};
use wealthfolio_business::investment_journal::*;
async fn decision(h: &Harness) -> (InvestmentJournalService, String) {
    let service = h.journal().with_context_reader(h.context.clone());
    let mut c = journal_caller();
    c.can_confirm = true;
    let body:DecisionBody=serde_json::from_value(json!({"title":"Original decision","action":"HOLD","thesis":"User supplied view","options":[{"action":"Wait","description":"Continue research"}]})).unwrap();
    let d = service
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
                request_key: "confirm".into(),
            },
            &c,
        )
        .await
        .unwrap();
    (service, d["currentRevisionId"].as_str().unwrap().into())
}
fn body() -> ReviewBody {
    serde_json::from_value(json!({"judgment":"尚待核实","executionAssessment":"尚未执行","scenarioTags":[" 波动 ","执行","波动"]})).unwrap()
}
fn create(h: &Harness, revision: String, key: &str) -> SaveReviewDraft {
    SaveReviewDraft {
        review_id: None,
        root_id: None,
        expected_version: None,
        decision_revision_id: revision,
        observation_cutoff: chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap(),
        body: body(),
        request_key: key.into(),
    }
}
#[tokio::test]
async fn review_root_locks_its_cutoff_and_timezone_and_preserves_concurrent_edits() {
    use std::sync::{Arc, RwLock};
    let h = Harness::new().await;
    let (service, revision) = decision(&h).await;
    let c = journal_caller();
    let initial = create(&h, revision, "first");
    let d = service
        .save_review_draft(initial.clone(), &c)
        .await
        .unwrap();
    let id = d["reviewId"].as_str().unwrap().to_string();
    assert_eq!(
        service.get_review(&id, &c).unwrap()["observationTimezone"],
        "Asia/Shanghai"
    );
    assert_eq!(
        service
            .save_review_draft(
                create(&h, initial.decision_revision_id.clone(), "duplicate-root"),
                &c
            )
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let mut edit = initial.clone();
    edit.review_id = Some(id.clone());
    edit.expected_version = Some(1);
    edit.request_key = "edit-one".into();
    edit.body.judgment = "新整理的判断".into();
    let mut other = edit.clone();
    other.request_key = "edit-two".into();
    other.body.judgment = "另一份修改".into();
    let (one, two) = tokio::join!(
        service.save_review_draft(edit, &c),
        service.save_review_draft(other, &c)
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert_eq!(one.err().or(two.err()).unwrap().code(), "state_conflict");
    let current = service.get_review(&id, &c).unwrap();
    let mut wrong = initial.clone();
    wrong.review_id = Some(id.clone());
    wrong.expected_version = Some(2);
    wrong.observation_cutoff -= chrono::Duration::days(1);
    wrong.request_key = "different-cutoff".into();
    assert_eq!(
        service
            .save_review_draft(wrong, &c)
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    let shifted = InvestmentJournalService::new(
        h.store.clone(),
        "ledger".into(),
        h.reader.clone(),
        Arc::new(RwLock::new("America/New_York".into())),
    );
    let mut after_zone = initial;
    after_zone.review_id = Some(id.clone());
    after_zone.expected_version = Some(2);
    after_zone.request_key = "edit-after-zone".into();
    after_zone.body = serde_json::from_value(current["body"].clone()).unwrap();
    shifted.save_review_draft(after_zone, &c).await.unwrap();
    assert_eq!(
        shifted.get_review(&id, &c).unwrap()["observationTimezone"],
        "Asia/Shanghai"
    );
    assert_eq!(
        service.get_review(&id, &c).unwrap()["body"]["scenarioTags"],
        json!(["执行", "波动"])
    );
}
#[tokio::test]
async fn discarded_initial_draft_can_restart_in_the_same_root_without_reusing_version_identity() {
    let h = Harness::new().await;
    let (service, revision) = decision(&h).await;
    let c = journal_caller();
    let initial = create(&h, revision, "first");
    let d = service
        .save_review_draft(initial.clone(), &c)
        .await
        .unwrap();
    let id = d["reviewId"].as_str().unwrap().to_string();
    service
        .discard_review_draft(
            DiscardReviewDraft {
                target: ReviewDraftTarget {
                    review_id: id.clone(),
                    expected_version: 1,
                },
                request_key: "discard".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .save_review_draft(initial.clone(), &c)
            .await
            .unwrap()["replayed"],
        true
    );
    let mut restart = initial;
    restart.root_id = Some(id.clone());
    restart.request_key = "restart".into();
    let next = service.save_review_draft(restart, &c).await.unwrap();
    assert_eq!(next["rootId"], id);
    assert_ne!(next["reviewId"], next["rootId"]);
    assert_eq!(next["revisionNumber"], 2);
    assert_eq!(service.get_review(&id, &c).unwrap()["status"], "DISCARDED");
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_review_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'late audit'); END;").unwrap();
    let before = service
        .get_review(next["reviewId"].as_str().unwrap(), &c)
        .unwrap();
    let mut edit = create(
        &h,
        before["decisionRevisionId"].as_str().unwrap().into(),
        "failed-edit",
    );
    edit.review_id = Some(before["id"].as_str().unwrap().into());
    edit.expected_version = Some(1);
    edit.body.judgment = "This must roll back".into();
    assert!(service.save_review_draft(edit, &c).await.is_err());
    assert_eq!(
        service
            .get_review(before["id"].as_str().unwrap(), &c)
            .unwrap(),
        before
    );
}
