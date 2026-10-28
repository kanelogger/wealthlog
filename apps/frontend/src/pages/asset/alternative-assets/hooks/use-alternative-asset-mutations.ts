import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";
import {
  createAlternativeAsset,
  updateAlternativeAssetValuation,
  updateAlternativeAssetMetadata,
  deleteAlternativeAsset,
  linkLiability,
  unlinkLiability,
} from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import { invalidatePerformanceCaches } from "@/lib/performance-cache";
import { logger } from "@/adapters";
import type {
  CreateAlternativeAssetRequest,
  CreateAlternativeAssetResponse,
  UpdateValuationRequest,
  LinkLiabilityRequest,
} from "@/lib/types";
import type { LoanSetup } from "@/adapters/shared/alternative-assets";
import { isLoanRefusal } from "../components/loan-error-text";

interface UseAlternativeAssetMutationsOptions {
  onCreateSuccess?: (response: CreateAlternativeAssetResponse) => void;
  onUpdateSuccess?: () => void;
  onDeleteSuccess?: () => void;
  onMetadataUpdateSuccess?: () => void;
}

export async function invalidateAlternativeAssetQueries(queryClient: QueryClient): Promise<void> {
  invalidatePerformanceCaches(queryClient);
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: [QueryKeys.HOLDINGS] }),
    queryClient.invalidateQueries({ queryKey: [QueryKeys.ACCOUNTS] }),
    queryClient.invalidateQueries({ queryKey: [QueryKeys.NET_WORTH] }),
    queryClient.invalidateQueries({ queryKey: [QueryKeys.NET_WORTH_HISTORY] }),
    queryClient.invalidateQueries({ queryKey: [QueryKeys.ALTERNATIVE_HOLDINGS] }),
    queryClient.invalidateQueries({ queryKey: [QueryKeys.ASSET_DATA] }),
  ]);
}

export function useAlternativeAssetMutations(options: UseAlternativeAssetMutationsOptions = {}) {
  const queryClient = useQueryClient();

  const invalidateQueries = () => invalidateAlternativeAssetQueries(queryClient);

  const createMutation = useMutation({
    mutationFn: (request: CreateAlternativeAssetRequest) => createAlternativeAsset(request),
    onSuccess: async (response) => {
      await invalidateQueries();
      toast({
        title: "资产创建成功",
        variant: "success",
      });
      options.onCreateSuccess?.(response);
    },
    onError: (error) => {
      logger.error(`Error creating alternative asset: ${error}`);
      toast({
        title: "创建资产失败",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
    },
  });

  const updateValuationMutation = useMutation({
    mutationFn: ({ assetId, request }: { assetId: string; request: UpdateValuationRequest }) =>
      updateAlternativeAssetValuation(assetId, request),
    onSuccess: async () => {
      await invalidateQueries();
      toast({
        title: "估值更新成功",
        variant: "success",
      });
      options.onUpdateSuccess?.();
    },
    onError: (error) => {
      logger.error(`Error updating valuation: ${error}`);
      toast({
        title: "更新估值失败",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (assetId: string) => deleteAlternativeAsset(assetId),
    onSuccess: async () => {
      await invalidateQueries();
      toast({
        title: "资产删除成功",
        variant: "success",
      });
      options.onDeleteSuccess?.();
    },
    onError: (error) => {
      logger.error(`Error deleting alternative asset: ${error}`);
      toast({
        title: "删除资产失败",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
    },
  });

  const linkLiabilityMutation = useMutation({
    mutationFn: ({
      liabilityId,
      request,
    }: {
      liabilityId: string;
      request: LinkLiabilityRequest;
    }) => linkLiability(liabilityId, request),
    onSuccess: async () => {
      await invalidateQueries();
      toast({
        title: "负债关联成功",
        variant: "success",
      });
    },
    onError: (error) => {
      logger.error(`Error linking liability: ${error}`);
      toast({
        title: "关联负债失败",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
    },
  });

  const unlinkLiabilityMutation = useMutation({
    mutationFn: (liabilityId: string) => unlinkLiability(liabilityId),
    onSuccess: async () => {
      await invalidateQueries();
      toast({
        title: "负债关联已取消",
        variant: "success",
      });
    },
    onError: (error) => {
      logger.error(`Error unlinking liability: ${error}`);
      toast({
        title: "取消负债关联失败",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
    },
  });

  const updateMetadataMutation = useMutation({
    mutationFn: ({
      assetId,
      metadata,
      name,
      notes,
      loan,
    }: {
      assetId: string;
      metadata: Record<string, string>;
      name?: string;
      notes?: string | null;
      loan?: LoanSetup;
    }) => updateAlternativeAssetMetadata(assetId, metadata, name, notes, loan),
    onSuccess: async () => {
      await invalidateQueries();
      options.onMetadataUpdateSuccess?.();
    },
    onError: (error) => {
      logger.error(`Error updating asset metadata: ${error}`);
      // The details sheet explains a refused loan setup itself.
      if (isLoanRefusal(error)) return;
      toast({
        title: "保存详情失败",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
    },
  });

  return {
    createMutation,
    updateValuationMutation,
    updateMetadataMutation,
    deleteMutation,
    linkLiabilityMutation,
    unlinkLiabilityMutation,
  };
}
