// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_bundle;
mod commands;
mod context;
mod data_dir;
mod database;
mod domain_events;
mod events;
mod listeners;
mod mcp;
mod portfolio_jobs;
mod profile_lifecycle;
mod profile_startup;
mod profiles;
mod secret_store;

#[cfg(desktop)]
mod menu;

use dotenvy::dotenv;
use log::error;

use tauri::{AppHandle, Manager};

use events::emit_app_ready;

// ─────────────────────────────────────────────────────────────────────────────
// Desktop-only setup
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(desktop)]
mod desktop {
    use super::*;

    /// Sets up the application menu and its event handler.
    pub fn setup_menu(handle: &AppHandle, menu_bar_visible: bool) {
        if menu_bar_visible {
            match menu::create_menu(handle) {
                Ok(menu) => {
                    if let Err(e) = handle.set_menu(menu) {
                        error!("Failed to set menu: {}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to create menu: {}", e);
                }
            }
        } else if let Err(e) = handle.remove_menu() {
            error!("Failed to remove menu: {}", e);
        }

        handle.on_menu_event(move |app, event| {
            menu::handle_menu_event(app, event.id().as_ref());
        });
    }

    /// Opens the database asynchronously so the startup gate can render.
    pub fn setup(handle: AppHandle) {
        // A close only hides the window, and the window-state plugin may have
        // persisted `visible: false`. Launch always presents the window.
        show_main_window(&handle);
        // Embedded MCP server: clear any stale lock file from an unclean
        // shutdown before the runtime's workers may start it again.
        mcp::remove_stale_lock(&handle);
        // Let the window render while the database opens. Database-dependent
        // commands stay gated until initialization succeeds; DatabaseRuntime
        // owns the background workers and retains their handles.
        tauri::async_runtime::spawn(async move {
            let context = handle
                .state::<profile_startup::ProfileStartup>()
                .initialize(&handle)
                .await;
            let menu_bar_visible = context
                .as_ref()
                .ok()
                .and_then(|context| context.as_ref())
                .and_then(|context| context.settings_service().get_settings().ok())
                .map(|settings| settings.menu_bar_visible)
                .unwrap_or(true);
            if let Err(error) = &context {
                // Keep the window open so the startup gate can show the error.
                error!("Failed to open the database: {}", error);
            }
            let ready_handle = handle.clone();
            // Install the native menu and its handlers once, on the main thread,
            // after initialization has resolved the menu visibility setting.
            if let Err(error) = handle.run_on_main_thread(move || {
                setup_menu(&ready_handle, menu_bar_visible);
                // Notify the startup gate on failure too so it can show stored status.
                // On success, frontend startup hooks own the initial portfolio update.
                emit_app_ready(&ready_handle);
            }) {
                error!("Failed to finish desktop setup: {}", error);
                emit_app_ready(&handle);
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Show, un-minimize and focus the main window. Single-instance callbacks, the
/// Dock `Reopen` event and the menu's "show main window" item all use this so
/// they cannot drift apart.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Returns the app data directory path.
fn get_app_data_dir(handle: &AppHandle) -> Result<String, Box<dyn std::error::Error>> {
    Ok(handle.path().app_data_dir()?.to_string_lossy().into_owned())
}

// ─────────────────────────────────────────────────────────────────────────────
// Application entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn run() {
    dotenv().ok();

    let builder = tauri::Builder::default();

    // Single-instance must be the first plugin registered (per Tauri docs).
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        show_main_window(app);
    }));

    let builder = builder
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());

    builder
        .setup(|app| {
            let handle = app.handle().clone();

            // Embedded MCP server state (commands need it managed up front)
            handle.manage(mcp::McpServerState::default());
            agent_bundle::setup(&handle);

            // Registry failures are recoverable. Platform setup opens profiles
            // asynchronously while this state serves the startup gate.
            handle.manage(profile_startup::ProfileStartup::new(
                get_app_data_dir(&handle)?,
                handle.config().identifier.clone(),
            ));

            // Platform-specific plugin initialization

            // Setup event listeners (platform-agnostic)
            profiles::start_lock_monitor(handle.clone());
            profile_lifecycle::install(&handle);

            // Platform-specific setup
            #[cfg(desktop)]
            desktop::setup(handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_bundle::agent_bundle_status,
            agent_bundle::agent_bundle_action,
            profiles::get_profile_state,
            profile_startup::retry_profile_startup,
            profile_startup::start_new_profile_setup,
            profile_startup::open_profile_data_folder,
            profiles::create_profile,
            profiles::delete_profile,
            profiles::unlock_profile,
            profiles::lock_profile,
            profiles::profile_activity,
            profiles::update_profile,
            profiles::set_profile_password,
            profiles::recover_profile_password,
            // Account commands
            commands::account::get_accounts,
            commands::account::create_account,
            commands::account::update_account,
            commands::account::delete_account,
            // Activity commands
            commands::activity::search_activities,
            commands::activity::create_activity,
            commands::pending::list_pending_activities,
            commands::pending::create_pending_activity,
            commands::pending::update_pending_activity,
            commands::pending::confirm_pending_activity,
            commands::pending::reject_pending_activity,
            commands::ordinary::query_ordinary_activities,
            commands::ordinary::get_ordinary_activity,
            commands::ordinary::create_ordinary_activity,
            commands::ordinary::update_ordinary_activity,
            commands::activity::update_activity,
            commands::activity::save_activities,
            commands::activity::delete_activity,
            commands::activity::get_transfer_pair_for_activity,
            commands::activity::find_transfer_match_candidates,
            commands::activity::save_internal_transfer_pair,
            commands::activity::link_transfer_activities,
            commands::activity::unlink_transfer_activities,
            // Settings commands
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_latest_exchange_rates,
            commands::settings::get_exchange_rates_for_dates,
            commands::settings::update_exchange_rate,
            commands::settings::add_exchange_rate,
            commands::settings::delete_exchange_rate,
            // Spending commands
            commands::spending::get_spending_settings,
            commands::spending::update_spending_settings,
            commands::spending::list_cash_activities,
            commands::spending::search_cash_activities,
            commands::spending::set_activity_event,
            commands::spending::get_activity_assignments,
            commands::spending::assign_activity_category,
            commands::spending::unassign_activity_category,
            commands::spending::get_activity_splits,
            commands::spending::replace_activity_splits,
            commands::spending::clear_activity_splits,
            commands::spending::bulk_assign_categories,
            commands::spending::list_categorization_rules,
            commands::spending::create_categorization_rule,
            commands::spending::update_categorization_rule,
            commands::spending::delete_categorization_rule,
            commands::spending::rerun_categorization_rules,
            commands::spending::list_event_types,
            commands::spending::create_event_type,
            commands::spending::update_event_type,
            commands::spending::delete_event_type,
            commands::spending::list_events,
            commands::spending::create_event,
            commands::spending::update_event,
            commands::spending::delete_event,
            commands::spending::get_budget,
            commands::spending::upsert_budget_target,
            commands::spending::delete_budget_target,
            commands::spending::upsert_budget_rollover_setting,
            commands::spending::delete_budget_rollover_setting,
            commands::spending::create_budget_group,
            commands::spending::update_budget_group,
            commands::spending::delete_budget_group,
            commands::spending::assign_category_to_group,
            commands::spending::reset_budget_groups,
            commands::spending::copy_budget_targets,
            commands::spending::get_spending_report,
            commands::spending::get_spending_insight,
            commands::spending::get_event_spending_summaries,
            // Portfolios (saved reporting scopes)
            commands::portfolios::get_portfolios,
            commands::portfolios::get_portfolio,
            commands::portfolios::create_portfolio,
            commands::portfolios::update_portfolio_entry,
            commands::portfolios::delete_portfolio_entry,
            // Portfolio commands
            commands::portfolio::get_holdings,
            commands::portfolio::get_holdings_list,
            commands::portfolio::get_holding,
            commands::portfolio::get_asset_holdings,
            commands::portfolio::get_asset_lots,
            commands::portfolio::get_portfolio_allocations,
            commands::portfolio::get_holdings_by_allocation,
            commands::portfolio::get_income_summary,
            commands::portfolio::get_historical_valuations,
            commands::portfolio::get_latest_valuations,
            commands::portfolio::get_current_valuation,
            commands::portfolio::calculate_accounts_simple_performance,
            commands::portfolio::update_portfolio,
            commands::portfolio::recalculate_portfolio,
            commands::portfolio::calculate_performance_summary,
            commands::portfolio::calculate_performance_history,
            commands::portfolio::get_performance_summaries,
            commands::portfolio::save_manual_holdings,
            commands::portfolio::get_snapshots,
            commands::portfolio::get_snapshot_by_date,
            commands::portfolio::delete_snapshot,
            // Utility commands
            commands::utilities::save_text_file_with_dialog,
            commands::utilities::save_file_with_dialog,
            commands::utilities::export_data_file,
            commands::utilities::open_external_url,
            commands::utilities::get_app_info,
            commands::utilities::profile_transfer_file,
            commands::utilities::backup_database,
            commands::utilities::list_database_backups,
            commands::utilities::open_database_backup_folder,
            commands::utilities::delete_database_backup,
            commands::utilities::export_database_backup,
            commands::utilities::get_database_startup_status,
            commands::utilities::retry_database_startup,
            // Database encryption commands
            commands::database::get_database_encryption_status,
            commands::database::set_database_encryption_enabled,
            // Asset commands
            commands::asset::get_asset_profile,
            commands::asset::get_assets,
            commands::asset::update_asset_profile,
            commands::asset::update_quote_mode,
            commands::asset::delete_asset,
            commands::asset::create_asset,
            // Asset logo commands
            commands::asset_logo::get_asset_logo,
            commands::asset_logo::list_asset_logos,
            commands::asset_logo::upsert_asset_logo,
            commands::asset_logo::delete_asset_logo,
            // Alternative asset commands
            commands::alternative_assets::create_alternative_asset,
            commands::alternative_assets::update_alternative_asset_valuation,
            commands::alternative_assets::update_alternative_asset_metadata,
            commands::alternative_assets::delete_alternative_asset,
            commands::alternative_assets::link_liability,
            commands::alternative_assets::unlink_liability,
            commands::alternative_assets::get_net_worth,
            commands::alternative_assets::get_net_worth_history,
            commands::alternative_assets::get_alternative_holdings,
            commands::alternative_assets::calculate_loan,
            commands::alternative_assets::recalculate_loan,
            commands::alternative_assets::apply_loan_action,
            commands::alternative_assets::get_loan_payments,
            commands::alternative_assets::preview_loan_terms,
            commands::alternative_assets::link_loan_payment,
            // Market data commands
            commands::market_data::search_symbol,
            commands::market_data::resolve_symbol_quote,
            commands::market_data::synch_quotes,
            commands::market_data::sync_market_data,
            commands::market_data::reset_provider_history,
            commands::market_data::reset_all_provider_history,
            commands::market_data::update_quote,
            commands::market_data::delete_quote,
            commands::market_data::get_quote_history,
            commands::market_data::get_latest_quotes,
            commands::market_data::get_market_data_providers,
            commands::market_data::get_exchanges,
            commands::market_data::fetch_dividends,
            // Taxonomy commands
            commands::taxonomy::get_taxonomies,
            commands::taxonomy::get_taxonomy,
            commands::taxonomy::create_taxonomy,
            commands::taxonomy::update_taxonomy,
            commands::taxonomy::delete_taxonomy,
            commands::taxonomy::create_category,
            commands::taxonomy::update_category,
            commands::taxonomy::delete_category,
            commands::taxonomy::move_category,
            commands::taxonomy::export_taxonomy_json,
            commands::taxonomy::get_asset_taxonomy_assignments,
            commands::taxonomy::assign_asset_to_category,
            commands::taxonomy::replace_asset_taxonomy_assignments,
            commands::taxonomy::remove_asset_taxonomy_assignment,
            // Taxonomy migration commands
            commands::taxonomy::get_migration_status,
            commands::taxonomy::migrate_legacy_classifications,
            // Platform commands
            commands::platform::get_platform,
            commands::platform::is_mobile,
            commands::platform::is_desktop,
            // Secrets commands
            commands::secrets::set_secret,
            commands::secrets::get_secret,
            commands::secrets::delete_secret,
            // Provider settings commands
            commands::providers_settings::get_market_data_providers_settings,
            commands::providers_settings::update_market_data_provider_settings,
            // AI provider commands
            // AI chat commands
            // MCP server (Agent Access) commands
            commands::mcp::mcp_get_status,
            commands::mcp::mcp_set_enabled,
            commands::mcp::mcp_set_audit_enabled,
            commands::mcp::mcp_set_auto_start,
            commands::mcp::mcp_start,
            commands::mcp::mcp_stop,
            commands::mcp::mcp_list_audit_log,
            commands::mcp::mcp_purge_audit_log,
            commands::mcp::mcp_list_tokens,
            commands::mcp::mcp_create_token,
            commands::mcp::mcp_delete_token,
            // Custom provider commands
            commands::custom_provider::get_custom_providers,
            commands::custom_provider::create_custom_provider,
            commands::custom_provider::update_custom_provider,
            commands::custom_provider::delete_custom_provider,
            commands::custom_provider::test_custom_provider_source,
            // Health commands
            commands::health::get_health_status,
            commands::health::run_health_checks,
            commands::health::dismiss_health_issue,
            commands::health::restore_health_issue,
            commands::health::get_dismissed_health_issues,
            commands::health::execute_health_fix,
            commands::health::get_health_config,
            commands::health::update_health_config,
            // Allocation target commands
            commands::allocation_targets::list_allocation_targets,
            commands::allocation_targets::get_allocation_target,
            commands::allocation_targets::create_allocation_target,
            commands::allocation_targets::update_allocation_target,
            commands::allocation_targets::archive_allocation_target,
            commands::allocation_targets::delete_allocation_target,
            commands::allocation_targets::list_allocation_target_weights,
            commands::allocation_targets::save_allocation_target_weights,
            commands::allocation_targets::save_allocation_target_with_weights,
            commands::allocation_targets::list_target_constraints,
            commands::allocation_targets::save_target_constraints,
            commands::allocation_targets::get_allocation_target_drift,
            commands::allocation_targets::calculate_rebalance_plan,
        ])
        .on_window_event(|window, event| {
            // Closing the window only hides it: a locked or running profile
            // keeps its lifecycle, and the Dock/menu can show it again.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        // Failure to construct the application is terminal; no command runtime exists yet.
        .expect("Failed to build Wealthfolio application")
        .run(|_handle, event| {
            #[cfg(desktop)]
            match event {
                // Dock icon click / `open -a` with no visible window.
                tauri::RunEvent::Reopen { .. } => show_main_window(_handle),
                // Release the database and stop the MCP server on exit.
                tauri::RunEvent::Exit => profiles::release_for_exit(_handle),
                _ => {}
            }
        });
}
