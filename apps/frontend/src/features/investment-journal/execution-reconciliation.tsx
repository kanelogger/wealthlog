import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { executeInvestmentJournal } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { ExactValue, Field, JournalErrorNotice, textAreaClass } from "./controls";
import { resourceLabel, sourceLabel } from "./context-labels";
import type {
  Execution,
  ExecutionSource,
  ExecutionReconciliationPreview,
  ExecutionResolutionDraft,
} from "./execution-work-types";
import { FundEditReview } from "./fund-edit-review";
import { FundSources } from "./fund-sources";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { OwnershipEntryFields } from "./ownership-entry-fields";
import { blankLine, resourceIdentity, type CorrectionLine } from "./ownership-entry-model";
import { useInvalidateJournal } from "./queries";
import type { ContextCapture, Purpose } from "./types";
import { useJournalWrite } from "./use-journal-write";

const sourceStatus: Record<ExecutionSource["sourceState"], JournalTextKey> = {
  VALID: "source_valid",
  INVALID: "source_invalid",
  DELETED: "source_deleted",
  UNAVAILABLE: "unavailable",
};

function Facts({ execution, capture }: { execution: Execution; capture?: ContextCapture }) {
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
        ).map(([key, value, currency]) => (
          <div key={key} className="flex flex-wrap justify-between gap-2">
            <dt className="text-muted-foreground">{t(key)}</dt>
            <dd>
              <ExactValue value={value} currency={currency} />
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
function SourceReview({ source, capture }: { source: ExecutionSource; capture?: ContextCapture }) {
  const { t } = useJournalLocale();
  const currentFacts =
    source.sourceState === "VALID" &&
    !source.projectionPending &&
    source.execution.sourceVersion === source.currentSourceVersion;
  return (
    <div className="space-y-4">
      <section className="space-y-3 border-t pt-4">
        <h4 className="font-semibold">
          {t(currentFacts ? "execution_current_source" : "execution_indexed_source")}
        </h4>
        <p className="break-all text-xs">
          {t(currentFacts ? "source_current_version" : "execution_indexed_version")} ·{" "}
          {source.execution.sourceVersion ?? t("unavailable")}
        </p>
        <p className="text-sm">
          {t("execution_current_status")} · {t(sourceStatus[source.sourceState])}
        </p>
        {!currentFacts && (
          <p className="break-all text-xs">
            {t("source_current_version")} · {source.currentSourceVersion ?? t("unavailable")}
          </p>
        )}
        <p className="break-all text-xs">
          {t("source_record")} · {source.execution.sourceId}
        </p>
        <Facts execution={source.execution} capture={capture} />
      </section>
      {source.activeLinks.map((link) => (
        <section key={link.id} className="space-y-3 border-t pt-4">
          <h4 className="font-semibold">
            {t("execution_original_evidence")} ·{" "}
            {t("version", { number: link.snapshot.decision.revisionNumber })}
          </h4>
          <p className="break-all text-xs">
            {t("source_original_version")} · {link.sourceVersion}
          </p>
          <p className="text-sm">
            {t("system_confirmed_at", { date: link.snapshot.decision.confirmedAt })}
          </p>
          <Facts execution={link.snapshot.execution} capture={capture} />
        </section>
      ))}
    </div>
  );
}

export function ExecutionReconciliation({
  source,
  capture,
  purposes,
  resourceStatus,
  retryResources,
  close,
}: {
  source: ExecutionSource;
  capture?: ContextCapture;
  purposes: Purpose[];
  resourceStatus: "loading" | "failed" | "ready";
  retryResources: () => void;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const [resolution, setResolution] = useState<ExecutionResolutionDraft["resolution"] | null>(null);
  const [lines, setLines] = useState<CorrectionLine[]>([blankLine]);
  const [reason, setReason] = useState("");
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [review, setReview] = useState<{
    draft: ExecutionResolutionDraft;
    preview: ExecutionReconciliationPreview;
    reason: string;
  } | null>(null);
  const [saved, setSaved] = useState(false);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const heading = useRef<HTMLHeadingElement>(null);
  const stage = saved ? "saved" : review ? "review" : "edit";
  useEffect(() => heading.current?.focus(), [stage]);
  const locked = reading || write.busy || write.uncertain;
  useJournalNavigationGuard(!saved && (reading || !!resolution || !!reason), {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  const resources =
    capture?.response.funding.data.allocation?.resources.map((row) => row.observation.resource) ??
    [];
  const canAdjust =
    resourceStatus === "ready" && !!capture && resources.length > 0 && purposes.length > 0;
  const adjusting = resolution === "ADJUST_OWNERSHIP";
  async function readPreview(event: FormEvent) {
    event.preventDefault();
    if (locked || !resolution || !reason.trim() || (adjusting && (!canAdjust || isBalanceHidden)))
      return;
    const entries = adjusting
      ? lines.flatMap((line) => {
          const resource = resources.find(
            (resource) => resourceIdentity(resource) === line.resourceKey,
          );
          return resource && line.purposeId && line.delta
            ? [{ resource, purposeId: line.purposeId, delta: line.delta }]
            : [];
        })
      : [];
    if (adjusting && entries.length !== lines.length) return;
    const draft = { executionId: source.execution.id, resolution, entries };
    setError(null);
    write.clearError();
    setReading(true);
    try {
      const preview = await executeInvestmentJournal<ExecutionReconciliationPreview>(
        "preview_investment_execution_reconciliation",
        draft,
      );
      if (
        preview.source.execution.id !== source.execution.id ||
        preview.resolution !== resolution ||
        !preview.previewHash
      )
        throw new Error(t("records_error"));
      setReview({ draft, preview, reason: reason.trim() });
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }
  const complete =
    review &&
    Array.isArray(review.preview.funding.sourceReferences) &&
    review.preview.funding.resources.every((row) => Array.isArray(row.observation.sources));
  function commit() {
    if (!review || !complete || locked || write.error) return;
    void write.run(
      "reconcile_investment_execution",
      {
        draft: review.draft,
        reason: review.reason,
        expectedPreviewHash: review.preview.previewHash,
        requestKey: crypto.randomUUID(),
      },
      () => {
        setSaved(true);
        void invalidate();
      },
    );
  }
  return (
    <section className="max-w-3xl space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <h2
          ref={stage === "edit" ? heading : undefined}
          tabIndex={-1}
          className="text-xl font-semibold"
        >
          {t("execution_reconcile")}
        </h2>
        <Button
          variant="ghost"
          disabled={locked}
          onClick={() => {
            if (allowJournalNavigation()) close();
          }}
        >
          {t("back_to_work_queue")}
        </Button>
      </header>
      {saved ? (
        <h3 ref={heading} tabIndex={-1} role="status" className="font-semibold">
          {t("execution_reconciliation_saved")}
        </h3>
      ) : review ? (
        <>
          <h3 ref={heading} tabIndex={-1} className="font-semibold">
            {t("execution_review_sources")}
          </h3>
          <SourceReview source={review.preview.source} capture={capture} />
          {review.draft.resolution === "ADJUST_OWNERSHIP" && capture && (
            <>
              <FundEditReview
                preview={review.preview}
                capture={capture}
                purposes={purposes}
                correction
              />
              <FundSources
                preview={review.preview.funding}
                describeResource={(resource) => resourceLabel(capture, resource, t)}
              />
            </>
          )}
          <p className="text-sm">
            {t(
              review.draft.resolution === "NO_OWNERSHIP_CHANGE"
                ? "execution_ack_only_hint"
                : "execution_adjust_hint",
            )}
          </p>
          <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
            {t("operation_reason")} · {review.reason}
          </p>
          {!complete && <p role="alert">{t("source_list_unavailable")}</p>}
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !complete || !!write.error} onClick={commit}>
              {t("execution_submit_reconciliation")}
            </Button>
            <Button
              variant="outline"
              disabled={locked}
              onClick={() => {
                write.clearError();
                setReview(null);
              }}
            >
              {t("return_edit")}
            </Button>
          </div>
        </>
      ) : (
        <>
          <SourceReview source={source} capture={capture} />
          {source.projectionPending && <p role="status">{t("index_pending")}</p>}
          <form className="space-y-4 border-t pt-4" onSubmit={readPreview}>
            <fieldset disabled={locked} className="space-y-3">
              <legend className="mb-2 font-semibold">{t("trade_ownership_mode")}</legend>
              {(["NO_OWNERSHIP_CHANGE", "ADJUST_OWNERSHIP"] as const).map((mode) => (
                <label key={mode} className="flex items-center gap-3 text-sm">
                  <input
                    type="radio"
                    name="executionResolution"
                    checked={resolution === mode}
                    disabled={mode === "ADJUST_OWNERSHIP" && !canAdjust}
                    onChange={() => setResolution(mode)}
                  />
                  {t(mode === "NO_OWNERSHIP_CHANGE" ? "execution_ack_only" : "execution_adjust")}
                </label>
              ))}
              {resourceStatus === "loading" && (
                <p role="status" className="text-sm">
                  {t("execution_resources_loading")}
                </p>
              )}
              {resourceStatus === "failed" && (
                <div className="space-y-2">
                  <p role="alert" className="text-sm">
                    {t("execution_resources_failed")}
                  </p>
                  <Button type="button" variant="outline" onClick={retryResources}>
                    {t("retry")}
                  </Button>
                </div>
              )}
              {resourceStatus === "ready" && !canAdjust && (
                <p className="text-muted-foreground text-sm">{t("execution_adjust_unavailable")}</p>
              )}
              {adjusting && capture && (
                <fieldset disabled={isBalanceHidden} className="space-y-4">
                  <p className="text-muted-foreground text-sm">{t("correction_signed_hint")}</p>
                  <OwnershipEntryFields
                    lines={lines}
                    setLines={setLines}
                    resources={resources}
                    purposes={purposes}
                    capture={capture}
                  />
                </fieldset>
              )}
              {adjusting && isBalanceHidden && (
                <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
              )}
              <Field label={t("operation_reason")} hint={t("correction_reason_required")}>
                {(id) => (
                  <textarea
                    id={id}
                    required
                    className={textAreaClass}
                    value={reason}
                    onChange={(event) => setReason(event.target.value)}
                  />
                )}
              </Field>
            </fieldset>
            <Button
              disabled={
                locked ||
                !resolution ||
                !reason.trim() ||
                (adjusting && (!canAdjust || isBalanceHidden))
              }
            >
              {t(reading ? "loading" : "execution_review_sources")}
            </Button>
          </form>
        </>
      )}
      {(Boolean(error) || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
    </section>
  );
}
