import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { Field, JournalErrorNotice, textAreaClass } from "./controls";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { useInvalidateJournal } from "./queries";
import { useJournalWrite } from "./use-journal-write";

export function DecisionExecutionState({
  decisionId,
  version,
  title,
  revisionNumber,
  operation,
  closeReason,
  close,
}: {
  decisionId: string;
  version: number;
  title: string;
  revisionNumber: number;
  operation: "CLOSE" | "REOPEN";
  closeReason?: string | null;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const [reason, setReason] = useState("");
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const locked = write.busy || write.uncertain;
  const closing = operation === "CLOSE";
  useJournalNavigationGuard(!!reason, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  function submit(event: FormEvent) {
    event.preventDefault();
    if (locked || write.error || (closing && !reason.trim())) return;
    void write.run(
      "update_investment_decision",
      {
        decisionId,
        expectedVersion: version,
        mutation: closing ? { operation, reason: reason.trim() } : { operation },
        requestKey: crypto.randomUUID(),
      },
      () => {
        void invalidate();
        close();
      },
    );
  }
  return (
    <form onSubmit={submit} className="max-w-xl space-y-5">
      <h2 ref={heading} tabIndex={-1} className="text-xl font-semibold">
        {t(closing ? "execution_close" : "execution_reopen")}
      </h2>
      <p className="font-medium [overflow-wrap:anywhere]">
        {title || t("not_recorded")} · {t("review_decision_version", { number: revisionNumber })}
      </p>
      <p className="text-muted-foreground text-sm">
        {t(closing ? "execution_close_hint" : "execution_reopen_hint")}
      </p>
      {!closing && closeReason && (
        <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
          {t("execution_close_reason")} · {closeReason}
        </p>
      )}
      {closing && (
        <Field label={t("execution_close_reason")}>
          {(id) => (
            <textarea
              id={id}
              required
              disabled={locked}
              className={textAreaClass}
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          )}
        </Field>
      )}
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
      <div className="flex flex-wrap gap-3">
        <Button disabled={locked || !!write.error || (closing && !reason.trim())}>
          {t(closing ? "execution_confirm_close" : "execution_confirm_reopen")}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={locked}
          onClick={() => {
            if (allowJournalNavigation()) {
              write.clearError();
              close();
            }
          }}
        >
          {t("back_to_decision")}
        </Button>
      </div>
    </form>
  );
}
