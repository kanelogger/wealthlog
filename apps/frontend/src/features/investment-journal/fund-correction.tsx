import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { executeInvestmentJournal } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { Field, JournalErrorNotice, textAreaClass } from "./controls";
import { resourceLabel } from "./context-labels";
import { FundEditReview } from "./fund-edit-review";
import { OwnershipEntryFields } from "./ownership-entry-fields";
import { blankLine, resourceIdentity, type CorrectionLine } from "./ownership-entry-model";
import { FundSources } from "./fund-sources";
import {
  sameResource,
  type AllocationDelta,
  type AllocationPreview,
  type FundHistoryItem,
  type FundReversalPreview,
} from "./fund-types";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { useInvalidateJournal } from "./queries";
import type { ContextCapture, Purpose } from "./types";
import { useJournalWrite } from "./use-journal-write";

interface CorrectionPreview extends FundReversalPreview {
  sourceAnchors: AllocationPreview["sourceReferences"];
}
export function FundCorrection({
  item,
  capture,
  purposes,
  close,
}: {
  item: FundHistoryItem;
  capture: ContextCapture;
  purposes: Purpose[];
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const [reason, setReason] = useState("");
  const [lines, setLines] = useState<CorrectionLine[]>([blankLine]);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [review, setReview] = useState<{
    preview: CorrectionPreview;
    entries: AllocationDelta[];
    reason: string;
  } | null>(null);
  const [saved, setSaved] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const step = saved ? "saved" : review ? "review" : "edit";
  useEffect(() => {
    heading.current?.focus();
  }, [step]);
  const locked = reading || write.busy || write.uncertain;
  useJournalNavigationGuard(
    !saved && (!!reason || lines.some((line) => Object.values(line).some(Boolean)) || reading),
    {
      pending: t("pending_navigation"),
      discard: t("discard_navigation"),
    },
  );

  const candidates = [
    ...item.entries.map((entry) => entry.resource),
    ...(capture.response.funding.data.allocation?.resources.map(
      (row) => row.observation.resource,
    ) ?? []),
  ];
  const resources = candidates.filter(
    (resource, index) => candidates.findIndex((other) => sameResource(resource, other)) === index,
  );
  async function preview(event: FormEvent) {
    event.preventDefault();
    if (locked || isBalanceHidden || !reason.trim()) return;
    const entries = lines.flatMap((line) => {
      const resource = resources.find(
        (resource) => resourceIdentity(resource) === line.resourceKey,
      );
      return line.resourceKey && resource && line.purposeId && line.delta
        ? [{ purposeId: line.purposeId, resource, delta: line.delta }]
        : [];
    });
    if (entries.length !== lines.length) return;
    setError(null);
    write.clearError();
    setReading(true);
    try {
      const preview = await executeInvestmentJournal<CorrectionPreview>("preview_fund_correction", {
        operationId: item.id,
        entries,
      });
      setReview({ preview, entries, reason: reason.trim() });
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }

  const sourceReferences =
    review &&
    Array.isArray(review.preview.sourceAnchors) &&
    Array.isArray(review.preview.funding.sourceReferences)
      ? [...review.preview.funding.sourceReferences, ...review.preview.sourceAnchors].filter(
          (source, index, all) =>
            all.findIndex(
              (other) =>
                other.operationId === source.operationId &&
                other.sourceKind === source.sourceKind &&
                other.sourceId === source.sourceId &&
                other.sourceCounterpartyId === source.sourceCounterpartyId &&
                other.originVersion === source.originVersion &&
                other.currentVersion === source.currentVersion &&
                other.state === source.state &&
                sameResource(other.resource, source.resource),
            ) === index,
        )
      : undefined;
  const complete =
    review &&
    sourceReferences &&
    review.preview.funding.resources.every((row) => Array.isArray(row.observation.sources));
  function commit() {
    if (!review || !complete || locked || write.error) return;
    void write.run(
      "correct_fund_operation",
      {
        operationId: item.id,
        entries: review.entries,
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
    <section className="max-w-3xl space-y-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-2">
          <h2
            ref={step === "edit" ? heading : undefined}
            tabIndex={-1}
            className="text-xl font-semibold"
          >
            {t("correct_ownership")}
          </h2>
          <p className="text-muted-foreground max-w-prose text-sm">{t("correct_ownership_hint")}</p>
          <p className="break-words text-sm">
            {item.createdAt} · {item.reason}
          </p>
        </div>
        <Button
          variant="ghost"
          disabled={locked}
          onClick={() => {
            if (allowJournalNavigation()) close();
          }}
        >
          {t("correction_back")}
        </Button>
      </header>
      {saved ? (
        <h3 ref={heading} tabIndex={-1} role="status" className="font-semibold">
          {t("correction_saved")}
        </h3>
      ) : review ? (
        <>
          <h3 ref={heading} tabIndex={-1} className="font-semibold">
            {t("allocation_preview")}
          </h3>
          <FundEditReview
            preview={review.preview}
            capture={capture}
            purposes={purposes}
            correction
          />
          {sourceReferences ? (
            <FundSources
              preview={{ ...review.preview.funding, sourceReferences }}
              describeResource={(resource) => resourceLabel(capture, resource, t)}
            />
          ) : (
            <p role="alert">{t("source_list_unavailable")}</p>
          )}
          {review.preview.pendingReconciliation.length > 0 && (
            <p role="status" className="text-sm">
              {t("correction_pending_hint")}
            </p>
          )}
          <p className="whitespace-pre-wrap break-words text-sm">
            {t("operation_reason")} · {review.reason}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !complete || !!write.error} onClick={commit}>
              {t("submit_correction")}
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
        <form onSubmit={preview} className="space-y-5">
          <p className="text-muted-foreground text-sm">{t("correction_signed_hint")}</p>
          <fieldset disabled={locked || isBalanceHidden} className="space-y-4">
            <OwnershipEntryFields
              lines={lines}
              setLines={setLines}
              resources={resources}
              purposes={purposes}
              capture={capture}
            />
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
          {isBalanceHidden && (
            <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
          )}
          <Button
            disabled={
              locked || isBalanceHidden || !resources.length || !purposes.length || !reason.trim()
            }
          >
            {t(reading ? "loading" : "preview_ownership")}
          </Button>
        </form>
      )}
      {(error || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
    </section>
  );
}
