import { zhCN, type Locale } from "date-fns/locale";
import { useLocalizationSettings } from "../components/formatting-provider";
import {
  resolveFormattingLocale,
  type SupportedFormattingLocale,
} from "../lib/formatting";

export function dateFnsLocaleFor(locale: SupportedFormattingLocale): Locale {
  resolveFormattingLocale(locale);
  return zhCN;
}

export function useDateFnsLocale(): Locale {
  const { locale } = useLocalizationSettings();
  return dateFnsLocaleFor(locale);
}
