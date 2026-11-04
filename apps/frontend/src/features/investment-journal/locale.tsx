import { useMemo, type ReactNode } from "react";
import { usePersistentState } from "@/hooks/use-persistent-state";
import {
  JournalLocale,
  journalTranslations,
  type JournalLanguage,
  type Locale,
} from "./locale-context";
export function JournalLocaleProvider({ children }: { children: ReactNode }) {
  const [stored, setLanguage] = usePersistentState<JournalLanguage>(
    "investment-journal-language",
    "zh",
  );
  const language = stored === "en" ? "en" : "zh";
  const value = useMemo<Locale>(
    () => ({
      language,
      setLanguage,
      t: (key, values) => journalTranslations.getFixedT(language)(key, values),
    }),
    [language, setLanguage],
  );
  return <JournalLocale.Provider value={value}>{children}</JournalLocale.Provider>;
}
