import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { QueryKeys } from "@/lib/query-keys";

import {
  createCategorizationRule,
  deleteCategorizationRule,
  listCategorizationRules,
  rerunCategorizationRules,
  updateCategorizationRule,
} from "../adapters/rules";
import { invalidateSpendingCaches } from "../lib/invalidation";
import type {
  CategorizationRule,
  NewCategorizationRule,
  UpdateCategorizationRule,
} from "../types/rule";

export function useCategorizationRules() {
  return useQuery<CategorizationRule[], Error>({
    queryKey: [QueryKeys.SPENDING_RULES],
    queryFn: listCategorizationRules,
  });
}

export function useCategorizationRuleMutations() {
  const qc = useQueryClient();
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: [QueryKeys.SPENDING_RULES] });
  };

  const create = useMutation({
    mutationFn: (rule: NewCategorizationRule) => createCategorizationRule(rule),
    onSuccess: () => {
      invalidate();
      toast.success("Rule created.");
    },
    onError: () => toast.error("Failed to create rule."),
  });

  const update = useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: UpdateCategorizationRule }) =>
      updateCategorizationRule(id, patch),
    onSuccess: () => {
      invalidate();
      toast.success("Rule updated.");
    },
    onError: () => toast.error("Failed to update rule."),
  });

  const remove = useMutation({
    mutationFn: (id: string) => deleteCategorizationRule(id),
    onSuccess: () => {
      invalidate();
      toast.success("Rule deleted.");
    },
    onError: () => toast.error("Failed to delete rule."),
  });

  const rerun = useMutation({
    mutationFn: (onlyUncategorized: boolean) => rerunCategorizationRules(onlyUncategorized),
    onSuccess: (count, onlyUncategorized) => {
      // Rerun touches every cash activity's category — refresh report/budget too.
      invalidateSpendingCaches(qc);
      const verb = onlyUncategorized ? "Categorized" : "Re-categorized";
      toast.success(`${verb} ${count} activit${count === 1 ? "y" : "ies"}.`);
    },
    onError: () => toast.error("Failed to re-run rules."),
  });

  return { create, update, remove, rerun };
}
