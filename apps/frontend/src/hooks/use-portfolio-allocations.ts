import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { localizePortfolioAllocations } from "@/lib/taxonomy-labels";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { AccountScope, PortfolioAllocations } from "@/lib/types";
import { getPortfolioAllocations } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";

interface UsePortfolioAllocationsOptions {
  keepPreviousData?: boolean;
}

export function usePortfolioAllocations(
  accountFilter: AccountScope,
  options: UsePortfolioAllocationsOptions = {},
) {
  const { t } = useTranslation();
  const select = useCallback(
    (data: PortfolioAllocations) => localizePortfolioAllocations(data, t),
    [t],
  );
  const isEnabled = accountFilter.type !== "account" || accountFilter.accountId.trim().length > 0;

  const {
    data: allocations,
    isLoading,
    isError,
    error,
  } = useQuery<PortfolioAllocations, Error>({
    queryKey: [QueryKeys.PORTFOLIO_ALLOCATIONS, accountFilter],
    queryFn: () => getPortfolioAllocations(accountFilter),
    enabled: isEnabled,
    select,
    placeholderData: options.keepPreviousData ? keepPreviousData : undefined,
  });

  return { allocations, isLoading, isError, error };
}
