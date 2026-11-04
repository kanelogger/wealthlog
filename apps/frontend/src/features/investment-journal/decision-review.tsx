import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { executeInvestmentJournal } from "@/adapters";
import { Field, JournalErrorNotice } from "./controls";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { useInvalidateJournal, useJournalQuery } from "./queries";
import { ReviewBodyView, ReviewFields } from "./review-fields";
import { ReviewFactsView } from "./review-facts-view";
import {
  newReviewBody,
  type ReviewPreview,
  type ReviewRecord,
  type ReviewPrinciple,
  type ReviewSaveResult,
} from "./review-types";
import { useJournalWrite } from "./use-journal-write";
import type { JournalStatus } from "./types";

type EditorMode = { kind: "new" } | { kind: "open" | "amend"; record: ReviewRecord };

export function DecisionReviews({
  revisionId,
  revisionNumber,
  open,
  canWrite,
  autoFocus,
  due,
  editReminder,
}: {
  revisionId: string;
  revisionNumber: number;
  open: (mode: EditorMode) => void;
  canWrite: boolean;
  autoFocus?: boolean;
  due?: string | null;
  editReminder?: () => void;
}) {
  const { t } = useJournalLocale();
  const title = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (autoFocus) title.current?.focus();
  }, [autoFocus]);
  const query = useJournalQuery<ReviewRecord[]>("list_investment_reviews", {
    decisionRevisionId: revisionId,
  });
  return (
    <section className="mt-6 space-y-4 border-t pt-5">
      <div className="flex flex-wrap justify-between gap-3">
        <h3 ref={title} tabIndex={-1} className="font-semibold">
          {t("decision_reviews")} · {t("review_decision_version", { number: revisionNumber })}
        </h3>
        {canWrite && (
          <Button variant="outline" onClick={() => open({ kind: "new" })}>
            {t("review_new")}
          </Button>
        )}
      </div>
      {editReminder && (
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <p>{due ? t("review_current_reminder", { date: due }) : t("review_no_reminder")}</p>
          <Button variant="outline" onClick={editReminder}>
            {t("review_reminder")}
          </Button>
        </div>
      )}
      {query.isLoading && <p role="status">{t("loading")}</p>}
      {query.isError && (
        <div className="space-y-3">
          <p role="alert">{t("records_error")}</p>
          <Button variant="outline" onClick={() => void query.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {query.data?.length === 0 && (
        <p className="text-muted-foreground text-sm">{t("review_empty")}</p>
      )}
      <ul className="divide-y">
        {query.data?.map((row) => {
          const siblings = query.data.filter((item) => item.rootId === row.rootId);
          const canAmend =
            canWrite &&
            row.status === "SUBMITTED" &&
            !siblings.some(
              (item) =>
                item.status === "DRAFT" ||
                (item.status === "SUBMITTED" && item.revisionNumber > row.revisionNumber),
            );
          const resume =
            canWrite &&
            row.status === "DISCARDED" &&
            !siblings.some(
              (item) => item.status === "DRAFT" || item.revisionNumber > row.revisionNumber,
            );
          return (
            <li key={row.id} className="flex flex-wrap items-center justify-between gap-3 py-4">
              <p className="text-sm">
                {t("review_cutoff")} · {row.observationCutoff} · {row.observationTimezone} ·{" "}
                {t("review_record_version", { number: row.revisionNumber })} ·{" "}
                {t(
                  row.status === "DRAFT"
                    ? "draft"
                    : row.status === "SUBMITTED"
                      ? "review_submitted"
                      : "review_discarded",
                )}
              </p>
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" onClick={() => open({ kind: "open", record: row })}>
                  {t(row.status === "DRAFT" && canWrite ? "continue_draft" : "review_view")}
                </Button>
                {(canAmend || resume) && (
                  <Button variant="ghost" onClick={() => open({ kind: "amend", record: row })}>
                    {t(canAmend ? "review_amend" : "review_restart")}
                  </Button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

export type ReviewEditorMode = EditorMode;
export function DecisionReviewEditor({
  revisionId,
  revisionNumber,
  mode,
  canWrite,
  draftPrinciple,
  close,
}: {
  revisionId: string;
  revisionNumber: number;
  mode: EditorMode;
  draftPrinciple?: (id: string) => void;
  canWrite: boolean;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const original = mode.kind === "new" ? null : mode.record;
  const readonly = mode.kind === "open" && (original?.status !== "DRAFT" || !canWrite);
  const [body, setBody] = useState(original?.body ?? newReviewBody);
  const [tags, setTags] = useState(body.scenarioTags.join(", "));
  const [cutoff, setCutoff] = useState(original?.observationCutoff ?? "");
  const [saved, setSaved] = useState<Pick<
    ReviewSaveResult,
    "reviewId" | "version" | "rootId" | "revisionNumber"
  > | null>(
    mode.kind === "open" && original?.status === "DRAFT"
      ? {
          reviewId: original.id,
          version: original.version,
          rootId: original.rootId,
          revisionNumber: original.revisionNumber,
        }
      : null,
  );
  const [dirty, setDirty] = useState(false);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [preview, setPreview] = useState<ReviewPreview | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const status = useJournalQuery<JournalStatus>("get_investment_journal_status", {}, !readonly);
  const principles = useJournalQuery<ReviewPrinciple[]>(
    "list_investment_principles",
    {},
    !readonly,
  );
  const tagSuggestions = useJournalQuery<string[]>("list_investment_scenario_tags", {}, !readonly);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const locked = reading || write.busy || write.uncertain;
  const phase = submitted ? "submitted" : preview ? "preview" : readonly ? "read" : "edit";
  useEffect(() => heading.current?.focus(), [phase]);
  useJournalNavigationGuard(dirty || reading, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });

  function save(event: FormEvent) {
    event.preventDefault();
    if (locked || readonly || !cutoff) return;
    setError(null);
    const payload = {
      ...body,
      scenarioTags: tags
        .split(/[,，]/)
        .map((tag) => tag.trim())
        .filter(Boolean),
    };
    void write.run<ReviewSaveResult>(
      "save_investment_review_draft",
      {
        reviewId: saved?.reviewId ?? null,
        rootId: saved?.rootId ?? (mode.kind === "amend" ? original?.rootId : null),
        expectedVersion: saved?.version ?? null,
        decisionRevisionId: revisionId,
        observationCutoff: cutoff,
        body: payload,
        requestKey: crypto.randomUUID(),
      },
      (result) => {
        setSaved(result);
        setDirty(false);
        void invalidate();
      },
    );
  }
  async function readPreview() {
    if (!saved || dirty || locked) return;
    setError(null);
    write.clearError();
    setReading(true);
    try {
      const target = { reviewId: saved.reviewId, expectedVersion: saved.version };
      const result = await executeInvestmentJournal<ReviewPreview>(
        "preview_investment_review_submission",
        target,
      );
      if (
        result.target.reviewId !== target.reviewId ||
        result.target.expectedVersion !== target.expectedVersion ||
        result.facts.query.decisionRevisionId !== revisionId ||
        result.facts.query.observationCutoff !== cutoff ||
        !result.expectedContentHash ||
        !result.expectedFactsHash ||
        !result.ledgerId
      )
        throw new Error(t("records_error"));
      setPreview(result);
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }
  function submit() {
    if (!preview || locked || write.error) return;
    const { ledgerId, target, expectedContentHash, expectedFactsHash, facts } = preview;
    void write.run(
      "submit_investment_review",
      {
        ledgerId,
        target,
        expectedContentHash,
        expectedFactsHash,
        facts,
        requestKey: crypto.randomUUID(),
      },
      () => {
        setSubmitted(true);
        void invalidate();
      },
    );
  }
  function discard() {
    if (!saved || locked || !window.confirm(t("review_discard_confirm"))) return;
    void write.run(
      "discard_investment_review_draft",
      {
        target: { reviewId: saved.reviewId, expectedVersion: saved.version },
        requestKey: crypto.randomUUID(),
      },
      () => {
        setDirty(false);
        void invalidate();
        close();
      },
    );
  }
  const sourceReviewId = !dirty ? (saved?.reviewId ?? original?.id) : undefined;
  const canPreview =
    !!saved &&
    !dirty &&
    !!body.judgment.trim() &&
    !!body.executionAssessment.trim() &&
    body.outcomeEvidence.every((evidence) => !!evidence.content.trim());
  return (
    <section className="max-w-4xl space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-2">
          <h2
            ref={phase === "read" || phase === "edit" ? heading : undefined}
            tabIndex={-1}
            className="text-xl font-semibold"
          >
            {t("decision_reviews")} · {t("review_decision_version", { number: revisionNumber })}
          </h2>
          {(saved || (original && mode.kind === "open")) && (
            <p className="text-sm">
              {t("review_record_version", {
                number: saved?.revisionNumber ?? original?.revisionNumber ?? 1,
              })}
            </p>
          )}
          {!saved && original && mode.kind === "amend" && (
            <p className="text-sm">
              {t("review_based_on_version", { number: original.revisionNumber })}
            </p>
          )}
          {original && (
            <p className="text-sm">
              {t("review_cutoff")} · {original.observationCutoff} · {original.observationTimezone}
            </p>
          )}
          {original?.submittedAt && (
            <p className="text-sm">
              {t("review_submitted")} · {original.submittedAt} ·{" "}
              {original.submittedActorKind ? t(original.submittedActorKind) : t("unavailable")}
            </p>
          )}
        </div>
        {draftPrinciple && sourceReviewId && !preview && (
          <Button
            variant="outline"
            disabled={locked}
            onClick={() => draftPrinciple(sourceReviewId)}
          >
            {t("principle_from_review")}
          </Button>
        )}
        <Button
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
      </header>
      {submitted ? (
        <h3 ref={heading} tabIndex={-1} role="status" className="font-semibold">
          {t("review_saved_submission")}
        </h3>
      ) : preview ? (
        <>
          <h3 ref={heading} tabIndex={-1} className="font-semibold">
            {t("review_submission_preview")}
          </h3>
          <ReviewBodyView body={preview.body} snapshots={preview.facts.observations.principles} />
          <ReviewFactsView facts={preview.facts} />
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !!write.error} onClick={submit}>
              {t("review_submit")}
            </Button>
            <Button
              variant="outline"
              disabled={locked}
              onClick={() => {
                write.clearError();
                setPreview(null);
              }}
            >
              {t("return_edit")}
            </Button>
          </div>
        </>
      ) : readonly && original ? (
        <>
          <ReviewBodyView
            body={original.body}
            snapshots={original.factsSnapshot?.observations.principles}
          />
          {original.factsSnapshot && <ReviewFactsView facts={original.factsSnapshot} />}
        </>
      ) : (
        <form onSubmit={save} className="space-y-5">
          <fieldset disabled={locked} className="space-y-5">
            <Field label={t("review_cutoff")} hint={t("review_cutoff_hint")}>
              {(id) => (
                <Input
                  id={id}
                  type="date"
                  required
                  max={status.data?.asOfDate}
                  readOnly={!!saved || !!original}
                  value={cutoff}
                  onChange={(event) => {
                    setCutoff(event.target.value);
                    setDirty(true);
                  }}
                />
              )}
            </Field>
            {principles.isError && (
              <div className="space-y-2">
                <p role="alert">{t("records_error")}</p>
                <Button type="button" variant="outline" onClick={() => void principles.refetch()}>
                  {t("retry")}
                </Button>
              </div>
            )}
            <ReviewFields
              body={body}
              change={(value) => {
                setBody(value);
                setDirty(true);
              }}
              principles={principles.data ?? []}
              suggestions={tagSuggestions.data ?? []}
              tags={tags}
              setTags={(value) => {
                setTags(value);
                setDirty(true);
              }}
            />
          </fieldset>
          {saved && !dirty && (
            <p role="status" className="text-sm">
              {t("saved")}
            </p>
          )}
          {!canPreview && (
            <p className="text-muted-foreground text-sm">{t("review_preview_hint")}</p>
          )}
          <div className="flex flex-wrap gap-3">
            <Button
              variant={canPreview ? "outline" : "default"}
              disabled={locked || !cutoff || (!!saved && !dirty)}
            >
              {t("save_draft")}
            </Button>
            <Button
              type="button"
              variant={canPreview ? "default" : "outline"}
              disabled={locked || !canPreview}
              onClick={() => void readPreview()}
            >
              {t("review_submission_preview")}
            </Button>
            {saved && (
              <Button type="button" variant="ghost" disabled={locked} onClick={discard}>
                {t("review_discard")}
              </Button>
            )}
          </div>
        </form>
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
