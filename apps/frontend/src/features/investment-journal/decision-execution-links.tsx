import type { Execution, WorkQueue } from "./execution-work-types";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@wealthfolio/ui";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { executeInvestmentJournal, getAccounts, getAssets } from "@/adapters";
import { ExactValue, Field, JournalErrorNotice, selectClass, textAreaClass } from "./controls";
import { sourceLabel } from "./context-labels";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { journalKey, useInvalidateJournal, useJournalQuery } from "./queries";
import type { ContextCapture } from "./types";
import { useJournalWrite } from "./use-journal-write";

interface LinkTarget {
  executionId: string;
  decisionRevisionId: string;
  deviationReason: string | null;
}
interface EvidenceSnapshot {
  execution: Execution;
  decision: { id: string; revisionNumber: number; confirmedAt: string };
}
interface LinkPreview {
  target: LinkTarget;
  temporalClassification: string;
  sourceVersion: string;
  snapshot: EvidenceSnapshot;
  previewHash: string;
}
interface ExecutionLink extends Omit<LinkPreview, "target" | "previewHash"> {
  id: string;
  decisionRevisionId: string;
  revisionNumber: number;
  linkedAt: string;
  unlinkedAt: string | null;
  deviationReason: string | null;
  unlinkReason: string | null;
}
const timing: Record<string, JournalTextKey> = {
  BEFORE_CONFIRMATION: "execution_before_confirmation",
  AFTER_CONFIRMATION: "execution_after_confirmation",
  UNDETERMINED: "execution_timing_unknown",
};

function Evidence({
  evidence,
  label,
}: {
  evidence: Pick<LinkPreview, "snapshot" | "temporalClassification" | "sourceVersion">;
  label: (id: string) => string;
}) {
  const { t } = useJournalLocale();
  const { execution, decision } = evidence.snapshot;
  return (
    <div className="space-y-3 text-sm">
      <p className="[overflow-wrap:anywhere]">
        {label(execution.accountId)} · {label(execution.assetId)} ·{" "}
        {t(execution.side === "SELL" ? "action_SELL" : "action_BUY")} · {execution.occurredAt}
      </p>
      <p>
        {t("system_confirmed_at", { date: decision.confirmedAt })} ·{" "}
        {t(timing[evidence.temporalClassification] ?? "execution_timing_unknown")}
      </p>
      <dl className="grid gap-3 sm:grid-cols-2">
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
      <p className="text-muted-foreground break-all text-xs">
        {t("source_original_version")} · {evidence.sourceVersion}
      </p>
    </div>
  );
}

export function DecisionExecutionLinks({
  decisionId,
  revisionId,
  revisionNumber,
  capture,
  canLink,
  onEditorActive,
  initialExecutionId,
  initialHistorical,
}: {
  decisionId: string;
  revisionId: string;
  revisionNumber: number;
  capture?: ContextCapture;
  canLink: boolean;
  onEditorActive: (active: boolean) => void;
  initialExecutionId?: string;
  initialHistorical?: boolean;
}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const [adding, setAdding] = useState(!!initialExecutionId);
  const [historical, setHistorical] = useState(!!initialHistorical);
  const [executionId, setExecutionId] = useState(initialExecutionId ?? "");
  const [deviation, setDeviation] = useState("");
  const [preview, setPreview] = useState<LinkPreview | null>(null);
  const [unlink, setUnlink] = useState<ExecutionLink | null>(null);
  const [reason, setReason] = useState("");
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [notice, setNotice] = useState<"execution_link_saved" | "execution_unlink_saved" | null>(
    null,
  );
  const heading = useRef<HTMLHeadingElement>(null);
  const focusEditor = useRef(!!initialExecutionId);
  const linkBaseline = useRef({
    executionId: initialExecutionId ?? "",
    historical: !!initialHistorical,
  });
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const links = useJournalQuery<ExecutionLink[]>("list_investment_execution_links", { decisionId });
  const queue = useJournalQuery<WorkQueue>(
    "get_investment_work_queue",
    {
      includeHistoricalExecutions: historical,
    },
    adding && !preview,
  );
  const namesNeeded = adding || (links.data?.length ?? 0) > 0;
  const accounts = useQuery({
    queryKey: [...journalKey(), "accounts"],
    queryFn: () => getAccounts(true),
    enabled: namesNeeded,
    retry: false,
  });
  const assets = useQuery({
    queryKey: [...journalKey(), "assets"],
    queryFn: getAssets,
    enabled: namesNeeded,
    retry: false,
  });
  const label = (id: string) => {
    const frozen = sourceLabel(capture, id);
    if (frozen !== id) return frozen;
    const asset = assets.data?.find((item) => item.id === id);
    return (
      accounts.data?.find((item) => item.id === id)?.name ?? asset?.name ?? asset?.displayCode ?? id
    );
  };
  const candidates = (queue.data?.items ?? []).flatMap((item) =>
    item.kind === "UNLINKED_EXECUTION" && item.execution ? [item.execution] : [],
  );
  const selectedExecution = candidates.find((execution) => execution.id === executionId);
  const locked = reading || write.busy || write.uncertain;
  const stage = unlink ? "unlink" : preview ? "review" : adding ? "edit" : "list";
  const editorActive = stage !== "list";
  useEffect(() => {
    onEditorActive(editorActive);
    return () => onEditorActive(false);
  }, [editorActive, onEditorActive]);
  useEffect(() => {
    if (focusEditor.current) heading.current?.focus();
  }, [stage]);
  useJournalNavigationGuard(
    reading ||
      (adding &&
        (executionId !== linkBaseline.current.executionId ||
          historical !== linkBaseline.current.historical ||
          !!deviation.trim())) ||
      !!reason.trim(),
    {
      pending: t("pending_navigation"),
      discard: t("discard_navigation"),
    },
  );

  function finish(result: typeof notice) {
    setAdding(false);
    setPreview(null);
    setExecutionId("");
    setDeviation("");
    setUnlink(null);
    setReason("");
    setError(null);
    setNotice(result);
    if (result) void invalidate();
  }

  async function readPreview(event: FormEvent) {
    event.preventDefault();
    if (locked || !candidates.some((execution) => execution.id === executionId)) return;
    setError(null);
    write.clearError();
    setReading(true);
    try {
      const target = {
        executionId,
        decisionRevisionId: revisionId,
        deviationReason: deviation.trim() || null,
      };
      const result = await executeInvestmentJournal<LinkPreview>(
        "preview_investment_execution_link",
        target,
      );
      if (
        result.target.executionId !== executionId ||
        result.target.decisionRevisionId !== revisionId ||
        result.snapshot.decision.id !== revisionId ||
        !result.previewHash
      )
        throw new Error(t("records_error"));
      setPreview(result);
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }

  function confirmLink() {
    if (!preview || locked || write.error) return;
    void write.run(
      "link_investment_execution",
      {
        target: preview.target,
        expectedPreviewHash: preview.previewHash,
        requestKey: crypto.randomUUID(),
      },
      () => finish("execution_link_saved"),
    );
  }

  function confirmUnlink(event: FormEvent) {
    event.preventDefault();
    if (!unlink || locked || write.error || !reason.trim()) return;
    void write.run(
      "unlink_investment_execution",
      {
        linkId: unlink.id,
        expectedLinkedAt: unlink.linkedAt,
        reason: reason.trim(),
        requestKey: crypto.randomUUID(),
      },
      () => finish("execution_unlink_saved"),
    );
  }

  async function reloadUnlink() {
    if (!unlink || locked) return;
    setReading(true);
    setError(null);
    try {
      const result = await links.refetch({ throwOnError: true });
      const current = result.data?.find((link) => link.id === unlink.id && !link.unlinkedAt);
      write.clearError();
      if (current) setUnlink(current);
      else finish(null);
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }

  const cancel = () => {
    if (!allowJournalNavigation()) return;
    write.clearError();
    finish(null);
  };
  return (
    <section className="mt-6 space-y-4 border-t pt-5">
      <h3 ref={heading} tabIndex={-1} className="font-semibold">
        {t(
          stage === "unlink"
            ? "unlink_execution"
            : stage === "review"
              ? "review_execution_link"
              : stage === "edit"
                ? "link_execution"
                : "execution_links",
        )}
      </h3>
      {notice && stage === "list" && (
        <p role="status" className="text-sm">
          {t(notice)}
        </p>
      )}
      {stage === "list" && (
        <>
          {canLink && (
            <Button
              variant="outline"
              disabled={locked}
              onClick={() => {
                if (!allowJournalNavigation()) return;
                focusEditor.current = true;
                linkBaseline.current = { executionId, historical };
                write.clearError();
                setNotice(null);
                setAdding(true);
              }}
            >
              {t("link_execution")}
            </Button>
          )}
          {links.isLoading && <p role="status">{t("loading")}</p>}
          {links.isError && (
            <div className="space-y-2">
              <p role="alert">{t("records_error")}</p>
              <Button variant="outline" onClick={() => void links.refetch()}>
                {t("retry")}
              </Button>
            </div>
          )}
          {links.data?.length === 0 && (
            <p className="text-muted-foreground text-sm">{t("no_execution_links")}</p>
          )}
          <ul className="divide-y">
            {links.data?.map((link) => (
              <li key={link.id} className="space-y-3 py-4">
                <p className="text-sm font-medium">
                  {t("version", { number: link.revisionNumber })} ·{" "}
                  {t(link.unlinkedAt ? "execution_unlinked" : "execution_linked")} · {link.linkedAt}
                </p>
                <Evidence evidence={link} label={label} />
                {link.deviationReason && (
                  <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
                    {t("deviation_reason")} · {link.deviationReason}
                  </p>
                )}
                {link.unlinkedAt && (
                  <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
                    {link.unlinkedAt} · {link.unlinkReason}
                  </p>
                )}
                {!link.unlinkedAt && (
                  <Button
                    variant="outline"
                    disabled={locked}
                    onClick={() => {
                      if (!allowJournalNavigation()) return;
                      focusEditor.current = true;
                      write.clearError();
                      setNotice(null);
                      setUnlink(link);
                    }}
                  >
                    {t("unlink_execution")}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
      {stage === "edit" && (
        <form className="space-y-4" onSubmit={readPreview}>
          <p className="text-sm">{t("link_execution_to_version", { number: revisionNumber })}</p>
          <fieldset disabled={locked} className="space-y-4">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={historical}
                onChange={(event) => {
                  setHistorical(event.target.checked);
                  setExecutionId("");
                }}
              />
              {t("execution_include_history")}
            </label>
            <Field label={t("existing_execution")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  required
                  value={executionId}
                  onChange={(event) => setExecutionId(event.target.value)}
                >
                  <option value="">{t("choose")}</option>
                  {candidates.map((execution) => (
                    <option key={execution.id} value={execution.id}>
                      {execution.occurredAt} · {label(execution.assetId)} ·{" "}
                      {t(execution.side === "SELL" ? "action_SELL" : "action_BUY")} ·{" "}
                      {label(execution.accountId)}
                      {!isBalanceHidden && execution.quantity != null
                        ? ` · ${execution.quantity}`
                        : ""}
                      {execution.sourceId ? ` · ${execution.sourceId}` : ""}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            {selectedExecution && (
              <p className="text-muted-foreground text-sm [overflow-wrap:anywhere]">
                {t("source_record")} · {selectedExecution.sourceId}
              </p>
            )}
            {queue.isLoading && <p role="status">{t("loading")}</p>}
            {queue.isError && (
              <div className="space-y-2">
                <p role="alert">{t("records_error")}</p>
                <Button type="button" variant="outline" onClick={() => void queue.refetch()}>
                  {t("retry")}
                </Button>
              </div>
            )}
            {queue.data && !queue.data.complete && <p role="status">{t("index_pending")}</p>}
            {queue.data?.complete &&
              executionId &&
              !candidates.some((execution) => execution.id === executionId) && (
                <p role="status" className="text-sm">
                  {t("queue_task_changed")}
                </p>
              )}
            {queue.data?.complete && candidates.length === 0 && (
              <p className="text-muted-foreground text-sm">{t("no_execution_candidates")}</p>
            )}
            <Field label={t("deviation_reason")}>
              {(id) => (
                <textarea
                  id={id}
                  className={textAreaClass}
                  value={deviation}
                  onChange={(event) => setDeviation(event.target.value)}
                />
              )}
            </Field>
          </fieldset>
          <div className="flex flex-wrap gap-3">
            <Button
              disabled={
                locked ||
                queue.isError ||
                !candidates.some((execution) => execution.id === executionId)
              }
            >
              {t(reading ? "loading" : "review_execution_link")}
            </Button>
            <Button type="button" variant="outline" disabled={locked} onClick={cancel}>
              {t("cancel")}
            </Button>
          </div>
        </form>
      )}
      {preview && (
        <div className="space-y-4">
          <p className="text-sm">
            {t("link_execution_to_version", { number: preview.snapshot.decision.revisionNumber })}
          </p>
          <Evidence evidence={preview} label={label} />
          {preview.target.deviationReason && (
            <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
              {t("deviation_reason")} · {preview.target.deviationReason}
            </p>
          )}
          <p className="text-muted-foreground text-sm">{t("execution_link_evidence_only")}</p>
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !!write.error} onClick={confirmLink}>
              {t("submit_execution_link")}
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
        </div>
      )}
      {unlink && (
        <form className="space-y-4" onSubmit={confirmUnlink}>
          <p className="text-sm">{t("unlink_execution_hint")}</p>
          <p className="text-sm">{t("version", { number: unlink.revisionNumber })}</p>
          <Evidence evidence={unlink} label={label} />
          <Field label={t("operation_reason")}>
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
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !!write.error || !reason.trim()}>
              {t("unlink_execution")}
            </Button>
            <Button type="button" variant="outline" disabled={locked} onClick={cancel}>
              {t("cancel")}
            </Button>
            {write.error && !write.uncertain && (
              <Button
                type="button"
                variant="outline"
                disabled={locked}
                onClick={() => void reloadUnlink()}
              >
                {t("execution_reload_link")}
              </Button>
            )}
          </div>
        </form>
      )}
      {editorActive && (Boolean(error) || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
    </section>
  );
}
