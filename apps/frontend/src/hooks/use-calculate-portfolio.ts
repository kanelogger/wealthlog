import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";
import { updatePortfolio, recalculatePortfolio } from "@/adapters";
import { logger } from "@/adapters";
import { invalidatePerformanceCaches } from "@/lib/performance-cache";

export function useUpdatePortfolioMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: updatePortfolio,
    onSuccess: () => {
      invalidatePerformanceCaches(queryClient);
    },
    onError: (error) => {
      queryClient.invalidateQueries();
      toast({
        title: "更新投资组合数据失败。",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
      logger.error(`Error calculating historical data: ${String(error)}`);
    },
  });
}

export function useRecalculatePortfolioMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: recalculatePortfolio,
    onSuccess: () => {
      invalidatePerformanceCaches(queryClient);
    },
    onError: (error) => {
      queryClient.invalidateQueries();
      toast({
        title: "重新计算投资组合失败。",
        description: "请重试；若问题持续，请反馈问题。",
        variant: "destructive",
      });
      console.warn("Error recalculating portfolio:", error);
      logger.error(`Error recalculating portfolio: ${String(error)}`);
    },
  });
}
