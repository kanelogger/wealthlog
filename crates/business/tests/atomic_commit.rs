//! One business transaction: a failing audit row must roll the activity and
//! request record back together.

#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::sync::Arc;

use diesel::prelude::*;
use rust_decimal::Decimal;
use tempfile::tempdir;
use wealthfolio_business::store::{
    BusinessStore, NewAuditRow, OrdinaryCommitKind, OrdinaryCommitPlan,
};
use wealthfolio_core::activities::{ActivityStatus, NewActivity};
use wealthfolio_storage_sqlite::db::{get_connection, write_actor::spawn_writer, DbAccess};

#[tokio::test]
async fn audit_failure_rolls_back_activity_and_request() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let access = DbAccess::plaintext(db_path.to_str().unwrap());
    access.run_migrations().expect("migrate db");
    let pool = access.create_pool().expect("create pool");
    let writer = spawn_writer(pool.as_ref().clone()).expect("spawn writer");
    {
        let mut conn = get_connection(&pool).unwrap();
        diesel::sql_query(
            "INSERT INTO accounts (id, name, account_type, `group`, currency, is_default, is_active, \
             created_at, updated_at, platform_id, account_number, meta, provider, provider_account_id, \
             is_archived, tracking_mode) VALUES ('cash', 'Cash', 'CASH', NULL, 'USD', 1, 1, \
             CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL, NULL, NULL, NULL, NULL, 0, 'portfolio')",
        )
        .execute(&mut conn)
        .unwrap();
    }

    let store = BusinessStore::new(pool.clone(), writer);
    let prepared = NewActivity {
        id: None,
        account_id: "cash".into(),
        asset: None,
        activity_type: "WITHDRAWAL".into(),
        subtype: None,
        activity_date: "2026-04-01".into(),
        quantity: None,
        unit_price: None,
        currency: "USD".into(),
        fee: None,
        tax: None,
        amount: Some(Decimal::new(1250, 2)),
        status: Some(ActivityStatus::Posted),
        notes: None,
        fx_rate: None,
        metadata: None,
        source_system: Some("MANUAL".into()),
        source_record_id: None,
        source_group_id: None,
        idempotency_key: None,
        needs_review: Some(false),
    };
    // `outcome` has a CHECK constraint; this forces the audit insert to fail
    // after the activity and request rows were written in the transaction.
    let bad_audit = NewAuditRow {
        id: "audit-bad".into(),
        ledger_id: "ledger".into(),
        channel: "gui".into(),
        actor: "session".into(),
        authenticated: 1,
        operation: "ordinary_activity_create".into(),
        entity_type: "activity".into(),
        entity_id: None,
        request_id: None,
        outcome: "not-a-valid-outcome".into(),
        detail_json: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let plan = OrdinaryCommitPlan {
        ledger_id: "ledger".into(),
        subject: "session".into(),
        operation: "ordinary_activity_create".into(),
        request_key: "form-1".into(),
        request_hash: "hash".into(),
        request_snapshot: "{}".into(),
        kind: OrdinaryCommitKind::Create(prepared),
        extras: Default::default(),
        audit: bad_audit,
        build_result: Box::new(|_, _, _| serde_json::json!({})),
    };
    let error = store.commit_ordinary(plan).await.unwrap_err();
    assert_eq!(error.code(), "storage_error");

    let mut conn = get_connection(&pool).unwrap();
    let activities: i64 = wealthfolio_storage_sqlite::schema::activities::table
        .count()
        .get_result(&mut conn)
        .unwrap();
    let requests: i64 = wealthfolio_storage_sqlite::schema::business_requests::table
        .count()
        .get_result(&mut conn)
        .unwrap();
    let audits: i64 = wealthfolio_storage_sqlite::schema::business_audit::table
        .count()
        .get_result(&mut conn)
        .unwrap();
    assert_eq!(activities, 0, "activity must roll back");
    assert_eq!(requests, 0, "request must roll back");
    assert_eq!(audits, 0, "audit must roll back");
    let _ = Arc::strong_count(&pool);
}
