import { describe, expect, it } from "vitest";
import { createI18n } from "@/i18n/create-i18n";
import type { RebalanceWarning, SuggestedManualTrade } from "./types";
import { localizeTradeReason, localizeRebalanceWarning } from "./rebalance-copy";
const { t } = createI18n();

describe("rebalance explanation templates", () => {
  it.each([
    ["Buy AAPL: Equity underweight by 200 bps.", "买入 AAPL：权益类资产低于目标 200 个基点。"],
    ["Sell AAPL: Equity overweight by 300 bps.", "卖出 AAPL：权益类资产高于目标 300 个基点。"],
    ["Buy AAPL: improves Equity drift.", "买入 AAPL：减小权益类资产的配置偏离。"],
    ["Sell AAPL: reduces Equity drift.", "卖出 AAPL：减小权益类资产的配置偏离。"],
    ["Buy AAPL: improves portfolio drift.", "买入 AAPL：减小投资组合的配置偏离。"],
    ["Sell AAPL: reduces portfolio drift.", "卖出 AAPL：减小投资组合的配置偏离。"],
    [
      "Equity is underweight — no classified holding available. Allocate manually.",
      "权益类资产低于目标，当前没有已分类持仓，请手动配置。",
    ],
  ])("translates %s", (reason, expected) => {
    expect(localizeTradeReason({ reason } as SuggestedManualTrade, "权益类资产", t)).toBe(expected);
  });

  it.each<[RebalanceWarning["kind"], string, string]>([
    [
      "no_buy_candidate",
      "No classifiable holdings in Equity. Allocate 123.45 to this category manually.",
      "权益类资产中没有可分类持仓，请手动向该类别配置 123.45。",
    ],
    [
      "tagged_cash",
      "Equity contains 123.45 of tagged cash. It is excluded from cash to deploy; move or reclassify that cash before the plan can rebalance this sleeve.",
      "权益类资产中有 123.45 已分类现金，未计入可投入现金；请先转移或重新分类这部分现金，再调整该类别。",
    ],
    [
      "missing_quote",
      "AAPL: no valid price for whole-share mode. Skipped.",
      "AAPL：整股模式缺少有效价格，已跳过。",
    ],
    [
      "unclassified_asset",
      "AAPL isn't classified for this target, so it's excluded from the plan. Classify it to include it.",
      "AAPL尚未按此目标分类，已从计划中排除；完成分类后即可纳入。",
    ],
    [
      "partial_classification",
      "AAPL is only partly classified, so only its known exposure counts toward the plan; the rest is ignored.",
      "AAPL仅完成部分分类，计划只计入已知配置，其余部分已忽略。",
    ],
    [
      "constraint_skipped_sell",
      "Skipped selling AAPL: do-not-sell constraint.",
      "已跳过卖出 AAPL：设置了禁止卖出约束。",
    ],
    [
      "constraint_skipped_sell",
      "Skipped selling AAPL from account: avoid-selling constraint.",
      "已跳过从该账户卖出 AAPL：设置了避免卖出约束。",
    ],
    [
      "turnover_cap_reached",
      "Turnover cap (20.0%) reached — 200.00 of 1000.00 portfolio already sold.",
      "已达到 20.0% 的换手上限，投资组合总额 1000.00 中已卖出 200.00。",
    ],
  ])("translates %s and retains its details", (kind, message, expected) => {
    expect(
      localizeRebalanceWarning({ kind, categoryId: "EQUITY", message }, () => "权益类资产", t),
    ).toBe(expected);
  });

  it("keeps unfamiliar text intact so no backend detail is discarded", () => {
    expect(
      localizeTradeReason({ reason: "New explanation" } as SuggestedManualTrade, "权益类资产", t),
    ).toBe("New explanation");
    expect(
      localizeRebalanceWarning(
        { kind: "missing_quote", categoryId: "EQUITY", message: "New detail" },
        () => "权益类资产",
        t,
      ),
    ).toBe("New detail");
  });
});
