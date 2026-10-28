import { localizeTradeReason, localizeRebalanceWarning } from "./rebalance-copy";
import type { TFunction } from "i18next";
import type {
  Taxonomy,
  TaxonomyCategory,
  TaxonomyWithCategories,
  TaxonomyAllocation,
  CategoryAllocation,
  PortfolioAllocations,
  Holding,
  DriftReport,
  RebalancePlan,
} from "@/lib/types";

const SYSTEM_TAXONOMY_NAMES: Record<string, string> = {
  instrument_type: "Instrument Type",
  asset_classes: "Asset Classes",
  industries_gics: "Industries (GICS)",
  regions: "Regions",
  risk_category: "Risk Category",
  custom_groups: "Custom Groups",
  spending_categories: "Spending Categories",
  income_sources: "Income Sources",
  savings_categories: "Savings",
};

export function getTaxonomyLabel(
  taxonomy: Pick<Taxonomy, "id" | "name"> & { isSystem?: boolean },
  t: TFunction,
): string {
  if (taxonomy.isSystem === false || SYSTEM_TAXONOMY_NAMES[taxonomy.id] !== taxonomy.name)
    return taxonomy.name;
  return t(`settings:tax_labels.${taxonomy.id}`, { defaultValue: taxonomy.name });
}

export function getCategoryLabel(
  category: Pick<TaxonomyCategory, "id" | "taxonomyId" | "name">,
  isSystem: boolean,
  t: TFunction,
): string {
  if (!isSystem || category.taxonomyId === "custom_groups") return category.name;
  const key = `settings:tax_category_labels.${category.taxonomyId}.${category.id}`;
  const sourceName = t(`${key}.source`, { defaultValue: category.name });
  // Keep user renames, including renames of built-in categories.
  if (category.name !== sourceName) return category.name;
  return t(`${key}.label`, {
    defaultValue: category.name,
  });
}

export function getAllocationCategoryLabel(
  taxonomyId: string,
  categoryId: string,
  name: string,
  t: TFunction,
): string {
  if ((categoryId === "__UNKNOWN__" || categoryId === "unknown") && name === "Unknown") {
    return t("holdings:unknown");
  }
  return getCategoryLabel({ taxonomyId, id: categoryId, name }, true, t);
}

export function localizeTaxonomy(
  data: TaxonomyWithCategories,
  t: TFunction,
): TaxonomyWithCategories {
  return {
    taxonomy: { ...data.taxonomy, name: getTaxonomyLabel(data.taxonomy, t) },
    categories: data.categories.map((category) => ({
      ...category,
      name: getCategoryLabel(category, data.taxonomy.isSystem, t),
    })),
  };
}

function localizeAllocation(data: TaxonomyAllocation, t: TFunction): TaxonomyAllocation {
  function category(row: CategoryAllocation, parentName?: string): CategoryAllocation {
    const name =
      row.isResidual && parentName
        ? t("holdings:allocation_residual", { name: parentName })
        : getAllocationCategoryLabel(data.taxonomyId, row.categoryId, row.categoryName, t);
    return {
      ...row,
      categoryName: name,
      ...(row.children ? { children: row.children.map((child) => category(child, name)) } : {}),
    };
  }
  return {
    ...data,
    taxonomyName: getTaxonomyLabel({ id: data.taxonomyId, name: data.taxonomyName }, t),
    categories: data.categories.map((row) => category(row)),
  };
}

export function localizePortfolioAllocations(
  data: PortfolioAllocations,
  t: TFunction,
): PortfolioAllocations {
  return {
    ...data,
    assetClasses: localizeAllocation(data.assetClasses, t),
    sectors: localizeAllocation(data.sectors, t),
    regions: localizeAllocation(data.regions, t),
    riskCategory: localizeAllocation(data.riskCategory, t),
    securityTypes: localizeAllocation(data.securityTypes, t),
    customGroups: data.customGroups.map((group) => localizeAllocation(group, t)),
  };
}

export function localizeHolding(holding: Holding, t: TFunction): Holding {
  const data = holding.instrument?.classifications;
  if (!data) return holding;
  const category = (row: TaxonomyCategory) => ({ ...row, name: getCategoryLabel(row, true, t) });
  const groups = (rows: typeof data.assetClasses) =>
    rows.map((row) => ({
      ...row,
      category: category(row.category),
      topLevelCategory: {
        ...row.topLevelCategory,
        name: getCategoryLabel(
          { ...row.topLevelCategory, taxonomyId: row.category.taxonomyId },
          true,
          t,
        ),
      },
    }));
  return {
    ...holding,
    instrument: {
      ...holding.instrument!,
      classifications: {
        ...data,
        assetType: data.assetType ? category(data.assetType) : data.assetType,
        riskCategory: data.riskCategory ? category(data.riskCategory) : data.riskCategory,
        assetClasses: groups(data.assetClasses),
        sectors: groups(data.sectors),
        regions: groups(data.regions),
        customGroups: groups(data.customGroups),
      },
    },
  };
}

export function localizeDriftReport(
  data: DriftReport,
  taxonomyId: string,
  t: TFunction,
): DriftReport {
  const row = <T extends { categoryId: string; categoryName: string }>(value: T): T => ({
    ...value,
    categoryName: getAllocationCategoryLabel(taxonomyId, value.categoryId, value.categoryName, t),
  });
  return {
    ...data,
    rows: data.rows.map(row),
    holdings: data.holdings
      ? { ...data.holdings, rows: data.holdings.rows.map(row) }
      : data.holdings,
  };
}

export function localizeRebalancePlan(
  data: RebalancePlan,
  taxonomyId: string,
  t: TFunction,
): RebalancePlan {
  const categoryLabel = (id: string, name: string) =>
    getAllocationCategoryLabel(taxonomyId, id, name, t);
  return {
    ...data,
    trades: data.trades.map((trade) => {
      const categoryName = categoryLabel(trade.categoryId, trade.categoryName);
      return { ...trade, categoryName, reason: localizeTradeReason(trade, categoryName, t) };
    }),
    warnings: data.warnings.map((warning) => ({
      ...warning,
      message: localizeRebalanceWarning(warning, categoryLabel, t),
    })),
  };
}
