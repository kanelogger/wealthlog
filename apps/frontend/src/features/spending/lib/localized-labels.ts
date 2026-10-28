import type { TFunction } from "i18next";
import { getCategoryLabel } from "@/lib/taxonomy-labels";
import type { BudgetSnapshot, BudgetGroup } from "../types/budget";
import type { SpendingInsight } from "../types/insight";
import type { EventSpendingSummary } from "../types/event";

export function getBudgetGroupLabel(group: BudgetGroup, t: TFunction): string {
  if (!group.isSystem) return group.name;
  const key = `settings:budget_group_labels.${group.id}`;
  if (t(`${key}.source`, { defaultValue: group.name }) !== group.name) return group.name;
  return t(`${key}.label`, { defaultValue: group.name });
}

function category<T extends { taxonomyId: string; categoryId: string; name: string }>(
  row: T,
  t: TFunction,
): T {
  return {
    ...row,
    name: getCategoryLabel(
      { id: row.categoryId, taxonomyId: row.taxonomyId, name: row.name },
      true,
      t,
    ),
  };
}

export function localizeBudget(data: BudgetSnapshot, t: TFunction): BudgetSnapshot {
  const group = (row: BudgetGroup) => ({ ...row, name: getBudgetGroupLabel(row, t) });
  return {
    ...data,
    state: { ...data.state, groups: data.state.groups.map(group) },
    computed: {
      ...data.computed,
      groupRows: data.computed.groupRows.map((row) => ({
        ...row,
        group: group(row.group),
        categories: row.categories.map((row) => category(row, t)),
      })),
      ungroupedRows: data.computed.ungroupedRows.map((row) => category(row, t)),
      incomeRows: data.computed.incomeRows.map((row) => category(row, t)),
    },
  };
}

export function localizeSpendingInsight(data: SpendingInsight, t: TFunction): SpendingInsight {
  return {
    ...data,
    groups: data.groups.map((row) => ({
      ...row,
      group: { ...row.group, name: getBudgetGroupLabel(row.group, t) },
      categories: row.categories.map((row) => category(row, t)),
    })),
  };
}

export function localizeEventSummary(
  data: EventSpendingSummary,
  t: TFunction,
): EventSpendingSummary {
  return {
    ...data,
    eventTypeName: getEventTypeLabel({ id: data.eventTypeId, name: data.eventTypeName }, t),
    byCategory: Object.fromEntries(
      Object.entries(data.byCategory).map(([key, row]) => [
        key,
        {
          ...row,
          categoryName: row.categoryId
            ? getCategoryLabel(
                { id: row.categoryId, taxonomyId: "spending_categories", name: row.categoryName },
                true,
                t,
              )
            : row.categoryName === "Uncategorized"
              ? t("spending:eventsCard.uncategorized")
              : row.categoryName,
        },
      ]),
    ),
  };
}

export function getEventTypeLabel(type: { id: string; name: string }, t: TFunction): string {
  const key = `settings:event_type_labels.${type.id}`;
  if (t(`${key}.source`, { defaultValue: type.name }) !== type.name) return type.name;
  return t(`${key}.label`, { defaultValue: type.name });
}
