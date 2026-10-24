use std::{path::Path as StdPath, sync::Arc};

use crate::{
    api::shared::{process_portfolio_job, PortfolioJobConfig},
    error::ApiResult,
    main_lib::AppState,
};
use axum::{routing::get, Json, Router};
use wealthfolio_core::{
    quotes::MarketSyncMode,
    settings::{Settings, SettingsServiceTrait, SettingsUpdate},
};

async fn get_settings(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
) -> ApiResult<Json<Settings>> {
    let s = state.settings_service.get_settings()?;
    Ok(Json(s))
}

async fn update_settings(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(payload): Json<SettingsUpdate>,
) -> ApiResult<Json<Settings>> {
    let previous_base_currency = state
        .base_currency
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let previous_timezone = state
        .timezone
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    state.settings_service.update_settings(&payload).await?;
    let updated_settings = state.settings_service.get_settings()?;
    *state
        .timezone
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = updated_settings.timezone.clone();
    state.health_service.clear_cache().await;

    let base_currency_changed = updated_settings.base_currency != previous_base_currency;
    let timezone_changed = updated_settings.timezone != previous_timezone;

    if base_currency_changed {
        *state
            .base_currency
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            updated_settings.base_currency.clone();

        let state_for_job = state.clone();
        tokio::spawn(async move {
            // Base currency change needs BackfillHistory mode to ensure FX pairs
            // have sufficient quote coverage from earliest activity date (US-006)
            let job_config = PortfolioJobConfig {
                account_ids: None,
                market_sync_mode: MarketSyncMode::BackfillHistory {
                    asset_ids: None,
                    days: wealthfolio_core::quotes::DEFAULT_HISTORY_DAYS,
                },
                ..PortfolioJobConfig::default()
            };

            if let Err(err) = process_portfolio_job(state_for_job, job_config).await {
                tracing::warn!("Base currency change recalculation failed: {}", err);
            }
        });
    } else if timezone_changed {
        let state_for_job = state.clone();
        tokio::spawn(async move {
            let job_config = PortfolioJobConfig {
                account_ids: None,
                market_sync_mode: MarketSyncMode::None,
                force_full: false,
            };

            if let Err(err) = process_portfolio_job(state_for_job, job_config).await {
                tracing::warn!("Timezone change recalculation failed: {}", err);
            }
        });
    }

    Ok(Json(updated_settings))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfoResponse {
    version: String,
    db_path: String,
    logs_dir: String,
}

async fn get_app_info(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
) -> ApiResult<Json<AppInfoResponse>> {
    let version = env!("CARGO_PKG_VERSION").to_string();

    let db_path = state.db_path.clone();

    // For web mode, logs typically go to the same directory or a subdirectory
    // In production, this would typically be configured via environment variables
    let logs_dir = std::env::var("WF_LOGS_DIR").unwrap_or_else(|_| {
        StdPath::new(&state.data_root)
            .join("logs")
            .to_str()
            .unwrap_or("")
            .to_string()
    });

    Ok(Json(AppInfoResponse {
        version,
        db_path,
        logs_dir,
    }))
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/settings", get(get_settings).put(update_settings))
        .route("/app/info", get(get_app_info))
}
