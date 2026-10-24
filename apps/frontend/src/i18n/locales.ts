// Single source of truth for the only supported interface language.
export const SUPPORTED_LOCALES = [{ code: "zh", label: "简体中文" }] as const;

export type LocaleCode = (typeof SUPPORTED_LOCALES)[number]["code"];

/** Dictionary code used by i18next. */
export const UI_LANGUAGE = "zh" as const satisfies LocaleCode;

/** HTML language and display formatting locale. */
export const HTML_LANGUAGE = "zh-CN" as const;

/** Persisted formatting region returned for every ledger. */
export const FORMATTING_REGION = "CN" as const;

export const DEFAULT_LOCALE: LocaleCode = UI_LANGUAGE;

export const SUPPORTED_LOCALE_CODES = SUPPORTED_LOCALES.map((locale) => locale.code);

export const NAMESPACES = [
  "common",
  "dashboard",
  "holdings",
  "activity",
  "performance",
  "account",
  "settings",
  "income",
  "insights",
  "asset",
  "spending",
  "ui",
  "allocation",
  "onboarding",
  "auth",
  "health",
] as const;

export type Namespace = (typeof NAMESPACES)[number];

export const DEFAULT_NAMESPACE: Namespace = "common";
