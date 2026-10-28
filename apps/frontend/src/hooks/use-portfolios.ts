import { createPortfolio, deletePortfolio, getPortfolios, updatePortfolioEntry } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import { invalidatePerformanceCaches } from "@/lib/performance-cache";
import type { NewPortfolio, PortfolioWithAccounts } from "@/lib/types";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

export function usePortfolios() {
  return useQuery<PortfolioWithAccounts[], Error>({
    queryKey: [QueryKeys.PORTFOLIOS],
    queryFn: getPortfolios,
  });
}

export function usePortfolioMutations() {
  const queryClient = useQueryClient();

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: [QueryKeys.PORTFOLIOS] });
    invalidatePerformanceCaches(queryClient);
  };

  const createMutation = useMutation({
    mutationFn: (portfolio: NewPortfolio) => createPortfolio(portfolio),
    onSuccess: () => {
      invalidate();
      toast.success("投资组合创建成功。");
    },
    onError: () => toast.error("创建投资组合失败。"),
  });

  const updateMutation = useMutation({
    mutationFn: (portfolio: PortfolioWithAccounts) => updatePortfolioEntry(portfolio),
    onSuccess: () => {
      invalidate();
      toast.success("投资组合更新成功。");
    },
    onError: () => toast.error("更新投资组合失败。"),
  });

  const deleteMutation = useMutation({
    mutationFn: (portfolioId: string) => deletePortfolio(portfolioId),
    onSuccess: () => {
      invalidate();
      toast.success("投资组合删除成功。");
    },
    onError: () => toast.error("删除投资组合失败。"),
  });

  return { createMutation, updateMutation, deleteMutation };
}
