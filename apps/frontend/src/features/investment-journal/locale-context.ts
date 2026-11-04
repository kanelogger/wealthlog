import i18next from "i18next";
import { createContext, useContext } from "react";
import zh from "./locales/zh.json";
import en from "./locales/en.json";

export type JournalLanguage = "zh" | "en";
export type JournalTextKey = keyof typeof zh;
const english: Record<JournalTextKey, string> = en;
export const journalTranslations = i18next.createInstance();
void journalTranslations.init({
  lng: "zh",
  fallbackLng: false,
  supportedLngs: ["zh", "en"],
  initAsync: false,
  resources: { zh: { translation: zh }, en: { translation: english } },
  interpolation: { escapeValue: false },
  returnNull: false,
});
export interface Locale {
  language: JournalLanguage;
  setLanguage: (language: JournalLanguage) => void;
  t: (key: JournalTextKey, values?: Record<string, string | number>) => string;
}
export const JournalLocale = createContext<Locale | null>(null);
export function useJournalLocale() {
  const value = useContext(JournalLocale);
  if (!value) throw new Error("JournalLocaleProvider is required");
  return value;
}
