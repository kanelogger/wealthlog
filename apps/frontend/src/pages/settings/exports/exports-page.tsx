import { Separator } from "@wealthfolio/ui/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@wealthfolio/ui/components/ui/tabs";
import { useTranslation } from "react-i18next";
import { SettingsHeader } from "../settings-header";
import { ExportForm } from "./exports-form";
import { LocalBackups } from "./local-backups";
export default function ExportSettingsPage() {
  const { t } = useTranslation();
  return (
    <div className="space-y-6">
      <SettingsHeader
        heading={t("settings:export_full_title")}
        text={t("settings:export_description")}
      />
      <Separator />
      <Tabs defaultValue="backup">
        <TabsList className="grid w-full grid-cols-2">
          <TabsTrigger value="backup">{t("settings:export_tab_backup")}</TabsTrigger>
          <TabsTrigger value="export">{t("settings:export_tab_export")}</TabsTrigger>
        </TabsList>
        <TabsContent value="backup" className="mt-6">
          <LocalBackups />
        </TabsContent>
        <TabsContent value="export" className="mt-6">
          <ExportForm />
        </TabsContent>
      </Tabs>
    </div>
  );
}
