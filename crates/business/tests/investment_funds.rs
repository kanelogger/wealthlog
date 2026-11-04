#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use chrono::NaiveDate;
use diesel::SqliteConnection;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use tempfile::TempDir;
use wealthfolio_business::{investment_journal::*, BusinessChannel, BusinessStore};
use wealthfolio_storage_sqlite::db::{write_actor::spawn_writer, DbAccess};

fn d(text: &str) -> ExactDecimal {
    ExactDecimal::parse(text).unwrap()
}
fn cash() -> FundResource {
    FundResource::Cash {
        account_id: "cash".into(),
        currency: "CNY".into(),
    }
}
fn caller(channel: BusinessChannel) -> JournalCaller {
    JournalCaller {
        channel,
        actor_id: "agent".into(),
        credential_id: Some("token".into()),
        actor_kind: JournalActorKind::Program,
        authenticated: true,
        can_read: true,
        can_write: true,
        can_confirm: false,
        fact_permissions: JournalFactPermissions::default(),
    }
}

struct Reader {
    actual: RwLock<Option<ExactDecimal>>,
    version: RwLock<String>,
    sources: RwLock<Vec<ConfirmedSource>>,
}
#[async_trait::async_trait]
impl InvestmentResourceReader for Reader {
    async fn observe(
        &self,
        resources: &[FundResource],
        day: NaiveDate,
    ) -> JournalResult<Vec<ResourceObservation>> {
        Ok(resources
            .iter()
            .map(|r| ResourceObservation {
                resource: r.clone(),
                actual: *self.actual.read().unwrap(),
                observed_at: chrono::Utc::now().to_rfc3339(),
                as_of_date: day,
                tracking_mode: "TRANSACTIONS".into(),
                state_version: self.version.read().unwrap().clone(),
                sources: self.sources.read().unwrap().clone(),
                unresolved_sources: vec![],
                issues: vec![],
            })
            .collect())
    }
    fn validate(
        &self,
        _: &mut SqliteConnection,
        rows: &[ResourceObservation],
    ) -> JournalResult<()> {
        if rows
            .iter()
            .any(|r| r.state_version != *self.version.read().unwrap())
        {
            Err(JournalError::StateConflict)
        } else {
            Ok(())
        }
    }
}
struct Harness {
    _dir: TempDir,
    access: DbAccess,
    service: InvestmentJournalService,
    reader: Arc<Reader>,
    timezone: Arc<RwLock<String>>,
}
impl Harness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let access = DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
        access.run_migrations().unwrap();
        let pool = access.create_pool().unwrap();
        let writer = spawn_writer(pool.as_ref().clone()).unwrap();
        let reader = Arc::new(Reader {
            actual: RwLock::new(Some(d("1000"))),
            version: RwLock::new("v1".into()),
            sources: RwLock::new(vec![]),
        });
        let timezone = Arc::new(RwLock::new("Asia/Shanghai".into()));
        let service = InvestmentJournalService::new(
            BusinessStore::new(pool, writer),
            "ledger".into(),
            reader.clone(),
            timezone.clone(),
        );
        Self {
            _dir: dir,
            access,
            service,
            reader,
            timezone,
        }
    }
    async fn purpose(&self, key: &str) -> String {
        let result = self
            .service
            .save_purpose(
                SaveFundPurpose {
                    id: None,
                    expected_version: None,
                    name: key.into(),
                    category: "CUSTOM".into(),
                    currency: "CNY".into(),
                    target_amount: None,
                    use_on: None,
                    liquidity: None,
                    minimum_cash: None,
                    loss_budget: None,
                    single_asset_limit_bps: None,
                    is_archived: false,
                    request_key: key.into(),
                },
                &caller(BusinessChannel::Cli),
            )
            .await
            .unwrap();
        result["purposeId"].as_str().unwrap().to_string()
    }
    async fn input(&self, purpose: &str, delta: &str, key: &str) -> ApplyFundAllocation {
        let preview = self
            .service
            .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
            .await
            .unwrap();
        ApplyFundAllocation {
            operation_kind: AllocationOperationKind::Allocate,
            reason: "Test ownership".into(),
            entries: vec![AllocationDelta {
                purpose_id: purpose.into(),
                resource: cash(),
                delta: d(delta),
            }],
            resources: vec![cash()],
            expected_preview_hash: preview.preview_hash,
            request_key: key.into(),
        }
    }
    fn count(&self, table: &str) -> i64 {
        self.access
            .connect_rusqlite()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn decimal_boundaries_and_resource_accounting_keep_unknown_separate() {
    assert_eq!(
        serde_json::to_value(d("9007199254740993.0100")).unwrap(),
        json!("9007199254740993.01")
    );
    assert!(serde_json::from_value::<ExactDecimal>(json!(1.1)).is_err());
    assert!(ExactDecimal::parse("0.12345678901234567890123456789").is_err());
    assert!(ExactDecimal::parse("1e2").is_err());
    let negative = ResourceBalance::calculate(Some(d("-30")), d("40")).unwrap();
    assert_eq!(negative.excess, Some(d("40")));
    assert_eq!(negative.negative_actual, Some(d("30")));
    assert_eq!(negative.unallocated, Some(d("0")));
    let unknown = ResourceBalance::calculate(None, d("40")).unwrap();
    assert!(unknown.excess.is_none() && unknown.unallocated.is_none());
    let normal = ResourceBalance::calculate(Some(d("1000")), d("600")).unwrap();
    assert_eq!(normal.actual, Some(d("1000")));
    assert_eq!(normal.unallocated, Some(d("400")));
}

#[test]
fn transfer_rounding_is_exact_stable_and_preserves_partial_migration() {
    let shares = vec![
        ("c".into(), d("1")),
        ("b".into(), d("1")),
        ("a".into(), d("1")),
    ];
    let full = apportion_transfer(d("3"), d("100"), "CNY", 0, &shares).unwrap();
    assert_eq!(full[0].destination_amount, d("33.34"));
    assert_eq!(full[1].destination_amount, d("33.33"));
    assert_eq!(
        full.iter()
            .try_fold(ExactDecimal::ZERO, |s, r| s
                .checked_add(r.destination_amount))
            .unwrap(),
        d("100")
    );
    let mut reversed = shares.clone();
    reversed.reverse();
    assert_eq!(
        full,
        apportion_transfer(d("3"), d("100"), "CNY", 0, &reversed).unwrap()
    );
    let partial = apportion_transfer(d("3"), d("100"), "CNY", 0, &shares[..2]).unwrap();
    assert_eq!(
        partial
            .iter()
            .try_fold(ExactDecimal::ZERO, |s, r| s
                .checked_add(r.destination_amount))
            .unwrap(),
        d("66.67")
    );
    let precise = apportion_transfer(d("3"), d("0.001"), "CNY", 0, &shares).unwrap();
    assert_eq!(precise[0].destination_amount, d("0.001"));
    let recorded = apportion_transfer(d("3"), d("1.2300"), "CNY", 4, &shares).unwrap();
    assert_eq!(recorded[0].destination_amount, d("0.41"));
    let recorded = apportion_transfer(d("3"), d("1.0000"), "CNY", 4, &shares).unwrap();
    assert_eq!(recorded[0].destination_amount, d("0.3334"));
    // Multiplication of these two Decimals would overflow without exact integer ratios.
    let large = apportion_transfer(
        d("79228162514264337593543950335"),
        d("79228162514264337593543950335"),
        "JPY",
        0,
        &[("a".into(), d("79228162514264337593543950335"))],
    )
    .unwrap();
    assert_eq!(
        large[0].destination_amount,
        d("79228162514264337593543950335")
    );
    assert!(apportion_transfer(d("1"), d("2"), "CNY", 0, &shares).is_err());
}

#[tokio::test]
async fn all_purposes_share_capacity_and_retry_is_frozen() {
    let h = Harness::new().await;
    let a = h.purpose("a").await;
    let b = h.purpose("b").await;
    let first = h.input(&a, "600", "allocate-a").await;
    let result = h
        .service
        .apply_allocation(first.clone(), &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    let overflow = h.input(&b, "500", "overflow").await;
    assert_eq!(
        h.service
            .apply_allocation(overflow, &caller(BusinessChannel::Mcp))
            .await
            .unwrap_err()
            .code(),
        "insufficient_resource"
    );
    *h.reader.version.write().unwrap() = "v2".into();
    *h.reader.actual.write().unwrap() = Some(d("500"));
    let replay = h
        .service
        .apply_allocation(first.clone(), &caller(BusinessChannel::Mcp))
        .await
        .unwrap();
    assert_eq!(result["operationId"], replay["operationId"]);
    assert_eq!(replay["replayed"], true);
    assert_eq!(h.count("fund_allocation_entries"), 1);
    let mut changed = first;
    changed.reason = "Different intent".into();
    assert_eq!(
        h.service
            .apply_allocation(changed, &caller(BusinessChannel::Cli))
            .await
            .unwrap_err()
            .code(),
        "request_conflict"
    );
    let preview = h
        .service
        .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    assert_eq!(preview.resources[0].balance.excess, Some(d("100")));
    let decrease = h.input(&a, "-601", "negative").await;
    assert_eq!(
        h.service
            .apply_allocation(decrease, &caller(BusinessChannel::Cli))
            .await
            .unwrap_err()
            .code(),
        "negative_allocation"
    );
    let decrease = h.input(&a, "-100", "release").await;
    h.service
        .apply_allocation(decrease, &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    assert_eq!(h.count("fund_allocation_entries"), 2);
}

#[tokio::test]
async fn first_write_enablement_rolls_back_with_audit_and_does_not_reset() {
    let h = Harness::new().await;
    let settings_before = h.count("app_settings");
    h.service
        .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    assert_eq!(h.count("app_settings"), settings_before);
    let db = h.access.connect_rusqlite().unwrap();
    db.execute_batch("CREATE TRIGGER fail_journal_audit BEFORE INSERT ON business_audit BEGIN SELECT RAISE(ABORT,'test audit failure'); END;").unwrap();
    let input: SaveFundPurpose = serde_json::from_value(
        json!({"name":"purpose","category":"CUSTOM","currency":"CNY","requestKey":"same-key"}),
    )
    .unwrap();
    assert!(h
        .service
        .save_purpose(input.clone(), &caller(BusinessChannel::Mcp))
        .await
        .is_err());
    assert_eq!(h.count("fund_purposes"), 0);
    assert_eq!(h.count("business_requests"), 0);
    assert_eq!(h.count("app_settings"), settings_before);
    db.execute("DROP TRIGGER fail_journal_audit", []).unwrap();
    h.service
        .save_purpose(input, &caller(BusinessChannel::Mcp))
        .await
        .unwrap();
    let markers = || {
        let mut stmt = db
            .prepare("SELECT setting_key,setting_value FROM app_settings ORDER BY setting_key")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<BTreeMap<_, _>, _>>()
            .unwrap()
    };
    let first = markers();
    assert_eq!(first["investments.enabled_timezone"], "Asia/Shanghai");
    *h.timezone.write().unwrap() = "America/New_York".into();
    h.purpose("second").await;
    assert_eq!(markers(), first);
}

#[tokio::test]
async fn concurrent_previews_conflict_unknown_does_not_allocate_and_refresh_is_stable() {
    let h = Harness::new().await;
    let a = h.purpose("a").await;
    let first = h.input(&a, "200", "one").await;
    let second = h.input(&a, "200", "two").await;
    assert_eq!(
        first.expected_preview_hash, second.expected_preview_hash,
        "refresh time is not financial state"
    );
    let cli = caller(BusinessChannel::Cli);
    let mcp = caller(BusinessChannel::Mcp);
    let (one, two) = tokio::join!(
        h.service.apply_allocation(first, &cli),
        h.service.apply_allocation(second, &mcp)
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert_eq!(one.err().or(two.err()).unwrap().code(), "state_conflict");
    *h.reader.actual.write().unwrap() = None;
    *h.reader.version.write().unwrap() = "unknown".into();
    let request = h.input(&a, "1", "unknown").await;
    assert_eq!(
        h.service
            .apply_allocation(request, &caller(BusinessChannel::Cli))
            .await
            .unwrap_err()
            .code(),
        "resource_unavailable"
    );
    let preview = h
        .service
        .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    let reconcile = ApplyFundAllocation {
        operation_kind: AllocationOperationKind::Reconcile,
        reason: "Confirm sources, keep the excess visible".into(),
        entries: vec![],
        resources: vec![cash()],
        expected_preview_hash: preview.preview_hash,
        request_key: "confirm-sources".into(),
    };
    h.service
        .apply_allocation(reconcile, &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    assert_eq!(h.count("fund_allocation_operations"), 2);
    assert_eq!(h.count("fund_allocation_entries"), 1);
    let mut unauthorized = caller(BusinessChannel::Cli);
    unauthorized.can_write = false;
    assert!(matches!(
        unauthorized.require_write(),
        Err(JournalError::PermissionDenied)
    ));
    assert!(matches!(
        caller(BusinessChannel::Cli).require_confirm(),
        Err(JournalError::PermissionDenied)
    ));
    assert!(caller(BusinessChannel::Rest).require_read().is_err());
}

#[tokio::test]
async fn balances_rebuild_from_entries_and_source_confirmation_cannot_hide_excess() {
    let h = Harness::new().await;
    let purpose = h.purpose("a").await;
    let input = h.input(&purpose, "600", "allocate").await;
    h.service
        .apply_allocation(input, &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    h.access
        .connect_rusqlite()
        .unwrap()
        .execute("UPDATE fund_allocations SET cash_amount='1'", [])
        .unwrap();
    assert_eq!(
        h.service
            .rebuild_allocations(&caller(BusinessChannel::Cli))
            .await
            .unwrap(),
        1
    );
    let preview = h
        .service
        .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    assert_eq!(preview.resources[0].balance.allocated, d("600"));
    *h.reader.actual.write().unwrap() = Some(d("500"));
    *h.reader.version.write().unwrap() = "after-expense".into();
    let items = h
        .service
        .fund_reconciliation(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, FundReconciliationKind::PurposeExcess);
    let preview = h
        .service
        .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    h.service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Confirm current sources without reallocating".into(),
                entries: vec![],
                resources: vec![cash()],
                expected_preview_hash: preview.preview_hash,
                request_key: "reconcile".into(),
            },
            &caller(BusinessChannel::Cli),
        )
        .await
        .unwrap();
    assert_eq!(
        h.service
            .fund_reconciliation(vec![cash()], &caller(BusinessChannel::Cli))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(h.count("fund_allocation_entries"), 1);
}

#[test]
fn source_reconciliation_keeps_offsetting_changes_and_ignores_included_events() {
    let source = |id: &str, version: &str, effect| ConfirmedSource {
        kind: InvestmentSourceKind::Activity,
        id: id.into(),
        version: version.into(),
        applied: true,
        effect,
    };
    let baseline = ResourceObservation {
        resource: cash(),
        actual: Some(d("1000")),
        observed_at: "then".into(),
        as_of_date: NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
        tracking_mode: "TRANSACTIONS".into(),
        state_version: "one".into(),
        sources: vec![source("old-split", "v1", SourceEffect::CorporateAction)],
        unresolved_sources: vec![],
        issues: vec![],
    };
    assert!(compare_fund_observation(&baseline, &baseline, d("600"))
        .unwrap()
        .is_empty());
    let mut current = baseline.clone();
    current
        .sources
        .push(source("ordinary-spend", "v1", SourceEffect::Ordinary));
    assert!(compare_fund_observation(&baseline, &current, d("600"))
        .unwrap()
        .is_empty());
    current.sources.push(source(
        "late-historical-split",
        "v1",
        SourceEffect::CorporateAction,
    ));
    current.sources.push(source(
        "offsetting-calibration",
        "v1",
        SourceEffect::Calibration,
    ));
    // Equal total B does not prove these separate sources were confirmed.
    let items = compare_fund_observation(&baseline, &current, d("600")).unwrap();
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .any(|i| i.kind == FundReconciliationKind::CorporateAction));
    assert!(compare_fund_observation(&current, &current, d("600"))
        .unwrap()
        .is_empty());
    current.sources[0].version = "v2".into();
    assert!(compare_fund_observation(&baseline, &current, d("600"))
        .unwrap()
        .iter()
        .any(|i| i.kind == FundReconciliationKind::SourceChanged));
    assert!(
        compare_fund_observation(&baseline, &current, ExactDecimal::ZERO)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn changing_ownership_does_not_implicitly_confirm_changed_sources() {
    let h = Harness::new().await;
    let purpose = h.purpose("a").await;
    h.service
        .apply_allocation(
            h.input(&purpose, "200", "first").await,
            &caller(BusinessChannel::Cli),
        )
        .await
        .unwrap();
    *h.reader.version.write().unwrap() = "v2".into();
    h.reader.sources.write().unwrap().push(ConfirmedSource {
        kind: InvestmentSourceKind::Snapshot,
        id: "new-observation".into(),
        version: "v1".into(),
        applied: true,
        effect: SourceEffect::Calibration,
    });
    assert_eq!(
        h.service
            .fund_reconciliation(vec![cash()], &caller(BusinessChannel::Cli))
            .await
            .unwrap()
            .len(),
        1
    );
    h.service
        .apply_allocation(
            h.input(&purpose, "40", "second").await,
            &caller(BusinessChannel::Cli),
        )
        .await
        .unwrap();
    assert_eq!(
        h.service
            .fund_reconciliation(vec![cash()], &caller(BusinessChannel::Cli))
            .await
            .unwrap()
            .len(),
        1
    );
    let preview = h
        .service
        .preview_allocations(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap();
    h.service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Explicitly confirm sources".into(),
                entries: vec![],
                resources: vec![cash()],
                expected_preview_hash: preview.preview_hash,
                request_key: "reconcile".into(),
            },
            &caller(BusinessChannel::Cli),
        )
        .await
        .unwrap();
    assert!(h
        .service
        .fund_reconciliation(vec![cash()], &caller(BusinessChannel::Cli))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn unknown_actual_balance_stays_unresolved_after_zero_entry_acknowledgement() {
    let h = Harness::new().await;
    let purpose = h.purpose("a").await;
    let c = caller(BusinessChannel::Cli);
    h.service
        .apply_allocation(h.input(&purpose, "200", "allocate").await, &c)
        .await
        .unwrap();
    *h.reader.actual.write().unwrap() = None;
    *h.reader.version.write().unwrap() = "unknown".into();
    let items = h
        .service
        .fund_reconciliation(vec![cash()], &c)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, FundReconciliationKind::ResourceUnavailable);
    let p = h
        .service
        .preview_allocations(vec![cash()], &c)
        .await
        .unwrap();
    h.service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "Acknowledge sources without inventing a zero balance".into(),
                entries: vec![],
                resources: vec![cash()],
                expected_preview_hash: p.preview_hash,
                request_key: "ack".into(),
            },
            &c,
        )
        .await
        .unwrap();
    assert_eq!(
        h.service
            .fund_reconciliation(vec![cash()], &c)
            .await
            .unwrap()[0]
            .kind,
        FundReconciliationKind::ResourceUnavailable
    );
    h.service
        .apply_allocation(h.input(&purpose, "-200", "release").await, &c)
        .await
        .unwrap();
    assert!(h
        .service
        .fund_reconciliation(vec![cash()], &c)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn purpose_valuation_currency_can_change_without_converting_real_resource_units() {
    let h = Harness::new().await;
    let id = h.purpose("a").await;
    let c = caller(BusinessChannel::Cli);
    h.service
        .apply_allocation(h.input(&id, "600", "allocate").await, &c)
        .await
        .unwrap();
    let before = h
        .service
        .preview_allocations(vec![cash()], &c)
        .await
        .unwrap();
    h.service
        .save_purpose(
            SaveFundPurpose {
                id: Some(id.clone()),
                expected_version: Some(1),
                name: "a".into(),
                category: "CUSTOM".into(),
                currency: "USD".into(),
                target_amount: None,
                use_on: None,
                liquidity: None,
                minimum_cash: None,
                loss_budget: None,
                single_asset_limit_bps: None,
                is_archived: false,
                request_key: "valuation-currency".into(),
            },
            &c,
        )
        .await
        .unwrap();
    let after = h
        .service
        .preview_allocations(vec![cash()], &c)
        .await
        .unwrap();
    assert_eq!(before.resources[0].balance, after.resources[0].balance);
    assert_eq!(after.resources[0].observation.resource, cash());
    assert_eq!(after.resources[0].balance.actual, Some(d("1000")));
    assert_eq!(after.resources[0].purposes[&id], d("600"));
    assert_eq!(h.service.list_purposes(&c).unwrap()[0]["currency"], "USD");
}

#[tokio::test]
async fn first_actual_ownership_uses_current_units_even_after_an_empty_prior_acknowledgement() {
    let h = Harness::new().await;
    let id = h.purpose("purpose").await;
    let c = caller(BusinessChannel::Cli);
    let p = h
        .service
        .preview_allocations(vec![cash()], &c)
        .await
        .unwrap();
    h.service
        .apply_allocation(
            ApplyFundAllocation {
                operation_kind: AllocationOperationKind::Reconcile,
                reason: "No ownership exists yet".into(),
                entries: vec![],
                resources: vec![cash()],
                expected_preview_hash: p.preview_hash,
                request_key: "empty-before-first".into(),
            },
            &c,
        )
        .await
        .unwrap();
    *h.reader.actual.write().unwrap() = Some(d("1100"));
    *h.reader.version.write().unwrap() = "after-corporate-action".into();
    h.reader.sources.write().unwrap().push(ConfirmedSource {
        kind: InvestmentSourceKind::Activity,
        id: "already-included".into(),
        version: "one".into(),
        applied: true,
        effect: SourceEffect::CorporateAction,
    });
    h.service
        .apply_allocation(h.input(&id, "200", "first-ownership").await, &c)
        .await
        .unwrap();
    assert!(h.service.fund_reconciliation(vec![cash()],&c).await.unwrap().is_empty(),"first ownership observes the current ledger; an earlier zero-entry source acknowledgement does not turn included events into new ownership changes");
}
