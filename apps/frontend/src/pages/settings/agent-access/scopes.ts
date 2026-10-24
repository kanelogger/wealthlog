// Canonical agent-access token scopes shared by the web (server PAT) and
// desktop (Tauri) surfaces. The backend rejects unknown scopes, empty scope
// sets, and write scopes without their prerequisites; this module keeps the
// UI in sync with that contract.

import type { TFunction } from "i18next";

export type ScopeKey =
  | "accounts:read"
  | "holdings:read"
  | "performance:read"
  | "activities:read"
  | "health:read"
  | "classification:read"
  | "activities:draft"
  | "activities:write"
  | "ordinary:write"
  | "classification:suggest"
  | "classification:write";

export type ScopeGroup = "read" | "write";

export interface ScopeMeta {
  key: ScopeKey;
  /** i18n key suffix under `settings:agentAccess.scope.<i18n>`. */
  i18n: string;
  group: ScopeGroup;
}

/** Ordered scopes, grouped reads first then write/suggest. */
export const SCOPES: ScopeMeta[] = [
  { key: "accounts:read", i18n: "accounts_read", group: "read" },
  { key: "holdings:read", i18n: "holdings_read", group: "read" },
  { key: "performance:read", i18n: "performance_read", group: "read" },
  { key: "activities:read", i18n: "activities_read", group: "read" },
  { key: "health:read", i18n: "health_read", group: "read" },
  { key: "classification:read", i18n: "classification_read", group: "read" },
  { key: "activities:draft", i18n: "activities_draft", group: "write" },
  { key: "activities:write", i18n: "activities_write", group: "write" },
  { key: "ordinary:write", i18n: "ordinary_write", group: "write" },
  { key: "classification:suggest", i18n: "classification_suggest", group: "write" },
  { key: "classification:write", i18n: "classification_write", group: "write" },
];

/** Read scopes, in canonical order. */
export const READ_SCOPES: ScopeKey[] = SCOPES.filter((scope) => scope.group === "read").map(
  (scope) => scope.key,
);

export interface ScopePreset {
  key: string;
  /** i18n key suffix under `settings:agentAccess.preset.<i18n>`. */
  i18n: string;
  scopes: ScopeKey[];
}

/** Named presets. Order matters for "summarize to preset name" matching. */
// Formal writes are disabled for MCP this release, so no preset grants a
// write scope: the write tools are filtered out and cannot execute.
export const SCOPE_PRESETS: ScopePreset[] = [
  {
    key: "read-only",
    i18n: "read_only",
    scopes: [...READ_SCOPES],
  },
  {
    key: "read-activity-draft",
    i18n: "read_activity_draft",
    scopes: [...READ_SCOPES, "activities:draft"],
  },
  {
    key: "read-activity-draft-classification-suggest",
    i18n: "read_activity_draft_classification_suggest",
    scopes: [...READ_SCOPES, "activities:draft", "classification:suggest"],
  },
];

/**
 * Scopes kept for existing tokens but not offered when creating one: the
 * formal-write tools they gated are disabled in this release.
 */
export const UNAVAILABLE_SCOPES: ScopeKey[] = [
  "activities:write",
  "classification:write",
  "ordinary:write",
];

/**
 * The web host never grants ordinary:write — its PAT endpoint rejects it —
 * while the desktop host offers it for direct CLI/MCP writes.
 */
export const WEB_UNAVAILABLE_SCOPES: ScopeKey[] = [
  "activities:write",
  "classification:write",
  "ordinary:write",
];

/** Scopes hidden from the create dialog for the current host. */
export function unavailableScopesForHost(isDesktop: boolean): ScopeKey[] {
  return isDesktop
    ? UNAVAILABLE_SCOPES.filter((scope) => scope !== "ordinary:write")
    : WEB_UNAVAILABLE_SCOPES;
}

/** Whether a token that carries this scope can use it on the current host. */
export function scopeAvailableOnHost(scope: string, isDesktop: boolean): boolean {
  return scope !== "ordinary:write" || isDesktop;
}

const SCOPE_I18N = new Map(SCOPES.map((scope) => [scope.key, scope.i18n]));

/** Human label for a scope key; falls back to the raw key for unknown scopes. */
export function scopeLabel(t: TFunction, key: string): string {
  const suffix = SCOPE_I18N.get(key as ScopeKey);
  return suffix ? t(`settings:agentAccess.scope.${suffix}.label`) : key;
}

/** Human description for a scope key; falls back to the raw key for unknown scopes. */
export function scopeDescription(t: TFunction, key: string): string {
  const suffix = SCOPE_I18N.get(key as ScopeKey);
  return suffix ? t(`settings:agentAccess.scope.${suffix}.description`) : key;
}

/** Localized label for a preset. */
export function presetLabel(t: TFunction, preset: ScopePreset): string {
  return t(`settings:agentAccess.preset.${preset.i18n}`);
}

/**
 * Apply dependency rules. Returns scopes in canonical order with duplicates removed.
 */
export function applyScopeDependencies(scopes: Iterable<string>): ScopeKey[] {
  const set = new Set<string>(scopes);
  if (set.has("activities:write")) {
    set.add("activities:draft");
  }
  if (set.has("classification:write")) {
    set.add("classification:suggest");
  }
  if (set.has("ordinary:write")) {
    set.add("activities:read");
    set.add("accounts:read");
  }
  return SCOPES.filter((scope) => set.has(scope.key)).map((scope) => scope.key);
}

/** Returns the preset whose scope set exactly matches `scopes`, if any. */
export function matchPreset(scopes: string[]): ScopePreset | undefined {
  const target = new Set(scopes);
  return SCOPE_PRESETS.find(
    (preset) =>
      preset.scopes.length === target.size && preset.scopes.every((scope) => target.has(scope)),
  );
}
