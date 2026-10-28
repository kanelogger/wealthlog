import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { localizeDriftReport } from "@/lib/taxonomy-labels";
import { useQuery } from "@tanstack/react-query";
import { getAllocationTargetDrift } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { AccountScope, DriftReport } from "@/lib/types";

interface UseAllocationTargetDriftOptions {
  includeHoldings?: boolean;
  taxonomyId?: string;
}

export function useAllocationTargetDrift(
  targetId: string | null,
  scope: AccountScope,
  options?: UseAllocationTargetDriftOptions,
) {
  const { t } = useTranslation();
  const taxonomyId = options?.taxonomyId;
  const select = useCallback(
    (data: DriftReport | null) =>
      data && taxonomyId ? localizeDriftReport(data, taxonomyId, t) : data,
    [taxonomyId, t],
  );
  const includeHoldings = options?.includeHoldings ?? false;
  const {
    data: driftReport,
    dataUpdatedAt,
    isLoading,
    isError,
  } = useQuery<DriftReport | null, Error>({
    queryKey: QueryKeys.allocationTargetDrift(targetId ?? "", scope, includeHoldings),
    queryFn: () => getAllocationTargetDrift(targetId!, scope, { includeHoldings }),
    enabled: !!targetId,
    select,
  });

  return { driftReport: driftReport ?? null, dataUpdatedAt, isLoading, isError };
}
