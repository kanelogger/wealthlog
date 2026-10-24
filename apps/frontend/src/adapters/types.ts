// Shared types for adapters

/**
 * Runtime environment constants
 */
export const RunEnvs = {
  DESKTOP: "desktop",
  WEB: "web",
} as const;

/**
 * Runtime environment type - either desktop (Tauri) or web
 */
export type RunEnv = (typeof RunEnvs)[keyof typeof RunEnvs];

/**
 * Callback function for event handling
 */
export type EventCallback<T> = (event: { event: string; payload: T; id: number }) => void;

/**
 * Function to unsubscribe from an event
 */
export type UnlistenFn = () => Promise<void>;

/**
 * Logger interface with standard logging methods
 */
export interface Logger {
  error: (...args: unknown[]) => void;
  warn: (...args: unknown[]) => void;
  info: (...args: unknown[]) => void;
  debug: (...args: unknown[]) => void;
  trace: (...args: unknown[]) => void;
}

export interface DataExportResult {
  status: "saved" | "empty" | "canceled";
  filename?: string;
}

// Provider capabilities from backend
export interface ProviderCapabilities {
  instruments: string;
  coverage: string;
  features: string[];
}

// Interface matching the backend struct
export interface MarketDataProviderSetting {
  id: string;
  name: string;
  description: string | null;
  url: string | null;
  priority: number;
  enabled: boolean;
  logoFilename: string | null;
  capabilities: ProviderCapabilities | null;
  providerType?: string;
  requiresApiKey: boolean;
  hasApiKey: boolean;
  assetCount: number;
  errorCount: number;
  lastSyncedAt: string | null;
  lastSyncError: string | null;
  uniqueErrors: string[];
}

/**
 * Application info including version and paths.
 */
export interface AppInfo {
  version: string;
  dbPath: string;
  logsDir: string;
}

export interface PlatformInfo {
  os: string;
  arch?: string;
  is_mobile: boolean;
  is_desktop: boolean;
  is_tauri?: boolean;
}

// ============================================================================
// Agent Access (MCP server + personal access tokens)
// ============================================================================

/** Embedded MCP server status (desktop only). */
export interface McpServerStatus {
  enabled: boolean;
  autoStart: boolean;
  auditEnabled: boolean;
  running: boolean;
  port: number | null;
  startedAt: string | null;
}

/** Agent access status for the web server's `/mcp` endpoint. */
export interface AgentAccessStatus {
  mcpEnabled: boolean;
  auditEnabled: boolean;
  endpoint: string;
  /** Any server authentication is configured (web only). */
  serverAuthEnabled: boolean;
  /** Coarse reachability; no address or port is exposed (web only). */
  listenScope: "loopback" | "non_loopback";
}

/** Personal access token metadata (web only; the secret is never returned). */
export interface AgentAccessToken {
  id: string;
  name: string;
  tokenPrefix: string;
  /** `sha256:<prefix>` matching audit `actorFingerprint`, for name attribution. */
  fingerprint: string;
  scopes: string[];
  createdAt: string;
  expiresAt: string | null;
  lastUsedAt: string | null;
  revokedAt: string | null;
}

/** Input for creating a personal access token. */
export interface CreateAgentAccessTokenInput {
  name: string;
  expiresAt?: string;
  scopes: string[];
}

/** Created personal access token. `token` is shown exactly once. */
export interface CreatedAgentAccessToken {
  token: string;
  id: string;
  name: string;
  tokenPrefix: string;
  scopes: string[];
  createdAt: string;
  expiresAt: string | null;
}

/** One MCP tool-call audit entry. */
export interface AgentAuditEntry {
  id: string;
  sessionId: string;
  actorKind: string;
  actorFingerprint: string;
  tool: string;
  scopes: string[];
  argsSummary: string | null;
  outcome: string;
  errorMessage: string | null;
  createdAt: string;
}

/** One page of audit entries. */
export interface AgentAuditPage {
  items: AgentAuditEntry[];
  totalCount: number;
  /** Distinct tool names across the whole log (for the Tool filter). */
  availableTools: string[];
}

/** Request to list a page of the agent audit log. All filters are server-side. */
export interface AgentAuditQuery {
  page: number;
  pageSize: number;
  /** Case-insensitive substring search on the tool name. */
  q?: string;
  /** Exact tool names to include. */
  tools?: string[];
  /** Outcomes to include (success | denied | error). */
  outcomes?: string[];
  /** Actor kinds to include (pat | local_token | desktop_bridge). */
  actorKinds?: string[];
}
