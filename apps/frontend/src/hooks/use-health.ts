import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { FixAction, HealthConfig, HealthStatus } from "@/lib/types";
import {
  dismissHealthIssue,
  executeHealthFix,
  getHealthConfig,
  getHealthStatus,
  restoreHealthIssue,
  runHealthChecks,
  updateHealthConfig,
} from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import { useAuth } from "@/context/auth-context";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";

/**
 * Hook for fetching health status.
 */
export function useHealthStatus() {
  const { isAuthenticated, statusLoading } = useAuth();

  return useQuery<HealthStatus, Error>({
    queryKey: [QueryKeys.HEALTH_STATUS],
    queryFn: getHealthStatus,
    enabled: !statusLoading && isAuthenticated,
    staleTime: 1000 * 60 * 5, // 5 minutes
  });
}

/**
 * Hook for running health checks.
 */
export function useRunHealthChecks(options?: { navigate?: (path: string) => void }) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: runHealthChecks,
    onSuccess: (data: HealthStatus) => {
      queryClient.setQueryData([QueryKeys.HEALTH_STATUS], data);
      const issueCount = data.issues?.length ?? 0;
      if (issueCount === 0) {
        toast.success("全部检查通过", { description: "未发现问题。" });
      } else {
        toast.error(`发现 ${issueCount} 个问题`, {
          description: "请在健康中心查看详情。",
          action: options?.navigate
            ? { label: "查看", onClick: () => options.navigate!("/health") }
            : undefined,
        });
      }
    },
    onError: (error: Error) => {
      toast.error("健康检查失败", { description: error.message });
    },
  });
}

/**
 * Hook for dismissing a health issue.
 */
export function useDismissHealthIssue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ issueId, dataHash }: { issueId: string; dataHash: string }) =>
      dismissHealthIssue(issueId, dataHash),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [QueryKeys.HEALTH_STATUS] });
      toast.success("已忽略问题");
    },
    onError: (error: Error) => {
      toast.error("忽略问题失败", { description: error.message });
    },
  });
}

/**
 * Hook for restoring a dismissed health issue.
 */
export function useRestoreHealthIssue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: restoreHealthIssue,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [QueryKeys.HEALTH_STATUS] });
      toast.success("已恢复问题");
    },
    onError: (error: Error) => {
      toast.error("恢复问题失败", { description: error.message });
    },
  });
}

/**
 * Hook for executing a fix action.
 */
export function useExecuteHealthFix() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (action: FixAction) => {
      // Execute the fix
      await executeHealthFix(action);
      if (action.id === "rebuild_account_history") {
        return { status: undefined, actionId: action.id };
      }
      // Run health checks to get fresh status
      const status = await runHealthChecks();
      return { status, actionId: action.id };
    },
    onSuccess: ({ status, actionId }) => {
      // Update health status with fresh data from health checks
      if (status) {
        queryClient.setQueryData([QueryKeys.HEALTH_STATUS], status);
      }
      // Invalidate holdings so related pages refresh
      queryClient.invalidateQueries({ queryKey: [QueryKeys.HOLDINGS] });
      queryClient.invalidateQueries({ queryKey: [QueryKeys.PORTFOLIO_ALLOCATIONS] });
      // Skip toast for sync actions — global event listeners handle feedback
      if (actionId === "rebuild_account_history") {
        toast.success("已开始重建历史数据", {
          description: "重新计算完成后将刷新健康状态。",
        });
      } else if (actionId !== "sync_prices" && actionId !== "retry_sync") {
        toast.success("修复成功");
      }
    },
    onError: (error: Error, action) => {
      if (action.id === "sync_prices" || action.id === "retry_sync") {
        toast.error("行情数据同步失败", {
          id: "market-sync-error",
          description: `${error.message}. Please try again later.`,
        });
      } else {
        toast.error("修复失败", { description: error.message });
      }
    },
  });
}

/**
 * Hook for fetching health configuration.
 */
export function useHealthConfig() {
  const { isAuthenticated, statusLoading } = useAuth();

  return useQuery<HealthConfig, Error>({
    queryKey: [QueryKeys.HEALTH_CONFIG],
    queryFn: getHealthConfig,
    enabled: !statusLoading && isAuthenticated,
  });
}

/**
 * Hook for updating health configuration.
 */
export function useUpdateHealthConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: updateHealthConfig,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [QueryKeys.HEALTH_CONFIG] });
      queryClient.invalidateQueries({ queryKey: [QueryKeys.HEALTH_STATUS] });
      toast.success("配置已更新");
    },
    onError: (error: Error) => {
      toast.error("更新配置失败", { description: error.message });
    },
  });
}
