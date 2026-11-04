//! Persistent cursor slices and projection refresh do not enable or replay ownership.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[path = "support/investment_harness.rs"]
mod support;
use support::Harness;
use wealthfolio_business::investment_journal::*;

#[tokio::test]
async fn background_slices_resume_refresh_old_entry_changes_and_keep_business_markers_empty() {
    let h = Harness::new().await;
    let db = h.access.connect_rusqlite().unwrap();
    for id in ["legacy-a", "legacy-b", "legacy-c"] {
        db.execute("INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at) VALUES(?1,'transactions','stock','BUY','POSTED',?2,'1','10','10','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",(id,&h.day)).unwrap();
    }
    let projector = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    projector
        .record_background_error(Some("synthetic_error"))
        .await
        .unwrap();
    let first = projector.maintenance_batch(1).await.unwrap();
    assert!(!first.historical_complete);
    let stored:String=db.query_row("SELECT setting_value FROM app_settings WHERE setting_key='investment_projection.backfill.ACTIVITY.cursor'",[],|r|r.get(0)).unwrap();
    assert_eq!(stored, "legacy-a");
    let restarted = InvestmentProjectionService::new(h.store.clone(), h.reader.clone());
    let mut complete = false;
    for _ in 0..30 {
        let slice = restarted.maintenance_batch(1).await.unwrap();
        if slice.historical_complete && slice.refreshed.remaining == 0 {
            complete = true;
            break;
        }
    }
    assert!(complete);
    assert_eq!(restarted.executions().unwrap().len(), 3);
    let original = restarted
        .executions()
        .unwrap()
        .into_iter()
        .find(|e| e.source_id == "legacy-a")
        .unwrap();
    db.execute(
        "UPDATE activities SET amount='12',version=version+1 WHERE id='legacy-a'",
        [],
    )
    .unwrap();
    for _ in 0..10 {
        let step = restarted.maintenance_batch(2).await.unwrap();
        if step.refreshed.remaining == 0 {
            break;
        }
    }
    let changed = restarted
        .executions()
        .unwrap()
        .into_iter()
        .find(|e| e.source_id == "legacy-a")
        .unwrap();
    assert_eq!(changed.id, original.id);
    assert_ne!(changed.source_version, original.source_version);
    db.execute("DELETE FROM activities WHERE id='legacy-a'", [])
        .unwrap();
    for _ in 0..10 {
        let step = restarted.maintenance_batch(2).await.unwrap();
        if step.refreshed.remaining == 0 {
            break;
        }
    }
    assert_eq!(
        restarted
            .executions()
            .unwrap()
            .into_iter()
            .find(|e| e.source_id == "legacy-a")
            .unwrap()
            .status,
        ExecutionStatus::Deleted
    );
    restarted.record_background_error(None).await.unwrap();
    for table in [
        "fund_allocation_operations",
        "fund_allocation_entries",
        "business_audit",
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
async fn financial_capture_waits_for_index_slice_then_reads_consistently() {
    use std::sync::Arc;
    use tokio::sync::Notify;

    struct HoldSlice {
        reader: Arc<AccountingResourceReader>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }
    #[async_trait::async_trait]
    impl InvestmentExecutionReader for HoldSlice {
        async fn prepare_execution(
            &self,
            source: &ProjectionSource,
        ) -> JournalResult<PreparedExecutionProjection> {
            self.entered.notify_one();
            self.release.notified().await;
            self.reader.prepare_execution(source).await
        }
        fn validate_execution(
            &self,
            conn: &mut diesel::SqliteConnection,
            prepared: &PreparedExecutionProjection,
        ) -> JournalResult<()> {
            self.reader.validate_execution(conn, prepared)
        }
    }

    let h = Harness::new().await;
    h.access.connect_rusqlite().unwrap().execute(
        "INSERT INTO activities(id,account_id,asset_id,activity_type,status,activity_date,quantity,unit_price,amount,fee,tax,currency,created_at,updated_at)
         VALUES('one','transactions','stock','BUY','POSTED',?1,'1','10','10','0','0','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
        [&h.day],
    ).unwrap();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let projector = InvestmentProjectionService::new(
        h.store.clone(),
        Arc::new(HoldSlice {
            reader: h.reader.clone(),
            entered: entered.clone(),
            release: release.clone(),
        }),
    );
    let task = tokio::spawn(async move { projector.maintenance_batch(1).await });
    entered.notified().await;
    let query = InvestmentContextQuery {
        account_ids: vec!["transactions".into()],
        purpose_ids: vec![],
    };
    let mut capture = std::pin::pin!(h.context.capture(&query, h.day.parse().unwrap()));
    assert!(futures::poll!(&mut capture).is_pending());
    release.notify_one();
    assert_eq!(task.await.unwrap().unwrap().refreshed.processed, 1);
    assert!(capture.await.is_ok());
}
