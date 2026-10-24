import { Separator } from "@wealthfolio/ui/components/ui/separator";
import { useTranslation } from "react-i18next";
import { SettingsHeader } from "../settings-header";
import { BaseCurrencySettings } from "./currency-settings";
import { DatabaseEncryptionSettings } from "./database-encryption-settings";
import { ExchangeRatesSettings } from "./exchange-rates/exchange-rates-settings";

export default function GeneralSettingsPage() {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      <SettingsHeader
        heading={t("settings:general")}
        text={t("settings:general_page_description")}
      />
      <Separator />
      <BaseCurrencySettings />
      <div className="pt-6">
        <ExchangeRatesSettings />
      </div>
      <div className="pt-6">
        <DatabaseEncryptionSettings />
      </div>
    </div>
  );
}
