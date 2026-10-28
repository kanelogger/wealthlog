import { expect, it } from "vitest";
import { createI18n } from "@/i18n/create-i18n";
import type {
  PortfolioAllocations,
  TaxonomyAllocation,
  Holding,
  DriftReport,
  RebalancePlan,
} from "./types";
import {
  getTaxonomyLabel,
  localizePortfolioAllocations,
  localizeHolding,
  localizeDriftReport,
  localizeRebalancePlan,
} from "./taxonomy-labels";
const { t } = createI18n();

it("keeps renamed taxonomies and translates defaults", () => {
  expect(getTaxonomyLabel({ id: "asset_classes", name: "Asset Classes" }, t)).toBe("资产类别");
  expect(getTaxonomyLabel({ id: "asset_classes", name: "My assets" }, t)).toBe("My assets");
});

it("localizes nested allocations, residual and unknown labels without changing amounts or IDs", () => {
  const allocation: TaxonomyAllocation = {
    taxonomyId: "asset_classes",
    taxonomyName: "Asset Classes",
    color: "#111111",
    categories: [
      {
        categoryId: "EQUITY",
        categoryName: "Equity",
        color: "#111111",
        value: 100,
        percentage: 50,
        children: [
          {
            categoryId: "EQUITY_PUBLIC",
            categoryName: "Public Stocks",
            color: "#111111",
            value: 60,
            percentage: 30,
          },
          {
            categoryId: "EQUITY__RESIDUAL__",
            categoryName: "Other Equity",
            isResidual: true,
            color: "#111111",
            value: 40,
            percentage: 20,
          },
        ],
      },
      {
        categoryId: "__UNKNOWN__",
        categoryName: "Unknown",
        color: "#111111",
        value: 100,
        percentage: 50,
      },
    ],
  };
  const data: PortfolioAllocations = {
    assetClasses: allocation,
    sectors: allocation,
    regions: allocation,
    riskCategory: allocation,
    securityTypes: allocation,
    customGroups: [
      {
        ...allocation,
        taxonomyId: "user-defined",
        taxonomyName: "My Group",
        categories: [{ ...allocation.categories[0], children: undefined }],
      },
    ],
    totalValue: 200,
  };
  const original = structuredClone(data);
  const result = localizePortfolioAllocations(data, t);
  expect(result.assetClasses.taxonomyName).toBe("资产类别");
  expect(result.assetClasses.categories[0]).toMatchObject({
    categoryId: "EQUITY",
    categoryName: "权益类资产",
    value: 100,
    percentage: 50,
  });
  expect(result.assetClasses.categories[0].children?.map((child) => child.categoryName)).toEqual([
    "上市股票",
    "其他权益类资产",
  ]);
  expect(result.assetClasses.categories[1].categoryName).toBe("未分类");
  expect(result.customGroups[0].categories[0].categoryName).toBe("Equity");
  expect(data).toEqual(original);
});

it("localizes holding classifications including top-level references without changing keys", () => {
  const holding = {
    id: "h",
    instrument: {
      id: "AAPL",
      symbol: "AAPL",
      classifications: {
        assetType: {
          id: "STOCK_PREFERRED",
          taxonomyId: "instrument_type",
          name: "Preferred Stock",
          key: "STOCK_PREFERRED",
        },
        assetClasses: [
          {
            category: { id: "EQUITY_PUBLIC", taxonomyId: "asset_classes", name: "Public Stocks" },
            topLevelCategory: { id: "EQUITY", name: "Equity" },
            weight: 100,
          },
        ],
        sectors: [],
        regions: [],
        customGroups: [],
      },
    },
  } as unknown as Holding;
  const result = localizeHolding(holding, t);
  expect(result.instrument?.classifications?.assetType).toMatchObject({
    name: "优先股",
    key: "STOCK_PREFERRED",
  });
  expect(result.instrument?.classifications?.assetClasses[0].topLevelCategory.name).toBe(
    "权益类资产",
  );
  expect(holding.instrument?.classifications?.assetType?.name).toBe("Preferred Stock");
});

it("localizes drift rows and nested holdings without changing the report", () => {
  const data = {
    rows: [{ categoryId: "EQUITY", categoryName: "Equity", driftBps: 200 }],
    holdings: { rows: [{ categoryId: "__UNKNOWN__", categoryName: "Unknown" }] },
  } as DriftReport;
  const result = localizeDriftReport(data, "asset_classes", t);
  expect(result.rows[0]).toMatchObject({ categoryName: "权益类资产", driftBps: 200 });
  expect(result.holdings?.rows[0].categoryName).toBe("未分类");
  expect(data.rows[0].categoryName).toBe("Equity");
});

it("localizes plan categories and explanatory text without changing trades", () => {
  const data = {
    trades: [
      {
        action: "buy",
        categoryId: "EQUITY",
        categoryName: "Equity",
        estimatedAmount: 100,
        reason: "Buy AAPL: Equity underweight by 200 bps.",
      },
    ],
    warnings: [
      {
        kind: "no_buy_candidate",
        categoryId: "EQUITY",
        message: "No classifiable holdings in Equity. Allocate 200.00 to this category manually.",
      },
    ],
  } as RebalancePlan;
  const result = localizeRebalancePlan(data, "asset_classes", t);
  expect(result.trades[0]).toMatchObject({
    action: "buy",
    estimatedAmount: 100,
    categoryName: "权益类资产",
    reason: "买入 AAPL：权益类资产低于目标 200 个基点。",
  });
  expect(result.warnings[0].message).toBe(
    "权益类资产中没有可分类持仓，请手动向该类别配置 200.00。",
  );
  expect(data.trades[0].categoryName).toBe("Equity");
});
