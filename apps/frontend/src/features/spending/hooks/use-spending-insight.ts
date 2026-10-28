import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { localizeSpendingInsight } from "../lib/localized-labels";
import { useQuery } from "@tanstack/react-query";

import { QueryKeys } from "@/lib/query-keys";

import { getSpendingInsight } from "../adapters/insight";
import type { SpendingInsight, SpendingInsightRequest } from "../types/insight";

export function useSpendingInsight(request: SpendingInsightRequest, enabled = true) {
  const { t } = useTranslation();
  const select = useCallback((data: SpendingInsight) => localizeSpendingInsight(data, t), [t]);
  return useQuery<SpendingInsight, Error>({
    queryKey: [QueryKeys.SPENDING_INSIGHT, request],
    queryFn: () => getSpendingInsight(request),
    enabled,
    select,
  });
}
