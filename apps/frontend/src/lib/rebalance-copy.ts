import type { TFunction } from "i18next";
import type { SuggestedManualTrade, RebalanceWarning } from "@/lib/types";

/** Render the current backend's structured reason templates without changing the plan. */
export function localizeTradeReason(
  trade: SuggestedManualTrade,
  category: string,
  t: TFunction,
): string {
  const text = trade.reason;
  let match = /^(Buy|Sell) (.+): (.+) (underweight|overweight) by (-?\d+) bps\.$/.exec(text);
  if (match)
    return t("allocation:reason.weight", {
      action: t(match[1] === "Buy" ? "allocation:trades.buy" : "allocation:trades.sell"),
      symbol: match[2],
      category,
      direction: t(
        match[4] === "underweight" ? "allocation:reason.below" : "allocation:reason.above",
      ),
      bps: match[5],
    });
  match = /^(Buy|Sell) (.+): (?:improves|reduces) (.+) drift\.$/.exec(text);
  if (match)
    return t("allocation:reason.drift", {
      action: t(match[1] === "Buy" ? "allocation:trades.buy" : "allocation:trades.sell"),
      symbol: match[2],
      category: match[3] === "portfolio" ? t("allocation:reason.portfolio") : category,
    });
  if (/^.+ is underweight — no classified holding available\. Allocate manually\.$/.test(text))
    return t("allocation:reason.manual", { category });
  return text;
}

export function localizeRebalanceWarning(
  warning: RebalanceWarning,
  categoryLabel: (id: string, name: string) => string,
  t: TFunction,
): string {
  const text = warning.message;
  let match: RegExpExecArray | null;
  switch (warning.kind) {
    case "no_buy_candidate":
      match = /^No classifiable holdings in (.+)\. Allocate (.+) to this category manually\.$/.exec(
        text,
      );
      if (match)
        return t("allocation:warningDetail.noBuy", {
          category: categoryLabel(warning.categoryId, match[1]),
          amount: match[2],
        });
      break;
    case "tagged_cash":
      match =
        /^(.+) contains (.+) of tagged cash\. It is excluded from cash to deploy; move or reclassify that cash before the plan can rebalance this sleeve\.$/.exec(
          text,
        );
      if (match)
        return t("allocation:warningDetail.taggedCash", {
          category: categoryLabel(warning.categoryId, match[1]),
          amount: match[2],
        });
      break;
    case "missing_quote":
      match = /^(.+): no valid price for whole-share mode\. Skipped\.$/.exec(text);
      if (match) return t("allocation:warningDetail.missingQuote", { symbol: match[1] });
      break;
    case "unclassified_asset":
      match =
        /^(.+) isn't classified for this target, so it's excluded from the plan\. Classify it to include it\.$/.exec(
          text,
        );
      if (match) return t("allocation:warningDetail.unclassified", { symbol: match[1] });
      break;
    case "partial_classification":
      match =
        /^(.+) is only partly classified, so only its known exposure counts toward the plan; the rest is ignored\.$/.exec(
          text,
        );
      if (match) return t("allocation:warningDetail.partial", { symbol: match[1] });
      break;
    case "constraint_skipped_sell":
      match = /^Skipped selling (.+): do-not-sell constraint\.$/.exec(text);
      if (match) return t("allocation:warningDetail.blockSell", { symbol: match[1] });
      match = /^Skipped selling (.+) from account: avoid-selling constraint\.$/.exec(text);
      if (match) return t("allocation:warningDetail.avoidSell", { symbol: match[1] });
      break;
    case "turnover_cap_reached":
      match = /^Turnover cap \((.+)%\) reached — (.+) of (.+) portfolio already sold\.$/.exec(text);
      if (match)
        return t("allocation:warningDetail.turnover", {
          percent: match[1],
          sold: match[2],
          total: match[3],
        });
      break;
  }
  return text;
}
