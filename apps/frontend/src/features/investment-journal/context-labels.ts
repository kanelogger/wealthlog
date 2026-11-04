import type { ContextCapture, Resource } from "./types";
import type { JournalTextKey } from "./locale-context";

export function sourceLabel(capture: ContextCapture | undefined, id: string | null) {
  if (!id) return "";
  const data = capture?.response.funding.data;
  return (
    data?.decisionReferences?.accounts.find((item) => item.id === id)?.name ??
    data?.decisionReferences?.assets.find((item) => item.id === id)?.name ??
    data?.resourceReferences?.accounts.find((item) => item.id === id)?.name ??
    data?.resourceReferences?.assets.find((item) => item.id === id)?.name ??
    data?.resourceReferences?.assets.find((item) => item.id === id)?.displayCode ??
    data?.purposes?.find((item) => item.id === id)?.name ??
    capture?.response.household.data.items?.find((item) => item.id === id)?.name ??
    id
  );
}
export function resourceLabel(
  capture: ContextCapture,
  resource: Resource,
  t: (key: JournalTextKey) => string,
) {
  return `${sourceLabel(capture, resource.accountId)} · ${resource.kind === "CASH" ? `${resource.currency} ${t("cash")}` : `${sourceLabel(capture, resource.assetId)} · ${t("security")}`}`;
}
export const issueLabels: Record<string, JournalTextKey> = {
  PROJECTION_NOT_CURRENT: "issue_projection",
  MISSING_QUOTE: "issue_quote",
  INVALID_QUOTE_SOURCE: "issue_quote",
  STORED_QUOTE_UNVERIFIED: "stored_quote_unverified",
  ARITHMETIC_OVERFLOW: "issue_resource",
  QUOTE_SOURCE_INVALID: "issue_quote",
  MISSING_FX: "issue_fx",
  COST_FALLBACK: "issue_cost",
  COST_BASIS_FALLBACK: "issue_cost",
  MISSING_ASSET: "issue_asset",
  ASSET_UNAVAILABLE: "issue_asset",
  MISSING_SNAPSHOT: "issue_snapshot",
  MISSING_FINAL_CASH: "missing_final_cash",
  RESOURCE_UNAVAILABLE: "issue_resource",
  RESOURCE_UNRECONCILED: "issue_resource",
  PURPOSE_VALUATION_INCOMPLETE: "issue_resource",
  ACCOUNT_VALUATION_UNAVAILABLE: "issue_resource",
  HOUSEHOLD_UNAVAILABLE: "issue_resource",
  CASH_FLOW_UNAVAILABLE: "issue_resource",
  FX_UNAVAILABLE: "issue_fx",
};
