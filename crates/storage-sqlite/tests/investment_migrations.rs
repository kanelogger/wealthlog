//! Synthetic databases only: journal migrations never touch a user ledger.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use wealthfolio_storage_sqlite::db::DbAccess;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[test]
fn journal_groups_down_up_preserve_legacy_data_and_never_enable_investments() {
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let db = access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO app_settings VALUES('legacy-test','keep');
        INSERT INTO goals(id,title,target_amount,goal_type,status_lifecycle,status_health,priority,created_at,updated_at)
        VALUES('legacy-goal','Keep goal',100,'custom_save_up','active','not_applicable',1,'2026-01-01','2026-01-01');").unwrap();
    let original: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='goals'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(db);
    let mut conn = access.connect().unwrap();
    // Reverse all installed journal groups in dependency order, not an assumed latest M1.
    while access
        .connect_rusqlite()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='fund_purposes'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0
    {
        conn.revert_last_migration(MIGRATIONS).unwrap();
    }
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='fund_purposes'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM goals WHERE id='legacy-goal'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    drop(db);
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT sql FROM sqlite_schema WHERE name='goals'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        original
    );
    assert_eq!(
        db.query_row(
            "SELECT setting_value FROM app_settings WHERE setting_key='legacy-test'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "keep"
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn resource_constraints_preserve_exact_text_and_append_only_history() {
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let db = access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO fund_purposes(id,name,category,currency,created_at,updated_at) VALUES('p','Purpose','CUSTOM','CNY','now','now');
        INSERT INTO business_requests VALUES('r','l','s','test','k','h','{}','completed','{}',NULL,'now','now');
        INSERT INTO fund_allocation_operations(id,operation_kind,request_id,reason,observation_json,source_confirmation_json,detail_json,created_at,created_channel,created_actor_kind,created_actor_id)
        VALUES('o','ALLOCATE','r','test','{\"schemaVersion\":1}','{\"schemaVersion\":1}','{\"schemaVersion\":1}','now','cli','PROGRAM','agent');").unwrap();
    for invalid in [
        "-1", "NaN", "1e2", "0.0", "01", "1.", ".1", "1.20", "1.2.3", "",
    ] {
        assert!(db.execute("INSERT INTO fund_allocations(id,purpose_id,cash_amount,last_operation_id,resource_kind,account_id,currency) VALUES('bad','p',?1,'o','CASH','a','CNY')",[invalid]).is_err(),"accepted {invalid}");
    }
    db.execute_batch("INSERT INTO fund_allocations(id,purpose_id,cash_amount,last_operation_id,resource_kind,account_id,currency) VALUES('ok','p','9007199254740993.01','o','CASH','a','CNY');
        INSERT INTO fund_allocation_entries(id,operation_id,purpose_id,cash_delta,created_at,resource_kind,account_id,currency) VALUES('e','o','p','0.01','now','CASH','a','CNY');").unwrap();
    let stored: (String, String) = db
        .query_row(
            "SELECT cash_amount,typeof(cash_amount) FROM fund_allocations WHERE id='ok'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, ("9007199254740993.01".into(), "text".into()));
    assert!(db
        .execute("UPDATE fund_allocation_entries SET cash_delta='2'", [])
        .is_err());
    assert!(db
        .execute("DELETE FROM fund_allocation_operations", [])
        .is_err());
    assert!(db.execute("INSERT INTO fund_allocations(id,purpose_id,cash_amount,last_operation_id,resource_kind,account_id,currency) VALUES('duplicate','p','1','o','CASH','a','CNY')",[]).is_err());
    assert!(db.execute("INSERT INTO fund_allocations(id,purpose_id,quantity,last_operation_id,resource_kind,account_id,currency,asset_id) VALUES('invalid-security','p','1','o','SECURITY','a','CNY','asset')",[]).is_err());
    assert!(db.execute("INSERT INTO fund_allocation_operations(id,operation_kind,request_id,reason,observation_json,source_confirmation_json,detail_json,created_at,created_channel,created_actor_kind,created_actor_id) VALUES('bad','ALLOCATE','r','test','{}','{}','{}','now','cli','PROGRAM','agent')",[]).is_err());
}

#[test]
fn source_triggers_keep_old_new_and_tombstones_without_double_version_bumps() {
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let db = access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('a','A','SECURITIES','CNY',0,1,'now','now',0,'TRANSACTIONS');
        INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES('trade','a','BUY','POSTED','2026-11-02','CNY','100','now','now');").unwrap();
    let generation = || {
        db.query_row(
            "SELECT generation FROM investment_projection_state WHERE source_id='trade'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
    };
    assert_eq!(generation(), 1);
    db.execute("UPDATE activities SET amount='110' WHERE id='trade'", [])
        .unwrap();
    assert_eq!(
        generation(),
        2,
        "activity version trigger must not increment journal generation twice"
    );
    db.execute("UPDATE activities SET amount='110' WHERE id='trade'", [])
        .unwrap();
    assert_eq!(generation(), 2, "no-op update is not a changed fact");
    db.execute(
        "UPDATE activities SET activity_type='DIVIDEND' WHERE id='trade'",
        [],
    )
    .unwrap();
    let before: String = db.query_row("SELECT json_extract(source_snapshot_json,'$.before.activity_type') FROM investment_projection_state WHERE source_id='trade'",[],|r| r.get(0)).unwrap();
    assert_eq!(before, "BUY");
    db.execute("DELETE FROM activities WHERE id='trade'", [])
        .unwrap();
    let state:(i64,i64,String) = db.query_row("SELECT generation,is_deleted,json_extract(source_snapshot_json,'$.before.activity_type') FROM investment_projection_state WHERE source_id='trade'",[],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(state, (4, 1, "DIVIDEND".into()));
    for (id, operation) in [
        ("manual", "manual_trade_record"),
        ("not-trade", "holdings_snapshot_patch"),
    ] {
        db.execute("INSERT INTO business_requests VALUES(?1,'l','s',?2,?1,'h','{}','completed','{}',NULL,'now','now')",[id,operation]).unwrap();
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM investment_projection_state WHERE source_kind='MANUAL_TRADE'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
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

#[test]
fn journal_upgrade_retains_prebackup_and_resumes_after_m2_failure() {
    use wealthfolio_storage_sqlite::db::DatabaseOwner;
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let mut conn = access.connect().unwrap();
    while access
        .connect_rusqlite()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='fund_purposes'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0
    {
        conn.revert_last_migration(MIGRATIONS).unwrap();
    }
    drop(conn);
    let db = access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO app_settings VALUES('journal-retry','preserve'); CREATE VIEW investment_decisions AS SELECT 'collision' AS id;").unwrap();
    drop(db);
    let owner = DatabaseOwner::acquire(access.path()).unwrap();
    let error = access
        .run_migrations_with_backup(dir.path().to_str().unwrap(), &owner)
        .unwrap_err();
    assert!(error.to_string().contains("Pre-migration backup retained"));
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='fund_purposes'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "M1 committed before M2 failed"
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='investment_decision_revisions'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "failed M2 leaves no partial group"
    );
    assert_eq!(
        db.query_row(
            "SELECT setting_value FROM app_settings WHERE setting_key='journal-retry'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "preserve"
    );
    db.execute("DROP VIEW investment_decisions", []).unwrap();
    drop(db);
    access
        .run_migrations_with_backup(dir.path().to_str().unwrap(), &owner)
        .unwrap();
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='investment_decision_revisions'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM app_settings WHERE setting_key LIKE 'investments.%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

fn decision_fixture(db: &rusqlite::Connection, id: &str, number: i32) {
    db.execute("INSERT OR IGNORE INTO investment_decisions(id,title,lifecycle,execution_state,created_at,updated_at) VALUES('decision','Fixture','DRAFT','NOT_APPLICABLE','now','now')",[]).unwrap();
    db.execute("INSERT INTO investment_decision_revisions(id,decision_id,revision_number,status,action,thesis,funding_status,scenario_tags_json,body_json,created_at,updated_at,created_channel,created_actor_kind,created_actor_id,updated_channel,updated_actor_kind,updated_actor_id,content_origin) VALUES(?1,'decision',?2,'DRAFT','HOLD','Fixture rationale','UNKNOWN','[]','{\"schemaVersion\":1}','now','now','gui','GUI_SESSION','session','gui','GUI_SESSION','session','UNSPECIFIED')",(id,number)).unwrap();
}
fn confirm_fixture(db: &rusqlite::Connection, id: &str) {
    db.execute("UPDATE investment_decision_revisions SET status='CONFIRMED',context_snapshot_json='{\"schemaVersion\":1}',content_hash='content',context_hash='context',confirmed_at='2026-01-01T00:00:00Z',confirmed_channel='gui',confirmed_actor_kind='GUI_SESSION',confirmed_actor_id='session' WHERE id=?1",[id]).unwrap();
    db.execute("UPDATE investment_decisions SET lifecycle='CONFIRMED',current_revision_id=?1,draft_revision_id=NULL WHERE id='decision'",[id]).unwrap();
}
fn review_fixture(
    db: &rusqlite::Connection,
    id: &str,
    root: &str,
    number: i32,
    parent: Option<&str>,
    cutoff: &str,
) -> rusqlite::Result<usize> {
    db.execute("INSERT INTO investment_reviews(id,decision_revision_id,root_id,revision_number,corrects_review_id,status,observation_cutoff,judgment,execution_assessment,outcome_evidence_json,scenario_tags_json,body_json,created_at,updated_at,created_channel,created_actor_kind,created_actor_id,updated_channel,updated_actor_kind,updated_actor_id,content_origin) VALUES(?1,'confirmed',?2,?3,?4,'DRAFT',?5,'Original judgment','Execution described','[]','[\"波动\"]','{\"schemaVersion\":1}','now','now','gui','GUI_SESSION','session','gui','GUI_SESSION','session','UNSPECIFIED')",(id,root,number,parent,cutoff))
}
fn submit_fixture(db: &rusqlite::Connection, id: &str) {
    db.execute("UPDATE investment_reviews SET status='SUBMITTED',facts_snapshot_json='{\"schemaVersion\":1}',content_hash='content',facts_hash='facts',submitted_at='2026-02-01T00:00:00Z',submitted_channel='gui',submitted_actor_kind='GUI_SESSION',submitted_actor_id='session',submitted_request_id='fixture-request' WHERE id=?1",[id]).unwrap();
}

#[test]
fn m3_history_enforces_cutoff_linear_revisions_and_frozen_principle_sources() {
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let db = access.connect_rusqlite().unwrap();
    db.execute_batch("INSERT INTO business_requests(id,ledger_id,subject,operation,request_key,request_hash,request_snapshot,state,result_json,created_at,updated_at) VALUES('fixture-request','ledger','fixture','fixture','fixture','fixture','{}','completed','{}','now','now');").unwrap();
    decision_fixture(&db, "confirmed", 1);
    confirm_fixture(&db, "confirmed");
    decision_fixture(&db, "draft-decision", 2);
    review_fixture(&db, "review-one", "review-one", 1, None, "2026-01-31").unwrap();
    assert!(db
        .execute(
            "UPDATE investment_reviews SET observation_cutoff='2026-02-01' WHERE id='review-one'",
            []
        )
        .is_err());
    db.execute_batch("INSERT INTO investment_principles(id,name,created_at,updated_at) VALUES('principle','Principle','now','now');
        INSERT INTO investment_principle_revisions(id,principle_id,revision_number,status,name,content,source_review_id,scenario_tags_json,body_json,created_at,updated_at,created_channel,created_actor_kind,created_actor_id,updated_channel,updated_actor_kind,updated_actor_id,content_origin) VALUES('principle-one','principle',1,'DRAFT','Principle','A user-provided rule','review-one','[\"波动\"]','{\"schemaVersion\":1}','now','now','gui','GUI_SESSION','session','gui','GUI_SESSION','session','UNSPECIFIED');
        UPDATE investment_principles SET draft_revision_id='principle-one' WHERE id='principle';").unwrap();
    assert!(db.execute("UPDATE investment_principle_revisions SET content='overwrite draft' WHERE id='principle-one'",[]).is_err(),"even a principle draft may already be a submitted review's source");
    assert!(db.execute("INSERT INTO investment_decision_principles VALUES('decision-rule','draft-decision','principle-one')",[]).is_err(),"decisions only reference published versions");
    db.execute_batch("UPDATE investment_principle_revisions SET status='EFFECTIVE',source_review_snapshot_json='{\"schemaVersion\":1,\"statusAtCapture\":\"DRAFT\",\"judgment\":\"Original judgment\"}',content_hash='content',source_context_hash='source',published_at='2026-02-01T00:00:00Z',published_channel='gui',published_actor_kind='GUI_SESSION',published_actor_id='session',published_request_id='fixture-request' WHERE id='principle-one';
        UPDATE investment_principles SET current_revision_id='principle-one',draft_revision_id=NULL WHERE id='principle';
        INSERT INTO investment_decision_principles VALUES('decision-rule','draft-decision','principle-one');
        INSERT INTO investment_review_principles(id,review_id,position,principle_revision_id,disposition,reason) VALUES('review-rule','review-one',0,'principle-one','KEEP','User assessment');").unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM investment_reviews WHERE id='review-one'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "DRAFT",
        "publishing a principle never implicitly submits its source review"
    );
    db.execute(
        "UPDATE investment_reviews SET judgment='Later draft wording' WHERE id='review-one'",
        [],
    )
    .unwrap();
    let snapshot:String=db.query_row("SELECT source_review_snapshot_json FROM investment_principle_revisions WHERE id='principle-one'",[],|r|r.get(0)).unwrap();
    assert!(snapshot.contains("Original judgment"));
    assert!(snapshot.contains("DRAFT"));
    submit_fixture(&db, "review-one");
    assert!(db
        .execute(
            "UPDATE investment_reviews SET judgment='overwrite' WHERE id='review-one'",
            []
        )
        .is_err());
    assert!(db
        .execute(
            "DELETE FROM investment_review_principles WHERE id='review-rule'",
            []
        )
        .is_err());
    assert!(review_fixture(
        &db,
        "changed-cutoff",
        "review-one",
        2,
        Some("review-one"),
        "2026-02-01"
    )
    .is_err());
    review_fixture(
        &db,
        "review-two",
        "review-one",
        2,
        Some("review-one"),
        "2026-01-31",
    )
    .unwrap();
    assert!(review_fixture(
        &db,
        "second-draft",
        "review-one",
        3,
        Some("review-one"),
        "2026-01-31"
    )
    .is_err());
    submit_fixture(&db, "review-two");
    assert!(review_fixture(
        &db,
        "fork",
        "review-one",
        3,
        Some("review-one"),
        "2026-01-31"
    )
    .is_err());
    review_fixture(
        &db,
        "review-three",
        "review-one",
        3,
        Some("review-two"),
        "2026-01-31",
    )
    .unwrap();
    confirm_fixture(&db, "draft-decision");
    assert!(db
        .execute(
            "DELETE FROM investment_decision_principles WHERE id='decision-rule'",
            []
        )
        .is_err());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    // Roll back to M2 in this disposable database, then upgrade.
    drop(db);
    let mut conn = access.connect().unwrap();
    // The follow-up cache provenance column owns no journal data.
    conn.revert_last_migration(MIGRATIONS).unwrap();
    conn.revert_last_migration(MIGRATIONS).unwrap();
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM investment_decision_revisions",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    drop(db);
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM investment_reviews", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn m3_interruption_rolls_back_its_group_and_preserves_m2_records_for_retry() {
    use wealthfolio_storage_sqlite::db::DatabaseOwner;
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let mut conn = access.connect().unwrap();
    // The follow-up cache provenance column owns no journal data.
    conn.revert_last_migration(MIGRATIONS).unwrap();
    conn.revert_last_migration(MIGRATIONS).unwrap();
    drop(conn);
    let db = access.connect_rusqlite().unwrap();
    decision_fixture(&db, "confirmed", 1);
    confirm_fixture(&db, "confirmed");
    db.execute_batch("CREATE VIEW investment_principles AS SELECT 'collision' AS id;")
        .unwrap();
    let before: String = db
        .query_row(
            "SELECT body_json FROM investment_decision_revisions WHERE id='confirmed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(db);
    let owner = DatabaseOwner::acquire(access.path()).unwrap();
    let error = access
        .run_migrations_with_backup(dir.path().to_str().unwrap(), &owner)
        .unwrap_err();
    assert!(error.to_string().contains("Pre-migration backup retained"));
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='investment_reviews'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "M3 created reviews before failing at principles, then rolled back the whole group"
    );
    assert_eq!(
        db.query_row(
            "SELECT body_json FROM investment_decision_revisions WHERE id='confirmed'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        before
    );
    db.execute("DROP VIEW investment_principles", []).unwrap();
    drop(db);
    access
        .run_migrations_with_backup(dir.path().to_str().unwrap(), &owner)
        .unwrap();
    let db = access.connect_rusqlite().unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('fund_purposes','fund_allocation_operations','fund_allocation_entries','fund_allocations','investment_decisions','investment_decision_revisions','investment_decision_options','investment_decision_evidence','investment_decision_funding','investment_executions','investment_execution_links','investment_projection_state','investment_reviews','investment_principles','investment_principle_revisions','investment_decision_principles','investment_review_principles')",[],|r|r.get::<_,i64>(0)).unwrap(),17);
    assert_eq!(
        db.query_row(
            "SELECT body_json FROM investment_decision_revisions WHERE id='confirmed'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        before
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn cached_quote_input_migration_does_not_rewrite_historical_valuations() {
    let dir = tempfile::tempdir().unwrap();
    let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.run_migrations().unwrap();
    let mut conn = access.connect().unwrap();
    conn.revert_last_migration(MIGRATIONS).unwrap();
    drop(conn);
    let db = access.connect_rusqlite().unwrap();
    db.execute_batch(
        "INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode)
         VALUES('legacy-cache','Legacy cache','SECURITIES','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS');
         INSERT INTO daily_account_valuation(id,account_id,valuation_date,account_currency,base_currency,fx_rate_to_base,cash_balance,investment_market_value,total_value,cost_basis,net_contribution,cash_balance_base,investment_market_value_base,total_value_base,calculated_at)
         VALUES('legacy-value','legacy-cache','2026-11-02','CNY','CNY','1','50','100','150','100','100','50','100','150','2026-11-02T00:00:00Z');",
    ).unwrap();
    drop(db);
    access.run_migrations().unwrap();
    let db = access.connect_rusqlite().unwrap();
    let result: (String, String, Option<String>) = db.query_row(
        "SELECT cash_balance_base,investment_market_value_base,quote_inputs FROM daily_account_valuation WHERE id='legacy-value'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(result, ("50".into(), "100".into(), None));
}
