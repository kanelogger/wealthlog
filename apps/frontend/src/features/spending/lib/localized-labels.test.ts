import { expect, it } from "vitest";
import { createI18n } from "@/i18n/create-i18n";
import {
  getBudgetGroupLabel,
  getEventTypeLabel,
  localizeBudget,
  localizeSpendingInsight,
  localizeEventSummary,
} from "./localized-labels";
import type { BudgetGroup, BudgetSnapshot } from "../types/budget";
import type { SpendingInsight } from "../types/insight";
import type { EventSpendingSummary } from "../types/event";
const { t } = createI18n();
const group = {
  id: "032ecb02-5912-42e8-9724-2cd566fc08d5",
  isSystem: true,
  name: "Needs",
} as BudgetGroup;
const category = {
  taxonomyId: "spending_categories",
  categoryId: "cat_food",
  name: "Food & Dining",
  actual: 123.45,
};

it("localizes budgets while preserving user renames and amounts", () => {
  const data = {
    state: { groups: [group] },
    computed: { groupRows: [{ group, categories: [category] }], ungroupedRows: [], incomeRows: [] },
  } as unknown as BudgetSnapshot;
  const original = structuredClone(data);
  const result = localizeBudget(data, t);
  expect(result.state.groups[0].name).toBe("必要开支");
  expect(result.computed.groupRows[0].categories[0]).toMatchObject({
    name: "餐饮",
    actual: 123.45,
  });
  expect(data).toEqual(original);
  expect(getBudgetGroupLabel({ ...group, name: "My Needs" }, t)).toBe("My Needs");
  expect(getBudgetGroupLabel({ ...group, isSystem: false }, t)).toBe("Needs");
});

it("localizes insight and event categories without changing event names", () => {
  const insight = { groups: [{ group, categories: [category] }] } as unknown as SpendingInsight;
  expect(localizeSpendingInsight(insight, t).groups[0].categories[0].name).toBe("餐饮");
  const summary = {
    eventName: "My trip",
    eventTypeId: "255600dc-c2a3-47e0-979d-a602c21fc337",
    eventTypeName: "Travel",
    byCategory: { cat_food: { categoryId: "cat_food", categoryName: "Food & Dining", amount: 10 } },
  } as unknown as EventSpendingSummary;
  const result = localizeEventSummary(summary, t);
  expect(result.eventTypeName).toBe("旅行");
  expect(result.eventName).toBe("My trip");
  expect(result.byCategory.cat_food).toMatchObject({ categoryName: "餐饮", amount: 10 });
  expect(summary.byCategory.cat_food.categoryName).toBe("Food & Dining");
  const uncategorized = {
    ...summary,
    byCategory: {
      unknown: { ...summary.byCategory.cat_food, categoryId: null, categoryName: "Uncategorized" },
    },
  };
  expect(localizeEventSummary(uncategorized, t).byCategory.unknown.categoryName).toBe("未归类");
  expect(getEventTypeLabel({ id: summary.eventTypeId, name: "My Travel" }, t)).toBe("My Travel");
  expect(getEventTypeLabel({ id: "custom", name: "Travel" }, t)).toBe("Travel");
});
