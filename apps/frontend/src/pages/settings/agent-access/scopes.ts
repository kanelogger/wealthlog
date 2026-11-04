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
  | "transfers:write"
  | "holdings:write"
  | "classification:suggest"
  | "classification:write"
  | "investments:read"
  | "investments:write"
  | "investments:confirm";

export type ScopeGroup = "read" | "write" | "confirm";

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
  { key: "investments:read", i18n: "investments_read", group: "read" },
  { key: "activities:draft", i18n: "activities_draft", group: "write" },
  { key: "activities:write", i18n: "activities_write", group: "write" },
  { key: "ordinary:write", i18n: "ordinary_write", group: "write" },
  { key: "transfers:write", i18n: "transfers_write", group: "write" },
  { key: "holdings:write", i18n: "holdings_write", group: "write" },
  { key: "classification:suggest", i18n: "classification_suggest", group: "write" },
  { key: "classification:write", i18n: "classification_write", group: "write" },
  { key: "investments:write", i18n: "investments_write", group: "write" },
  { key: "investments:confirm", i18n: "investments_confirm", group: "confirm" },
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
// Presets leave formal writes and investment confirmation as separate opt-ins.
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
  "transfers:write",
  "holdings:write",
];

/**
 * The web host never grants ordinary:write — its PAT endpoint rejects it —
 * while the desktop host offers it for direct CLI/MCP writes.
 */
export const DESKTOP_ONLY_SCOPES: ScopeKey[] = [
  "ordinary:write",
  "transfers:write",
  "holdings:write",
  "investments:read",
  "investments:write",
  "investments:confirm",
];

export const WEB_UNAVAILABLE_SCOPES: ScopeKey[] = [
  "activities:write",
  "classification:write",
  "ordinary:write",
  "transfers:write",
  "holdings:write",
  "investments:read",
  "investments:write",
  "investments:confirm",
];

/** Scopes hidden from the create dialog for the current host. */
export function unavailableScopesForHost(isDesktop: boolean): ScopeKey[] {
  return isDesktop
    ? UNAVAILABLE_SCOPES.filter(
        (scope) => !["ordinary:write", "transfers:write", "holdings:write"].includes(scope),
      )
    : WEB_UNAVAILABLE_SCOPES;
}

/** Whether a token that carries this scope can use it on the current host. */
export function scopeAvailableOnHost(scope: string, isDesktop: boolean): boolean {
  return !DESKTOP_ONLY_SCOPES.includes(scope as ScopeKey) || isDesktop;
}

/** New grants follow host capabilities; stored token scopes are never expanded. */
export function readScopesForHost(isDesktop: boolean): ScopeKey[] {
  return READ_SCOPES.filter((scope) => scopeAvailableOnHost(scope, isDesktop));
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
  if (set.has("investments:confirm")) set.add("investments:write");
  if (set.has("investments:write")) set.add("investments:read");
  if (set.has("investments:read")) {
    set.add("accounts:read");
    set.add("holdings:read");
    set.add("activities:read");
  }

  if (set.has("activities:write")) {
    set.add("activities:draft");
  }
  if (set.has("classification:write")) {
    set.add("classification:suggest");
  }
  if (set.has("ordinary:write") || set.has("transfers:write") || set.has("holdings:write")) {
    set.add("activities:read");
    set.add("accounts:read");
  }
  if (set.has("holdings:write")) set.add("holdings:read");
  return SCOPES.filter((scope) => set.has(scope.key)).map((scope) => scope.key);
}

/** Returns the preset whose scope set exactly matches `scopes`, if any. */
export function matchPreset(scopes: string[]): ScopePreset | undefined {
  const target = new Set(scopes);
  // Old read presets retain their label and exact grants when a new module ships.
  return SCOPE_PRESETS.flatMap((preset) => [
    preset,
    { ...preset, scopes: preset.scopes.filter((scope) => scope !== "investments:read") },
  ]).find(
    (preset) =>
      preset.scopes.length === target.size && preset.scopes.every((scope) => target.has(scope)),
  );
}
