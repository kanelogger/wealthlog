import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { localizeHolding } from "@/lib/taxonomy-labels";
import { useQuery } from "@tanstack/react-query";
import { AccountScope, Holding, HoldingType } from "@/lib/types";
import { getHoldingsList } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";

interface UseHoldingsOptions {
  includeClosed?: boolean;
  enabled?: boolean;
  /** Investment positions exclude activity-derived cash balances. */
  excludeCash?: boolean;
}

interface UseHoldingsWithClosedProbeOptions {
  includeClosed: boolean;
  probeClosedWhenEmpty: boolean;
  excludeCash?: boolean;
}

export function useHoldings(accountFilter: AccountScope, options: UseHoldingsOptions = {}) {
  const { t } = useTranslation();
  const excludeCash = options.excludeCash ?? false;
  const select = useCallback(
    (data: Holding[]) =>
      data
        .filter((holding) => !excludeCash || holding.holdingType !== HoldingType.CASH)
        .map((holding) => localizeHolding(holding, t)),
    [excludeCash, t],
  );
  const includeClosed = options.includeClosed ?? false;
  const hasValidScope = (() => {
    switch (accountFilter.type) {
      case "account":
        return accountFilter.accountId.trim().length > 0;
      case "accounts":
        return accountFilter.accountIds.length > 0;
      case "portfolio":
        return accountFilter.portfolioId.trim().length > 0;
      case "all":
        return true;
      default:
        return false;
    }
  })();
  const isEnabled = hasValidScope && (options.enabled ?? true);

  const {
    data: holdings = [],
    dataUpdatedAt,
    isLoading,
    isError,
    error,
  } = useQuery<Holding[], Error>({
    queryKey: [QueryKeys.HOLDINGS, accountFilter, { includeClosed }],
    queryFn: () => getHoldingsList(accountFilter, { includeClosed }),
    enabled: isEnabled,
    select,
  });

  return { holdings, dataUpdatedAt, isLoading, isError, error };
}

export function useHoldingsWithClosedProbe(
  accountFilter: AccountScope,
  options: UseHoldingsWithClosedProbeOptions,
) {
  const primaryQuery = useHoldings(accountFilter, {
    includeClosed: options.includeClosed,
    excludeCash: options.excludeCash,
  });
  const shouldProbeClosedPositions =
    options.probeClosedWhenEmpty &&
    !options.includeClosed &&
    !primaryQuery.isLoading &&
    primaryQuery.holdings.length === 0;
  const closedProbeQuery = useHoldings(accountFilter, {
    includeClosed: true,
    enabled: shouldProbeClosedPositions,
    excludeCash: options.excludeCash,
  });

  return {
    ...primaryQuery,
    isLoading: primaryQuery.isLoading || (shouldProbeClosedPositions && closedProbeQuery.isLoading),
    hasHiddenClosedPositions: shouldProbeClosedPositions && closedProbeQuery.holdings.length > 0,
  };
}
