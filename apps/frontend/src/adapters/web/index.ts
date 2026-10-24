// Web adapter - Browser implementation
// This file re-exports shared modules and platform-specific modules

import type { RunEnv } from "../types";
import { RunEnvs } from "../types";

// Platform constants
export { isTauri, isWeb, logger } from "./core";

// Re-export types and constants from shared types
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

/**
 * Runtime environment identifier - always "web" for web builds
 */
export const RUN_ENV: RunEnv = RunEnvs.WEB;

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
export { createAccount, deleteAccount, getAccounts, updateAccount } from "../shared/accounts";

// Activity Commands
export {
  createActivity,
  deleteActivity,
  findTransferMatchCandidates,
  getActivities,
  getTransferPairForActivity,
  linkTransferActivities,
  saveActivities,
  saveInternalTransferPair,
  searchActivities,
  unlinkTransferActivities,
  updateActivity,
  queryOrdinaryActivities,
  createOrdinaryActivity,
  updateOrdinaryActivity,
} from "../shared/activities";

// Pending entries (待确认收支)
export {
  confirmPendingActivity,
  createPendingActivity,
  listPendingActivities,
  rejectPendingActivity,
  updatePendingActivity,
} from "../shared/pending-activities";

// Secrets Commands
export { deleteSecret, getSecret, setSecret } from "../shared/secrets";

// Taxonomy Commands
export {
  assignAssetToCategory,
  createCategory,
  createTaxonomy,
  deleteCategory,
  deleteTaxonomy,
  exportTaxonomyJson,
  getAssetTaxonomyAssignments,
  getMigrationStatus,
  getTaxonomies,
  getTaxonomy,
  migrateLegacyClassifications,
  moveCategory,
  removeAssetTaxonomyAssignment,
  replaceAssetTaxonomyAssignments,
  updateCategory,
  updateTaxonomy,
} from "../shared/taxonomies";

// Portfolio Commands
export {
  calculateAccountsSimplePerformance,
  calculatePerformanceHistory,
  calculatePerformanceSummaries,
  calculatePerformanceSummary,
  deleteSnapshot,
  getAssetHoldings,
  getAssetLots,
  getCurrentValuation,
  getHistoricalValuations,
  getHolding,
  getHoldings,
  getHoldingsByAllocation,
  getHoldingsList,
  getIncomeSummary,
  getLatestValuations,
  getPortfolioAllocations,
  getSnapshotByDate,
  getSnapshots,
  performanceSummaryScopeKey,
  recalculatePortfolio,
  saveManualHoldings,
  updatePortfolio,
} from "../shared/portfolio";

// Market Data Commands
export {
  createAsset,
  deleteAsset,
  deleteQuote,
  fetchDividends,
  getAssetProfile,
  getAssets,
  getExchanges,
  getLatestQuotes,
  getMarketDataProviders,
  getMarketDataProviderSettings,
  getQuoteHistory,
  resetAllProviderHistory,
  resetProviderHistory,
  resolveSymbolQuote,
  searchTicker,
  syncHistoryQuotes,
  syncMarketData,
  updateAssetProfile,
  updateMarketDataProviderSettings,
  updateQuote,
  updateQuoteMode,
} from "../shared/market-data";

// Custom Provider Commands
export {
  createCustomProvider,
  deleteCustomProvider,
  getCustomProviders,
  testCustomProviderSource,
  updateCustomProvider,
} from "../shared/custom-provider";

// Exchange Rates Commands
export {
  addExchangeRate,
  deleteExchangeRate,
  getExchangeRates,
  getExchangeRatesForDates,
  updateExchangeRate,
} from "../shared/exchange-rates";

// Spending Categorization Commands
export {
  createCategorizationRule,
  deleteCategorizationRule,
  isSpendingEnabled,
  listCategorizationRules,
  rerunCategorizationRules,
  updateCategorizationRule,
} from "../shared/spending";

// Alternative Assets Commands
export {
  applyLoanAction,
  calculateLoan,
  createAlternativeAsset,
  deleteAlternativeAsset,
  getAlternativeHoldings,
  getLoanPayments,
  getNetWorth,
  getNetWorthHistory,
  linkLiability,
  linkLoanPayment,
  previewLoanTerms,
  recalculateLoan,
  unlinkLiability,
  updateAlternativeAssetMetadata,
  updateAlternativeAssetValuation,
} from "../shared/alternative-assets";

// Asset Logo Commands
export {
  deleteAssetLogo,
  getAssetLogo,
  listAssetLogos,
  upsertAssetLogo,
} from "../shared/asset-logos";

// Health Center Commands
export {
  dismissHealthIssue,
  executeHealthFix,
  getDismissedHealthIssues,
  getHealthConfig,
  getHealthStatus,
  restoreHealthIssue,
  runHealthChecks,
  updateHealthConfig,
} from "../shared/health";

// Allocation Target Commands
export {
  archiveAllocationTarget,
  calculateRebalancePlan,
  canonicalizeEligibleAssetIds,
  createAllocationTarget,
  deleteAllocationTarget,
  getAllocationTarget,
  getAllocationTargetDrift,
  listAllocationTargets,
  listAllocationTargetWeights,
  listTargetConstraints,
  saveAllocationTargetWeights,
  saveAllocationTargetWithWeights,
  saveTargetConstraints,
  updateAllocationTarget,
} from "../shared/allocation-targets";

// Data Export Commands
export { exportDataFile } from "./exports";

// ============================================================================
// Platform-specific modules (different implementations for web vs desktop)
// ============================================================================

// Agent Access Commands (PATs + audit log; MCP server controls are desktop-only stubs)
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

// Event Listeners (web-specific SSE implementation)
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

// File Dialogs (web-specific implementations)
export {
  openFileSaveDialog,
  openFolderDialog,
  openUrlInBrowser,
  saveAppDataFileViaPicker,
} from "./files";

// Settings Commands (web-specific API for backups and updates)
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
export * from "./agent-bundle";
export type * from "../agent-bundle-types";
