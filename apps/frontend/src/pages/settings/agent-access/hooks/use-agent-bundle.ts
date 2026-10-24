import { getAgentBundleStatus, updateAgentBundle } from "@/adapters";
import type { AgentBundleAction, AgentBundleTool } from "@/adapters";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect } from "react";

export function useAgentBundle() {
  const query = useQuery({
    queryKey: ["agent-bundle"],
    queryFn: getAgentBundleStatus,
    retry: false,
  });
  const { refetch } = query;

  useEffect(() => {
    const refresh = () => {
      if (document.visibilityState === "visible" && document.hasFocus()) {
        void refetch();
      }
    };
    const timer = window.setInterval(refresh, 2000);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [refetch]);

  const mutation = useMutation({
    mutationFn: ({ action, tool }: { action: AgentBundleAction; tool?: AgentBundleTool }) =>
      updateAgentBundle(action, tool),
    onSettled: () => refetch(),
  });
  return { data: query.data, error: query.error, isLoading: query.isLoading, mutation };
}
