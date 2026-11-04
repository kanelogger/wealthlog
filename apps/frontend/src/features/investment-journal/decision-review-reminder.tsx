import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Field, JournalErrorNotice } from "./controls";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { useInvalidateJournal } from "./queries";
import { useJournalWrite } from "./use-journal-write";

export function DecisionReviewReminder({
  decisionId,
  version,
  due,
  close,
}: {
  decisionId: string;
  version: number;
  due: string | null;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const [date, setDate] = useState(due ?? "");
  const heading = useRef<HTMLHeadingElement>(null);
  const invalidate = useInvalidateJournal();
  const write = useJournalWrite();
  const locked = write.busy || write.uncertain;
  const dirty = date !== (due ?? "");
  useEffect(() => heading.current?.focus(), []);
  useJournalNavigationGuard(dirty, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  function writeReminder(
    mutation: { operation: "SET_REVIEW_DATE"; date: string } | { operation: "CANCEL_REVIEW" },
  ) {
    if (locked || write.error) return;
    void write.run(
      "update_investment_decision",
      {
        decisionId,
        expectedVersion: version,
        mutation,
        requestKey: crypto.randomUUID(),
      },
      () => {
        void invalidate();
        close();
      },
    );
  }
  function save(event: FormEvent) {
    event.preventDefault();
    if (date && dirty) writeReminder({ operation: "SET_REVIEW_DATE", date });
  }
  return (
    <form className="max-w-xl space-y-5" onSubmit={save}>
      <h2 ref={heading} tabIndex={-1} className="text-xl font-semibold">
        {t("review_reminder")}
      </h2>
      <p className="text-muted-foreground text-sm">{t("review_reminder_hint")}</p>
      {due && <p className="text-sm">{t("review_current_reminder", { date: due })}</p>}
      <Field label={t("review_reminder_date")}>
        {(id) => (
          <Input
            id={id}
            type="date"
            required
            disabled={locked}
            value={date}
            onChange={(event) => setDate(event.target.value)}
          />
        )}
      </Field>
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
      <div className="flex flex-wrap gap-3">
        <Button disabled={locked || !!write.error || !dirty || !date}>
          {t("review_save_reminder")}
        </Button>
        {due && (
          <Button
            type="button"
            variant="outline"
            disabled={locked || !!write.error}
            onClick={() => writeReminder({ operation: "CANCEL_REVIEW" })}
          >
            {t("review_cancel_reminder")}
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
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
