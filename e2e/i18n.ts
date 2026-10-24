import { createI18n } from "../apps/frontend/src/i18n/create-i18n";

const i18n = createI18n();

export function t(key: string, options?: Record<string, unknown>) {
  return i18n.t(key, options);
}

export { i18n };
