use super::agent_environment::TauriAgentEnvironment;
use super::registry::ServiceContext;
use crate::domain_events::TauriDomainEventSink;

use log::error;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

use wealthfolio_core::portfolio::coordinator::{
    CoordinatorDeps, FactSources, PortfolioCoordinator, WindowCadence,
};
use wealthfolio_core::portfolio::projection::ProjectionStoreTrait;
use wealthfolio_core::secrets::SecretStore;
use wealthfolio_core::{
    accounts::AccountService,
    activities::{rebuild_pending_final_cash_accounts, run_final_cash_migration, ActivityService},
    assets::{AlternativeAssetService, AssetClassificationService, AssetLogoService, AssetService},
    events::DomainEvent,
    fx::{FxService, FxServiceTrait},
    health::HealthService,
    portfolio::{
        allocation::AllocationService,
        allocation_targets::{AllocationTargetService, DriftService, RebalanceService},
        holdings::{HoldingsService, HoldingsValuationService},
        income::IncomeService,
        net_worth::NetWorthService,
        performance::PerformanceService,
        snapshot::SnapshotService,
        valuation::ValuationService,
    },
    portfolios::PortfolioService,
    quotes::{QuoteService, QuoteServiceTrait},
    settings::{SettingsService, SettingsServiceTrait},
    taxonomies::TaxonomyService,
};

use wealthfolio_storage_sqlite::{
    accounts::AccountRepository,
    activities::ActivityRepository,
    agent::{McpAuditRepository, PatRepository},
    assets::{AlternativeAssetRepository, AssetLogoRepository, AssetRepository},
    db::{self, write_actor, WriteHandle, WriterTask},
    fx::FxRepository,
    health::HealthDismissalRepository,
    market_data::{MarketDataRepository, QuoteSyncStateRepository},
    portfolio::{
        allocation_targets::AllocationTargetRepository, snapshot::SnapshotRepository,
        valuation::ValuationRepository,
    },
    portfolios::PortfolioRepository,
    settings::SettingsRepository,
    taxonomies::TaxonomyRepository,
};

/// Result of context initialization, including the receiver for domain events.
pub struct ContextInitResult {
    pub context: ServiceContext,
    pub event_receiver: mpsc::UnboundedReceiver<DomainEvent>,
    /// Handed to the database runtime so maintenance can stop the writer and
    /// wait for its pooled connection to be released.
    pub writer: WriteHandle,
    pub writer_task: WriterTask,
    pub final_cash_rebuild: Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>>,
}

/// Builds every repository and service over an already-resolved database.
///
/// `access` carries the path and, when the database is encrypted, the key — so
/// that `PRAGMA key` is applied at every connection site, including the pool the
/// write actor draws from.
pub async fn initialize_context(
    app_data_dir: &str,
    access: &db::DbAccess,
    owner: Arc<db::DatabaseOwner>,
    profile_id: uuid::Uuid,
    secret_store: Arc<dyn SecretStore>,
) -> Result<ContextInitResult, Box<dyn std::error::Error>> {
    let migration_access = access.clone();
    let migration_owner = owner.clone();
    let backup_root = app_data_dir.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        migration_access.run_migrations_with_backup(&backup_root, &migration_owner)
    })
    .await??;

    let pool = access.create_pool_with_owner(owner)?;
    initialize_with_pool(pool, profile_id, secret_store).await
}

async fn initialize_with_pool(
    pool: Arc<db::DbPool>,
    profile_id: uuid::Uuid,
    secret_store: Arc<dyn SecretStore>,
) -> Result<ContextInitResult, Box<dyn std::error::Error>> {
    let (writer, writer_task) = write_actor::spawn_writer_with_task(pool.as_ref().clone())
        .map_err(|e| {
            error!("Failed to initialize writer actor: {}", e);
            e
        })?;

    let built = build_context(pool, writer.clone(), profile_id, secret_store)
        .await
        .map_err(|error| error.to_string());
    match built {
        Ok(built) => Ok(ContextInitResult {
            context: built.context,
            event_receiver: built.event_receiver,
            writer,
            writer_task,
            final_cash_rebuild: built.final_cash_rebuild,
        }),
        Err(error) => {
            // Failed construction has dropped every repository before stopping
            // the writer's independent pooled connection. Rollback may follow.
            writer.shutdown().await;
            writer_task.join().await;
            Err(error.into())
        }
    }
}

struct BuiltContext {
    context: ServiceContext,
    event_receiver: mpsc::UnboundedReceiver<DomainEvent>,
    final_cash_rebuild: Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>>,
}

async fn build_context(
    pool: Arc<db::DbPool>,
    writer: WriteHandle,
    profile_id: uuid::Uuid,
    secret_store: Arc<dyn SecretStore>,
) -> Result<BuiltContext, Box<dyn std::error::Error>> {
    // Instantiate Repositories
    let settings_repository = Arc::new(SettingsRepository::new(pool.clone(), writer.clone()));
    let account_repository = Arc::new(AccountRepository::new(pool.clone(), writer.clone()));
    let activity_repository = Arc::new(ActivityRepository::new(pool.clone(), writer.clone()));
    let asset_repository = Arc::new(AssetRepository::new(pool.clone(), writer.clone()));
    let market_data_repo = Arc::new(MarketDataRepository::new(pool.clone(), writer.clone()));
    let fx_repository = Arc::new(FxRepository::new(pool.clone(), writer.clone()));
    let snapshot_repository = Arc::new(SnapshotRepository::new(pool.clone(), writer.clone()));
    let lots_repository = Arc::new(wealthfolio_storage_sqlite::lots::LotsRepository::new(
        pool.clone(),
        writer.clone(),
    ));
    let valuation_repository = Arc::new(ValuationRepository::new(pool.clone(), writer.clone()));

    // Domain event sink - TauriDomainEventSink sends events to a channel
    // The worker will be started by the caller after the context is managed
    // Must be created before services that emit events
    let (domain_event_sink, event_receiver) = TauriDomainEventSink::new();
    let domain_event_sink: Arc<dyn wealthfolio_core::events::DomainEventSink> =
        Arc::new(domain_event_sink);

    let fx_service =
        Arc::new(FxService::new(fx_repository.clone()).with_event_sink(domain_event_sink.clone()));
    fx_service.initialize()?;

    let settings_service = Arc::new(SettingsService::new(
        settings_repository.clone(),
        fx_service.clone(),
    ));

    // Spending settings service (uses the same app_settings k/v store)
    let spending_settings_repo: Arc<
        dyn wealthfolio_spending::settings::SpendingSettingsRepositoryTrait,
    > = Arc::new(
        wealthfolio_storage_sqlite::spending::settings::SpendingSettingsRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let spending_settings_service = Arc::new(
        wealthfolio_spending::settings::SpendingSettingsService::new(spending_settings_repo),
    );

    // Spending: activity_taxonomy_assignments adapter (built before activity_service so we can pass
    // both into the cash_activity service after activity_repository is created)
    let activity_assignments_repo: Arc<
        dyn wealthfolio_spending::activity_assignments::ActivityTaxonomyAssignmentRepositoryTrait,
    > = Arc::new(
        wealthfolio_storage_sqlite::spending::activity_assignments::ActivityTaxonomyAssignmentRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    // Activity ↔ event tag join table (sidecar to activities; see
    // crates/spending/src/activity_events for the design rationale).
    let activity_events_repo: Arc<
        dyn wealthfolio_spending::activity_events::ActivityEventsRepositoryTrait,
    > = Arc::new(
        wealthfolio_storage_sqlite::spending::activity_events::ActivityEventsRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let activity_splits_repo: Arc<
        dyn wealthfolio_spending::activity_splits::ActivitySplitRepositoryTrait,
    > = Arc::new(
        wealthfolio_storage_sqlite::spending::activity_splits::ActivitySplitRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let activity_taxonomy_assignment_service = Arc::new(
        wealthfolio_spending::activity_assignments::ActivityTaxonomyAssignmentService::new(
            activity_assignments_repo.clone(),
        ),
    );
    let settings = settings_service.get_settings()?;
    let base_currency_string = settings.base_currency.clone();
    let base_currency = Arc::new(RwLock::new(base_currency_string.clone()));
    let timezone = Arc::new(RwLock::new(settings.timezone.clone()));

    // Custom provider repository
    let custom_provider_repository = Arc::new(
        wealthfolio_storage_sqlite::custom_provider::CustomProviderSqliteRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );

    // Quote sync state repository for optimized quote syncing
    let quote_sync_state_repository =
        Arc::new(QuoteSyncStateRepository::new(pool.clone(), writer.clone()));

    // QuoteService provides all quote operations via QuoteServiceTrait
    let quote_service: Arc<dyn QuoteServiceTrait> = Arc::new(
        QuoteService::new_with_custom_provider(
            market_data_repo.clone(),            // QuoteStore
            quote_sync_state_repository.clone(), // SyncStateStore
            market_data_repo.clone(),            // ProviderSettingsStore
            asset_repository.clone(),            // AssetRepositoryTrait
            activity_repository.clone(),         // ActivityRepositoryTrait
            secret_store.clone(),
            Some(custom_provider_repository.clone()),
        )
        .await?
        .with_event_sink(domain_event_sink.clone()),
    );

    // Portfolio service
    let portfolio_repository = Arc::new(PortfolioRepository::new(pool.clone(), writer.clone()));
    let portfolio_service = Arc::new(PortfolioService::new(
        portfolio_repository,
        account_repository.clone(),
    ));

    // Custom provider service
    let custom_provider_service = Arc::new(
        wealthfolio_core::custom_provider::CustomProviderService::new(
            custom_provider_repository.clone(),
            secret_store.clone(),
        ),
    );

    // Create taxonomy service before asset service (needed for auto-classification)
    let taxonomy_repository = Arc::new(TaxonomyRepository::new(pool.clone(), writer.clone()));
    let taxonomy_service = Arc::new(
        TaxonomyService::new(taxonomy_repository).with_event_sink(domain_event_sink.clone()),
    );

    let asset_service = Arc::new(
        AssetService::with_taxonomy_service(
            asset_repository.clone(),
            quote_service.clone(),
            taxonomy_service.clone(),
        )?
        .with_event_sink(domain_event_sink.clone()),
    );

    let account_service = Arc::new(AccountService::new(
        account_repository.clone(),
        fx_service.clone(),
        base_currency.clone(),
        domain_event_sink.clone(),
        asset_repository.clone(),
        quote_sync_state_repository.clone(),
    ));

    // Spending: events + event_types
    let event_types_repo: Arc<dyn wealthfolio_spending::events::EventTypesRepositoryTrait> =
        Arc::new(
            wealthfolio_storage_sqlite::spending::events::EventTypesRepository::new(
                pool.clone(),
                writer.clone(),
            ),
        );
    let events_repo: Arc<dyn wealthfolio_spending::events::EventsRepositoryTrait> = Arc::new(
        wealthfolio_storage_sqlite::spending::events::EventsRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let events_service = Arc::new(wealthfolio_spending::events::EventsService::new(
        event_types_repo,
        events_repo,
        activity_repository.clone(),
        activity_events_repo.clone(),
    ));

    // Spending: cash_activity_service depends on the activity_repository + spending settings
    //          + the assignments service (so search() can batch-fetch assignments and apply
    //          status/category filters server-side).
    let cash_activity_service = Arc::new(
        wealthfolio_spending::cash_activities::CashActivityService::new(
            activity_repository.clone(),
            account_repository.clone(),
            spending_settings_service.clone(),
            activity_taxonomy_assignment_service.clone(),
            activity_splits_repo.clone(),
            activity_events_repo.clone(),
            events_service.clone(),
            fx_service.clone(),
            taxonomy_service.clone(),
        ),
    );

    // Spending: categorization_rules
    let categorization_rules_repo: Arc<
        dyn wealthfolio_spending::categorization_rules::CategorizationRulesRepositoryTrait,
    > = Arc::new(
        wealthfolio_storage_sqlite::spending::categorization_rules::CategorizationRulesRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let categorization_rules_service = Arc::new(
        wealthfolio_spending::categorization_rules::CategorizationRulesService::new(
            categorization_rules_repo,
            activity_repository.clone(),
            activity_taxonomy_assignment_service.clone(),
        ),
    );

    // Spending: budget
    let budget_repo: Arc<dyn wealthfolio_spending::budget::BudgetRepositoryTrait> = Arc::new(
        wealthfolio_storage_sqlite::spending::budget::BudgetRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let budget_service = Arc::new(wealthfolio_spending::budget::BudgetService::new(
        budget_repo,
        activity_repository.clone(),
        account_repository.clone(),
        activity_assignments_repo.clone(),
        activity_splits_repo.clone(),
        spending_settings_service.clone(),
        taxonomy_service.clone(),
        fx_service.clone(),
    ));

    // Spending: analytics — needs activity repo + assignment repo (re-built since the
    // assignment service doesn't expose the trait). Cheap.
    let analytics_assignment_repo: Arc<
        dyn wealthfolio_spending::activity_assignments::ActivityTaxonomyAssignmentRepositoryTrait,
    > = Arc::new(
        wealthfolio_storage_sqlite::spending::activity_assignments::ActivityTaxonomyAssignmentRepository::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let spending_analytics_service =
        Arc::new(wealthfolio_spending::analytics::AnalyticsService::new(
            activity_repository.clone(),
            account_repository.clone(),
            analytics_assignment_repo.clone(),
            activity_splits_repo.clone(),
            spending_settings_service.clone(),
            taxonomy_service.clone(),
            events_service.clone(),
            fx_service.clone(),
            activity_events_repo.clone(),
        ));

    // Spending: reconciled period insight (powers the Spending Insight dashboard).
    let spending_insight_repo: Arc<dyn wealthfolio_spending::budget::BudgetRepositoryTrait> =
        Arc::new(
            wealthfolio_storage_sqlite::spending::budget::BudgetRepository::new(
                pool.clone(),
                writer.clone(),
            ),
        );
    let spending_insight_service = Arc::new(wealthfolio_spending::insight::InsightService::new(
        spending_insight_repo,
        activity_repository.clone(),
        account_repository.clone(),
        analytics_assignment_repo,
        activity_splits_repo,
        spending_settings_service.clone(),
        taxonomy_service.clone(),
        fx_service.clone(),
    ));

    let activity_service = Arc::new(
        ActivityService::new(
            activity_repository.clone(),
            account_service.clone(),
            asset_service.clone(),
            fx_service.clone(),
            quote_service.clone(),
        )
        .with_timezone(timezone.clone())
        .with_event_sink(domain_event_sink.clone()),
    );
    let final_cash_migration = run_final_cash_migration(
        settings_service.as_ref(),
        activity_repository.as_ref(),
        account_service.as_ref(),
        asset_service.as_ref(),
    )
    .await?;

    let income_service = Arc::new(IncomeService::new_with_timezone(
        fx_service.clone(),
        activity_repository.clone(),
        base_currency.clone(),
        timezone.clone(),
    ));

    let snapshot_service = Arc::new(
        SnapshotService::new(
            timezone.clone(),
            account_repository.clone(),
            snapshot_repository.clone(),
            activity_repository.clone(),
        )
        .with_event_sink(domain_event_sink.clone()),
    );

    let holdings_valuation_service = Arc::new(HoldingsValuationService::new_with_timezone(
        fx_service.clone(),
        quote_service.clone(),
        timezone.clone(),
    ));

    let projection_store: Arc<dyn ProjectionStoreTrait> = Arc::new(
        wealthfolio_storage_sqlite::portfolio::projection::ProjectionStore::new(
            pool.clone(),
            writer.clone(),
        ),
    );
    let fact_sources = FactSources {
        accounts: account_repository.clone(),
        activities: activity_repository.clone(),
        assets: asset_repository.clone(),
        quotes: quote_service.clone(),
        fx_rates: fx_repository.clone(),
        snapshots: snapshot_repository.clone(),
        projections: projection_store.clone(),
    };
    let valuation_service = Arc::new(ValuationService::new(
        valuation_repository.clone(),
        fact_sources.clone(),
        lots_repository.clone(),
        timezone.clone(),
    ));

    let portfolio_coordinator = Arc::new(PortfolioCoordinator::new(CoordinatorDeps {
        base_currency: base_currency.clone(),
        timezone: timezone.clone(),
        sources: fact_sources.clone(),
        fx_service: fx_service.clone(),
        snapshot_service: snapshot_service.clone(),
        projections: projection_store.clone(),
        lots: lots_repository.clone(),
        window_cadence: WindowCadence::Year,
    }));

    let final_cash_rebuild: Option<
        std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    > = if !final_cash_migration.pending_account_ids.is_empty() {
        // The coordinator serializes and forces a full rebuild per pending
        // account, so the rebuild can run in the background instead of
        // blocking (or failing) startup.
        log::info!(
            "Rebuilding {} account(s) after final-cash migration in the background",
            final_cash_migration.pending_account_ids.len()
        );
        let settings_service = settings_service.clone();
        let coordinator = portfolio_coordinator.clone();
        Some(Box::pin(async move {
            if let Err(error) =
                rebuild_pending_final_cash_accounts(settings_service.as_ref(), coordinator.as_ref())
                    .await
            {
                log::warn!("Background final-cash rebuild failed: {}", error);
            }
        }))
    } else {
        None
    };

    let performance_service = Arc::new(PerformanceService::new(
        base_currency.clone(),
        timezone.clone(),
        fact_sources.clone(),
        valuation_repository.clone(),
        lots_repository.clone(),
    ));

    let classification_service =
        Arc::new(AssetClassificationService::new(taxonomy_service.clone()));
    let holdings_service = Arc::new(
        HoldingsService::new_with_timezone(
            asset_service.clone(),
            snapshot_service.clone(),
            holdings_valuation_service.clone(),
            classification_service.clone(),
            timezone.clone(),
        )
        .with_income_dependencies(
            activity_repository.clone(),
            fx_service.clone(),
            projection_store,
        )
        .with_lot_repository(lots_repository.clone()),
    );

    let allocation_service = Arc::new(
        AllocationService::new(holdings_service.clone(), taxonomy_service.clone())
            .with_account_service(account_service.clone()),
    );

    let allocation_target_repository = Arc::new(AllocationTargetRepository::new(
        pool.clone(),
        writer.clone(),
    ));
    let allocation_target_service = Arc::new(AllocationTargetService::new(
        allocation_target_repository,
        taxonomy_service.clone(),
    ));
    let drift_service = Arc::new(
        DriftService::new(
            allocation_target_service.clone(),
            allocation_service.clone(),
        )
        .with_holdings_service(holdings_service.clone())
        .with_taxonomy_service(taxonomy_service.clone()),
    );
    let rebalance_service = Arc::new(RebalanceService::new(
        allocation_target_service.clone(),
        drift_service.clone(),
        allocation_service.clone(),
        holdings_service.clone(),
    ));

    let alternative_asset_repository = Arc::new(AlternativeAssetRepository::new(
        pool.clone(),
        writer.clone(),
    ));

    let net_worth_service = Arc::new(
        NetWorthService::new(
            base_currency.clone(),
            account_repository.clone(),
            asset_repository.clone(),
            snapshot_service.clone(),
            quote_service.clone(),
            valuation_repository.clone(),
            fx_service.clone(),
        )
        .with_loan_payments(alternative_asset_repository.clone(), timezone.clone()),
    );

    let alternative_asset_service = Arc::new(
        AlternativeAssetService::new(
            alternative_asset_repository.clone(),
            asset_repository.clone(),
            quote_service.clone(),
        )
        .with_timezone(timezone.clone())
        .with_event_sink(domain_event_sink.clone()),
    );

    let asset_logo_repository = Arc::new(AssetLogoRepository::new(pool.clone(), writer.clone()));
    let asset_logo_service = Arc::new(AssetLogoService::new(
        asset_logo_repository,
        asset_repository.clone(),
    ));

    // Health service for portfolio health diagnostics
    let health_dismissal_repository =
        Arc::new(HealthDismissalRepository::new(pool.clone(), writer.clone()));
    let health_service = Arc::new(
        HealthService::new(health_dismissal_repository)
            .with_projection_freshness(portfolio_coordinator.clone()),
    );

    // Unified business use cases: pending entries and ordinary activity writes.
    let business_store = Arc::new(wealthfolio_business::BusinessStore::new(
        pool.clone(),
        writer.clone(),
    ));
    let business_service = Arc::new(wealthfolio_business::BusinessService::new(
        activity_service.clone(),
        account_service.clone(),
        (*business_store).clone(),
        profile_id.to_string(),
    ));
    let pending_service = Arc::new(wealthfolio_business::PendingService::new(
        activity_service.clone(),
        account_service.clone(),
        (*business_store).clone(),
        profile_id.to_string(),
        spending_settings_service.clone(),
    ));

    let investment_service = Arc::new(wealthfolio_business::investments::InvestmentService::new(
        (*business_store).clone(),
        profile_id.to_string(),
        activity_service.clone(),
        account_service.clone(),
        snapshot_service.clone(),
        asset_service.clone(),
        fx_service.clone(),
        base_currency.clone(),
        timezone.clone(),
    ));

    // Service initialization follows successful profile migrations. Reuse the
    // same native services and profile settings as the existing bookkeeping path.
    let investment_resources = Arc::new(
        wealthfolio_business::investment_journal::AccountingResourceReader::new(
            (*business_store).clone(),
            fact_sources.clone(),
            snapshot_service.clone(),
            base_currency.clone(),
            timezone.clone(),
        ),
    );
    let investment_context = Arc::new(
        wealthfolio_business::investment_journal::AccountingContextReader::new(
            (*business_store).clone(),
            investment_resources.clone(),
            account_service.clone(),
            snapshot_service.clone(),
            asset_service.clone(),
            quote_service.clone(),
            fx_service.clone(),
            net_worth_service.clone(),
            base_currency.clone(),
            timezone.clone(),
        ),
    );
    let investment_journal = Arc::new(
        wealthfolio_business::investment_journal::InvestmentJournalService::new(
            (*business_store).clone(),
            profile_id.to_string(),
            investment_resources.clone(),
            timezone.clone(),
        )
        .with_bookkeeping(investment_service.clone())
        .with_context_reader(investment_context.clone())
        .with_review_facts_reader(investment_context),
    );
    let investment_projection = Arc::new(
        wealthfolio_business::investment_journal::InvestmentProjectionService::new(
            (*business_store).clone(),
            investment_resources,
        ),
    );

    // Expose local business services to external Agent tools.
    let agent_environment = Arc::new(
        TauriAgentEnvironment::new(
            base_currency.clone(),
            account_service.clone(),
            activity_service.clone(),
            holdings_service.clone(),
            valuation_service.clone(),
            settings_service.clone(),
            quote_service.clone(),
            asset_service.clone(),
            allocation_service.clone(),
            performance_service.clone(),
            income_service.clone(),
            health_service.clone(),
            taxonomy_service.clone(),
            portfolio_service.clone(),
            net_worth_service.clone(),
            cash_activity_service.clone(),
            categorization_rules_service.clone(),
            business_service.clone(),
            pending_service.clone(),
        )
        .with_investments(investment_service)
        .with_investment_journal(investment_journal),
    );
    let agent_environment: Arc<dyn wealthfolio_agent_tools::AgentEnvironment> =
        agent_environment.clone();

    // MCP audit log repository (agent access audit trail)
    let mcp_audit_repository = Arc::new(McpAuditRepository::new(pool.clone(), writer.clone()));

    // Personal Access Token repository (per-client scoped MCP auth)
    let pat_repository = Arc::new(PatRepository::new(pool.clone(), writer.clone()));

    Ok(BuiltContext {
        context: ServiceContext {
            investment_projection,
            business_service: business_service.clone(),
            pending_service: pending_service.clone(),
            portfolio_tasks: crate::listeners::PortfolioTasks::new(),

            active: std::sync::atomic::AtomicBool::new(true),
            profile_id,
            base_currency,
            timezone,
            domain_event_sink,
            settings_service,
            account_service,
            activity_service,
            asset_service,

            quote_service,

            fx_service,
            performance_service,
            income_service,
            snapshot_service,
            lots_repository,
            holdings_service,
            allocation_service,
            allocation_target_service,
            drift_service,
            rebalance_service,
            valuation_service,
            portfolio_coordinator,
            net_worth_service,

            alternative_asset_service,
            asset_logo_service,
            taxonomy_service,

            agent_environment,
            mcp_audit_repository,
            pat_repository,

            health_service,
            custom_provider_service,
            portfolio_service,
            spending_settings_service,
            cash_activity_service,
            categorization_rules_service,
            events_service,
            budget_service,
            spending_analytics_service,
            spending_insight_service,
        },
        event_receiver,
        final_cash_rebuild,
    })
}

#[cfg(test)]
mod initialization_tests {
    use super::*;

    #[tokio::test]
    async fn failed_service_build_releases_the_writer_pool() {
        let directory = tempfile::tempdir().unwrap();
        let access = db::DbAccess::new(directory.path().join("app.db").to_str().unwrap(), None);
        access.prepare().unwrap();
        access.run_migrations().unwrap();
        access
            .connect_rusqlite()
            .unwrap()
            .execute_batch("DROP TABLE app_settings")
            .unwrap();
        let pool = access.create_pool().unwrap();
        // The writer receives an r2d2 clone rather than this Arc. Check that
        // both repository ownership and every checked-out connection are gone.
        let probe = pool.as_ref().clone();
        let weak = Arc::downgrade(&pool);
        assert!(initialize_with_pool(
            pool,
            uuid::Uuid::nil(),
            crate::secret_store::shared_secret_store(crate::data_dir::PRODUCTION_APP_IDENTIFIER),
        )
        .await
        .is_err());
        assert!(weak.upgrade().is_none());
        let state = probe.state();
        assert_eq!(state.connections, state.idle_connections);
        access
            .connect_rusqlite()
            .unwrap()
            .execute_batch("BEGIN EXCLUSIVE; ROLLBACK;")
            .unwrap();
    }
}

#[cfg(test)]
#[path = "portfolio_agent_tests.rs"]
mod portfolio_agent_tests;
