import { useEffect, useRef, useState } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { useQuery } from "@tanstack/react-query";
import { getAccounts, getAssets } from "@/adapters";
import { ExactValue, JournalErrorNotice } from "./controls";
import { ExecutionReconciliation } from "./execution-reconciliation";
import type {
  ExecutionSource,
  initialQueueFilters,
  QueueItem,
  WorkQueue,
} from "./execution-work-types";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { journalKey, useJournalQuery } from "./queries";
import { useJournalWrite } from "./use-journal-write";
import type { ContextCapture, Purpose, Resource } from "./types";

const queueLabels: Record<string, JournalTextKey> = {
  UNLINKED_EXECUTION: "queue_unlinked",
  EXECUTION_SOURCE_CHANGED: "fund_issue_changed",
  EXECUTION_SOURCE_INVALIDATED: "fund_issue_invalidated",
  EXECUTION_OWNERSHIP_PENDING: "queue_ownership_pending",
  EXECUTION_DATE_UNAVAILABLE: "queue_execution_date_missing",
  DRAFT_CHECK_DUE: "queue_draft_due",
  REVIEW_DUE: "queue_review_due",
  PURPOSE_EXCESS: "fund_issue_excess",
  RESOURCE_UNAVAILABLE: "issue_resource",
  SOURCE_CHANGED: "fund_issue_changed",
  SOURCE_INVALIDATED: "fund_issue_invalidated",
  CORPORATE_ACTION: "fund_issue_corporate",
  CALIBRATION: "fund_issue_calibration",
  TRANSFER_CHANGED: "fund_issue_transfer",
};

const PAGE_SIZE = 25;

export function InvestmentWorkQueue({
  filters,
  onFiltersChange,
  openDecision,
  openFunds,
  openDecisions,
}: {
  filters: typeof initialQueueFilters;
  onFiltersChange: (filters: typeof initialQueueFilters) => void;
  openDecision: (id: string, task: QueueItem) => void;
  openFunds: (resource?: Resource) => void;
  openDecisions: (executionId: string, historical: boolean) => void;
}) {
  const { t } = useJournalLocale();
  const { historical, search, page } = filters;
  const [editor, setEditor] = useState<ExecutionSource | null>(null);
  const listTitle = useRef<HTMLHeadingElement>(null);
  const returning = useRef(false);
  const write = useJournalWrite();
  const queue = useJournalQuery<WorkQueue>("get_investment_work_queue", {
    includeHistoricalExecutions: historical,
  });
  const items = queue.data?.items ?? [];
  const purposes = useJournalQuery<Purpose[]>("list_fund_purposes", {}, !!editor);
  const accounts = useQuery({
    queryKey: [...journalKey(), "accounts"],
    queryFn: () => getAccounts(true),
    enabled: !!editor || items.some((item) => item.execution || item.source || item.resource),
    retry: false,
  });
  const assets = useQuery({
    queryKey: [...journalKey(), "assets"],
    queryFn: getAssets,
    enabled: items.some(
      (item) => item.execution || item.source || item.resource?.kind === "SECURITY",
    ),
    retry: false,
  });
  const context = useJournalQuery<ContextCapture>(
    "get_investment_context",
    {
      purposeIds: purposes.data?.map((purpose) => purpose.id) ?? [],
      accountIds: accounts.data?.map((account) => account.id) ?? [],
    },
    !!editor && !!purposes.data && !!accounts.data,
  );
  useEffect(() => {
    if (!editor && returning.current) {
      returning.current = false;
      listTitle.current?.focus();
    }
  }, [editor]);
  const accountNames = new Map(accounts.data?.map(({ id, name }) => [id, name]));
  const assetNames = new Map(
    assets.data?.map(({ id, name, displayCode }) => [
      id,
      displayCode && name && name !== displayCode
        ? `${displayCode} · ${name}`
        : displayCode || name || id,
    ]),
  );
  const queryText = search.trim().toLocaleLowerCase();
  const matching = queryText
    ? items.filter((item) => {
        const execution = item.source?.execution ?? item.execution;
        const accountId = execution?.accountId ?? item.resource?.accountId;
        const assetId =
          execution?.assetId ??
          (item.resource?.kind === "SECURITY" ? item.resource.assetId : undefined);
        return [
          item.title,
          accountId && accountNames.get(accountId),
          assetId && assetNames.get(assetId),
          execution?.occurredAt,
          execution?.sourceId,
          accountId,
          assetId,
        ].some((value) => value?.toLocaleLowerCase().includes(queryText));
      })
    : items;
  const totalPages = Math.max(1, Math.ceil(matching.length / PAGE_SIZE));
  const currentPage = Math.min(page, totalPages - 1);
  if (editor)
    return (
      <ExecutionReconciliation
        key={editor.execution.id}
        source={
          queue.data?.items.find((item) => item.source?.execution.id === editor.execution.id)
            ?.source ?? editor
        }
        resourceStatus={
          purposes.isError || accounts.isError || context.isError
            ? "failed"
            : purposes.isSuccess && accounts.isSuccess && context.isSuccess
              ? "ready"
              : "loading"
        }
        retryResources={() => {
          if (purposes.isError) void purposes.refetch();
          if (accounts.isError) void accounts.refetch();
          if (context.isError) void context.refetch();
        }}
        capture={context.data}
        purposes={purposes.data ?? []}
        close={() => {
          returning.current = true;
          setEditor(null);
        }}
      />
    );
  return (
    <section className="space-y-5">
      <h2 ref={listTitle} tabIndex={-1} className="text-xl font-semibold">
        {t("investment_work_queue")}
      </h2>
      <div className="flex flex-wrap items-end gap-4">
        <label className="grid min-w-0 gap-2 text-sm sm:w-80">
          {t("queue_filter")}
          <Input
            type="search"
            value={search}
            onChange={(event) =>
              onFiltersChange({ ...filters, search: event.target.value, page: 0 })
            }
          />
        </label>
        <label className="flex min-h-9 items-center gap-3 text-sm">
          <input
            type="checkbox"
            checked={historical}
            onChange={(event) =>
              onFiltersChange({ ...filters, historical: event.target.checked, page: 0 })
            }
          />
          {t("execution_include_history")}
        </label>
      </div>
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
      {queue.isLoading && <p role="status">{t("loading")}</p>}
      {queue.isError && (
        <div className="space-y-2">
          <p role="alert">{t("records_error")}</p>
          <Button variant="outline" onClick={() => void queue.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {queue.data && (
        <>
          <p className="text-muted-foreground text-sm">
            {t("as_of", { date: queue.data.asOfDate })}
          </p>
          {!queue.data.complete && <p role="status">{t("queue_incomplete")}</p>}
          {queue.data.complete && !items.length && (
            <p className="text-muted-foreground text-sm">{t("queue_empty")}</p>
          )}
          {items.length > 0 && !matching.length && (
            <p role="status" className="text-muted-foreground text-sm">
              {t("queue_filter_empty")}
            </p>
          )}
          {matching.length > 0 && (
            <div className="flex flex-wrap items-center gap-3">
              <p role="status" className="text-muted-foreground me-auto text-sm">
                {t("queue_page", {
                  page: currentPage + 1,
                  pages: totalPages,
                  count: matching.length,
                })}
              </p>
              {totalPages > 1 && (
                <>
                  <Button
                    variant="outline"
                    disabled={currentPage === 0}
                    onClick={() => onFiltersChange({ ...filters, page: currentPage - 1 })}
                  >
                    {t("previous")}
                  </Button>
                  <Button
                    variant="outline"
                    disabled={currentPage + 1 >= totalPages}
                    onClick={() => onFiltersChange({ ...filters, page: currentPage + 1 })}
                  >
                    {t("next")}
                  </Button>
                </>
              )}
            </div>
          )}
          <ul className="divide-y">
            {matching.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE).map((item) => {
              const execution = item.source?.execution ?? item.execution;
              return (
                <li key={item.id} className="flex flex-wrap items-start justify-between gap-3 py-4">
                  <div className="min-w-0 space-y-2 [overflow-wrap:anywhere]">
                    <h3 className="font-semibold">
                      {t(queueLabels[item.kind] ?? "issue_unknown")}
                    </h3>
                    {execution && (
                      <>
                        <p id={`queue-trade-${item.id}`} className="text-sm">
                          {assetNames.get(execution.assetId) ?? execution.assetId} ·{" "}
                          {accountNames.get(execution.accountId) ?? execution.accountId} ·{" "}
                          {t(execution.side === "SELL" ? "action_SELL" : "action_BUY")} ·{" "}
                          {execution.occurredAt} · <ExactValue value={execution.quantity} />
                        </p>
                        <p id={`queue-source-${item.id}`} className="text-muted-foreground text-xs">
                          {t("source_record")} · {execution.sourceId}
                        </p>
                      </>
                    )}
                    {item.title && (
                      <p className="text-sm">
                        {item.title}
                        {item.revisionNumber != null &&
                          ` · ${t("version", { number: item.revisionNumber })}`}
                        {item.dueOn && ` · ${item.dueOn}`}
                      </p>
                    )}
                    {item.resource && (
                      <p className="text-sm">
                        {accountNames.get(item.resource.accountId) ?? item.resource.accountId} ·{" "}
                        {item.resource.kind === "CASH"
                          ? `${item.resource.currency} ${t("cash")}`
                          : (assetNames.get(item.resource.assetId) ?? item.resource.assetId)}{" "}
                        ·{" "}
                        <ExactValue
                          value={item.excess}
                          currency={
                            item.resource.kind === "CASH" ? item.resource.currency : undefined
                          }
                        />
                      </p>
                    )}
                  </div>
                  {item.source && (
                    <Button
                      variant="outline"
                      disabled={write.busy || write.uncertain}
                      onClick={() => {
                        write.clearError();
                        setEditor(item.source ?? null);
                      }}
                    >
                      {t("execution_reconcile")}
                    </Button>
                  )}
                  {item.decisionId && (
                    <Button
                      variant="outline"
                      onClick={() => item.decisionId && openDecision(item.decisionId, item)}
                    >
                      {t(
                        item.kind === "DRAFT_CHECK_DUE" ? "queue_open_draft" : "queue_open_review",
                      )}
                    </Button>
                  )}
                  {item.category === "FUND_RECONCILIATION" && (
                    <Button variant="outline" onClick={() => openFunds(item.resource)}>
                      {t("funds")}
                    </Button>
                  )}
                  {item.kind === "UNLINKED_EXECUTION" && execution && (
                    <Button
                      variant="outline"
                      aria-describedby={`queue-trade-${item.id} queue-source-${item.id}`}
                      onClick={() => openDecisions(execution.id, historical)}
                    >
                      {t("queue_pick_decision")}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </section>
  );
}
