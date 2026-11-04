import { ContextPanel } from "./context-panel";
import { ExactValue } from "./controls";
import { DecisionBodyView } from "./decision-body-view";
import type { Execution } from "./execution-work-types";
import { issueLabels, sourceLabel } from "./context-labels";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import type { ReviewFacts } from "./review-types";
import type { ContextCapture } from "./types";

const scopeLabels: Record<string, JournalTextKey> = {
  IN_SCOPE: "review_in_scope",
  AFTER_CUTOFF: "review_after_cutoff",
  UNDETERMINED: "review_scope_unknown",
};
const sourceLabels: Record<string, JournalTextKey> = {
  VALID: "source_valid",
  INVALID: "source_invalid",
  DELETED: "source_deleted",
  UNAVAILABLE: "unavailable",
};
const reviewIssueLabels: Record<string, JournalTextKey> = {
  QUOTE_UNAVAILABLE: "issue_quote",
  QUOTE_STALE: "stale",
  EXECUTION_DATE_UNAVAILABLE: "queue_execution_date_missing",
  EXECUTION_INDEX_PENDING: "index_pending",
  LINKED_SOURCE_CHANGED: "fund_issue_changed",
  EXECUTION_REQUIRES_RECONCILIATION: "queue_ownership_pending",
};

function ExecutionFacts({
  execution,
  capture,
}: {
  execution: Execution;
  capture?: ContextCapture;
}) {
  const { t } = useJournalLocale();
  return (
    <div className="space-y-2 text-sm">
      <p className="[overflow-wrap:anywhere]">
        {sourceLabel(capture, execution.accountId)} · {sourceLabel(capture, execution.assetId)} ·{" "}
        {t(execution.side === "SELL" ? "action_SELL" : "action_BUY")} · {execution.occurredAt}
      </p>
      <dl className="grid gap-2 sm:grid-cols-2">
        {(
          [
            ["actual_quantity", execution.quantity, undefined],
            ["actual_net_cash", execution.netCash, execution.settlementCurrency ?? undefined],
            ["trade_fee", execution.fee, execution.activityCurrency],
            ["trade_tax", execution.tax, execution.activityCurrency],
          ] as const
        ).map(([label, amount, currency]) => (
          <div key={label} className="flex flex-wrap justify-between gap-2">
            <dt className="text-muted-foreground">{t(label)}</dt>
            <dd>
              <ExactValue value={amount} currency={currency} />
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export function ReviewFactsView({ facts }: { facts: ReviewFacts }) {
  const { t } = useJournalLocale();
  const decision = facts.observations.decision;
  const capture = decision.contextAtConfirmation ?? undefined;
  const quote = facts.subjectMarket?.quote;
  return (
    <section className="space-y-5 border-t pt-5">
      <h3 className="font-semibold">{t("review_observed_facts")}</h3>
      <p className="text-sm">
        {t("review_cutoff")} · {facts.query.observationCutoff} · {facts.query.observationTimezone}
      </p>
      <p className="text-sm">
        {t(
          facts.quality === "COMPLETE"
            ? "review_quality_complete"
            : facts.quality === "PARTIAL"
              ? "review_quality_partial"
              : "unavailable",
        )}
      </p>
      <ul className="text-muted-foreground space-y-1 text-sm">
        {facts.issues.map((issue, i) => (
          <li key={i}>
            {t(reviewIssueLabels[issue.code] ?? issueLabels[issue.code] ?? "issue_unknown")}
          </li>
        ))}
      </ul>
      <p className="text-muted-foreground text-sm">{t("review_no_attribution")}</p>
      <section className="space-y-3 border-t pt-4">
        <h4 className="font-semibold">{t("review_observed_quote")}</h4>
        {quote ? (
          <div className="space-y-1 text-sm">
            <ExactValue value={quote.price} currency={quote.currency} />
            <p>
              {quote.sourceTimestamp} · {quote.source}
              {quote.stale && ` · ${t("stale")}`}
            </p>
          </div>
        ) : (
          <p className="text-sm">{t("unavailable")}</p>
        )}
      </section>
      <section className="space-y-3 border-t pt-4">
        <h4 className="font-semibold">{t("execution_links")}</h4>
        {!facts.observations.executions.length && (
          <p className="text-sm">{t("no_execution_links")}</p>
        )}
        {facts.observations.executions.map(
          ({ link, eventDate, observationScope, currentSource }) => (
            <details key={link.id} className="space-y-3 rounded-lg border p-3" open>
              <summary className="cursor-pointer text-sm font-medium">
                {eventDate ?? t("unavailable")} ·{" "}
                {t(scopeLabels[observationScope] ?? "review_scope_unknown")} ·{" "}
                {t(link.unlinkedAt ? "execution_unlinked" : "execution_linked")}
              </summary>
              <p className="break-all text-xs">
                {t("execution_original_evidence")} · {t("source_original_version")} ·{" "}
                {link.sourceVersion}
              </p>
              <ExecutionFacts execution={link.snapshotAtLink.execution} capture={capture} />
              <p className="break-all text-xs">
                {t("execution_indexed_version")} ·{" "}
                {currentSource.execution.sourceVersion ?? t("unavailable")}
              </p>
              <p className="break-all text-xs">
                {t("source_current_version")} ·{" "}
                {currentSource.currentSourceVersion ?? t("unavailable")} ·{" "}
                {t(sourceLabels[currentSource.sourceState] ?? "unavailable")}
              </p>
              <ExecutionFacts execution={currentSource.execution} capture={capture} />
            </details>
          ),
        )}
      </section>
      {facts.observations.principles.length > 0 && (
        <section className="space-y-3 border-t pt-4">
          <h4 className="font-semibold">{t("review_principle_snapshots")}</h4>
          {facts.observations.principles.map((item, i) => (
            <details key={item.id ?? i} className="space-y-2 text-sm [overflow-wrap:anywhere]">
              <summary className="cursor-pointer font-medium">
                {item.body?.name ?? item.name ?? item.revisionId ?? item.id ?? t("not_recorded")}
                {item.revisionNumber != null &&
                  ` · ${t("version", { number: item.revisionNumber })}`}
                {item.statusAtCapture === "DRAFT" && ` · ${t("draft")}`}
              </summary>
              {item.body?.content && <p className="whitespace-pre-wrap">{item.body.content}</p>}
              {item.body?.applicableConditions && (
                <p className="whitespace-pre-wrap">{item.body.applicableConditions}</p>
              )}
              {item.body?.exceptions && (
                <p className="whitespace-pre-wrap">{item.body.exceptions}</p>
              )}
            </details>
          ))}
        </section>
      )}
      <details className="space-y-4 rounded-lg border p-4">
        <summary className="cursor-pointer text-sm font-semibold">
          {t("review_original_decision")} · {t("version", { number: decision.revisionNumber })}
        </summary>
        <p className="text-sm">{t("system_confirmed_at", { date: decision.confirmedAt })}</p>
        <DecisionBodyView body={decision.body} capture={capture} />
        {capture && <ContextPanel capture={capture} frozen />}
      </details>
    </section>
  );
}
