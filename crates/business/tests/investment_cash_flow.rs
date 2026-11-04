#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use support::Harness;
use wealthfolio_business::OrdinaryActivityQuery;

#[tokio::test]
async fn ordinary_page_and_native_aggregation_share_account_eligibility() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('cash','cash','CASH','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');").unwrap();
    for (id, kind, amount) in [
        ("income", "DEPOSIT", "9007199254740993.01"),
        ("expense", "WITHDRAWAL", "0.01"),
    ] {
        db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES(?1,'cash',?2,'POSTED',?3,'CNY',?4,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,kind,format!("{}T12:00:00+08:00",h.day),amount)).unwrap();
    }
    let query = OrdinaryActivityQuery {
        page: None,
        page_size: Some(1),
        account_ids: None,
        date_from: None,
        date_to: None,
    };
    let page = h.ordinary.query_ordinary(query.clone()).await.unwrap();
    assert_eq!(page.total_count, 2);
    assert_eq!(page.rows.len(), 1);
    let day = chrono::NaiveDate::parse_from_str(&h.day, "%Y-%m-%d").unwrap();
    let native = h
        .ordinary
        .aggregate_ordinary_native(day, day, "Asia/Shanghai")
        .unwrap();
    assert_eq!(native.record_count, 2);
    assert_eq!(
        native.currencies[0].income.unwrap().to_string(),
        "9007199254740993.01"
    );
    // Active securities deposits never enter ordinary income; changing the cash
    // account's eligibility affects both reads, independent of page size.
    assert_eq!(native.account_ids, vec!["cash"]);
    db.execute("UPDATE accounts SET is_active=0 WHERE id='cash'", [])
        .unwrap();
    assert_eq!(
        h.ordinary.query_ordinary(query).await.unwrap().total_count,
        0
    );
    let inactive = h
        .ordinary
        .aggregate_ordinary_native(day, day, "Asia/Shanghai")
        .unwrap();
    assert_eq!(inactive.record_count, 0);
    assert!(inactive.currencies.is_empty());
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
