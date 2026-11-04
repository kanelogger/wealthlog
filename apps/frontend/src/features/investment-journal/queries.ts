import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { selectedProfileId } from "@/features/profiles/session";
import { executeInvestmentJournal, type InvestmentJournalCommand } from "@/adapters";

export function journalKey() {
  return ["investment-journal", selectedProfileId() ?? "unbound"] as const;
}
export function useJournalQuery<T>(
  command: InvestmentJournalCommand,
  args: Record<string, unknown> = {},
  enabled = true,
  poll?: (data: T | undefined) => number | false,
) {
  return useQuery<T>({
    queryKey: [...journalKey(), command, args],
    queryFn: () => executeInvestmentJournal<T>(command, args),
    enabled,
    retry: false,
    staleTime: 0,
    refetchOnWindowFocus: false,
    refetchInterval: (query) => poll?.(query.state.data) ?? false,
  });
}
export function useInvalidateJournal() {
  const client = useQueryClient();
  const profile = selectedProfileId() ?? "unbound";
  return useCallback(
    () => client.invalidateQueries({ queryKey: ["investment-journal", profile] }),
    [client, profile],
  );
}
