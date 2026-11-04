import { useEffect, useRef, useState } from "react";
import { Button } from "@wealthfolio/ui";
import { ExactValue, Field, selectClass } from "./controls";
import { resourceLabel } from "./context-labels";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { useJournalQuery } from "./queries";
import type { ContextCapture, Purpose } from "./types";
import type { FundHistoryItem } from "./fund-types";
import { FundCorrection } from "./fund-correction";
import { allowJournalNavigation } from "./navigation-guard";
import { useJournalWrite } from "./use-journal-write";
import { FundReversal } from "./fund-reversal";

const operationLabels: Record<string, JournalTextKey> = {
  ALLOCATE: "operation_ALLOCATE",
  REALLOCATE: "operation_REALLOCATE",
  RECONCILE: "operation_RECONCILE",
  REVERSE: "operation_REVERSAL",
  CORRECTION: "operation_CORRECTION",
  TRADE: "operation_TRADE",
  TRANSFER: "operation_TRANSFER",
};
export function FundHistory({
  capture,
  purposes,
}: {
  capture: ContextCapture;
  purposes: Purpose[];
}) {
  const { t } = useJournalLocale();
  const [purposeId, setPurpose] = useState("");
  const [page, setPage] = useState(0);
  const write = useJournalWrite();
  const [correcting, setCorrecting] = useState<FundHistoryItem | null>(null);
  const correctionTrigger = useRef<HTMLButtonElement>(null);
  const lastCorrectionId = useRef<string | null>(null);
  const historyTitle = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (!correcting && lastCorrectionId.current) {
      (correctionTrigger.current ?? historyTitle.current)?.focus();
      lastCorrectionId.current = null;
    }
  }, [correcting]);
  const [reversing, setReversing] = useState<FundHistoryItem | null>(null);
  const history = useJournalQuery<FundHistoryItem[]>("list_fund_history", {
    purposeId: purposeId || null,
    operationId: null,
    limit: 50,
    offset: page * 50,
  });
  if (correcting)
    return (
      <FundCorrection
        key={correcting.id}
        item={correcting}
        capture={capture}
        purposes={purposes}
        close={() => setCorrecting(null)}
      />
    );
  if (reversing)
    return (
      <FundReversal
        key={reversing.id}
        item={reversing}
        capture={capture}
        purposes={purposes}
        close={() => setReversing(null)}
      />
    );
  return (
    <section className="space-y-5" aria-label={t("ownership_history")}>
      <div className="space-y-2">
        <h2 ref={historyTitle} tabIndex={-1} className="text-xl font-semibold">
          {t("ownership_history")}
        </h2>
        <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
          {t("history_hint")}
        </p>
      </div>
      <div className="max-w-md">
        <Field label={t("purpose")}>
          {(id) => (
            <select
              id={id}
              className={selectClass}
              value={purposeId}
              onChange={(e) => {
                setPurpose(e.target.value);
                setPage(0);
              }}
            >
              <option value="">{t("all_purposes")}</option>
              {purposes.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          )}
        </Field>
      </div>
      {history.isLoading && <p role="status">{t("loading")}</p>}
      {history.isError && (
        <div className="space-y-3">
          <p role="alert">{t("records_error")}</p>
          <Button variant="outline" onClick={() => void history.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {history.data?.length === 0 && (
        <p className="text-muted-foreground py-6 text-sm">{t("empty_history")}</p>
      )}
      <ol className="divide-y">
        {history.data?.map((item) => (
          <li key={item.id} className="space-y-3 py-5">
            <div className="flex flex-wrap justify-between gap-3">
              <h3 className="font-semibold">
                {t(operationLabels[item.operationKind] ?? "ownership_changes")}
              </h3>
              <span className="text-muted-foreground text-xs">
                {item.createdAt} · {t(item.createdActorKind)}
              </span>
            </div>
            <p className="whitespace-pre-wrap break-words text-sm">{item.reason}</p>
            {item.correctionOf && (
              <p className="text-muted-foreground break-all text-xs">
                {t("corrects_operation")}: {item.correctionOf}
              </p>
            )}
            {item.entries.length === 0 && (
              <p className="text-muted-foreground text-sm">{t("acknowledge_sources_only")}</p>
            )}
            <ul className="space-y-3">
              {item.entries.map((entry, index) => (
                <li key={index} className="flex flex-wrap justify-between gap-3 text-sm">
                  <span className="break-words">
                    {purposes.find((p) => p.id === entry.purposeId)?.name ?? entry.purposeId} ·{" "}
                    {resourceLabel(capture, entry.resource, t)}
                  </span>
                  <ExactValue
                    value={entry.delta}
                    currency={entry.resource.kind === "CASH" ? entry.resource.currency : undefined}
                  />
                </li>
              ))}
            </ul>
            <div className="flex flex-wrap gap-2">
              {item.entries.length > 0 && item.operationKind !== "REVERSE" && (
                <Button variant="outline" onClick={() => setReversing(item)}>
                  {t("preview_reversal")}
                </Button>
              )}
              <Button
                variant="outline"
                ref={lastCorrectionId.current === item.id ? correctionTrigger : undefined}
                disabled={write.busy || write.uncertain || purposes.length === 0}
                onClick={() => {
                  if (allowJournalNavigation()) {
                    write.clearError();
                    lastCorrectionId.current = item.id;
                    setCorrecting(item);
                  }
                }}
              >
                {t("correct_ownership")}
              </Button>
            </div>
          </li>
        ))}
      </ol>
      <div className="flex justify-end gap-3">
        <Button
          variant="outline"
          disabled={page === 0 || history.isFetching}
          onClick={() => setPage((p) => p - 1)}
        >
          {t("previous")}
        </Button>
        <Button
          variant="outline"
          disabled={history.isFetching || (history.data?.length ?? 0) < 50}
          onClick={() => setPage((p) => p + 1)}
        >
          {t("next")}
        </Button>
      </div>
    </section>
  );
}
