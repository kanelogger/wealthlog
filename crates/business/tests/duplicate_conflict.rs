//! Concurrent duplicate identity: the core pre-check can miss a writer that
//! commits between check and insert, so the unique index must map to the same
//! typed `duplicate_activity` error without touching other constraints.

#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use diesel::prelude::*;
use rust_decimal::Decimal;
use tempfile::tempdir;
use wealthfolio_business::store::{
    BusinessStore, NewAuditRow, OrdinaryCommitKind, OrdinaryCommitPlan,
};
use wealthfolio_core::activities::{ActivityStatus, NewActivity};
use wealthfolio_storage_sqlite::db::{get_connection, write_actor::spawn_writer, DbAccess};

fn plan(request_key: &str, idempotency_key: &str) -> OrdinaryCommitPlan {
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
        amount: Some(Decimal::new(3000, 2)),
        status: Some(ActivityStatus::Posted),
        notes: Some("Coffee".into()),
        fx_rate: None,
        metadata: None,
        source_system: Some("MANUAL".into()),
        source_record_id: None,
        source_group_id: None,
        idempotency_key: Some(idempotency_key.into()),
        needs_review: Some(false),
    };
    let audit = NewAuditRow {
        id: format!("audit-{request_key}"),
        ledger_id: "ledger".into(),
        channel: "cli".into(),
        actor: "pat-1".into(),
        authenticated: 1,
        operation: "ordinary_activity_create".into(),
        entity_type: "activity".into(),
        entity_id: None,
        request_id: None,
        outcome: "success".into(),
        detail_json: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    OrdinaryCommitPlan {
        ledger_id: "ledger".into(),
        subject: "pat-1".into(),
        operation: "ordinary_activity_create".into(),
        request_key: request_key.into(),
        request_hash: format!("hash-{request_key}"),
        request_snapshot: "{}".into(),
        kind: OrdinaryCommitKind::Create(prepared),
        extras: Default::default(),
        audit,
        build_result: Box::new(|_, _, _| serde_json::json!({})),
    }
}

#[tokio::test]
async fn concurrent_same_identity_maps_to_duplicate_activity_and_rolls_back() {
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
    let (first, second) = tokio::join!(
        store.commit_ordinary(plan("request-1", "same-identity")),
        store.commit_ordinary(plan("request-2", "same-identity")),
    );

    let successes = [&first, &second]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(successes, 1, "exactly one insert wins the unique index");
    let duplicate = [&first, &second]
        .into_iter()
        .find_map(|result| result.as_ref().err())
        .expect("the losing insert reports a duplicate");
    assert_eq!(duplicate.code(), "duplicate_activity");
    assert!(!duplicate.retryable());
    assert_eq!(duplicate.existing_activity_id(), None);

    let mut conn = get_connection(&pool).unwrap();
    let activities: i64 = wealthfolio_storage_sqlite::schema::activities::table
        .count()
        .get_result(&mut conn)
        .unwrap();
    let audits: i64 = wealthfolio_storage_sqlite::schema::business_audit::table
        .count()
        .get_result(&mut conn)
        .unwrap();
    let requests: i64 = wealthfolio_storage_sqlite::schema::business_requests::table
        .count()
        .get_result(&mut conn)
        .unwrap();
    assert_eq!(activities, 1);
    assert_eq!(audits, 1, "the losing transaction writes nothing");
    assert_eq!(requests, 1, "the losing transaction writes nothing");
}
