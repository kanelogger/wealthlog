import { HoldingType, QuoteMode } from "@/lib/constants";
import type { Holding } from "@/lib/types";
import zhHoldings from "@/i18n/locales/zh/holdings.json";
import { describe, expect, it } from "vitest";
import {
  CASH_HOLDING_TYPE_KEY,
  filterHoldingsByType,
  getHoldingTypeFilterOption,
  getHoldingTypeFilterValue,
  getHoldingTypeTranslationKey,
} from "./holdings-type-filter";

function holding(
  id: string,
  holdingType: Holding["holdingType"],
  assetType?: { key: string; name: string },
): Holding {
  return {
    id,
    holdingType,
    accountId: "account-1",
    instrument: assetType
      ? {
          id: `asset-${id}`,
          symbol: id,
          currency: "USD",
          quoteMode: QuoteMode.MARKET,
          classifications: {
            assetType: {
              id: assetType.key,
              taxonomyId: "asset-type",
              name: assetType.name,
              key: assetType.key,
              color: "#000000",
              sortOrder: 0,
              createdAt: "2026-08-18",
              updatedAt: "2026-08-18",
            },
            assetClasses: [],
            sectors: [],
            regions: [],
            customGroups: [],
          },
        }
      : undefined,
    quantity: 1,
    localCurrency: "USD",
    baseCurrency: "USD",
    marketValue: { local: 100, base: 100 },
    weight: 1,
    asOfDate: "2026-08-18",
  };
}

const stock = holding("stock", HoldingType.SECURITY, { key: "STOCK_COMMON", name: "股票" });
const etf = holding("etf", HoldingType.SECURITY, { key: "ETF", name: "ETF" });
const cash = holding("cash", HoldingType.CASH);

describe("holdings type filters", () => {
  it("maps holdings to stable system taxonomy keys", () => {
    expect(getHoldingTypeFilterValue(cash)).toBe(CASH_HOLDING_TYPE_KEY);
    expect(getHoldingTypeFilterValue(stock)).toBe("STOCK_COMMON");
  });

  it("keeps the stored name as the localization fallback", () => {
    expect(getHoldingTypeFilterOption(stock, "现金")).toEqual({
      value: "STOCK_COMMON",
      fallbackLabel: "股票",
    });
    expect(getHoldingTypeTranslationKey("STOCK_COMMON")).toBe(
      "holdings:instrument_types.STOCK_COMMON",
    );
  });

  it("includes every holding when no asset type is selected", () => {
    expect(filterHoldingsByType([stock, etf, cash], [])).toEqual([stock, etf, cash]);
  });

  it("filters cash alongside taxonomy-backed asset types", () => {
    expect(filterHoldingsByType([stock, etf, cash], [CASH_HOLDING_TYPE_KEY])).toEqual([cash]);
    expect(filterHoldingsByType([stock, etf, cash], ["ETF", "STOCK_COMMON"])).toEqual([stock, etf]);
  });

  it("keeps cash balances distinct from securities classified as Cash Balance", () => {
    // "CASH" is a real instrument_type taxonomy key (the "现金余额" category), so
    // the synthetic cash pseudo-type must not reuse it.
    const cashBalanceFund = holding("mmf", HoldingType.SECURITY, {
      key: "CASH",
      name: "现金余额",
    });

    expect(getHoldingTypeFilterValue(cashBalanceFund)).not.toBe(CASH_HOLDING_TYPE_KEY);
    expect(filterHoldingsByType([cash, cashBalanceFund], [CASH_HOLDING_TYPE_KEY])).toEqual([cash]);
    expect(filterHoldingsByType([cash, cashBalanceFund], ["CASH"])).toEqual([cashBalanceFund]);
    expect(getHoldingTypeFilterOption(cashBalanceFund, "现金")).toEqual({
      value: "CASH",
      fallbackLabel: "现金余额",
    });
  });

  it("resolves the cash label outside the instrument type namespace", () => {
    expect(getHoldingTypeTranslationKey(CASH_HOLDING_TYPE_KEY)).toBe("holdings:cash");
    expect(getHoldingTypeTranslationKey("CASH")).toBe("holdings:instrument_types.CASH");
  });

  it("defines distinct Simplified Chinese labels for cash and the Cash Balance category", () => {
    expect(zhHoldings.instrument_types.CASH).toBeTruthy();
    expect(zhHoldings.instrument_types.CASH).not.toBe(zhHoldings.cash);
    expect(zhHoldings.instrument_types.STOCK_COMMON).toBe("股票");
  });
});
