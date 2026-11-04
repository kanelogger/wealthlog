import { Button } from "@wealthfolio/ui";
import type { ReactNode } from "react";
import { useJournalLocale } from "./locale-context";
import type { Block, ContextCapture } from "./types";
import { ExactValue } from "./controls";
import { Comparisons } from "./comparisons";
import { issueLabels, resourceLabel, sourceLabel } from "./context-labels";

const purposeFlowGroups = [
  ["allocationIn", "purpose_flow_in"],
  ["allocationOut", "purpose_flow_out"],
  ["tradeConversions", "purpose_flow_trades"],
  ["adjustments", "purpose_flow_adjustments"],
] as const;

function SourceBlock({
  block,
  title,
  scope,
  children,
  capture,
}: {
  block: Block<unknown>;
  title: string;
  scope: string;
  children: ReactNode;
  capture?: ContextCapture;
}) {
  const { t } = useJournalLocale();
  return (
    <section className="space-y-4 border-t pt-5 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-semibold">{title}</h3>
        <span className="text-muted-foreground rounded-md border px-2 py-1 text-xs">
          {t(
            block.quality === "COMPLETE"
              ? "complete"
              : block.quality === "PARTIAL"
                ? "partial"
                : "unavailable",
          )}
        </span>
      </div>
      <div className="text-muted-foreground space-y-1 text-xs leading-relaxed">
        <p>{scope}</p>
        <p>{t("as_of", { date: block.asOfDate })}</p>
        <p>
          {block.sourceDataAsOf
            ? t("source_date", { date: block.sourceDataAsOf })
            : t("source_unknown")}
          {block.stale ? ` · ${t("stale")}` : ""}
        </p>
      </div>
      {children}
      {block.issues.length > 0 && (
        <details className="text-sm">
          <summary className="focus-visible:ring-ring cursor-pointer rounded py-2 focus-visible:ring-2">
            {t("quality_issues", { count: block.issues.length })}
          </summary>
          <ul className="mt-2 space-y-2">
            {block.issues.map((issue, i) => (
              <li key={i} className="break-words leading-relaxed">
                {t(issueLabels[issue.code] ?? "issue_unknown")}
                {issue.sourceId ? ` · ${sourceLabel(capture, issue.sourceId)}` : ""}
                {issue.currency ? ` · ${issue.currency}` : ""}
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
export function ContextPanel({
  capture,
  loading,
  error,
  retry,
  frozen = false,
  householdOnly = false,
}: {
  capture?: ContextCapture;
  loading?: boolean;
  error?: boolean;
  retry?: () => void;
  frozen?: boolean;
  householdOnly?: boolean;
}) {
  const { t } = useJournalLocale();
  if (!capture)
    return (
      <aside aria-label={t("live_context")} className="space-y-4 rounded-xl border p-5">
        <p role="status" className="text-muted-foreground text-sm">
          {t(error ? "context_error" : "loading")}
        </p>
        {error && retry && (
          <Button variant="outline" onClick={retry}>
            {t("retry")}
          </Button>
        )}
      </aside>
    );
  const { household, recentCashFlow: flows, funding } = capture.response;
  return (
    <aside
      aria-label={t(frozen ? "source_frozen" : "live_context")}
      className="min-w-0 space-y-6 rounded-xl border p-5 xl:sticky xl:top-28 xl:max-h-[calc(100dvh-8rem)] xl:overflow-y-auto"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-semibold">{t(frozen ? "source_frozen" : "live_context")}</h2>
        {!frozen && retry && (
          <Button variant="ghost" size="sm" disabled={loading} onClick={retry}>
            {t("refresh_context")}
          </Button>
        )}
      </div>
      {!frozen && error && (
        <p role="alert" className="text-sm leading-relaxed">
          {t("context_error")}
        </p>
      )}
      {loading && (
        <p role="status" className="text-muted-foreground text-sm">
          {t("loading")}
        </p>
      )}
      {!householdOnly && (
        <>
          <SourceBlock
            capture={capture}
            block={funding}
            title={t("funding_context")}
            scope={t("scope_accounts")}
          >
            {!funding.data.allocation?.resources.length && (
              <p className="text-muted-foreground text-sm">{t("scope_none")}</p>
            )}
            {funding.data.allocation?.resources.map((row) => (
              <dl key={JSON.stringify(row.observation.resource)} className="space-y-2 text-sm">
                <dt className="font-medium">
                  {resourceLabel(capture, row.observation.resource, t)}
                </dt>
                {(["actual", "allocated", "unallocated", "excess", "negativeActual"] as const).map(
                  (key) => (
                    <div className="flex flex-wrap justify-between gap-2" key={key}>
                      <dt className="text-muted-foreground">
                        {t(key === "negativeActual" ? "liability" : key)}
                      </dt>
                      <dd>
                        <ExactValue value={row.balance[key]} />
                      </dd>
                    </div>
                  ),
                )}
              </dl>
            ))}
            {funding.data.accountValuations?.accounts.map((account) => (
              <div key={account.accountId} className="space-y-2 border-t pt-3 text-sm">
                <h4 className="font-medium [overflow-wrap:anywhere]">
                  {t("account_valuation", { name: sourceLabel(capture, account.accountId) })}
                </h4>
                <dl className="space-y-2">
                  {(
                    [
                      ["account_valuation_cash", account.cashBalanceBase],
                      ["account_valuation_securities", account.investmentMarketValueBase],
                    ] as const
                  ).map(([label, value]) => (
                    <div key={label} className="flex flex-wrap justify-between gap-2">
                      <dt className="text-muted-foreground">{t(label)}</dt>
                      <dd>
                        <ExactValue value={value} currency={account.baseCurrency} />
                      </dd>
                    </div>
                  ))}
                </dl>
                <div className="text-muted-foreground space-y-1 text-xs [overflow-wrap:anywhere]">
                  <p>
                    {account.sourceDataAsOf
                      ? t("source_date", { date: account.sourceDataAsOf })
                      : t("source_unknown")}
                    {account.stale ? ` · ${t("stale")}` : ""}
                  </p>
                  <p>{t("valuation_calculated_at", { date: account.calculatedAt })}</p>
                  {account.issues.length > 0 && (
                    <ul>
                      {account.issues.map((issue, i) => (
                        <li key={i}>
                          {t(issueLabels[issue.code] ?? "issue_unknown")}
                          {issue.assetId ? ` · ${sourceLabel(capture, issue.assetId)}` : ""}
                          {issue.currency ? ` · ${issue.currency}` : ""}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </div>
            ))}
            <Comparisons capture={capture} />
          </SourceBlock>
          <SourceBlock
            capture={capture}
            block={flows}
            title={t("cash_flow")}
            scope={t("scope_flows")}
          >
            <dl className="space-y-3 text-sm">
              {(["income", "expense"] as const).map((key) => (
                <div key={key} className="flex flex-wrap justify-between gap-2">
                  <dt className="text-muted-foreground">{t(key)}</dt>
                  <dd>
                    <ExactValue
                      value={flows.data.convertedOrdinary?.[key]}
                      currency={flows.currency}
                    />
                  </dd>
                </div>
              ))}
            </dl>
            <details className="text-sm">
              <summary className="focus-visible:ring-ring cursor-pointer rounded py-2 underline-offset-4 hover:underline focus-visible:ring-2">
                {t("details")}
              </summary>
              <ul className="mt-3 space-y-4">
                {flows.data.ordinary?.currencies.map((row) => (
                  <li key={row.currency} className="space-y-1">
                    <p className="font-medium">{row.currency}</p>
                    <p>
                      {t("income")}: <ExactValue value={row.income} currency={row.currency} />
                    </p>
                    {row.income == null && (
                      <p>
                        {t("known_part")}:{" "}
                        <ExactValue value={row.knownIncome} currency={row.currency} />
                      </p>
                    )}
                    <p>
                      {t("expense")}: <ExactValue value={row.expense} currency={row.currency} />
                    </p>
                    {row.expense == null && (
                      <p>
                        {t("known_part")}:{" "}
                        <ExactValue value={row.knownExpense} currency={row.currency} />
                      </p>
                    )}
                  </li>
                ))}
              </ul>
              {flows.data.convertedOrdinary?.fxSources &&
                flows.data.convertedOrdinary.fxSources.length > 0 && (
                  <div className="mt-4 space-y-2">
                    <h4 className="font-medium">{t("dated_fx_sources")}</h4>
                    <ul className="text-muted-foreground space-y-2 text-xs">
                      {flows.data.convertedOrdinary.fxSources.map((source, index) => (
                        <li key={index} className="break-words">
                          {source.sourceCurrency} ·{" "}
                          {t("activity_day", { date: source.activityDate })} ·{" "}
                          {t("fx_source_date", { date: source.sourceDate })}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              <p className="text-muted-foreground mt-2">
                {t("internal_transfers")}:{" "}
                {flows.data.internalCashTransfers?.length ?? t("unavailable")}
              </p>
              <ul className="mt-2 space-y-3">
                {flows.data.internalCashTransfers?.map((row) => (
                  <li key={row.outActivityId} className="space-y-1 break-words">
                    <p className="text-muted-foreground text-xs">{row.occurredAt}</p>
                    <p>
                      {resourceLabel(capture, row.sourceResource, t)}:{" "}
                      <ExactValue
                        value={row.sourceAmount}
                        currency={
                          row.sourceResource.kind === "CASH"
                            ? row.sourceResource.currency
                            : undefined
                        }
                      />
                    </p>
                    <p>
                      {resourceLabel(capture, row.destinationResource, t)}:{" "}
                      <ExactValue
                        value={row.destinationAmount}
                        currency={
                          row.destinationResource.kind === "CASH"
                            ? row.destinationResource.currency
                            : undefined
                        }
                      />
                    </p>
                  </li>
                ))}
              </ul>
              <p className="text-muted-foreground mt-2">
                {t("purpose_flows")}:{" "}
                {flows.data.purposeFlows
                  ? Object.values(flows.data.purposeFlows).reduce(
                      (sum, rows) => sum + rows.length,
                      0,
                    )
                  : t("unavailable")}
              </p>
              {purposeFlowGroups.map(([key, label]) => {
                const rows = flows.data.purposeFlows?.[key];
                if (!rows?.length) return null;
                return (
                  <section key={key} className="mt-3 space-y-2 [overflow-wrap:anywhere]">
                    <h4 className="font-medium">{t(label)}</h4>
                    <ul className="space-y-3">
                      {rows.map(({ operationId, occurredOn, entry }, i) => (
                        <li key={`${operationId}-${i}`} className="space-y-1">
                          <p className="text-muted-foreground text-xs">
                            {occurredOn}
                            {entry.purposeId ? ` · ${sourceLabel(capture, entry.purposeId)}` : ""}
                          </p>
                          <p>
                            {resourceLabel(capture, entry.resource, t)} ·{" "}
                            <ExactValue
                              value={entry.delta}
                              currency={
                                entry.resource.kind === "CASH" ? entry.resource.currency : undefined
                              }
                            />
                          </p>
                        </li>
                      ))}
                    </ul>
                  </section>
                );
              })}
            </details>
          </SourceBlock>
        </>
      )}
      <SourceBlock
        capture={capture}
        block={household}
        title={t("household")}
        scope={t("scope_household")}
      >
        <dl className="space-y-3 text-sm">
          {(
            [
              ["total_assets", "totalAssets"],
              ["financial_assets", "financialAssets"],
              ["other_assets", "otherAssets"],
              ["liabilities", "totalLiabilities"],
              ["net_worth", "netWorth"],
            ] as const
          ).map(([label, key]) => (
            <div key={key} className="flex flex-wrap justify-between gap-2">
              <dt className="text-muted-foreground">{t(label)}</dt>
              <dd>
                <ExactValue value={household.data[key]} currency={household.currency} />
              </dd>
            </div>
          ))}
        </dl>
        <details className="text-sm">
          <summary className="focus-visible:ring-ring cursor-pointer rounded py-2 underline-offset-4 hover:underline focus-visible:ring-2">
            {t("details")}
          </summary>
          <ul className="mt-3 space-y-4">
            {household.data.items?.map((item) => (
              <li key={item.id} className="space-y-1">
                <div className="flex flex-wrap justify-between gap-2">
                  <span>{item.name}</span>
                  <ExactValue value={item.value} currency={item.currency} />
                </div>
                <p className="text-muted-foreground text-xs">
                  {item.valuationDate ?? t("source_unknown")}
                  {item.stale ? ` · ${t("stale")}` : ""}
                  {item.basis === "CALCULATED_LOAN" ? ` · ${t("calculated_loan")}` : ""}
                </p>
                {item.fxSourceDate && (
                  <p className="text-muted-foreground text-xs">
                    {t("fx_source_date", { date: item.fxSourceDate })}
                  </p>
                )}
                {item.value == null && item.substituteValue != null && (
                  <p className="text-muted-foreground text-xs">
                    <ExactValue value={item.substituteValue} currency={item.currency} /> ·{" "}
                    {t("cost_substitute")}
                  </p>
                )}
              </li>
            ))}
          </ul>
        </details>
      </SourceBlock>
    </aside>
  );
}
