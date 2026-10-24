import { exportDataFile, logger } from "@/adapters";
import { ExportDataType, ExportedFileFormat } from "@/lib/types";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";

interface ExportParams {
  format: ExportedFileFormat;
  data: ExportDataType;
}

interface FileExportResult {
  filename?: string;
}

type ExportMutationResult = FileExportResult | null;

const datasetLabelKeys: Record<ExportDataType, string> = {
  accounts: "settings:export_type_accounts",
  activities: "settings:export_type_activities",
  holdings: "common:holdings",
  "portfolio-history": "settings:export_type_portfolio_history",
};

export function useExportData() {
  const { t } = useTranslation();
  const {
    mutateAsync: exportDataMutation,
    isPending: isExporting,
    variables: mutationVariables,
  } = useMutation<ExportMutationResult, Error, ExportParams>({
    mutationFn: async (params: ExportParams) => {
      const { format, data: desiredData } = params;
      const result = await exportDataFile(format, desiredData);
      if (result.status === "empty") {
        toast({
          title: t("settings:export_nothing"),
          description: t("settings:export_nothing_description", {
            name: t(datasetLabelKeys[desiredData]),
          }),
        });
        return null;
      }

      if (result.status === "canceled") {
        return null;
      }

      return { filename: result.filename };
    },
    onSuccess: (result) => {
      if (!result) {
        // User cancelled the operation, don't show any message
        return;
      }

      toast({
        title: t("settings:export_completed"),
        description: t("settings:export_completed_description"),
        variant: "success",
      });
    },
    onError: (e) => {
      logger.error(`Error while exporting: ${String(e)}`);
      toast({
        title: t("settings:export_failed"),
        description: e.message || t("settings:export_failed_description"),
        variant: "destructive",
      });
    },
  });

  const exportData = async (params: ExportParams) => {
    try {
      await exportDataMutation(params);
    } catch (error) {
      logger.error(`Error while exporting: ${String(error)}`);
    }
  };

  return {
    exportData,
    isExporting,
    exportingFormat: isExporting ? mutationVariables?.format : null,
    exportingData: isExporting ? mutationVariables?.data : null,
  };
}
