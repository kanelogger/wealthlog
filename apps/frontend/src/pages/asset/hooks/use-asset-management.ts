import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { logger, createAsset, deleteAsset, updateAssetProfile } from "@/adapters";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";
import { QueryKeys } from "@/lib/query-keys";
import { NewAsset, UpdateAssetProfile } from "@/lib/types";

interface UpdateAssetArgs {
  payload: UpdateAssetProfile;
}

export const useAssetManagement = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation("asset");

  const invalidateCaches = (assetId: string) => {
    queryClient.invalidateQueries({ queryKey: [QueryKeys.ASSETS] });
    queryClient.invalidateQueries({ queryKey: [QueryKeys.ASSET_DATA, assetId] });
    queryClient.invalidateQueries({ queryKey: [QueryKeys.HOLDINGS] });
    queryClient.invalidateQueries({ queryKey: [QueryKeys.ACTIVITY_DATA] });
    queryClient.invalidateQueries({ queryKey: [QueryKeys.CURRENT_VALUATION] });
  };

  const createAssetMutation = useMutation({
    mutationFn: (payload: NewAsset) => createAsset(payload),
    onSuccess: (asset) => {
      invalidateCaches(asset.id);
      toast({
        title: t("createSecurity.created_title"),
        description: t("createSecurity.created_description", {
          name: asset.displayCode ?? asset.name ?? t("createSecurity.created_fallback_name"),
        }),
        variant: "success",
      });
    },
    onError: (error) => {
      logger.error(`Error creating asset: ${error}`);
      toast({
        title: t("createSecurity.create_failed_title"),
        description:
          error instanceof Error ? error.message : t("createSecurity.create_failed_description"),
        variant: "destructive",
      });
    },
  });

  const updateAssetMutation = useMutation({
    mutationFn: async ({ payload }: UpdateAssetArgs) => {
      return await updateAssetProfile(payload);
    },
    onSuccess: (asset) => {
      invalidateCaches(asset.id);
      queryClient.invalidateQueries({ queryKey: [QueryKeys.ASSET_LOGO_INDEX] });
      toast({
        title: t("createSecurity.updated_title"),
        description: t("createSecurity.updated_description"),
        variant: "success",
      });
    },
    onError: (error) => {
      logger.error(`Error updating asset: ${error}`);
      toast({
        title: t("createSecurity.update_failed_title"),
        description: t("createSecurity.update_failed_description"),
        variant: "destructive",
      });
    },
  });

  const deleteAssetMutation = useMutation({
    mutationFn: (assetId: string) => deleteAsset(assetId),
    onSuccess: (_, assetId) => {
      invalidateCaches(assetId);
      queryClient.invalidateQueries({ queryKey: [QueryKeys.ASSET_LOGO_INDEX] });
      toast({
        title: t("createSecurity.deleted_title"),
        description: t("createSecurity.deleted_description"),
        variant: "success",
      });
    },
    onError: (error) => {
      logger.error(`Error deleting asset: ${error}`);

      // Extract user-friendly error message
      const errorMessage = error instanceof Error ? error.message : String(error);

      // Check if it's the "asset has activities" error
      const isActivitiesError = errorMessage.toLowerCase().includes("existing activities");

      toast({
        title: t("createSecurity.delete_failed_title"),
        description: isActivitiesError
          ? t("createSecurity.delete_failed_has_activities")
          : errorMessage || t("createSecurity.delete_failed_description"),
        variant: "destructive",
      });
    },
  });

  return { createAssetMutation, updateAssetMutation, deleteAssetMutation };
};
