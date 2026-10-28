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
      toast.success("规则已创建。");
    },
    onError: () => toast.error("创建规则失败。"),
  });

  const update = useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: UpdateCategorizationRule }) =>
      updateCategorizationRule(id, patch),
    onSuccess: () => {
      invalidate();
      toast.success("规则已更新。");
    },
    onError: () => toast.error("更新规则失败。"),
  });

  const remove = useMutation({
    mutationFn: (id: string) => deleteCategorizationRule(id),
    onSuccess: () => {
      invalidate();
      toast.success("规则已删除。");
    },
    onError: () => toast.error("删除规则失败。"),
  });

  const rerun = useMutation({
    mutationFn: (onlyUncategorized: boolean) => rerunCategorizationRules(onlyUncategorized),
    onSuccess: (count, onlyUncategorized) => {
      // Rerun touches every cash activity's category — refresh report/budget too.
      invalidateSpendingCaches(qc);
      const verb = onlyUncategorized ? "已分类" : "已重新分类";
      toast.success(`${verb} ${count} 笔活动。`);
    },
    onError: () => toast.error("重新执行规则失败。"),
  });

  return { create, update, remove, rerun };
}
