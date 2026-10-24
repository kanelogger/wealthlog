//! Unit tests for the business request/audit contracts.
//!
//! Full create/update/confirm flows run against the real server stack in
//! `apps/server/tests/agent_access.rs`; these tests pin the pure decisions
//! that are easy to get subtly wrong.

use serde_json::json;

use wealthfolio_business::audit::{BusinessAuditEntry, BusinessChannel, BusinessOutcome};
use wealthfolio_business::requests::{canonicalize, hash_json, RequestRecord, RequestReplay};

#[test]
fn canonical_json_ignores_key_order_but_not_values() {
    let a = json!({ "amount": "10.00", "accountId": "cash-1", "fee": null });
    let b = json!({ "fee": null, "accountId": "cash-1", "amount": "10.00" });
    assert_eq!(canonicalize(a.clone()), canonicalize(b.clone()));
    let hash_a = hash_json(&canonicalize(a));
    let hash_b = hash_json(&canonicalize(b));
    assert_eq!(hash_a, hash_b);

    let c = json!({ "amount": "10.01", "accountId": "cash-1", "fee": null });
    assert_ne!(hash_json(&canonicalize(c)), hash_b);
}

#[test]
fn arrays_keep_their_order() {
    let first = json!({ "rows": [1, 2] });
    let second = json!({ "rows": [2, 1] });
    assert_ne!(
        hash_json(&canonicalize(first)),
        hash_json(&canonicalize(second))
    );
}

#[test]
fn request_record_freezes_the_hash() {
    let record = RequestRecord::new(
        "ledger",
        "pat-1",
        "ordinary_activity_create",
        "form-1",
        json!({ "b": 2, "a": 1 }),
    );
    assert_eq!(record.request_hash, hash_json(&record.request_snapshot));
    assert_eq!(record.request_key, "form-1");
}

#[test]
fn replay_decision_follows_the_stored_state() {
    let hash = "abc";
    assert_eq!(RequestReplay::decide(None, hash), RequestReplay::Fresh);

    let completed = row("completed", hash, Some("{\"ok\":true}"));
    assert_eq!(
        RequestReplay::decide(Some(&completed), hash),
        RequestReplay::Completed("{\"ok\":true}".to_string())
    );
    assert_eq!(
        RequestReplay::decide(Some(&completed), "different"),
        RequestReplay::Conflict
    );

    let in_progress = row("in_progress", hash, None);
    assert_eq!(
        RequestReplay::decide(Some(&in_progress), hash),
        RequestReplay::Retry
    );
    let failed = row("failed", hash, None);
    assert_eq!(
        RequestReplay::decide(Some(&failed), hash),
        RequestReplay::Retry
    );
}

fn row(state: &str, hash: &str, result: Option<&str>) -> wealthfolio_business::store::RequestRow {
    wealthfolio_business::store::RequestRow {
        id: "req-1".into(),
        ledger_id: "ledger".into(),
        subject: "pat-1".into(),
        operation: "ordinary_activity_create".into(),
        request_key: "form-1".into(),
        request_hash: hash.into(),
        request_snapshot: "{}".into(),
        state: state.into(),
        result_json: result.map(str::to_string),
        error_message: None,
        created_at: "2026-04-01T00:00:00Z".into(),
        updated_at: "2026-04-01T00:00:00Z".into(),
    }
}

#[test]
fn audit_entries_record_credential_source_not_a_person() {
    let entry = BusinessAuditEntry::new(
        "ledger",
        BusinessChannel::Mcp,
        "pat-row-id",
        true,
        "pending_activity_confirm",
        "pending_activity",
    )
    .entity("pend-1")
    .request("req-1")
    .outcome(BusinessOutcome::Success)
    .detail(json!({ "activityId": "act-1" }));

    let row = entry.to_row();
    assert_eq!(row.channel, "mcp");
    assert_eq!(row.actor, "pat-row-id");
    assert_eq!(row.authenticated, 1);
    assert_eq!(row.entity_id.as_deref(), Some("pend-1"));
    assert_eq!(row.request_id.as_deref(), Some("req-1"));
    assert_eq!(row.outcome, "success");
    assert!(row.detail_json.unwrap().contains("act-1"));
    assert_eq!(entry.channel.as_str(), "mcp");
}

#[test]
fn anonymous_audit_rows_are_marked_unauthenticated() {
    let entry = BusinessAuditEntry::new(
        "ledger",
        BusinessChannel::Rest,
        "anonymous",
        false,
        "ordinary_activity_create",
        "activity",
    );
    let row = entry.to_row();
    assert_eq!(row.authenticated, 0);
    assert_eq!(row.actor, "anonymous");
}
