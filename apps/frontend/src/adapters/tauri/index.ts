// Tauri adapter - Desktop implementation
// This file re-exports all domain-specific modules

import type { RunEnv } from "../types";
import { RunEnvs } from "../types";

// Platform constants from core
export * from "./agent-bundle";
export type * from "../agent-bundle-types";
export { isTauri, isWeb, logger } from "./core";

/**
 * Runtime environment identifier - always "desktop" for Tauri builds
 */
export const RUN_ENV: RunEnv = RunEnvs.DESKTOP;

// Re-export types and constants from ../types
export { RunEnvs } from "../types";
export type {
  AgentAccessStatus,
  AgentAccessToken,
  AgentAuditEntry,
  AgentAuditPage,
  AgentAuditQuery,
  AppInfo,
  CreateAgentAccessTokenInput,
  CreatedAgentAccessToken,
  DataExportResult,
  EventCallback,
  Logger,
  MarketDataProviderSetting,
  McpServerStatus,
  PlatformInfo,
  ProviderCapabilities,
  RunEnv,
  UnlistenFn,
} from "../types";

// ============================================================================
// Shared domain modules (identical logic for both platforms)
// ============================================================================

// Portfolio Commands
export {
  createPortfolio,
  deletePortfolio,
  getPortfolios,
  updatePortfolioEntry,
} from "../shared/portfolios";

// Account Commands
export * from "../shared/accounts";

// Activity Commands
export * from "../shared/activities";

// Pending entries (待确认收支)
export * from "../shared/pending-activities";
export * from "../shared/investment-journal";

// Portfolio Commands
export * from "../shared/portfolio";

// Market Data Commands
export * from "../shared/market-data";

// Custom Provider Commands
export * from "../shared/custom-provider";

// Taxonomy Commands
export * from "../shared/taxonomies";

// Alternative Assets Commands
export * from "../shared/alternative-assets";

// Asset Logo Commands
export * from "../shared/asset-logos";

// Exchange Rates Commands
export * from "../shared/exchange-rates";

// Spending Categorization Commands
export * from "../shared/spending";

// Secrets Commands
export * from "../shared/secrets";

// Health Center Commands
export * from "../shared/health";

// Allocation Target Commands
export * from "../shared/allocation-targets";

// Data Export Commands
export { exportDataFile } from "./exports";

// ============================================================================
// Platform-specific modules (different implementations)
// ============================================================================

// Settings Commands (contains platform-specific backupDatabase, etc.)
export {
  backupDatabase,
  deleteDatabaseBackup,
  exportDatabaseBackup,
  getAppInfo,
  getDatabaseBackupDownloadUrl,
  getDatabaseEncryptionStatus,
  getPlatform,
  getSettings,
  listDatabaseBackups,
  openDatabaseBackupFolder,
  setDatabaseEncryptionEnabled,
  updateSettings,
} from "./settings";
export type { DatabaseBackup, DatabaseEncryptionStatus } from "./settings";

// Agent Access Commands (embedded MCP server; PATs are web-only stubs)
export {
  createAgentAccessToken,
  deleteAgentAccessToken,
  getAgentAccessStatus,
  getMcpStatus,
  listAgentAccessTokens,
  listAgentAuditLog,
  purgeAgentAuditLog,
  setMcpAuditEnabled,
  setMcpAutoStart,
  setMcpEnabled,
  startMcp,
  stopMcp,
} from "./agent-access";

// Event Listeners (Tauri listen() implementation)
export {
  listenAssetClassificationsChanged,
  listenFileDrop,
  listenFileDropCancelled,
  listenFileDropHover,
  listenMarketSyncComplete,
  listenMarketSyncError,
  listenMarketSyncStart,
  listenNavigateToRoute,
  listenPortfolioUpdateComplete,
  listenPortfolioUpdateError,
  listenPortfolioUpdateStart,
} from "./events";

// File Dialogs (Tauri file dialogs)
export {
  openFileSaveDialog,
  openFolderDialog,
  openUrlInBrowser,
  saveAppDataFileViaPicker,
} from "./files";

export { listenInvestmentIndexUpdate } from "./events";
