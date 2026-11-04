import { hasOptionContent } from "./types";
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@wealthfolio/ui";
import { executeInvestmentJournal, getAccounts, getAssets } from "@/adapters";
import { useJournalLocale } from "./locale-context";
import { DecisionForm, type CheckSchedule } from "./decision-form";
import { DecisionBodyView } from "./decision-body-view";
import { ContextPanel } from "./context-panel";
import { JournalErrorNotice } from "./controls";
import { journalKey, useJournalQuery, useInvalidateJournal } from "./queries";
import { useJournalWrite } from "./use-journal-write";
import { useJournalNavigationGuard } from "./navigation-guard";
import {
  emptyDecision,
  type ConfirmationPreview,
  type ContextCapture,
  type DecisionBody,
  type DecisionDetail,
  type DecisionTarget,
  type PrincipleSummary,
  type Purpose,
  type Revision,
  type WriteResult,
} from "./types";

export function DecisionEditor({
  detail,
  draft,
  completed,
  focusOnEntry = false,
}: {
  detail?: DecisionDetail;
  draft?: Revision;
  completed: (id: string) => void;
  focusOnEntry?: boolean;
}) {
  const { t } = useJournalLocale();
  const [initial] = useState(() =>
    structuredClone(
      draft?.body ??
        detail?.revisions.find((r) => r.id === detail.currentRevisionId)?.body ??
        emptyDecision(),
    ),
  );
  const [body, setBody] = useState(initial);
  const [base] = useState(() => ({
    decisionId: detail?.decisionId ?? null,
    version: detail?.version ?? null,
  }));
  const [savedCheckDate, setSavedCheckDate] = useState(draft?.nextCheckAt ?? null);
  const [dirty, setDirty] = useState(false);
  const [target, setTarget] = useState<DecisionTarget | null>(() =>
    detail && draft
      ? {
          decisionId: detail.decisionId,
          draftRevisionId: draft.id,
          expectedVersion: detail.version,
        }
      : null,
  );
  const [preview, setPreview] = useState<ConfirmationPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<unknown>(null);
  const [saved, setSaved] = useState(false);
  const draftHeading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (focusOnEntry) draftHeading.current?.focus();
  }, [focusOnEntry]);
  const previewHeading = useRef<HTMLHeadingElement>(null);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  useJournalNavigationGuard(dirty, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  useEffect(() => {
    if (preview) {
      previewHeading.current?.focus({ preventScroll: true });
      previewHeading.current?.scrollIntoView({ block: "start" });
    }
  }, [preview, write.error]);
  const purposes = useJournalQuery<Purpose[]>("list_fund_purposes");
  const principles = useJournalQuery<PrincipleSummary[]>("list_investment_principles", {
    limit: 500,
  });
  const tags = useJournalQuery<string[]>("list_investment_scenario_tags");
  const accounts = useQuery({
    queryKey: [...journalKey(), "accounts"],
    queryFn: () => getAccounts(true),
    retry: false,
  });
  const assets = useQuery({
    queryKey: [...journalKey(), "assets"],
    queryFn: getAssets,
    retry: false,
  });
  const [contextBody, setContextBody] = useState(initial);
  useEffect(() => {
    const timer = window.setTimeout(() => setContextBody(body), 450);
    return () => window.clearTimeout(timer);
  }, [body]);
  const context = useJournalQuery<ContextCapture>(
    "preview_investment_decision_context",
    {
      ...contextBody,
      options: contextBody.options.filter(hasOptionContent),
    },
    !preview,
  );

  async function loadPreview(next: DecisionTarget) {
    setPreviewLoading(true);
    setPreviewError(null);
    try {
      const shown = await executeInvestmentJournal<ConfirmationPreview>(
        "preview_investment_decision_confirmation",
        { ...next },
      );
      setBody(shown.body);
      setSavedCheckDate(shown.nextCheckAt);
      setPreview(shown);
    } catch (error) {
      setPreviewError(error);
    } finally {
      setPreviewLoading(false);
    }
  }
  function save(next: DecisionBody, checkSchedule: CheckSchedule, prepare: boolean) {
    setSaved(false);
    setPreviewError(null);
    void write.run<WriteResult>(
      "save_investment_decision_draft",
      {
        decisionId: target?.decisionId ?? base.decisionId,
        draftRevisionId: target?.draftRevisionId ?? null,
        expectedVersion: target?.expectedVersion ?? base.version,
        body: next,
        checkSchedule,
        requestKey: crypto.randomUUID(),
      },
      (result) => {
        const nextTarget = {
          decisionId: result.decisionId!,
          draftRevisionId: result.draftRevisionId!,
          expectedVersion: result.version!,
        };
        setTarget(nextTarget);
        setSaved(true);
        setDirty(false);
        if (checkSchedule.operation === "SET") setSavedCheckDate(checkSchedule.date);
        if (checkSchedule.operation === "CANCEL") setSavedCheckDate(null);
        void invalidate();
        if (prepare) void loadPreview(nextTarget);
      },
    );
  }
  if (preview)
    return (
      <div className="space-y-6">
        <div className="space-y-2">
          <h2
            ref={previewHeading}
            tabIndex={-1}
            className="scroll-mt-28 text-xl font-semibold outline-none"
          >
            {t("confirmation_title")} · {t("version", { number: preview.revisionNumber })}
          </h2>
          <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
            {t("confirmation_hint")}
          </p>
        </div>
        {write.error && (
          <JournalErrorNotice
            error={write.error}
            replay={write.uncertain ? () => void write.retry() : undefined}
            busy={write.busy}
          />
        )}
        <div className="grid items-start gap-8 xl:grid-cols-[minmax(0,1fr)_20rem]">
          <div className="min-w-0">
            <DecisionBodyView body={preview.body} capture={preview.context} />
            <p className="mt-5 text-sm">
              {t("next_check")}: {preview.nextCheckAt ?? t("not_recorded")}
            </p>
            <div className="bg-background sticky bottom-0 mt-6 flex flex-wrap justify-end gap-3 border-t py-4">
              <Button
                variant="outline"
                disabled={write.busy || write.uncertain}
                onClick={() => {
                  setPreview(null);
                  write.clearError();
                }}
              >
                {t("return_edit")}
              </Button>
              <Button
                disabled={write.busy || write.uncertain || !!write.error}
                onClick={() =>
                  void write.run<WriteResult>(
                    "confirm_investment_decision",
                    {
                      ledgerId: preview.ledgerId,
                      target: preview.target,
                      expectedContentHash: preview.expectedContentHash,
                      expectedContextHash: preview.expectedContextHash,
                      context: preview.context,
                      requestKey: crypto.randomUUID(),
                    },
                    (result) => {
                      void invalidate();
                      completed(result.decisionId!);
                    },
                  )
                }
              >
                {t("confirm")}
              </Button>
            </div>
          </div>
          <ContextPanel capture={preview.context} frozen />
        </div>
      </div>
    );
  return (
    <div className="space-y-5">
      {focusOnEntry && (
        <h2 ref={draftHeading} tabIndex={-1} className="text-xl font-semibold">
          {t("queue_open_draft")}
        </h2>
      )}
      {[purposes, principles, accounts, assets, tags].some((query) => query.isError) && (
        <div role="alert" className="space-y-3 rounded-xl border p-4">
          <p className="text-sm">{t("context_error")}</p>
          <Button
            variant="outline"
            onClick={() => {
              void Promise.allSettled([
                purposes.refetch(),
                principles.refetch(),
                accounts.refetch(),
                assets.refetch(),
                tags.refetch(),
              ]);
            }}
          >
            {t("retry")}
          </Button>
        </div>
      )}
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
      {previewError != null && <JournalErrorNotice error={previewError} />}
      {previewLoading && (
        <p role="status" className="text-muted-foreground text-sm">
          {t("loading")}
        </p>
      )}
      {target && detail?.lifecycle !== "ARCHIVED" && (
        <Button
          type="button"
          variant="outline"
          disabled={write.busy || write.uncertain || previewLoading}
          onClick={() => {
            if (!window.confirm(t("discard_decision_confirm"))) return;
            void write.run(
              "update_investment_decision",
              {
                decisionId: target.decisionId,
                expectedVersion: target.expectedVersion,
                mutation: { operation: "DISCARD_DRAFT", draftRevisionId: target.draftRevisionId },
                requestKey: crypto.randomUUID(),
              },
              () => {
                setDirty(false);
                void invalidate();
                completed(target.decisionId);
              },
            );
          }}
        >
          {t("discard_decision_draft")}
        </Button>
      )}
      <div className="grid items-start gap-8 xl:grid-cols-[minmax(0,1fr)_20rem]">
        <DecisionForm
          initial={body}
          nextCheckAt={savedCheckDate}
          busy={write.busy || previewLoading}
          saved={saved}
          locked={write.uncertain}
          accounts={accounts.data ?? []}
          assets={(assets.data ?? []).filter((asset) => asset.kind === "INVESTMENT")}
          purposes={purposes.data ?? []}
          purposesReady={purposes.isSuccess}
          principles={principles.data ?? []}
          tags={tags.data ?? []}
          changed={(next) => {
            setSaved(false);
            setDirty(true);
            setBody(next);
          }}
          save={save}
        />
        <ContextPanel
          capture={context.data}
          loading={context.isFetching}
          error={context.isError}
          retry={() => void context.refetch()}
        />
      </div>
    </div>
  );
}
