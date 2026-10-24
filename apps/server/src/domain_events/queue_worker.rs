//! Event queue worker for processing domain events.
//!
//! Receives events from an mpsc channel, debounces them with a 500ms window,
//! then processes the batch to trigger portfolio recalculation, asset enrichment,
//! and broker sync.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use wealthfolio_core::{assets::AssetServiceTrait, events::DomainEvent};

use super::planner::{
    plan_asset_classification_change, plan_asset_enrichment, plan_categorization_job,
    plan_portfolio_job,
};
use crate::events::EventBus;

/// Debounce window for collecting events before processing.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(1000);
/// A batch is processed at most this long after its first event.
const MAX_BATCH_WAIT: Duration = Duration::from_millis(5000);

/// Dependencies needed by the queue worker for processing events.
pub struct QueueWorkerDeps {
    pub asset_service: Arc<dyn AssetServiceTrait + Send + Sync>,

    pub event_bus: EventBus,

    pub health_service: Arc<dyn wealthfolio_core::health::HealthServiceTrait + Send + Sync>,
    pub portfolio_coordinator: Arc<wealthfolio_core::portfolio::coordinator::PortfolioCoordinator>,
    pub fx_service: Arc<dyn wealthfolio_core::fx::FxServiceTrait + Send + Sync>,
    /// Spending settings — used to filter ActivitiesChanged events to opted-in accounts.
    pub spending_settings_service: Arc<wealthfolio_spending::settings::SpendingSettingsService>,
    /// Categorization rules service — auto-runs rules against newly-changed activities.
    pub categorization_rules_service:
        Arc<wealthfolio_spending::categorization_rules::CategorizationRulesService>,
}

/// Runs the event queue worker.
///
/// Receives events from the channel, debounces with a 500ms window,
/// and processes batches to trigger appropriate actions.
///
/// Uses an `is_processing` guard to prevent new batches from being processed
/// while a previous batch (e.g., broker sync or portfolio recalc) is still running.
pub async fn event_queue_worker(
    mut rx: mpsc::UnboundedReceiver<DomainEvent>,
    deps: Arc<QueueWorkerDeps>,
) {
    tracing::info!("Domain event queue worker started");

    let mut pending_events: Vec<DomainEvent> = Vec::new();
    let mut batch_started: Option<std::time::Instant> = None;
    let is_processing = Arc::new(AtomicBool::new(false));

    loop {
        // Wait for the first event, then debounce; a sustained event stream
        // never postpones the batch beyond MAX_BATCH_WAIT (architecture §3.3).
        let maybe_event = if pending_events.is_empty() {
            rx.recv().await
        } else {
            let wait = batch_started
                .map(|started| MAX_BATCH_WAIT.saturating_sub(started.elapsed()))
                .unwrap_or(DEBOUNCE_DURATION)
                .min(DEBOUNCE_DURATION);
            tokio::select! {
                event = rx.recv() => event,
                _ = tokio::time::sleep(wait) => None,
            }
        };

        match maybe_event {
            Some(event) => {
                pending_events.push(event);
                let started = *batch_started.get_or_insert_with(std::time::Instant::now);
                if started.elapsed() < MAX_BATCH_WAIT {
                    continue;
                }
            }
            None if pending_events.is_empty() => {
                while is_processing.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                tracing::info!("Domain event queue worker shutting down");
                return;
            }
            None => {}
        }

        if is_processing.load(Ordering::SeqCst) {
            tracing::debug!("Debounce expired but previous batch still processing, continuing to collect events");
            continue;
        }
        let batch = std::mem::take(&mut pending_events);
        batch_started = None;
        is_processing.store(true, Ordering::SeqCst);
        process_event_batch(&batch, deps.clone()).await;
        is_processing.store(false, Ordering::SeqCst);
    }
}

/// Processes a batch of domain events.
async fn process_event_batch(events: &[DomainEvent], deps: Arc<QueueWorkerDeps>) {
    tracing::info!("Processing batch of {} domain event(s)", events.len());

    if let Some(plan) = plan_asset_classification_change(events) {
        deps.event_bus
            .publish(crate::events::ServerEvent::with_payload(
                crate::events::ASSET_CLASSIFICATIONS_CHANGED,
                serde_json::json!({
                    "assetIds": plan.asset_ids,
                    "taxonomyIds": plan.taxonomy_ids,
                }),
            ));
    }

    // 1. Plan and run asset enrichment FIRST so that bond metadata (coupon rate,
    //    maturity date, etc.) is available before the portfolio job tries to
    //    sync quotes and calculate snapshots.
    let enrichment_assets = plan_asset_enrichment(events);
    if !enrichment_assets.is_empty() {
        tracing::info!(
            "Triggering asset enrichment for {} asset(s)",
            enrichment_assets.len()
        );

        let total = enrichment_assets.len();
        deps.event_bus
            .publish(crate::events::ServerEvent::with_payload(
                crate::events::ASSET_ENRICHMENT_START,
                serde_json::json!({ "total": total }),
            ));

        let mut total_enriched: usize = 0;
        let mut total_skipped: usize = 0;
        let mut total_failed: usize = 0;

        let chunk_size = 5;

        for chunk in enrichment_assets.chunks(chunk_size) {
            match tokio::time::timeout(
                Duration::from_secs(30),
                deps.asset_service.enrich_assets(chunk.to_vec()),
            )
            .await
            {
                Ok(Ok((enriched, skipped, failed))) => {
                    total_enriched += enriched;
                    total_skipped += skipped;
                    total_failed += failed;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Asset enrichment chunk failed: {}", e);
                    total_failed += chunk.len();
                }
                Err(_) => {
                    tracing::warn!(
                        "Asset enrichment chunk timed out ({} asset(s))",
                        chunk.len()
                    );
                    total_failed += chunk.len();
                }
            }

            let completed = total_enriched + total_skipped + total_failed;
            deps.event_bus
                .publish(crate::events::ServerEvent::with_payload(
                    crate::events::ASSET_ENRICHMENT_PROGRESS,
                    serde_json::json!({
                        "completed": completed,
                        "total": total,
                    }),
                ));
        }

        deps.event_bus
            .publish(crate::events::ServerEvent::with_payload(
                crate::events::ASSET_ENRICHMENT_COMPLETE,
                serde_json::json!({
                    "enriched": total_enriched,
                    "skipped": total_skipped,
                    "failed": total_failed,
                }),
            ));
    }

    // 2. Plan and trigger portfolio job
    if let Some(config) = plan_portfolio_job(events) {
        let prices_changed = events
            .iter()
            .any(|event| matches!(event, DomainEvent::PriceHistoryChanged));
        if prices_changed {
            deps.health_service.clear_cache().await;
            // Saved prices changed: reload the in-memory FX converter so every
            // later read sees the new rates, as the job's facts will.
            if let Err(error) = deps.fx_service.initialize() {
                deps.event_bus
                    .publish(crate::events::ServerEvent::with_payload(
                        crate::events::PORTFOLIO_UPDATE_ERROR,
                        serde_json::json!(error.to_string()),
                    ));
                return;
            }
        }
        tracing::info!(
            "Triggering portfolio job for accounts: {:?}, market_sync: {:?}",
            config.account_ids,
            config.market_sync_mode
        );

        // Run the portfolio job directly (not spawned) so that is_processing
        // guard properly tracks completion and prevents concurrent jobs
        run_portfolio_job(deps.clone(), config).await;
    }

    // 2b. Auto-categorize newly-changed activities on opted-in spending accounts.
    spawn_auto_categorize_for_batch(events, deps.clone()).await;
}

async fn spawn_auto_categorize_for_batch(events: &[DomainEvent], deps: Arc<QueueWorkerDeps>) {
    let settings = match deps.spending_settings_service.get().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                "Skipping auto-categorization: failed to load spending settings: {}",
                e
            );
            return;
        }
    };
    if !settings.enabled || settings.account_ids.is_empty() {
        return;
    }
    let opted_in: std::collections::HashSet<String> =
        settings.account_ids.iter().cloned().collect();
    let account_ids = plan_categorization_job(events, &opted_in);
    if account_ids.is_empty() {
        return;
    }
    tracing::info!(
        "Triggering auto-categorization for {} account(s)",
        account_ids.len()
    );
    let rules_service = deps.categorization_rules_service.clone();
    tokio::spawn(async move {
        match rules_service
            .rerun_all(&account_ids, /* only_uncategorized */ true)
            .await
        {
            Ok(count) if count > 0 => {
                tracing::info!("Auto-categorization wrote {} assignment(s)", count);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("Auto-categorization failed: {}", e),
        }
    });
}

async fn run_portfolio_job(
    deps: Arc<QueueWorkerDeps>,
    config: crate::api::shared::PortfolioJobConfig,
) {
    let observer = crate::api::shared::ServerJobObserver::with_parts(
        deps.event_bus.clone(),
        deps.health_service.clone(),
    );
    if let Err(err) = deps
        .portfolio_coordinator
        .run_job_with_retry(
            config.into_request(),
            &observer,
            wealthfolio_core::portfolio::coordinator::RetryPolicy::default(),
        )
        .await
    {
        tracing::error!("Portfolio job failed: {}", err);
    }
}
