//! Read-only workbench simulations, reference snapshots and untruncated library reads.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use serde_json::json;
use support::{journal_caller, Harness};
use wealthfolio_business::investment_journal::*;
fn body() -> DecisionBody {
    serde_json::from_value(json!({"title":"Confirmed title","action":"WAIT","assetId":"stock","thesis":"Explicit user view","options":[{"action":"Buy","description":"Unchosen"}]})).unwrap()
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

#[tokio::test]
async fn incomplete_thinking_can_read_native_comparisons_without_saving_or_enabling() {
    let h = Harness::new().await;
    let service = h.journal().with_context_reader(h.context.clone());
    let c = journal_caller();
    let mut partial = body();
    partial.action = None;
    partial.thesis.clear();
    partial.options.clear();
    let context = service.preview_decision_context(partial, &c).await.unwrap();
    assert_eq!(
        context.response.funding.data["decisionReferences"]["assets"][0]["name"],
        "Stock"
    );
    assert_eq!(
        context.response.funding.data["decisionAssessment"]["reservesResources"],
        false
    );
    let db = h.access.connect_rusqlite().unwrap();
    for table in [
        "investment_decisions",
        "business_requests",
        "fund_allocations",
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
}

#[tokio::test]
async fn confirmation_binds_displayed_names_and_separate_drafts_do_not_rename_confirmed_list_entries(
) {
    let h = Harness::new().await;
    let service = h.journal().with_context_reader(h.context.clone());
    let mut c = journal_caller();
    c.can_confirm = true;
    let d = service
        .save_decision_draft(save(body(), "first"), &c)
        .await
        .unwrap();
    let target = DecisionDraftTarget {
        decision_id: d["decisionId"].as_str().unwrap().into(),
        draft_revision_id: d["draftRevisionId"].as_str().unwrap().into(),
        expected_version: 1,
    };
    let old = service
        .preview_decision_confirmation(target.clone(), &c)
        .await
        .unwrap();
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute(
            "UPDATE assets SET name='Renamed subject' WHERE id='stock'",
            [],
        )
        .unwrap();
    assert_eq!(
        service
            .confirm_decision_draft(
                ConfirmDecisionDraft {
                    ledger_id: old.ledger_id,
                    target: old.target,
                    expected_content_hash: old.expected_content_hash,
                    expected_context_hash: old.expected_context_hash,
                    context: old.context,
                    request_key: "old-label".into()
                },
                &c
            )
            .await
            .unwrap_err()
            .code(),
        "state_conflict"
    );
    let shown = service
        .preview_decision_confirmation(target, &c)
        .await
        .unwrap();
    assert_eq!(
        shown.context.response.funding.data["decisionReferences"]["assets"][0]["name"],
        "Renamed subject"
    );
    let confirmed = service
        .confirm_decision_draft(
            ConfirmDecisionDraft {
                ledger_id: shown.ledger_id,
                target: shown.target,
                expected_content_hash: shown.expected_content_hash,
                expected_context_hash: shown.expected_context_hash,
                context: shown.context,
                request_key: "confirm".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let mut amendment = body();
    amendment.title = "Unconfirmed replacement title".into();
    service
        .save_decision_draft(
            SaveDecisionDraft {
                decision_id: Some(d["decisionId"].as_str().unwrap().into()),
                draft_revision_id: None,
                expected_version: Some(2),
                body: amendment,
                check_schedule: CheckScheduleChange::Keep,
                request_key: "amend".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let entries = service
        .list_decisions(DecisionQuery::default(), &c)
        .unwrap();
    assert_eq!(entries[0]["title"], "Confirmed title");
    assert_eq!(entries[0]["draftTitle"], "Unconfirmed replacement title");
    assert_eq!(
        entries[0]["currentRevisionId"],
        confirmed["currentRevisionId"]
    );
    service
        .save_decision_draft(save(body(), "another-decision"), &c)
        .await
        .unwrap();
    let first = service
        .list_decisions(
            DecisionQuery {
                limit: Some(1),
                ..Default::default()
            },
            &c,
        )
        .unwrap();
    let second = service
        .list_decisions(
            DecisionQuery {
                limit: Some(1),
                offset: Some(1),
                ..Default::default()
            },
            &c,
        )
        .unwrap();
    assert_ne!(first[0]["decisionId"], second[0]["decisionId"]);
}
