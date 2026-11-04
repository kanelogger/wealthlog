import { useState } from "react";
import { Button } from "@wealthfolio/ui";
import { Field, JournalErrorNotice, textAreaClass } from "./controls";
import { FundEditReview } from "./fund-edit-review";
import type { FundHistoryItem, FundReversalPreview } from "./fund-types";
import { useJournalLocale } from "./locale-context";
import { useJournalNavigationGuard } from "./navigation-guard";
import { useInvalidateJournal, useJournalQuery } from "./queries";
import type { ContextCapture, Purpose } from "./types";
import { useJournalWrite } from "./use-journal-write";

export function FundReversal({
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
  const [reason, setReason] = useState("");
  const preview = useJournalQuery<FundReversalPreview>("preview_fund_reversal", {
    operationId: item.id,
  });
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const locked = write.busy || write.uncertain;
  useJournalNavigationGuard(!!reason, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  return (
    <form
      className="max-w-3xl space-y-6"
      onSubmit={(event) => {
        event.preventDefault();
        if (!preview.data) return;
        void write.run(
          "reverse_fund_operation",
          {
            operationId: item.id,
            reason,
            expectedPreviewHash: preview.data.previewHash,
            requestKey: crypto.randomUUID(),
          },
          () => {
            void invalidate();
            close();
          },
        );
      }}
    >
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">{t("reverse_ownership")}</h2>
        <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
          {t("reverse_ownership_hint")}
        </p>
        <p className="break-words text-sm">
          {item.createdAt} · {item.reason}
        </p>
      </div>
      {preview.isLoading && <p role="status">{t("loading")}</p>}
      {preview.isError && (
        <div className="space-y-3">
          <JournalErrorNotice error={preview.error} />
          <Button type="button" variant="outline" onClick={() => void preview.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {preview.data && (
        <>
          <FundEditReview preview={preview.data} capture={capture} purposes={purposes} />
          {preview.data.pendingReconciliation.length > 0 && (
            <p role="status" className="text-sm">
              {t("reversal_keeps_issues")}
            </p>
          )}
          <fieldset disabled={locked}>
            <Field label={t("operation_reason")}>
              {(id) => (
                <textarea
                  id={id}
                  required
                  className={textAreaClass}
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              )}
            </Field>
          </fieldset>
        </>
      )}
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
      <div className="flex flex-wrap gap-3">
        <Button disabled={!preview.data || preview.isFetching || preview.isError || locked}>
          {t("submit_reversal")}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={locked}
          onClick={() => {
            if (!reason || window.confirm(t("discard_navigation"))) close();
          }}
        >
          {t("cancel")}
        </Button>
        {write.error && !write.uncertain && (
          <Button
            type="button"
            variant="outline"
            disabled={preview.isFetching}
            onClick={() => {
              write.clearError();
              void preview.refetch();
            }}
          >
            {t("preview_again")}
          </Button>
        )}
      </div>
    </form>
  );
}
