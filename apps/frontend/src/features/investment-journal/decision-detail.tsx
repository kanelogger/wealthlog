import { useState } from "react";
import { cn } from "@/lib/utils";
import { Button } from "@wealthfolio/ui";
import { useJournalLocale } from "./locale-context";
import { useInvalidateJournal, useJournalQuery } from "./queries";
import { useJournalWrite } from "./use-journal-write";
import { JournalErrorNotice, selectClass } from "./controls";
import { DecisionReviews, DecisionReviewEditor, type ReviewEditorMode } from "./decision-review";
import { DecisionReviewReminder } from "./decision-review-reminder";
import { DecisionExecutionState } from "./decision-execution-state";
import { DecisionEditor } from "./decision-editor";
import { DecisionBodyView } from "./decision-body-view";
import { ContextPanel } from "./context-panel";
import { DecisionTradeComposer } from "./decision-trade";
import { DecisionExecutionLinks } from "./decision-execution-links";
import { TradeProgress } from "./trade-progress";
import type { ExecutionProgress, TradeBinding } from "./trade-types";
import { allowJournalNavigation } from "./navigation-guard";
import type { DecisionDetail as Detail } from "./types";

export function DecisionDetail({
  id,
  task,
  taskRevision,
  taskDate,
  pendingExecution,
  historicalExecution,
  draftPrincipleFromReview,
}: {
  id: string;
  task?: string | null;
  taskRevision?: string | null;
  taskDate?: string | null;
  pendingExecution?: string | null;
  historicalExecution?: boolean;
  draftPrincipleFromReview?: (reviewId: string) => void;
}) {
  const { t } = useJournalLocale();
  const query = useJournalQuery<Detail>("get_investment_decision", { decisionId: id });
  const [executionEditorActive, setExecutionEditorActive] = useState(false);
  const [routeAction, setRouteAction] = useState(true);
  const [reminder, setReminder] = useState<{ version: number; due: string | null } | null>(null);
  const [executionMutation, setExecutionMutation] = useState<{
    operation: "CLOSE" | "REOPEN";
    version: number;
    title: string;
    revisionNumber: number;
    reason?: string | null;
  } | null>(null);
  const [editing, setEditing] = useState(false);
  const [reviewMode, setReviewMode] = useState<{
    mode: ReviewEditorMode;
    revisionId: string;
    revisionNumber: number;
    canWrite: boolean;
  } | null>(null);
  const [recording, setRecording] = useState<TradeBinding | null>(null);
  const [revisionId, setRevisionId] = useState<string | null>(null);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const detail = query.data;
  if (executionMutation)
    return (
      <DecisionExecutionState
        decisionId={id}
        version={executionMutation.version}
        title={executionMutation.title}
        revisionNumber={executionMutation.revisionNumber}
        operation={executionMutation.operation}
        closeReason={executionMutation.reason}
        close={() => {
          setExecutionMutation(null);
          void query.refetch();
        }}
      />
    );
  if (reminder)
    return (
      <DecisionReviewReminder
        decisionId={id}
        version={reminder.version}
        due={reminder.due}
        close={() => {
          setReminder(null);
          void query.refetch();
        }}
      />
    );
  if (reviewMode)
    return (
      <DecisionReviewEditor
        revisionId={reviewMode.revisionId}
        revisionNumber={reviewMode.revisionNumber}
        mode={reviewMode.mode}
        draftPrinciple={draftPrincipleFromReview}
        canWrite={reviewMode.canWrite}
        close={() => setReviewMode(null)}
      />
    );
  if (recording)
    return (
      <DecisionTradeComposer
        binding={recording}
        close={() => {
          setRecording(null);
          void query.refetch();
        }}
      />
    );
  if (!detail)
    return (
      <div className="space-y-4">
        <p role="status">{t(query.isError ? "context_error" : "loading")}</p>
        {query.isError && (
          <Button variant="outline" onClick={() => void query.refetch()}>
            {t("retry")}
          </Button>
        )}
      </div>
    );
  const draft = detail.revisions.find((revision) => revision.id === detail.draftRevisionId);
  const current = detail.revisions.find(
    (revision) => revision.id === (revisionId ?? detail.currentRevisionId),
  );
  const openDueDraft =
    routeAction &&
    task === "DRAFT_CHECK_DUE" &&
    !!draft &&
    draft.id === taskRevision &&
    draft.nextCheckAt === taskDate &&
    detail.lifecycle !== "ARCHIVED";
  if (editing || openDueDraft || (!current && draft && !(routeAction && task)))
    return (
      <DecisionEditor
        key={`editor-${id}`}
        detail={detail}
        draft={draft}
        focusOnEntry={openDueDraft}
        completed={() => {
          setEditing(false);
          setRouteAction(false);
          void query.refetch();
        }}
      />
    );
  if (!current)
    return (
      <div className="space-y-4">
        <p role="status">{t(routeAction && task ? "queue_task_changed" : "empty_filter")}</p>
        {draft && detail.lifecycle !== "ARCHIVED" && (
          <Button
            variant="outline"
            onClick={() => {
              setRouteAction(false);
              setEditing(true);
            }}
          >
            {t("continue_draft")}
          </Button>
        )}
      </div>
    );
  const trade =
    current.status === "CONFIRMED" &&
    (current.body.action === "BUY" || current.body.action === "SELL")
      ? {
          decisionId: id,
          revisionId: current.id,
          revisionNumber: current.revisionNumber,
          body: current.body,
        }
      : null;
  const isCurrent = detail.lifecycle === "CONFIRMED" && current.id === detail.currentRevisionId;
  const openDueReview =
    routeAction &&
    task === "REVIEW_DUE" &&
    isCurrent &&
    current.id === taskRevision &&
    detail.nextReviewAt === taskDate;
  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-2 text-sm">
          <span className="rounded-md border px-2 py-1">
            {t(detail.lifecycle === "ARCHIVED" ? "archived" : "confirmed")}
          </span>
          <span className="rounded-md border px-2 py-1">{t(detail.executionState)}</span>
        </div>
        <div className="flex flex-wrap gap-2">
          {trade && detail.lifecycle === "CONFIRMED" && current.id === detail.currentRevisionId && (
            <Button
              disabled={write.busy || write.uncertain}
              onClick={() => {
                if (!allowJournalNavigation()) return;
                write.clearError();
                setRecording(trade);
              }}
            >
              {t("record_confirmed_trade", { number: trade.revisionNumber })}
            </Button>
          )}
          {isCurrent && detail.executionState !== "NOT_APPLICABLE" && (
            <Button
              variant="outline"
              disabled={write.busy || write.uncertain}
              onClick={() => {
                if (!allowJournalNavigation()) return;
                write.clearError();
                setRouteAction(false);
                setExecutionMutation({
                  operation: detail.executionState === "OPEN" ? "CLOSE" : "REOPEN",
                  version: detail.version,
                  title: current.body.title,
                  revisionNumber: current.revisionNumber,
                  reason: detail.closeReason,
                });
              }}
            >
              {t(detail.executionState === "OPEN" ? "execution_close" : "execution_reopen")}
            </Button>
          )}
          <select
            aria-label={t("decisions")}
            className={cn(selectClass, "w-auto")}
            value={current.id}
            onChange={(event) => {
              if (allowJournalNavigation()) setRevisionId(event.target.value);
            }}
          >
            {detail.revisions
              .filter((revision) => revision.status === "CONFIRMED")
              .map((revision) => (
                <option key={revision.id} value={revision.id}>
                  {t("version", { number: revision.revisionNumber })}
                </option>
              ))}
          </select>
          {detail.lifecycle !== "ARCHIVED" && (
            <Button
              variant="outline"
              onClick={() => {
                if (allowJournalNavigation()) setEditing(true);
              }}
            >
              {t(draft ? "continue_draft" : "amend")}
            </Button>
          )}
          <Button
            variant="ghost"
            disabled={write.busy || write.uncertain}
            onClick={() => {
              if (!allowJournalNavigation()) return;
              void write.run(
                "update_investment_decision",
                {
                  decisionId: id,
                  expectedVersion: detail.version,
                  mutation: { operation: detail.lifecycle === "ARCHIVED" ? "RESTORE" : "ARCHIVE" },
                  requestKey: crypto.randomUUID(),
                },
                () => {
                  void invalidate();
                },
              );
            }}
          >
            {t(detail.lifecycle === "ARCHIVED" ? "restore" : "archive")}
          </Button>
        </div>
      </div>
      {routeAction && task && !openDueReview && (
        <p role="status" className="text-sm">
          {t("queue_task_changed")}
        </p>
      )}
      {detail.executionState === "CLOSED" && detail.closeReason && (
        <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
          {t("execution_close_reason")} · {detail.closeReason}
        </p>
      )}
      {draft && <p className="text-muted-foreground text-sm">{t("draft_separate")}</p>}
      {write.error && !executionEditorActive && (
        <JournalErrorNotice
          error={write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
      <p className="text-muted-foreground text-sm">
        {t("system_confirmed_at", { date: current.confirmedAt ?? "" })} ·{" "}
        {current.confirmedActorKind ? t(current.confirmedActorKind) : t("not_recorded")}
      </p>
      <div className="grid items-start gap-8 xl:grid-cols-[minmax(0,1fr)_20rem]">
        <div>
          <DecisionBodyView body={current.body} capture={current.context ?? undefined} />
          <p className="mt-5 text-sm">
            {t("next_check")}: {current.nextCheckAt ?? t("not_recorded")}
          </p>
          {trade && <DecisionProgress binding={trade} />}
          {current.status === "CONFIRMED" && (
            <DecisionReviews
              revisionId={current.id}
              revisionNumber={current.revisionNumber}
              autoFocus={openDueReview}
              due={isCurrent ? detail.nextReviewAt : null}
              editReminder={
                isCurrent
                  ? () => {
                      if (!allowJournalNavigation()) return;
                      write.clearError();
                      setRouteAction(false);
                      setReminder({ version: detail.version, due: detail.nextReviewAt });
                    }
                  : undefined
              }
              canWrite={detail.lifecycle === "CONFIRMED"}
              open={(mode) => {
                if (allowJournalNavigation()) {
                  write.clearError();
                  setRouteAction(false);
                  setReviewMode({
                    mode,
                    revisionId: current.id,
                    revisionNumber: current.revisionNumber,
                    canWrite: detail.lifecycle === "CONFIRMED",
                  });
                }
              }}
            />
          )}
          {current.status === "CONFIRMED" && (
            <DecisionExecutionLinks
              key={current.id}
              decisionId={id}
              revisionId={current.id}
              revisionNumber={current.revisionNumber}
              capture={current.context ?? undefined}
              onEditorActive={setExecutionEditorActive}
              initialExecutionId={
                routeAction && isCurrent ? (pendingExecution ?? undefined) : undefined
              }
              initialHistorical={historicalExecution}
              canLink={detail.lifecycle === "CONFIRMED"}
            />
          )}
        </div>
        {current.context && <ContextPanel capture={current.context} frozen />}
      </div>
    </div>
  );
}

function DecisionProgress({ binding }: { binding: TradeBinding }) {
  const { t } = useJournalLocale();
  const query = useJournalQuery<ExecutionProgress>("get_investment_execution_progress", {
    decisionRevisionId: binding.revisionId,
  });
  return (
    <div className="mt-5 space-y-3">
      {!query.data && <p role="status">{t(query.isError ? "records_error" : "loading")}</p>}
      {query.isError && (
        <Button variant="outline" onClick={() => void query.refetch()}>
          {t("retry")}
        </Button>
      )}
      <TradeProgress binding={binding} progress={query.isError ? undefined : query.data} />
    </div>
  );
}
