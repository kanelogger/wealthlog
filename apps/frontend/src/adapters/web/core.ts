import { profileFetch } from "@/features/profiles/session";
// Web adapter core - Internal invoke function, COMMANDS map, and helpers
// This module exports invoke, logger, and platform constants for shared modules

import { notifyUnauthorized } from "@/lib/auth-token";
import type { Logger } from "../types";

/** True in the desktop Tauri runtime. */
export const isTauri = false;

/** True when running in the web environment */
export const isWeb = true;

export const API_PREFIX = "/api/v1";
export const EVENTS_ENDPOINT = `${API_PREFIX}/events/stream`;

type CommandMap = Record<string, { method: string; path: string }>;

export const COMMANDS: CommandMap = {
  get_accounts: { method: "GET", path: "/accounts" },
  create_account: { method: "POST", path: "/accounts" },
  update_account: { method: "PUT", path: "/accounts" },
  delete_account: { method: "DELETE", path: "/accounts" },
  get_portfolios: { method: "GET", path: "/portfolios" },
  create_portfolio: { method: "POST", path: "/portfolios" },
  update_portfolio_entry: { method: "PUT", path: "/portfolios" },
  delete_portfolio_entry: { method: "DELETE", path: "/portfolios" },
  get_settings: { method: "GET", path: "/settings" },
  update_settings: { method: "PUT", path: "/settings" },
  get_app_info: { method: "GET", path: "/app/info" },
  get_database_encryption_status: {
    method: "GET",
    path: "/utilities/database/encryption",
  },
  backup_database: { method: "POST", path: "/utilities/database/backup" },
  list_database_backups: { method: "GET", path: "/utilities/database/backups" },
  delete_database_backup: { method: "DELETE", path: "/utilities/database/backups" },
  get_holdings: { method: "POST", path: "/holdings/query" },
  get_holdings_list: { method: "POST", path: "/holdings/list/query" },
  get_holding: { method: "GET", path: "/holdings/item" },
  get_asset_holdings: { method: "GET", path: "/holdings/by-asset" },
  get_asset_lots: { method: "GET", path: "/holdings/lots" },
  get_historical_valuations: { method: "GET", path: "/valuations/history" },
  get_latest_valuations: { method: "GET", path: "/valuations/latest" },
  get_current_valuation: { method: "POST", path: "/valuations/current/query" },
  get_portfolio_allocations: { method: "POST", path: "/allocations/query" },
  get_holdings_by_allocation: { method: "POST", path: "/allocations/holdings/query" },
  // Snapshot management
  get_snapshots: { method: "GET", path: "/snapshots" },
  get_snapshot_by_date: { method: "GET", path: "/snapshots/holdings" },
  delete_snapshot: { method: "DELETE", path: "/snapshots" },
  save_manual_holdings: { method: "POST", path: "/snapshots" },
  update_portfolio: { method: "POST", path: "/portfolio/update" },
  recalculate_portfolio: { method: "POST", path: "/portfolio/recalculate" },
  // Performance
  calculate_accounts_simple_performance: { method: "POST", path: "/performance/accounts/simple" },
  calculate_performance_history: { method: "POST", path: "/performance/history" },
  calculate_performance_summary: { method: "POST", path: "/performance/summary" },
  get_performance_summaries: { method: "POST", path: "/performance/summaries" },
  get_income_summary: { method: "POST", path: "/income/summary/query" },
  // FX
  get_latest_exchange_rates: { method: "GET", path: "/exchange-rates/latest" },
  update_exchange_rate: { method: "PUT", path: "/exchange-rates" },
  add_exchange_rate: { method: "POST", path: "/exchange-rates" },
  delete_exchange_rate: { method: "DELETE", path: "/exchange-rates" },
  get_exchange_rates_for_dates: { method: "POST", path: "/exchange-rates/historical" },
  // Activities
  search_activities: { method: "POST", path: "/activities/search" },
  create_activity: { method: "POST", path: "/activities" },
  update_activity: { method: "PUT", path: "/activities" },
  save_activities: { method: "POST", path: "/activities/bulk" },
  delete_activity: { method: "DELETE", path: "/activities" },
  get_transfer_pair_for_activity: { method: "GET", path: "/activities" },
  find_transfer_match_candidates: { method: "POST", path: "/activities/transfer-match-candidates" },
  save_internal_transfer_pair: { method: "POST", path: "/activities/transfer-pair" },
  link_transfer_activities: { method: "POST", path: "/activities/link" },
  unlink_transfer_activities: { method: "POST", path: "/activities/unlink" },
  // Unified business interface
  query_ordinary_activities: { method: "POST", path: "/ordinary-activities/query" },
  create_ordinary_activity: { method: "POST", path: "/ordinary-activities" },
  update_ordinary_activity: { method: "PUT", path: "/ordinary-activities" },
  // Pending entries (待确认收支)
  list_pending_activities: { method: "GET", path: "/pending-activities" },
  create_pending_activity: { method: "POST", path: "/pending-activities" },
  update_pending_activity: { method: "PUT", path: "/pending-activities" },
  confirm_pending_activity: { method: "POST", path: "/pending-activities" },
  reject_pending_activity: { method: "POST", path: "/pending-activities" },
  // Market data providers
  get_exchanges: { method: "GET", path: "/exchanges" },
  get_market_data_providers: { method: "GET", path: "/providers" },
  get_market_data_providers_settings: { method: "GET", path: "/providers/settings" },
  update_market_data_provider_settings: { method: "PUT", path: "/providers/settings" },
  // Custom providers
  get_custom_providers: { method: "GET", path: "/custom-providers" },
  create_custom_provider: { method: "POST", path: "/custom-providers" },
  update_custom_provider: { method: "PUT", path: "/custom-providers" },
  delete_custom_provider: { method: "DELETE", path: "/custom-providers" },
  test_custom_provider_source: { method: "POST", path: "/custom-providers/test-source" },
  // Asset profile
  get_assets: { method: "GET", path: "/assets" },
  create_asset: { method: "POST", path: "/assets" },
  delete_asset: { method: "DELETE", path: "/assets" },
  get_asset_profile: { method: "GET", path: "/assets/profile" },
  update_asset_profile: { method: "PUT", path: "/assets/profile" },
  update_quote_mode: { method: "PUT", path: "/assets/pricing-mode" },
  // Asset logos
  list_asset_logos: { method: "GET", path: "/assets/logos" },
  get_asset_logo: { method: "GET", path: "/assets/logo" },
  upsert_asset_logo: { method: "PUT", path: "/assets/logo" },
  delete_asset_logo: { method: "DELETE", path: "/assets/logo" },
  // Market data
  search_symbol: { method: "GET", path: "/market-data/search" },
  resolve_symbol_quote: { method: "GET", path: "/market-data/resolve-currency" },
  get_quote_history: { method: "GET", path: "/market-data/quotes/history" },
  fetch_dividends: { method: "GET", path: "/market-data/dividends" },
  get_latest_quotes: { method: "POST", path: "/market-data/quotes/latest" },
  update_quote: { method: "PUT", path: "/market-data/quotes" },
  delete_quote: { method: "DELETE", path: "/market-data/quotes/id" },
  synch_quotes: { method: "POST", path: "/market-data/sync/history" },
  reset_all_provider_history: {
    method: "POST",
    path: "/market-data/quotes/reset",
  },
  reset_provider_history: { method: "POST", path: "/market-data/quotes" },
  sync_market_data: { method: "POST", path: "/market-data/sync" },
  // Secrets
  set_secret: { method: "POST", path: "/secrets" },
  get_secret: { method: "GET", path: "/secrets" },
  delete_secret: { method: "DELETE", path: "/secrets" },
  // Taxonomies
  get_taxonomies: { method: "GET", path: "/taxonomies" },
  get_taxonomy: { method: "GET", path: "/taxonomies" },
  create_taxonomy: { method: "POST", path: "/taxonomies" },
  update_taxonomy: { method: "PUT", path: "/taxonomies" },
  delete_taxonomy: { method: "DELETE", path: "/taxonomies" },
  create_category: { method: "POST", path: "/taxonomies/categories" },
  update_category: { method: "PUT", path: "/taxonomies/categories" },
  delete_category: { method: "DELETE", path: "/taxonomies" },
  move_category: { method: "POST", path: "/taxonomies/categories/move" },
  export_taxonomy_json: { method: "GET", path: "/taxonomies" },
  get_asset_taxonomy_assignments: { method: "GET", path: "/taxonomies/assignments/asset" },
  assign_asset_to_category: { method: "POST", path: "/taxonomies/assignments" },
  replace_asset_taxonomy_assignments: { method: "PUT", path: "/taxonomies/assignments/asset" },
  remove_asset_taxonomy_assignment: { method: "DELETE", path: "/taxonomies/assignments" },
  get_migration_status: { method: "GET", path: "/taxonomies/migration/status" },
  migrate_legacy_classifications: { method: "POST", path: "/taxonomies/migration/run" },
  // Spending budget
  get_budget: { method: "GET", path: "/spending/budget" },
  upsert_budget_target: { method: "POST", path: "/spending/budget/targets" },
  delete_budget_target: { method: "DELETE", path: "/spending/budget/targets" },
  upsert_budget_rollover_setting: { method: "POST", path: "/spending/budget/rollovers" },
  delete_budget_rollover_setting: { method: "DELETE", path: "/spending/budget/rollovers" },
  create_budget_group: { method: "POST", path: "/spending/budget/groups" },
  update_budget_group: { method: "PUT", path: "/spending/budget/groups" },
  delete_budget_group: { method: "DELETE", path: "/spending/budget/groups" },
  assign_category_to_group: { method: "POST", path: "/spending/budget/group-assignments" },
  reset_budget_groups: { method: "POST", path: "/spending/budget/groups/reset" },
  copy_budget_targets: { method: "POST", path: "/spending/budget/copy" },
  // Spending settings
  get_spending_settings: { method: "GET", path: "/spending/settings" },
  update_spending_settings: { method: "PUT", path: "/spending/settings" },
  // Spending cash activities + assignments
  list_cash_activities: { method: "GET", path: "/spending/cash-activities" },
  search_cash_activities: { method: "POST", path: "/spending/cash-activities/search" },
  set_activity_event: { method: "PUT", path: "/spending/cash-activities" },
  get_activity_assignments: { method: "GET", path: "/spending/activities" },
  assign_activity_category: { method: "PUT", path: "/spending/activities" },
  unassign_activity_category: { method: "DELETE", path: "/spending/activities" },
  get_activity_splits: { method: "GET", path: "/spending/activities" },
  replace_activity_splits: { method: "PUT", path: "/spending/activities" },
  clear_activity_splits: { method: "DELETE", path: "/spending/activities" },
  bulk_assign_categories: { method: "POST", path: "/spending/assignments/bulk" },
  // Spending categorization rules
  list_categorization_rules: { method: "GET", path: "/spending/rules" },
  create_categorization_rule: { method: "POST", path: "/spending/rules" },
  update_categorization_rule: { method: "PUT", path: "/spending/rules" },
  delete_categorization_rule: { method: "DELETE", path: "/spending/rules" },
  rerun_categorization_rules: { method: "POST", path: "/spending/rules/rerun" },
  // Spending events + event types
  list_event_types: { method: "GET", path: "/spending/event-types" },
  create_event_type: { method: "POST", path: "/spending/event-types" },
  update_event_type: { method: "PUT", path: "/spending/event-types" },
  delete_event_type: { method: "DELETE", path: "/spending/event-types" },
  list_events: { method: "GET", path: "/spending/events" },
  create_event: { method: "POST", path: "/spending/events" },
  update_event: { method: "PUT", path: "/spending/events" },
  delete_event: { method: "DELETE", path: "/spending/events" },
  get_event_spending_summaries: { method: "POST", path: "/spending/event-spending-summaries" },
  // Spending analytics
  get_spending_report: { method: "POST", path: "/spending/report" },
  get_spending_insight: { method: "POST", path: "/spending/insight" },
  // Health Center
  get_health_status: { method: "GET", path: "/health/status" },
  run_health_checks: { method: "POST", path: "/health/check" },
  dismiss_health_issue: { method: "POST", path: "/health/dismiss" },
  restore_health_issue: { method: "POST", path: "/health/restore" },
  get_dismissed_health_issues: { method: "GET", path: "/health/dismissed" },
  execute_health_fix: { method: "POST", path: "/health/fix" },
  get_health_config: { method: "GET", path: "/health/config" },
  update_health_config: { method: "PUT", path: "/health/config" },
  // Net Worth
  get_net_worth: { method: "GET", path: "/net-worth" },
  get_net_worth_history: { method: "GET", path: "/net-worth/history" },
  // Allocation Targets
  list_allocation_targets: { method: "GET", path: "/allocation-targets" },
  get_allocation_target: { method: "GET", path: "/allocation-targets" },
  create_allocation_target: { method: "POST", path: "/allocation-targets" },
  update_allocation_target: { method: "PUT", path: "/allocation-targets" },
  archive_allocation_target: { method: "POST", path: "/allocation-targets" },
  delete_allocation_target: { method: "DELETE", path: "/allocation-targets" },
  list_allocation_target_weights: { method: "GET", path: "/allocation-targets" },
  save_allocation_target_weights: { method: "POST", path: "/allocation-targets" },
  save_allocation_target_with_weights: {
    method: "POST",
    path: "/allocation-targets/save-with-weights",
  },
  get_allocation_target_drift: { method: "POST", path: "/allocation-targets" },
  list_target_constraints: { method: "GET", path: "/allocation-targets" },
  save_target_constraints: { method: "POST", path: "/allocation-targets" },
  calculate_rebalance_plan: { method: "POST", path: "/allocation-targets/rebalance/calculate" },
  // Alternative Assets
  create_alternative_asset: { method: "POST", path: "/alternative-assets" },
  update_alternative_asset_valuation: { method: "PUT", path: "/alternative-assets" },
  delete_alternative_asset: { method: "DELETE", path: "/alternative-assets" },
  link_liability: { method: "POST", path: "/alternative-assets" },
  unlink_liability: { method: "DELETE", path: "/alternative-assets" },
  update_alternative_asset_metadata: { method: "PUT", path: "/alternative-assets" },
  calculate_loan: { method: "POST", path: "/loans/calculate" },
  recalculate_loan: { method: "POST", path: "/loans/recalculate" },
  preview_loan_terms: { method: "POST", path: "/loans/preview" },
  apply_loan_action: { method: "POST", path: "/loans" },
  get_loan_payments: { method: "GET", path: "/loans" },
  link_loan_payment: { method: "POST", path: "/loans/payments" },
  get_alternative_holdings: { method: "GET", path: "/alternative-holdings" },
  // Agent Access (PATs + audit log)
  get_agent_access_status: { method: "GET", path: "/agent-access/status" },
  list_agent_access_tokens: { method: "GET", path: "/agent-access/tokens" },
  create_agent_access_token: { method: "POST", path: "/agent-access/tokens" },
  delete_agent_access_token: { method: "DELETE", path: "/agent-access/tokens" },
  list_agent_audit_log: { method: "GET", path: "/agent-access/audit" },
  purge_agent_audit_log: { method: "POST", path: "/agent-access/audit/purge" },
};

/**
 * Logger implementation using console
 */
export const logger: Logger = {
  error: (...args: unknown[]) => console.error(...args),
  warn: (...args: unknown[]) => console.warn(...args),
  info: (...args: unknown[]) => console.warn(...args),
  debug: (...args: unknown[]) => console.warn(...args),
  trace: (...args: unknown[]) => console.warn(...args),
};

/**
 * Invoke a command via REST API (internal - use typed adapter functions instead)
 */
export const invoke = async <T>(command: string, payload?: Record<string, unknown>): Promise<T> => {
  const config = COMMANDS[command];
  if (!config) throw new Error(`Unsupported command ${command}`);
  let url = `${API_PREFIX}${config.path}`;
  let method = config.method;
  let body: BodyInit | undefined;

  const addPeriodKey = (periodKey?: string) => {
    if (!periodKey) return;
    const params = new URLSearchParams();
    params.set("periodKey", periodKey);
    url += `?${params.toString()}`;
  };

  switch (command) {
    case "update_account": {
      const data = payload as { accountUpdate: { id: string } & Record<string, unknown> };
      url += `/${data.accountUpdate.id}`;
      body = JSON.stringify(data.accountUpdate);
      break;
    }
    case "delete_account": {
      const data = payload as { accountId: string };
      url += `/${data.accountId}`;
      break;
    }
    case "create_account": {
      const data = payload as { account: Record<string, unknown> };
      body = JSON.stringify(data.account);
      break;
    }
    case "create_portfolio": {
      const { portfolio } = payload as { portfolio: Record<string, unknown> };
      body = JSON.stringify(portfolio);
      break;
    }
    case "update_portfolio_entry": {
      const { portfolio } = payload as { portfolio: { id: string } & Record<string, unknown> };
      url += `/${encodeURIComponent(portfolio.id)}`;
      body = JSON.stringify(portfolio);
      break;
    }
    case "delete_portfolio_entry": {
      const { portfolioId } = payload as { portfolioId: string };
      url += `/${encodeURIComponent(portfolioId)}`;
      break;
    }
    case "delete_database_backup": {
      const { filename } = payload as { filename: string };
      url += `/${encodeURIComponent(filename)}`;
      break;
    }
    case "update_settings": {
      const data = payload as { settingsUpdate: Record<string, unknown> };
      body = JSON.stringify(data.settingsUpdate);
      break;
    }
    case "get_holdings":
    case "get_holdings_list": {
      const p = payload as {
        filter: { type: string; accountId?: string };
        includeClosed?: boolean;
      };
      if (p.filter?.type === "account" && p.filter.accountId) {
        const path = command === "get_holdings_list" ? "/holdings/list" : "/holdings";
        const params = new URLSearchParams({ accountId: p.filter.accountId });
        if (p.includeClosed) params.set("includeClosed", "true");
        url = `${API_PREFIX}${path}?${params.toString()}`;
        method = "GET";
      } else {
        body = JSON.stringify({
          filter: p.filter,
          ...(p.includeClosed ? { includeClosed: true } : {}),
        });
      }
      break;
    }
    case "get_holding": {
      const { accountId, assetId } = payload as { accountId: string; assetId: string };
      const params = new URLSearchParams();
      params.set("accountId", accountId);
      params.set("assetId", assetId);
      url += `?${params.toString()}`;
      break;
    }
    case "get_asset_holdings": {
      const p = payload as { assetId: string };
      url += `?assetId=${encodeURIComponent(p.assetId)}`;
      break;
    }
    case "get_asset_lots": {
      const p = payload as { assetId: string; includeSnapshotPositions?: boolean };
      const params = new URLSearchParams();
      params.set("assetId", p.assetId);
      if (p.includeSnapshotPositions !== undefined) {
        params.set("includeSnapshotPositions", String(p.includeSnapshotPositions));
      }
      url += `?${params.toString()}`;
      break;
    }
    case "get_historical_valuations": {
      const p = payload as {
        accountId?: string;
        filter?: { type: string; accountId?: string; portfolioId?: string; accountIds?: string[] };
        startDate?: string;
        endDate?: string;
      };
      if (p?.filter) {
        url = `${API_PREFIX}/valuations/history/query`;
        method = "POST";
        body = JSON.stringify({
          filter: p.filter,
          startDate: p.startDate,
          endDate: p.endDate,
        });
        break;
      }
      const params = new URLSearchParams();
      if (p?.accountId) params.set("accountId", p.accountId);
      if (p?.startDate) params.set("startDate", p.startDate);
      if (p?.endDate) params.set("endDate", p.endDate);
      const qs = params.toString();
      if (qs) url += `?${qs}`;
      break;
    }
    case "get_latest_valuations": {
      const p = payload as { accountIds?: string[] };
      const params = new URLSearchParams();
      if (Array.isArray(p?.accountIds)) {
        for (const id of p.accountIds) params.append("accountIds[]", id);
      }
      const qs = params.toString();
      if (qs) url += `?${qs}`;
      break;
    }
    case "get_current_valuation": {
      const { filter, includeAccounts } = (payload ?? {}) as {
        filter?: unknown;
        includeAccounts?: boolean;
      };
      body = JSON.stringify({ filter, includeAccounts: includeAccounts ?? false });
      break;
    }
    case "get_portfolio_allocations": {
      const p = payload as { filter: { type: string; accountId?: string } };
      if (p.filter?.type === "account" && p.filter.accountId) {
        url = `${API_PREFIX}/allocations?accountId=${encodeURIComponent(p.filter.accountId)}`;
        method = "GET";
      } else {
        body = JSON.stringify({ filter: p.filter });
      }
      break;
    }
    case "get_holdings_by_allocation": {
      const p = payload as {
        filter: { type: string; accountId?: string };
        taxonomyId: string;
        categoryId: string;
      };
      if (p.filter?.type === "account" && p.filter.accountId) {
        const params = new URLSearchParams();
        params.set("accountId", p.filter.accountId);
        params.set("taxonomyId", p.taxonomyId);
        params.set("categoryId", p.categoryId);
        url = `${API_PREFIX}/allocations/holdings?${params.toString()}`;
        method = "GET";
      } else {
        body = JSON.stringify({
          filter: p.filter,
          taxonomyId: p.taxonomyId,
          categoryId: p.categoryId,
        });
      }
      break;
    }
    // Snapshot management
    case "get_snapshots": {
      const { accountId, dateFrom, dateTo } = payload as {
        accountId: string;
        dateFrom?: string;
        dateTo?: string;
      };
      const params = new URLSearchParams();
      params.set("accountId", accountId);
      if (dateFrom) params.set("dateFrom", dateFrom);
      if (dateTo) params.set("dateTo", dateTo);
      url += `?${params.toString()}`;
      break;
    }
    case "get_snapshot_by_date": {
      const { accountId, date } = payload as { accountId: string; date: string };
      const params = new URLSearchParams();
      params.set("accountId", accountId);
      params.set("date", date);
      url += `?${params.toString()}`;
      break;
    }
    case "delete_snapshot": {
      const { accountId, date, snapshotId } = payload as {
        accountId: string;
        date: string;
        snapshotId?: string;
      };
      const params = new URLSearchParams();
      params.set("accountId", accountId);
      params.set("date", date);
      if (snapshotId) params.set("snapshotId", snapshotId);
      url += `?${params.toString()}`;
      break;
    }
    case "save_manual_holdings": {
      const { accountId, holdings, cashBalances, snapshotDate } = payload as {
        accountId: string;
        holdings: unknown[];
        cashBalances: Record<string, string>;
        snapshotDate?: string;
      };
      body = JSON.stringify({ accountId, holdings, cashBalances, snapshotDate });
      break;
    }
    case "calculate_accounts_simple_performance": {
      const { accountIds } = (payload ?? {}) as { accountIds?: string[] };
      body = JSON.stringify({ accountIds });
      break;
    }
    case "get_accounts": {
      const { includeArchived } = (payload ?? {}) as { includeArchived?: boolean };
      if (includeArchived) {
        const params = new URLSearchParams();
        params.set("includeArchived", "true");
        url += `?${params.toString()}`;
      }
      break;
    }
    case "calculate_performance_history": {
      const { itemType, itemId, startDate, endDate, trackingMode, filter } = payload as {
        itemType: string;
        itemId: string;
        startDate?: string;
        endDate?: string;
        trackingMode?: string;
        filter?: unknown;
      };
      body = JSON.stringify({ itemType, itemId, startDate, endDate, trackingMode, filter });
      break;
    }
    case "calculate_performance_summary": {
      const { itemType, itemId, startDate, endDate, trackingMode, filter, profile } = payload as {
        itemType: string;
        itemId: string;
        startDate?: string;
        endDate?: string;
        trackingMode?: string;
        filter?: unknown;
        profile?: string;
      };
      body = JSON.stringify({
        itemType,
        itemId,
        startDate,
        endDate,
        trackingMode,
        filter,
        profile,
      });
      break;
    }
    case "get_performance_summaries": {
      const { scopes, startDate, endDate, profile } = payload as {
        scopes: unknown[];
        startDate?: string | null;
        endDate?: string | null;
        profile?: string;
      };
      body = JSON.stringify({ scopes, startDate, endDate, profile });
      break;
    }
    case "get_income_summary": {
      const p = payload as { filter?: { type: string; accountId?: string } };
      if (p?.filter?.type === "account" && p.filter.accountId) {
        url = `${API_PREFIX}/income/summary?accountId=${encodeURIComponent(p.filter.accountId)}`;
        method = "GET";
      } else {
        body = JSON.stringify({ filter: p?.filter ?? null });
      }
      break;
    }
    case "update_exchange_rate": {
      const { rate } = payload as { rate: Record<string, unknown> };
      body = JSON.stringify(rate);
      break;
    }
    case "add_exchange_rate": {
      const { newRate } = payload as { newRate: Record<string, unknown> };
      body = JSON.stringify(newRate);
      break;
    }
    case "delete_exchange_rate": {
      const { rateId } = payload as { rateId: string };
      url += `/${encodeURIComponent(rateId)}`;
      break;
    }
    case "get_exchange_rates_for_dates": {
      const { request } = payload as { request: Record<string, unknown> };
      body = JSON.stringify(request);
      break;
    }
    case "get_exchanges":
    case "synch_quotes":
      break;
    case "search_activities": {
      body = JSON.stringify(payload);
      break;
    }
    case "create_activity": {
      const { activity } = payload as { activity: Record<string, unknown> };
      body = JSON.stringify(activity);
      break;
    }
    case "update_activity": {
      const { activity } = payload as { activity: Record<string, unknown> };
      body = JSON.stringify(activity);
      break;
    }
    case "save_activities": {
      const { request } = payload as { request: Record<string, unknown> };
      body = JSON.stringify(request);
      break;
    }
    case "delete_activity": {
      const { activityId } = payload as { activityId: string };
      url += `/${encodeURIComponent(activityId)}`;
      break;
    }
    case "get_transfer_pair_for_activity": {
      const { activityId } = payload as { activityId: string };
      url += `/${encodeURIComponent(activityId)}/transfer-pair`;
      break;
    }
    case "find_transfer_match_candidates": {
      const { request } = payload as { request: Record<string, unknown> };
      body = JSON.stringify(request);
      break;
    }
    case "save_internal_transfer_pair": {
      const { request } = payload as { request: Record<string, unknown> };
      body = JSON.stringify(request);
      break;
    }
    case "query_ordinary_activities": {
      const data = payload as { query: Record<string, unknown> };
      body = JSON.stringify(data.query);
      break;
    }
    case "create_ordinary_activity": {
      const data = payload as { input: Record<string, unknown> };
      body = JSON.stringify(data.input);
      break;
    }
    case "update_ordinary_activity": {
      const data = payload as { input: Record<string, unknown> };
      body = JSON.stringify(data.input);
      break;
    }
    case "list_pending_activities": {
      const data = payload as { includeResolved?: boolean } | undefined;
      if (data?.includeResolved) url += "?includeResolved=true";
      break;
    }
    case "create_pending_activity": {
      const data = payload as { pending: Record<string, unknown> };
      body = JSON.stringify(data.pending);
      break;
    }
    case "update_pending_activity": {
      const data = payload as { id: string; pending: Record<string, unknown> };
      url += `/${data.id}`;
      body = JSON.stringify(data.pending);
      break;
    }
    case "confirm_pending_activity": {
      const data = payload as { args: { id: string; expectedVersion: number } };
      url += `/${data.args.id}/confirm`;
      body = JSON.stringify({ expectedVersion: data.args.expectedVersion });
      break;
    }
    case "reject_pending_activity": {
      const data = payload as { args: { id: string; expectedVersion: number } };
      url += `/${data.args.id}/reject`;
      body = JSON.stringify({ expectedVersion: data.args.expectedVersion });
      break;
    }
    case "link_transfer_activities": {
      const { activityAId, activityBId } = payload as {
        activityAId: string;
        activityBId: string;
      };
      body = JSON.stringify({ activityAId, activityBId });
      break;
    }
    case "unlink_transfer_activities": {
      const { activityAId, activityBId } = payload as {
        activityAId: string;
        activityBId: string;
      };
      body = JSON.stringify({ activityAId, activityBId });
      break;
    }
    case "update_market_data_provider_settings": {
      body = JSON.stringify(payload);
      break;
    }
    case "create_custom_provider": {
      const { payload: cp } = payload as { payload: Record<string, unknown> };
      body = JSON.stringify(cp);
      break;
    }
    case "update_custom_provider": {
      const { providerId, payload: cp } = payload as {
        providerId: string;
        payload: Record<string, unknown>;
      };
      url += `/${encodeURIComponent(providerId)}`;
      body = JSON.stringify(cp);
      break;
    }
    case "delete_custom_provider": {
      const { providerId } = payload as { providerId: string };
      url += `/${encodeURIComponent(providerId)}`;
      break;
    }
    case "test_custom_provider_source": {
      const { payload: tp } = payload as { payload: Record<string, unknown> };
      body = JSON.stringify(tp);
      break;
    }
    case "create_asset": {
      const { payload: assetPayload } = payload as { payload: Record<string, unknown> };
      body = JSON.stringify(assetPayload);
      break;
    }
    case "delete_asset": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "get_asset_profile": {
      const { assetId } = payload as { assetId: string };
      const params = new URLSearchParams();
      params.set("assetId", assetId);
      url += `?${params.toString()}`;
      break;
    }
    case "update_asset_profile": {
      const { id, payload: bodyPayload } = payload as {
        id: string;
        payload: Record<string, unknown>;
      };
      url += `/${encodeURIComponent(id)}`;
      body = JSON.stringify(bodyPayload);
      break;
    }
    case "get_asset_logo":
    case "delete_asset_logo": {
      const { assetId } = payload as { assetId: string };
      url += `/${encodeURIComponent(assetId)}`;
      break;
    }
    case "upsert_asset_logo": {
      const { assetId, payload: logoPayload } = payload as {
        assetId: string;
        payload: Record<string, unknown>;
      };
      url += `/${encodeURIComponent(assetId)}`;
      body = JSON.stringify(logoPayload);
      break;
    }
    case "update_quote_mode": {
      const { id, quoteMode } = payload as { id: string; quoteMode: string };
      url += `/${encodeURIComponent(id)}`;
      body = JSON.stringify({ quoteMode });
      break;
    }
    case "search_symbol": {
      const { query } = payload as { query: string };
      const params = new URLSearchParams();
      params.set("query", query);
      url += `?${params.toString()}`;
      break;
    }
    case "resolve_symbol_quote": {
      const { symbol, exchangeMic, instrumentType, providerId, quoteCcy, providerSymbol } =
        payload as {
          symbol: string;
          exchangeMic?: string;
          instrumentType?: string;
          providerId?: string;
          quoteCcy?: string;
          providerSymbol?: string;
        };
      const params = new URLSearchParams();
      params.set("symbol", symbol);
      if (exchangeMic) params.set("exchangeMic", exchangeMic);
      if (instrumentType) params.set("instrumentType", instrumentType);
      if (providerId) params.set("providerId", providerId);
      if (quoteCcy) params.set("quoteCcy", quoteCcy);
      if (providerSymbol) params.set("providerSymbol", providerSymbol);
      url += `?${params.toString()}`;
      break;
    }
    case "get_quote_history": {
      const { symbol } = payload as { symbol: string };
      const params = new URLSearchParams();
      params.set("symbol", symbol);
      url += `?${params.toString()}`;
      break;
    }
    case "fetch_dividends": {
      const { symbol, exchangeMic, instrumentType, quoteCcy, providerId, startDate, endDate } =
        payload as {
          symbol: string;
          exchangeMic?: string;
          instrumentType?: string;
          quoteCcy?: string;
          providerId?: string;
          startDate?: string;
          endDate?: string;
        };
      const params = new URLSearchParams();
      params.set("symbol", symbol);
      if (exchangeMic) params.set("exchangeMic", exchangeMic);
      if (instrumentType) params.set("instrumentType", instrumentType);
      if (quoteCcy) params.set("quoteCcy", quoteCcy);
      if (providerId) params.set("providerId", providerId);
      if (startDate) params.set("startDate", startDate);
      if (endDate) params.set("endDate", endDate);
      url += `?${params.toString()}`;
      break;
    }
    case "get_latest_quotes": {
      const { assetIds } = payload as { assetIds: string[] };
      body = JSON.stringify({ assetIds });
      break;
    }
    case "update_quote": {
      const { symbol, quote } = payload as { symbol: string; quote: Record<string, unknown> };
      url += `/${encodeURIComponent(symbol)}`;
      body = JSON.stringify(quote);
      break;
    }
    case "delete_quote": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "reset_provider_history": {
      const { assetId } = payload as { assetId: string };
      url += `/${encodeURIComponent(assetId)}/reset`;
      break;
    }
    case "sync_market_data": {
      body = JSON.stringify(payload);
      break;
    }
    case "set_secret": {
      const { secretKey, secret } = payload as { secretKey: string; secret: string };
      body = JSON.stringify({ secretKey, secret });
      break;
    }
    case "get_secret": {
      const { secretKey } = payload as { secretKey: string };
      const params = new URLSearchParams();
      params.set("secretKey", secretKey);
      url += `?${params.toString()}`;
      break;
    }
    case "delete_secret": {
      const { secretKey } = payload as { secretKey: string };
      const params = new URLSearchParams();
      params.set("secretKey", secretKey);
      url += `?${params.toString()}`;
      break;
    }
    // Taxonomy commands
    case "get_taxonomies":
      break;
    case "get_taxonomy": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "create_taxonomy": {
      const { taxonomy } = payload as { taxonomy: Record<string, unknown> };
      body = JSON.stringify(taxonomy);
      break;
    }
    case "update_taxonomy": {
      const { taxonomy } = payload as { taxonomy: Record<string, unknown> };
      body = JSON.stringify(taxonomy);
      break;
    }
    case "delete_taxonomy": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "create_category": {
      const { category } = payload as { category: Record<string, unknown> };
      body = JSON.stringify(category);
      break;
    }
    case "update_category": {
      const { category } = payload as { category: Record<string, unknown> };
      body = JSON.stringify(category);
      break;
    }
    case "delete_category": {
      const { taxonomyId, categoryId } = payload as { taxonomyId: string; categoryId: string };
      url += `/${encodeURIComponent(taxonomyId)}/categories/${encodeURIComponent(categoryId)}`;
      break;
    }
    case "move_category": {
      const { taxonomyId, categoryId, newParentId, position } = payload as {
        taxonomyId: string;
        categoryId: string;
        newParentId: string | null;
        position: number;
      };
      body = JSON.stringify({ taxonomyId, categoryId, newParentId, position });
      break;
    }
    case "export_taxonomy_json": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}/export`;
      break;
    }
    case "get_asset_taxonomy_assignments": {
      const { assetId } = payload as { assetId: string };
      url += `/${encodeURIComponent(assetId)}`;
      break;
    }
    case "assign_asset_to_category": {
      const { assignment } = payload as { assignment: Record<string, unknown> };
      body = JSON.stringify(assignment);
      break;
    }
    case "replace_asset_taxonomy_assignments": {
      const { assetId, taxonomyId, assignments } = payload as {
        assetId: string;
        taxonomyId: string;
        assignments: Record<string, unknown>[];
      };
      url += `/${encodeURIComponent(assetId)}/taxonomy/${encodeURIComponent(taxonomyId)}`;
      body = JSON.stringify(assignments);
      break;
    }
    case "remove_asset_taxonomy_assignment": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "get_migration_status":
      break;
    case "migrate_legacy_classifications":
      break;
    // Spending budget commands
    case "get_budget": {
      const { periodKey } = (payload ?? {}) as { periodKey?: string };
      addPeriodKey(periodKey);
      break;
    }
    case "upsert_budget_target": {
      const { target, periodKey } = payload as {
        target: Record<string, unknown>;
        periodKey?: string;
      };
      addPeriodKey(periodKey);
      body = JSON.stringify(target);
      break;
    }
    case "delete_budget_target": {
      const { id, periodKey } = payload as { id: string; periodKey?: string };
      url += `/${encodeURIComponent(id)}`;
      addPeriodKey(periodKey);
      break;
    }
    case "upsert_budget_rollover_setting": {
      const { setting, periodKey } = payload as {
        setting: Record<string, unknown>;
        periodKey?: string;
      };
      addPeriodKey(periodKey);
      body = JSON.stringify(setting);
      break;
    }
    case "delete_budget_rollover_setting": {
      const { id, periodKey } = payload as { id: string; periodKey?: string };
      url += `/${encodeURIComponent(id)}`;
      addPeriodKey(periodKey);
      break;
    }
    case "create_budget_group": {
      const { group, periodKey } = payload as {
        group: Record<string, unknown>;
        periodKey?: string;
      };
      addPeriodKey(periodKey);
      body = JSON.stringify(group);
      break;
    }
    case "update_budget_group": {
      const { id, patch, periodKey } = payload as {
        id: string;
        patch: Record<string, unknown>;
        periodKey?: string;
      };
      url += `/${encodeURIComponent(id)}`;
      addPeriodKey(periodKey);
      body = JSON.stringify(patch);
      break;
    }
    case "delete_budget_group": {
      const { id, reassignToGroupId, periodKey } = payload as {
        id: string;
        reassignToGroupId: string;
        periodKey?: string;
      };
      url += `/${encodeURIComponent(id)}`;
      addPeriodKey(periodKey);
      body = JSON.stringify({ reassignToGroupId });
      break;
    }
    case "assign_category_to_group": {
      const { categoryId, groupId, periodKey } = payload as {
        categoryId: string;
        groupId: string;
        periodKey?: string;
      };
      addPeriodKey(periodKey);
      body = JSON.stringify({ categoryId, groupId });
      break;
    }
    case "reset_budget_groups": {
      const { periodKey } = (payload ?? {}) as { periodKey?: string };
      addPeriodKey(periodKey);
      break;
    }
    case "copy_budget_targets": {
      const { sourcePeriodKey, targetPeriodKey, overwrite } = payload as {
        sourcePeriodKey: string;
        targetPeriodKey: string;
        overwrite?: boolean;
      };
      body = JSON.stringify({ sourcePeriodKey, targetPeriodKey, overwrite: !!overwrite });
      break;
    }
    // Spending settings
    case "get_spending_settings":
      break;
    case "update_spending_settings": {
      const { update } = payload as { update: Record<string, unknown> };
      body = JSON.stringify(update);
      break;
    }
    // Spending cash activities + assignments
    case "list_cash_activities": {
      const { filter } = (payload ?? {}) as { filter?: Record<string, unknown> };
      if (filter) {
        const params = new URLSearchParams();
        // Stringify only primitives so the query string never gets
        // "[object Object]" from an accidental nested value. Filter shape is
        // string/number/boolean/string[]; the guard keeps that contract
        // visible (and silences @typescript-eslint/no-base-to-string).
        const toQs = (val: unknown): string | null => {
          if (typeof val === "string") return val;
          if (typeof val === "number" || typeof val === "boolean") return String(val);
          return null;
        };
        for (const [k, v] of Object.entries(filter)) {
          if (v === undefined || v === null) continue;
          if (Array.isArray(v)) {
            for (const item of v) {
              const s = toQs(item);
              if (s !== null) params.append(`${k}[]`, s);
            }
          } else {
            const s = toQs(v);
            if (s !== null) params.set(k, s);
          }
        }
        const qs = params.toString();
        if (qs) url += `?${qs}`;
      }
      break;
    }
    case "search_cash_activities": {
      const { request } = (payload ?? {}) as { request?: Record<string, unknown> };
      body = JSON.stringify(request ?? {});
      break;
    }
    case "set_activity_event": {
      const { activityId, eventId } = payload as {
        activityId: string;
        eventId: string | null;
      };
      url += `/${encodeURIComponent(activityId)}/event`;
      body = JSON.stringify({ eventId });
      break;
    }
    case "get_activity_assignments": {
      const { activityId } = payload as { activityId: string };
      url += `/${encodeURIComponent(activityId)}/assignments`;
      break;
    }
    case "assign_activity_category": {
      const { activityId, taxonomyId, categoryId } = payload as {
        activityId: string;
        taxonomyId: string;
        categoryId: string;
      };
      url += `/${encodeURIComponent(activityId)}/assignments`;
      body = JSON.stringify({ taxonomyId, categoryId });
      break;
    }
    case "unassign_activity_category": {
      const { activityId, taxonomyId } = payload as {
        activityId: string;
        taxonomyId: string;
      };
      url += `/${encodeURIComponent(activityId)}/assignments/${encodeURIComponent(taxonomyId)}`;
      break;
    }
    case "get_activity_splits": {
      const { activityId } = payload as { activityId: string };
      url += `/${encodeURIComponent(activityId)}/splits`;
      break;
    }
    case "replace_activity_splits": {
      const { activityId, splits } = payload as {
        activityId: string;
        splits: unknown[];
      };
      url += `/${encodeURIComponent(activityId)}/splits`;
      body = JSON.stringify(splits);
      break;
    }
    case "clear_activity_splits": {
      const { activityId } = payload as { activityId: string };
      url += `/${encodeURIComponent(activityId)}/splits`;
      break;
    }
    case "bulk_assign_categories": {
      const { items } = payload as { items: unknown[] };
      body = JSON.stringify(items);
      break;
    }
    // Spending categorization rules
    case "list_categorization_rules":
      break;
    case "create_categorization_rule": {
      const { rule } = payload as { rule: Record<string, unknown> };
      body = JSON.stringify(rule);
      break;
    }
    case "update_categorization_rule": {
      const { id, patch } = payload as { id: string; patch: Record<string, unknown> };
      url += `/${encodeURIComponent(id)}`;
      body = JSON.stringify(patch);
      break;
    }
    case "delete_categorization_rule": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "rerun_categorization_rules": {
      const { onlyUncategorized } = payload as { onlyUncategorized: boolean };
      body = JSON.stringify({ onlyUncategorized });
      break;
    }
    // Spending events + event types
    case "list_event_types":
    case "list_events":
      break;
    case "create_event_type": {
      const { newType } = payload as { newType: Record<string, unknown> };
      body = JSON.stringify(newType);
      break;
    }
    case "update_event_type": {
      const { id, patch } = payload as {
        id: string;
        patch: { name?: string; color?: string | null };
      };
      url += `/${encodeURIComponent(id)}`;
      body = JSON.stringify(patch);
      break;
    }
    case "delete_event_type": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "create_event": {
      const { event } = payload as { event: Record<string, unknown> };
      body = JSON.stringify(event);
      break;
    }
    case "update_event": {
      const { id, patch } = payload as { id: string; patch: Record<string, unknown> };
      url += `/${encodeURIComponent(id)}`;
      body = JSON.stringify(patch);
      break;
    }
    case "delete_event": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "get_event_spending_summaries": {
      const { request } = (payload ?? {}) as { request?: Record<string, unknown> };
      body = JSON.stringify(request ?? null);
      break;
    }
    // Spending analytics
    case "get_spending_report":
    case "get_spending_insight": {
      const { request } = payload as { request: Record<string, unknown> };
      body = JSON.stringify(request);
      break;
    }
    // Health Center commands
    case "get_health_status":
    case "run_health_checks":
    case "get_dismissed_health_issues":
    case "get_health_config":
      break;
    case "dismiss_health_issue": {
      const { issueId, dataHash } = payload as { issueId: string; dataHash: string };
      body = JSON.stringify({ issueId, dataHash });
      break;
    }
    case "restore_health_issue": {
      const { issueId } = payload as { issueId: string };
      body = JSON.stringify({ issueId });
      break;
    }
    case "execute_health_fix": {
      const { action } = payload as { action: Record<string, unknown> };
      body = JSON.stringify(action);
      break;
    }
    case "update_health_config": {
      const { config } = payload as { config: Record<string, unknown> };
      body = JSON.stringify(config);
      break;
    }
    // Net Worth commands
    case "get_net_worth": {
      const { date } = (payload ?? {}) as { date?: string };
      if (date) {
        const params = new URLSearchParams();
        params.set("date", date);
        url += `?${params.toString()}`;
      }
      break;
    }
    case "get_net_worth_history": {
      const { startDate, endDate } = payload as { startDate: string; endDate: string };
      const params = new URLSearchParams();
      params.set("startDate", startDate);
      params.set("endDate", endDate);
      url += `?${params.toString()}`;
      break;
    }
    // Alternative Assets commands
    case "get_loan_payments": {
      const { assetId } = payload as { assetId: string };
      url += `/${encodeURIComponent(assetId)}/payments`;
      break;
    }
    case "link_loan_payment": {
      const { activityId, link } = payload as { activityId: string; link: Record<string, unknown> };
      url += `/${encodeURIComponent(activityId)}`;
      body = JSON.stringify(link);
      break;
    }
    case "apply_loan_action": {
      const { assetId, action } = payload as { assetId: string; action: Record<string, unknown> };
      url += `/${encodeURIComponent(assetId)}/actions`;
      body = JSON.stringify(action);
      break;
    }
    case "preview_loan_terms":
      body = JSON.stringify(payload);
      break;
    case "recalculate_loan":
    case "calculate_loan":
    case "create_alternative_asset": {
      const { request } = payload as { request: Record<string, unknown> };
      body = JSON.stringify(request);
      break;
    }
    case "update_alternative_asset_valuation": {
      const { assetId, request } = payload as { assetId: string; request: Record<string, unknown> };
      url += `/${encodeURIComponent(assetId)}/valuation`;
      body = JSON.stringify(request);
      break;
    }
    case "delete_alternative_asset": {
      const { assetId } = payload as { assetId: string };
      url += `/${encodeURIComponent(assetId)}`;
      break;
    }
    case "link_liability": {
      const { liabilityId, request } = payload as {
        liabilityId: string;
        request: Record<string, unknown>;
      };
      url += `/${encodeURIComponent(liabilityId)}/link`;
      body = JSON.stringify(request);
      break;
    }
    case "unlink_liability": {
      const { liabilityId } = payload as { liabilityId: string };
      url += `/${encodeURIComponent(liabilityId)}/unlink`;
      break;
    }
    case "update_alternative_asset_metadata": {
      const { assetId, metadata, name, notes, loan } = payload as {
        assetId: string;
        metadata: Record<string, string>;
        name?: string;
        notes?: string | null;
        loan?: unknown;
      };
      url += `/${encodeURIComponent(assetId)}/metadata`;
      body = JSON.stringify({ metadata, name, notes, loan });
      break;
    }
    case "get_alternative_holdings":
      break;
    // Allocation Targets
    case "list_allocation_targets":
      break;
    case "get_allocation_target": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "create_allocation_target": {
      const { input } = payload as { input: Record<string, unknown> };
      body = JSON.stringify(input);
      break;
    }
    case "update_allocation_target": {
      const { id, input } = payload as { id: string; input: Record<string, unknown> };
      url += `/${encodeURIComponent(id)}`;
      body = JSON.stringify(input);
      break;
    }
    case "archive_allocation_target": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}/archive`;
      break;
    }
    case "delete_allocation_target": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "list_allocation_target_weights": {
      const { targetId } = payload as { targetId: string };
      url += `/${encodeURIComponent(targetId)}/weights`;
      break;
    }
    case "save_allocation_target_weights": {
      const { targetId, weights } = payload as { targetId: string; weights: unknown[] };
      url += `/${encodeURIComponent(targetId)}/weights`;
      body = JSON.stringify(weights);
      break;
    }
    case "save_allocation_target_with_weights": {
      const { id, input, weights } = payload as {
        id: string | null;
        input: Record<string, unknown>;
        weights: unknown[];
      };
      body = JSON.stringify({ id, input, weights });
      break;
    }
    case "get_allocation_target_drift": {
      const { targetId, filter, includeHoldings } = payload as {
        targetId: string;
        filter: unknown;
        includeHoldings?: boolean;
      };
      url += `/${encodeURIComponent(targetId)}/drift`;
      body = JSON.stringify({ filter, includeHoldings: includeHoldings ?? false });
      break;
    }
    case "list_target_constraints": {
      const { targetId } = payload as { targetId: string };
      url += `/${encodeURIComponent(targetId)}/constraints`;
      break;
    }
    case "save_target_constraints": {
      const { targetId, constraints } = payload as {
        targetId: string;
        constraints: unknown[];
      };
      url += `/${encodeURIComponent(targetId)}/constraints`;
      body = JSON.stringify(constraints);
      break;
    }
    case "calculate_rebalance_plan": {
      const { targetId, availableCash, filter, scenarioMode, eligibleAssetIds } = payload as {
        targetId: string;
        availableCash: number;
        filter: unknown;
        scenarioMode: string;
        eligibleAssetIds?: string[];
      };
      body = JSON.stringify({
        targetId,
        availableCash,
        filter,
        scenarioMode,
        ...(eligibleAssetIds === undefined ? {} : { eligibleAssetIds }),
      });
      break;
    }
    // Agent Access
    case "create_agent_access_token": {
      const { name, expiresAt, scopes } = payload as {
        name: string;
        expiresAt?: string;
        scopes: string[];
      };
      body = JSON.stringify({ name, expiresAt, scopes });
      break;
    }
    case "delete_agent_access_token": {
      const { id } = payload as { id: string };
      url += `/${encodeURIComponent(id)}`;
      break;
    }
    case "list_agent_audit_log": {
      const { page, pageSize, q, tools, outcomes, actorKinds } = payload as {
        page: number;
        pageSize: number;
        q?: string;
        tools?: string[];
        outcomes?: string[];
        actorKinds?: string[];
      };
      const params = new URLSearchParams();
      params.set("page", String(page));
      params.set("pageSize", String(pageSize));
      if (q) params.set("q", q);
      if (tools?.length) params.set("tools", tools.join(","));
      if (outcomes?.length) params.set("outcomes", outcomes.join(","));
      if (actorKinds?.length) params.set("actorKinds", actorKinds.join(","));
      url += `?${params.toString()}`;
      break;
    }
  }

  const headers: HeadersInit = {};
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
  }
  if (command === "get_health_status" || command === "run_health_checks") {
    const payloadTimezone =
      typeof payload === "object" && payload !== null && "clientTimezone" in payload
        ? String((payload as { clientTimezone?: string }).clientTimezone ?? "").trim()
        : "";
    const clientTimezone = payloadTimezone || Intl.DateTimeFormat().resolvedOptions().timeZone;
    if (clientTimezone) {
      headers["X-Client-Timezone"] = clientTimezone;
    }
  }

  const res = await profileFetch(url, {
    method,
    headers,
    body,
    credentials: "same-origin",
    signal: AbortSignal.timeout(300_000),
  });

  // 401 = app auth failure (JWT expired/invalid). Cloud auth failures return 403.
  if (res.status === 401) {
    notifyUnauthorized();
  }
  if (!res.ok) {
    let msg = res.statusText;
    try {
      const err = (await res.json()) as unknown;
      if (
        typeof err === "object" &&
        err !== null &&
        "message" in err &&
        typeof err.message === "string"
      ) {
        msg = err.message;
      }
    } catch (_e) {
      // ignore JSON parse error from non-JSON error bodies
      void 0;
    }
    console.error(`[Invoke] Command "${command}" failed: ${msg}`);
    // The server's 408 timeout leaves owned reset tasks running; 5xx responses
    // can also follow a committed reset whose completion failed.
    throw Object.assign(new Error(msg), {
      outcomeUnknown: res.status === 408 || res.status >= 500,
    });
  }
  // Handle responses with no body (204 No Content, 202 Accepted, or empty 200)
  if (res.status === 204 || res.status === 202) {
    return undefined as T;
  }
  const text = await res.text();
  if (!text) {
    return undefined as T;
  }
  return JSON.parse(text) as T;
};
