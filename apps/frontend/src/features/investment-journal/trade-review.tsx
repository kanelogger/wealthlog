import { ExactValue } from "./controls";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { ResourceBalances } from "./fund-allocation-editor";
import { FundSources } from "./fund-sources";
import { TradeProgress } from "./trade-progress";
import type { Purpose, Resource } from "./types";
import type { PreparedTrade, TradeBinding, TradePreview } from "./trade-types";

const issues: Record<string, JournalTextKey> = {
  FeeUnknown: "fee_unknown",
  TaxUnknown: "tax_unknown",
  IndependentCashCalibration: "independent_cash_calibration",
  MissingFinalCash: "missing_final_cash",
  ResourceImpactMismatch: "impact_mismatch",
  NegativeCash: "negative_cash_notice",
  FxUnavailable: "issue_fx",
};
function describeTradeResource(
  prepared: PreparedTrade,
  resource: Resource,
  t: (key: JournalTextKey) => string,
) {
  const account =
    resource.accountId === prepared.account.id ? prepared.account.name : resource.accountId;
  const holding =
    resource.kind === "SECURITY" && resource.assetId === prepared.asset.id
      ? (prepared.asset.name ?? prepared.asset.displayCode ?? prepared.asset.id)
      : resource.kind === "SECURITY"
        ? resource.assetId
        : "";
  return resource.kind === "CASH"
    ? `${account} · ${resource.currency} ${t("cash")}`
    : `${account} · ${holding} · ${t("security")}`;
}
export function TradeImpact({
  prepared,
  preview,
  binding,
}: {
  prepared: PreparedTrade;
  preview: TradePreview;
  binding: TradeBinding;
}) {
  const { t } = useJournalLocale();
  const describe = (resource: Resource) => describeTradeResource(prepared, resource, t);
  const manual = prepared.account.trackingMode === "HOLDINGS";
  const estimated = manual && (preview.trade.fee == null || preview.trade.tax == null);
  return (
    <div className="space-y-6">
      <section className="space-y-4">
        <h3 className="font-semibold">{t("actual_trade_impact")}</h3>
        <p className="break-words text-sm">
          {prepared.account.name} ·{" "}
          {prepared.asset.name ?? prepared.asset.displayCode ?? prepared.asset.id} ·{" "}
          {t(prepared.fields.side === "BUY" ? "action_BUY" : "action_SELL")} ·{" "}
          {prepared.fields.activityDate}
        </p>
        <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
          {(
            [
              ["actual_quantity", prepared.fields.quantity, undefined],
              ["actual_price", prepared.fields.price || null, prepared.fields.currency],
              [
                estimated ? "calculated_trade_cash" : "actual_net_cash",
                preview.trade.netCash,
                preview.trade.settlementCurrency ?? undefined,
              ],
              ...(manual
                ? [
                    [
                      "observed_cash_balance",
                      prepared.fields.observedCashBalance || null,
                      preview.trade.settlementCurrency ?? undefined,
                    ] as const,
                  ]
                : [
                    [
                      "final_posted_cash",
                      prepared.fields.amount || null,
                      prepared.fields.currency,
                    ] as const,
                    ["trade_fx_rate", prepared.fields.fxRate || null, undefined] as const,
                  ]),
              ["trade_fee", preview.trade.fee, preview.trade.activityCurrency],
              ["trade_tax", preview.trade.tax, preview.trade.activityCurrency],
              [
                "independent_cash_calibration",
                preview.trade.cashAdjustment,
                preview.trade.settlementCurrency ?? undefined,
              ],
            ] as const
          ).map(([label, value, currency]) => (
            <div key={label} className="flex flex-wrap justify-between gap-3">
              <dt className="text-muted-foreground">{t(label)}</dt>
              <dd>
                <ExactValue value={value} currency={currency} />
              </dd>
            </div>
          ))}
        </dl>
        <ul className="space-y-2 text-sm">
          {preview.trade.changes.map((change, index) => (
            <li key={index} className="flex flex-wrap justify-between gap-3">
              <span>{describe(change.resource)}</span>
              <ExactValue
                value={change.delta}
                currency={change.resource.kind === "CASH" ? change.resource.currency : undefined}
              />
            </li>
          ))}
        </ul>
        {preview.trade.issues.length > 0 && (
          <ul className="text-muted-foreground space-y-2 text-sm">
            {preview.trade.issues.map((code) => (
              <li key={code}>{t(issues[code] ?? "issue_unknown")}</li>
            ))}
          </ul>
        )}
        {estimated && (
          <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
            {t("manual_unknown_cash_hint")}
          </p>
        )}
      </section>
      <TradeProgress binding={binding} progress={preview.decision.progress} beforeTrade />
    </div>
  );
}

export function OwnershipImpact({
  prepared,
  preview,
  purposes,
}: {
  prepared: PreparedTrade;
  preview: TradePreview;
  purposes: Purpose[];
}) {
  const { t } = useJournalLocale();
  const describe = (resource: Resource) => describeTradeResource(prepared, resource, t);
  if (!preview.ownership || !preview.funding)
    return (
      <p className="rounded-lg border p-4 text-sm leading-relaxed">
        {t("deferred_ownership_preview")}
      </p>
    );
  return (
    <div className="space-y-6 border-t pt-5">
      {preview.funding.resources.length > 0 && (
        <h3 className="font-semibold">{t("ownership_changes")}</h3>
      )}
      {preview.funding.resources.map((row) => (
        <section key={JSON.stringify(row.observation.resource)} className="space-y-3">
          <h4 className="text-sm font-medium">{describe(row.observation.resource)}</h4>
          <ResourceBalances row={row} />
        </section>
      ))}
      <FundSources preview={preview.funding} describeResource={describe} trade />
      {(
        [
          ["explicit_top_ups", preview.ownership.topUpEntries],
          ["trade_conversions", preview.ownership.conversionEntries],
        ] as const
      ).map(([label, entries]) => (
        <section key={label} className="space-y-3">
          <h4 className="font-medium">{t(label)}</h4>
          {entries.length === 0 && (
            <p className="text-muted-foreground text-sm">{t("not_recorded")}</p>
          )}
          <ul className="space-y-3 text-sm">
            {entries.map((entry, index) => (
              <li key={index} className="flex flex-wrap justify-between gap-3">
                <span>
                  {purposes.find((p) => p.id === entry.purposeId)?.name ?? entry.purposeId} ·{" "}
                  {describe(entry.resource)}
                </span>
                <ExactValue
                  value={entry.delta}
                  currency={entry.resource.kind === "CASH" ? entry.resource.currency : undefined}
                />
              </li>
            ))}
          </ul>
        </section>
      ))}
      <section className="space-y-3">
        <h4 className="font-medium">{t("trade_ownership_after")}</h4>
        <ul className="space-y-3 text-sm">
          {preview.ownership.balancesAfter.map((row, index) => (
            <li key={index} className="flex flex-wrap justify-between gap-3">
              <span>
                {purposes.find((p) => p.id === row.purposeId)?.name ?? row.purposeId} ·{" "}
                {describe(row.resource)}
              </span>
              <ExactValue
                value={row.amount}
                currency={row.resource.kind === "CASH" ? row.resource.currency : undefined}
              />
            </li>
          ))}
        </ul>
      </section>
      <section className="space-y-3">
        <h4 className="font-medium">{t("allocated_charges")}</h4>
        <p className="text-muted-foreground text-sm">{t("charges_detail_only")}</p>
        <ul className="space-y-3 text-sm">
          {preview.ownership.charges.map((row) => (
            <li key={row.purposeId} className="space-y-1">
              <p>{purposes.find((p) => p.id === row.purposeId)?.name ?? row.purposeId}</p>
              <p>
                {t("trade_fee")}: <ExactValue value={row.fee} currency={row.currency} />
              </p>
              <p>
                {t("trade_tax")}: <ExactValue value={row.tax} currency={row.currency} />
              </p>
            </li>
          ))}
        </ul>
      </section>
      {preview.pendingReconciliation.length > 0 && (
        <p className="text-muted-foreground text-sm">{t("trade_existing_reconciliation")}</p>
      )}
    </div>
  );
}
