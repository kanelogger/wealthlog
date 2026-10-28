import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { localizeBudget, getBudgetGroupLabel } from "../lib/localized-labels";
import type { BudgetSnapshot } from "../types/budget";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { QueryKeys } from "@/lib/query-keys";

import {
  assignCategoryToGroup,
  copyBudgetTargets,
  createBudgetGroup,
  deleteBudgetGroup,
  deleteBudgetRolloverSetting,
  deleteBudgetTarget,
  getBudget,
  resetBudgetGroups,
  updateBudgetGroup,
  upsertBudgetRolloverSetting,
  upsertBudgetTarget,
} from "../adapters/budget";
import type {
  NewBudgetGroup,
  NewBudgetRolloverSetting,
  NewBudgetTarget,
  UpdateBudgetGroup,
} from "../types/budget";

export function useBudget(periodKey?: string) {
  const { t } = useTranslation();
  const select = useCallback((data: BudgetSnapshot) => localizeBudget(data, t), [t]);
  return useQuery({
    queryKey: [QueryKeys.SPENDING_BUDGET, periodKey ?? null],
    queryFn: () => getBudget(periodKey),
    select,
  });
}

export function useBudgetMutations(periodKey?: string) {
  const qc = useQueryClient();
  const { t } = useTranslation();
  // Budget edits affect report rollups (group→category mapping changes how
  // spending sums roll up), so blow both buckets.
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: [QueryKeys.SPENDING_BUDGET] });
    qc.invalidateQueries({ queryKey: [QueryKeys.SPENDING_REPORT] });
    qc.invalidateQueries({ queryKey: [QueryKeys.SPENDING_INSIGHT] });
  };

  const upsertTarget = useMutation({
    mutationFn: (target: NewBudgetTarget) => upsertBudgetTarget(target, periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("保存预算目标失败。"),
  });

  const removeTarget = useMutation({
    mutationFn: (id: string) => deleteBudgetTarget(id, periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("删除预算目标失败。"),
  });

  const upsertRollover = useMutation({
    mutationFn: (setting: NewBudgetRolloverSetting) =>
      upsertBudgetRolloverSetting(setting, periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("保存结转设置失败。"),
  });

  const removeRollover = useMutation({
    mutationFn: (id: string) => deleteBudgetRolloverSetting(id, periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("删除结转设置失败。"),
  });

  const createGroup = useMutation({
    mutationFn: (group: NewBudgetGroup) => createBudgetGroup(group, periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("创建预算分组失败。"),
  });

  const updateGroup = useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: UpdateBudgetGroup }) => {
      const raw = qc
        .getQueryData<BudgetSnapshot>([QueryKeys.SPENDING_BUDGET, periodKey ?? null])
        ?.state.groups.find((group) => group.id === id);
      const name = raw && patch.name === getBudgetGroupLabel(raw, t) ? raw.name : patch.name;
      return updateBudgetGroup(
        id,
        { ...patch, ...(patch.name !== undefined ? { name } : {}) },
        periodKey,
      );
    },
    onSuccess: invalidate,
    onError: () => toast.error("更新预算分组失败。"),
  });

  const removeGroup = useMutation({
    mutationFn: ({ id, reassignToGroupId }: { id: string; reassignToGroupId: string }) =>
      deleteBudgetGroup(id, reassignToGroupId, periodKey),
    onSuccess: invalidate,
    onError: (error) =>
      toast.error(
        error instanceof Error && error.message ? error.message : "Failed to delete budget group.",
      ),
  });

  const assignCategory = useMutation({
    mutationFn: ({ categoryId, groupId }: { categoryId: string; groupId: string }) =>
      assignCategoryToGroup(categoryId, groupId, periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("移动类别失败。"),
  });

  const resetGroups = useMutation({
    mutationFn: () => resetBudgetGroups(periodKey),
    onSuccess: invalidate,
    onError: () => toast.error("重置预算分组失败。"),
  });

  const copyFromMonth = useMutation({
    mutationFn: ({
      sourcePeriodKey,
      overwrite,
    }: {
      sourcePeriodKey: string;
      overwrite: boolean;
    }) => {
      const target = periodKey && periodKey !== "default" ? periodKey : sourcePeriodKey;
      return copyBudgetTargets(sourcePeriodKey, target, overwrite);
    },
    onSuccess: () => {
      invalidate();
      toast.success("计划已复制。");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : "Failed to copy plan.";
      toast.error(message);
    },
  });

  return {
    upsertTarget,
    removeTarget,
    upsertRollover,
    removeRollover,
    createGroup,
    updateGroup,
    removeGroup,
    assignCategory,
    resetGroups,
    copyFromMonth,
  };
}
