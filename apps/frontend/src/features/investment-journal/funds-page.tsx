import { useEffect, useRef, useState } from "react";
import { Button } from "@wealthfolio/ui";
import { useQuery } from "@tanstack/react-query";
import { getAccounts } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { ExactValue, JournalErrorNotice } from "./controls";
import { resourceLabel, sourceLabel } from "./context-labels";
import { FundAllocationEditor, ResourceBalances } from "./fund-allocation-editor";
import { FundTransfer } from "./fund-transfer";
import { FundHistory } from "./fund-history";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { journalKey, useJournalQuery } from "./queries";
import { PurposeEditor } from "./purpose-editor";
import type { ContextCapture, Purpose, Resource } from "./types";
import { sameResource, type FundIssue } from "./fund-types";
import { useJournalWrite } from "./use-journal-write";
import { allowJournalNavigation } from "./navigation-guard";

const issueLabels: Record<FundIssue["kind"], JournalTextKey> = {
  PURPOSE_EXCESS: "fund_issue_excess",
  RESOURCE_UNAVAILABLE: "issue_resource",
  SOURCE_CHANGED: "fund_issue_changed",
  SOURCE_INVALIDATED: "fund_issue_invalidated",
  CORPORATE_ACTION: "fund_issue_corporate",
  CALIBRATION: "fund_issue_calibration",
  TRANSFER_CHANGED: "fund_issue_transfer",
};
export function FundsPage({ initialResource }: { initialResource?: Resource } = {}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const write = useJournalWrite();
  const [editor, setEditor] = useState<{ purpose?: Purpose } | null>(null);
  const [resource, setResource] = useState<Resource | null>(initialResource ?? null);
  const [transfer, setTransfer] = useState(false);
  const transferTrigger = useRef<HTMLButtonElement>(null);
  const restoreTransferFocus = useRef(false);
  useEffect(() => {
    if (!transfer && restoreTransferFocus.current) {
      restoreTransferFocus.current = false;
      transferTrigger.current?.focus();
    }
  }, [transfer]);
  const [tab, setTab] = useState<"resources" | "history">("resources");
  const purposes = useJournalQuery<Purpose[]>("list_fund_purposes");
  const accounts = useQuery({
    queryKey: [...journalKey(), "accounts"],
    queryFn: () => getAccounts(true),
    retry: false,
  });
  const context = useJournalQuery<ContextCapture>(
    "get_investment_context",
    {
      purposeIds: purposes.data?.map((p) => p.id) ?? [],
      accountIds: accounts.data?.map((a) => a.id) ?? [],
    },
    !!purposes.data && !!accounts.data,
  );
  const rows = context.data?.response.funding.data.allocation?.resources ?? [];
  const resources = rows.map((row) => row.observation.resource);
  const issues = useJournalQuery<FundIssue[]>(
    "get_fund_reconciliation",
    { resources },
    resources.length > 0,
  );
  if (transfer && accounts.data && purposes.data && context.data)
    return (
      <FundTransfer
        accounts={accounts.data}
        purposes={purposes.data}
        day={context.data.response.asOfDate}
        close={() => {
          restoreTransferFocus.current = true;
          setTransfer(false);
        }}
      />
    );
  if (editor)
    return (
      <PurposeEditor
        key={editor.purpose?.id ?? "new"}
        initial={editor.purpose}
        currency={context.data?.response.currency ?? ""}
        close={() => setEditor(null)}
      />
    );
  if (resource && context.data && purposes.data)
    return (
      <FundAllocationEditor
        resource={resource}
        reconcileFirst={!!initialResource && sameResource(resource, initialResource)}
        capture={context.data}
        purposes={purposes.data}
        close={() => setResource(null)}
      />
    );
  return (
    <div className="space-y-9">
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
      <section className="space-y-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="space-y-2">
            <h2 className="text-xl font-semibold">{t("funding_purposes")}</h2>
            <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
              {t("purpose_definition_hint")}
            </p>
          </div>
          <Button
            onClick={() => {
              if (allowJournalNavigation()) setEditor({});
            }}
          >
            {t("new_purpose")}
          </Button>
        </div>
        {purposes.isLoading && <p role="status">{t("loading")}</p>}
        {purposes.isError && (
          <div className="space-y-3">
            <p role="alert">{t("records_error")}</p>
            <Button variant="outline" onClick={() => void purposes.refetch()}>
              {t("retry")}
            </Button>
          </div>
        )}
        {purposes.data?.length === 0 && (
          <p className="text-muted-foreground text-sm">{t("no_purposes")}</p>
        )}
        <ul className="divide-y">
          {purposes.data?.map((purpose) => (
            <li key={purpose.id} className="flex flex-wrap items-start justify-between gap-4 py-4">
              <div className="min-w-0 space-y-2">
                <h3 className="break-words font-semibold">
                  {purpose.name}{" "}
                  <span className="text-muted-foreground text-sm font-normal">
                    {purpose.currency}
                    {purpose.isArchived ? ` · ${t("archived")}` : ""}
                  </span>
                </h3>
                <dl className="flex flex-wrap gap-x-6 gap-y-2 text-sm">
                  {(
                    [
                      ["targetAmount", "target_amount"],
                      ["minimumCash", "minimum_cash"],
                      ["lossBudget", "loss_budget"],
                    ] as const
                  ).map(([key, label]) => (
                    <div key={key}>
                      <dt className="text-muted-foreground">{t(label)}</dt>
                      <dd>
                        <ExactValue value={purpose[key]} currency={purpose.currency} />
                      </dd>
                    </div>
                  ))}
                </dl>
              </div>
              <Button
                variant="outline"
                onClick={() => {
                  if (allowJournalNavigation()) setEditor({ purpose });
                }}
              >
                {t("edit_purpose")}
              </Button>
            </li>
          ))}
        </ul>
      </section>
      <div
        className="flex flex-wrap gap-2 border-b pb-3"
        role="group"
        aria-label={t("ownership_views")}
      >
        <Button
          variant={tab === "resources" ? "secondary" : "ghost"}
          aria-pressed={tab === "resources"}
          onClick={() => {
            if (allowJournalNavigation()) setTab("resources");
          }}
        >
          {t("resources_and_checks")}
        </Button>
        <Button
          variant={tab === "history" ? "secondary" : "ghost"}
          aria-pressed={tab === "history"}
          onClick={() => {
            if (allowJournalNavigation()) setTab("history");
          }}
        >
          {t("ownership_history")}
        </Button>
        <Button
          variant="outline"
          ref={transferTrigger}
          disabled={
            !context.data || !accounts.data || !purposes.data || write.busy || write.uncertain
          }
          onClick={() => {
            if (allowJournalNavigation()) {
              write.clearError();
              setTransfer(true);
            }
          }}
        >
          {t("operation_TRANSFER")}
        </Button>
      </div>
      {(context.isLoading || accounts.isLoading) && <p role="status">{t("loading")}</p>}
      {(context.isError || accounts.isError) && (
        <div className="space-y-3">
          <p role="alert">{t("records_error")}</p>
          <Button
            variant="outline"
            onClick={() => {
              if (accounts.isError) void accounts.refetch();
              else void context.refetch();
            }}
          >
            {t("retry")}
          </Button>
        </div>
      )}
      {context.data &&
        (tab === "history" ? (
          <FundHistory capture={context.data} purposes={purposes.data ?? []} />
        ) : (
          <section className="space-y-5">
            <h2 className="text-xl font-semibold">{t("resources_and_checks")}</h2>
            <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
              {t("resource_balances_hint")}
            </p>
            <p className="text-muted-foreground text-xs">
              {t("as_of", { date: context.data.response.asOfDate })}
            </p>
            {isBalanceHidden && (
              <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
            )}
            {context.data.response.funding.quality !== "COMPLETE" && (
              <p role="status" className="text-sm">
                {t("funding_partial")}
              </p>
            )}
            {rows.length === 0 && (
              <p className="text-muted-foreground py-6 text-sm">{t("empty_resources")}</p>
            )}
            {issues.isError && (
              <div className="space-y-3">
                <p role="alert">{t("reconciliation_error")}</p>
                <Button variant="outline" onClick={() => void issues.refetch()}>
                  {t("retry")}
                </Button>
              </div>
            )}
            <ul className="divide-y">
              {rows.map((row) => {
                const rowResource = row.observation.resource;
                const related =
                  issues.data?.filter((item) => sameResource(item.resource, rowResource)) ?? [];
                return (
                  <li key={JSON.stringify(rowResource)} className="space-y-5 py-6">
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <h3 className="break-words font-semibold">
                        {resourceLabel(context.data, rowResource, t)}
                      </h3>
                      <Button
                        variant="outline"
                        disabled={!purposes.data}
                        onClick={() => setResource(rowResource)}
                      >
                        {t("change_ownership")}
                      </Button>
                    </div>
                    <ResourceBalances row={row} />
                    {Object.keys(row.purposes).length > 0 && (
                      <dl className="space-y-2 border-t pt-4 text-sm">
                        {Object.entries(row.purposes).map(([id, amount]) => (
                          <div key={id} className="flex flex-wrap justify-between gap-3">
                            <dt>{purposes.data?.find((p) => p.id === id)?.name ?? id}</dt>
                            <dd>
                              <ExactValue
                                value={amount}
                                currency={
                                  rowResource.kind === "CASH" ? rowResource.currency : undefined
                                }
                              />
                            </dd>
                          </div>
                        ))}
                      </dl>
                    )}
                    {related.length > 0 && (
                      <div className="space-y-2 text-sm">
                        <h4 className="font-medium">{t("needs_reconciliation")}</h4>
                        <ul className="space-y-2">
                          {related.map((item) => (
                            <li key={item.id} className="break-words">
                              {t(issueLabels[item.kind])}
                              {item.sourceId
                                ? ` · ${sourceLabel(context.data, item.sourceId)}`
                                : ""}
                              {item.excess != null && (
                                <span className="ml-2">
                                  <ExactValue value={item.excess} />
                                </span>
                              )}
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
    </div>
  );
}
