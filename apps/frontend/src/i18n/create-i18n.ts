import i18next, { type i18n as I18nInstance, type TOptions } from "i18next";
import { initReactI18next } from "react-i18next";

import { DEFAULT_NAMESPACE, NAMESPACES, UI_LANGUAGE } from "./locales";
import { resources } from "./resources";

export interface CreateI18nOptions {
  saveMissing?: boolean;
  onMissingKey?: (namespace: string, key: string) => void;
}

/** Shared init for the app, Vitest, and Playwright. Each caller owns its instance. */
export function createI18n(options: CreateI18nOptions = {}): I18nInstance {
  const instance = i18next.createInstance();
  instance.use(initReactI18next);
  instance.init({
    lng: UI_LANGUAGE,
    fallbackLng: false,
    supportedLngs: [UI_LANGUAGE],
    load: "currentOnly",
    ns: [...NAMESPACES],
    defaultNS: DEFAULT_NAMESPACE,
    resources,
    initAsync: false,
    interpolation: { escapeValue: false },
    returnNull: false,
    react: { useSuspense: false },
    saveMissing: options.saveMissing ?? false,
    missingKeyHandler: options.saveMissing
      ? (_languages, namespace, key, _fallbackValue, _updateMissing, tOptions) => {
          const optionsRecord = tOptions as TOptions | undefined;
          if (optionsRecord && optionsRecord.defaultValue != null) return;
          options.onMissingKey?.(namespace, key);
        }
      : undefined,
  });
  return instance;
}
