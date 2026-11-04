import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { writeSessionKey } from "./write-session";
import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";
import { InvestmentWorkQueue } from "./investment-work-queue";
import { initialQueueFilters } from "./execution-work-types";
import { Navigate, useSearchParams } from "react-router-dom";
import { Button, Icons, Page, PageContent, PageHeader } from "@wealthfolio/ui";
import { isTauri, listenInvestmentIndexUpdate } from "@/adapters";
import { Field, JournalErrorNotice, JournalPrivacyButton, selectClass } from "./controls";
import { ContextPanel } from "./context-panel";
import { DecisionDetail } from "./decision-detail";
import { DecisionEditor } from "./decision-editor";
import { JournalLocaleProvider } from "./locale";
import { useJournalLocale } from "./locale-context";
import { useInvalidateJournal, useJournalQuery } from "./queries";
import type { ContextCapture, DecisionSummary, JournalStatus } from "./types";
import { useJournalWrite } from "./use-journal-write";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { PurposeIntroduction } from "./purpose-introduction";
import { InvestmentCases } from "./investment-cases";
import { PrincipleLibrary } from "./principle-library";
import { FundsPage } from "./funds-page";

function Introduction({ done }: { done: (decision: boolean) => void }) {
  const { t } = useJournalLocale();
  const context = useJournalQuery<ContextCapture>("get_investment_context", {
    purposeIds: [],
    accountIds: [],
  });
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  function finish(decision: boolean) {
    void write.run("complete_investment_onboarding", { requestKey: crypto.randomUUID() }, () => {
      void invalidate();
      done(decision);
    });
  }
  return (
    <div className="grid items-start gap-8 lg:grid-cols-[minmax(0,1fr)_24rem]">
      <div className="space-y-4 py-2">
        <h2 className="text-2xl font-semibold">{t("onboard_title")}</h2>
        <p className="text-muted-foreground max-w-prose leading-relaxed">{t("onboard_intro")}</p>
      </div>
      <div className="lg:col-start-2 lg:row-span-2 lg:row-start-1">
        <ContextPanel
          capture={context.data}
          error={context.isError}
          retry={() => void context.refetch()}
          householdOnly
        />
      </div>
      <div className="space-y-6">
        {write.error && (
          <JournalErrorNotice
            error={write.error}
            replay={write.uncertain ? () => void write.retry() : undefined}
            busy={write.busy}
          />
        )}
        <div className="flex flex-wrap gap-3">
          <Button disabled={write.busy || write.uncertain} onClick={() => finish(true)}>
            {t("onboard_decision")}
          </Button>
          <Button
            variant="outline"
            disabled={write.busy || write.uncertain}
            onClick={() => finish(false)}
          >
            {t("onboard_later")}
          </Button>
        </div>
        <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
          {t("onboard_independent")}
        </p>
        <PurposeIntroduction
          currency={context.data?.response.currency ?? ""}
          done={() => finish(false)}
        />
      </div>
    </div>
  );
}

function DecisionList({
  open,
  confirmedOnly = false,
}: {
  open: (id: string) => void;
  confirmedOnly?: boolean;
}) {
  const { t } = useJournalLocale();
  const [page, setPage] = useState(0);
  const [lifecycle, setLifecycle] = useState("");
  const [tags, setTags] = useState("");
  const query = useJournalQuery<DecisionSummary[]>("list_investment_decisions", {
    lifecycle: confirmedOnly ? "CONFIRMED" : lifecycle || null,
    scenarioTags: tags
      .split(/[,，]/)
      .map((tag) => tag.trim())
      .filter(Boolean),
    limit: 50,
    offset: page * 50,
  });
  return (
    <div className="space-y-6">
      <div className="grid max-w-2xl gap-4 sm:grid-cols-2">
        <Field label={t("filter_state")}>
          {(id) => (
            <select
              id={id}
              className={selectClass}
              value={confirmedOnly ? "CONFIRMED" : lifecycle}
              disabled={confirmedOnly}
              onChange={(e) => {
                setLifecycle(e.target.value);
                setPage(0);
              }}
            >
              <option value="">{t("all_states")}</option>
              <option value="DRAFT">{t("draft")}</option>
              <option value="CONFIRMED">{t("confirmed")}</option>
              <option value="ARCHIVED">{t("archived")}</option>
            </select>
          )}
        </Field>
        <Field label={t("filter_tags")}>
          {(id) => (
            <Input
              id={id}
              value={tags}
              onChange={(e) => {
                setTags(e.target.value);
                setPage(0);
              }}
            />
          )}
        </Field>
      </div>
      {query.isLoading && <p role="status">{t("loading")}</p>}
      {query.isError && (
        <div className="space-y-3">
          <p role="alert">{t("context_error")}</p>
          <Button variant="outline" onClick={() => void query.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {query.data?.length === 0 && (
        <div className="space-y-3 py-10">
          <h2 className="text-xl font-semibold">
            {t(lifecycle || tags ? "empty_filter" : "empty_decisions")}
          </h2>
          <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
            {t("empty_decisions_hint")}
          </p>
        </div>
      )}
      <ul className="divide-y rounded-xl border">
        {query.data?.map((decision) => (
          <li key={decision.decisionId}>
            <button
              className="hover:bg-muted/50 focus-visible:ring-ring block w-full rounded-xl p-5 text-left outline-none focus-visible:ring-2"
              onClick={() => open(decision.decisionId)}
            >
              <div className="flex flex-wrap items-center justify-between gap-3">
                <h2 className="break-words font-semibold">{decision.title || t("decisions")}</h2>
                <span className="text-muted-foreground text-xs">
                  {t(
                    decision.lifecycle === "ARCHIVED"
                      ? "archived"
                      : decision.currentRevisionId
                        ? "confirmed"
                        : "draft",
                  )}
                  {decision.currentRevisionNumber
                    ? ` · ${t("version", { number: decision.currentRevisionNumber })}`
                    : ""}
                </span>
              </div>
              {decision.draftRevisionId && decision.currentRevisionId && (
                <p className="text-muted-foreground mt-2 text-sm">{t("draft_separate")}</p>
              )}
              {decision.nextReviewAt && (
                <p className="text-muted-foreground mt-2 text-xs">
                  {t("review_at")}: {decision.nextReviewAt}
                </p>
              )}
            </button>
          </li>
        ))}
      </ul>
      <div className="flex justify-end gap-3">
        <Button
          variant="outline"
          disabled={page === 0 || query.isFetching}
          onClick={() => setPage((p) => p - 1)}
        >
          {t("previous")}
        </Button>
        <Button
          variant="outline"
          disabled={query.isFetching || (query.data?.length ?? 0) < 50}
          onClick={() => setPage((p) => p + 1)}
        >
          {t("next")}
        </Button>
      </div>
    </div>
  );
}

function InvestmentWorkspace() {
  const { t, language, setLanguage } = useJournalLocale();
  useJournalNavigationGuard(false, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  const [params, setParams] = useSearchParams();
  const [caseTags, setCaseTags] = useState<string[]>([]);
  const [queueFilters, setQueueFilters] = useState(initialQueueFilters);
  const [casePrinciple, setCasePrinciple] = useState<{ id: string; label: string } | null>(null);
  const selected = params.get("decision");
  const queueRevision = params.get("taskRevision");
  const queueTask = params.get("task");
  const queueDate = params.get("taskDate");
  const pendingExecution = params.get("execution");
  const historicalExecution = params.get("history") === "1";
  const resourceAccount = params.get("resourceAccount");
  const resourceCurrency = params.get("resourceCurrency");
  const resourceAsset = params.get("resourceAsset");
  const sourceReview = params.get("sourceReview");
  const requestedView = params.get("view");
  const view =
    requestedView === "funds" ||
    requestedView === "queue" ||
    requestedView === "principles" ||
    requestedView === "cases"
      ? requestedView
      : "decisions";
  const status = useJournalQuery<JournalStatus>(
    "get_investment_journal_status",
    {},
    true,
    (data) =>
      data &&
      (data.projection.pendingSources > 0 ||
        Object.values(data.projection.backfillComplete).some((complete) => !complete) ||
        !!data.projection.error)
        ? 2000
        : false,
  );
  const [introduced, setIntroduced] = useState(false);
  const recovery = useJournalWrite();
  const invalidate = useInvalidateJournal();
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => Promise<void>) | undefined;
    void listenInvestmentIndexUpdate(() => {
      void invalidate();
    })
      .then((stop) => {
        if (disposed) void stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      void unlisten?.();
    };
  }, [invalidate]);
  useEffect(() => {
    const recovered = (event: Event) => {
      const detail = (event as CustomEvent<{ profile: string; result: { decisionId?: string } }>)
        .detail;
      if (detail.profile !== writeSessionKey()) return;
      void invalidate();
      if (detail.result.decisionId) setParams({ decision: detail.result.decisionId });
    };
    window.addEventListener("wealthlog:investment-recovered", recovered);
    return () => window.removeEventListener("wealthlog:investment-recovered", recovered);
  }, [invalidate, setParams]);
  const needsIntroduction = status.data && !status.data.onboardingCompletedAt && !introduced;
  const indexing =
    status.data &&
    (status.data.projection.pendingSources > 0 ||
      Object.values(status.data.projection.backfillComplete).some((complete) => !complete));
  return (
    <Page lang={language === "en" ? "en" : "zh-CN"}>
      <PageHeader
        heading={t("investments")}
        actions={
          <>
            <select
              aria-label={t("language")}
              className={cn(selectClass, "w-28")}
              value={language}
              onChange={(e) => setLanguage(e.target.value === "en" ? "en" : "zh")}
            >
              <option value="zh">简体中文</option>
              <option value="en">English</option>
            </select>
            <JournalPrivacyButton />
            {!selected && view === "decisions" && !needsIntroduction && (
              <Button
                onClick={() => {
                  if (allowJournalNavigation()) setParams({ decision: "new" });
                }}
              >
                <Icons.Plus className="mr-2 size-4" />
                {t("new_decision")}
              </Button>
            )}
          </>
        }
      />
      <PageContent>
        <div className="mx-auto max-w-7xl space-y-6 pb-12">
          {selected && !needsIntroduction && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                if (allowJournalNavigation())
                  setParams(queueTask || pendingExecution ? { view: "queue" } : {});
              }}
            >
              <Icons.ArrowLeft className="mr-2 size-4" />
              {t(queueTask || pendingExecution ? "back_to_work_queue" : "decisions")}
            </Button>
          )}
          {!selected && view === "decisions" && recovery.error && (
            <JournalErrorNotice
              error={recovery.error}
              replay={recovery.uncertain ? () => void recovery.retry() : undefined}
              busy={recovery.busy}
            />
          )}
          {!status.data && (
            <div className="space-y-4">
              <p role="status">{t(status.isError ? "context_error" : "loading")}</p>
              {status.isError && (
                <Button variant="outline" onClick={() => void status.refetch()}>
                  {t("retry")}
                </Button>
              )}
            </div>
          )}
          {status.data?.projection.error && (
            <p role="status" className="rounded-lg border px-4 py-3 text-sm">
              {t("index_retry")}
            </p>
          )}
          {indexing && (
            <p
              role="status"
              className="text-muted-foreground rounded-lg border px-4 py-3 text-sm leading-relaxed"
            >
              {t("index_pending")}
            </p>
          )}
          {status.data && !needsIntroduction && !selected && (
            <nav
              className="flex flex-wrap gap-2 border-b pb-3"
              aria-label={t("investment_sections")}
            >
              <Button
                variant={view === "decisions" ? "secondary" : "ghost"}
                aria-current={view === "decisions" ? "page" : undefined}
                onClick={() => {
                  if (allowJournalNavigation()) setParams({});
                }}
              >
                {t("decisions")}
              </Button>
              <Button
                variant={view === "funds" ? "secondary" : "ghost"}
                aria-current={view === "funds" ? "page" : undefined}
                onClick={() => {
                  if (allowJournalNavigation()) setParams({ view: "funds" });
                }}
              >
                {t("funds")}
              </Button>
              <Button
                variant={view === "queue" ? "secondary" : "ghost"}
                aria-current={view === "queue" ? "page" : undefined}
                onClick={() => {
                  if (allowJournalNavigation()) setParams({ view: "queue" });
                }}
              >
                {t("investment_work_queue")}
              </Button>
              <Button
                variant={view === "principles" ? "secondary" : "ghost"}
                aria-current={view === "principles" ? "page" : undefined}
                onClick={() => {
                  if (allowJournalNavigation()) setParams({ view: "principles" });
                }}
              >
                {t("principle_library")}
              </Button>
              <Button
                variant={view === "cases" ? "secondary" : "ghost"}
                aria-current={view === "cases" ? "page" : undefined}
                onClick={() => {
                  if (allowJournalNavigation() && view !== "cases") {
                    setCaseTags([]);
                    setCasePrinciple(null);
                    setParams({ view: "cases" });
                  }
                }}
              >
                {t("case_library")}
              </Button>
            </nav>
          )}
          {needsIntroduction ? (
            <Introduction
              done={(decision) => {
                setIntroduced(true);
                if (decision) setParams({ decision: "new" });
              }}
            />
          ) : (
            status.data &&
            (selected === "new" ? (
              <DecisionEditor key="new" completed={(id) => setParams({ decision: id })} />
            ) : selected ? (
              <DecisionDetail
                key={selected}
                id={selected}
                draftPrincipleFromReview={(reviewId) => {
                  if (allowJournalNavigation())
                    setParams({ view: "principles", sourceReview: reviewId });
                }}
                taskRevision={queueRevision}
                task={queueTask}
                taskDate={queueDate}
                pendingExecution={pendingExecution}
                historicalExecution={historicalExecution}
              />
            ) : view === "cases" ? (
              <InvestmentCases
                initialTags={caseTags}
                principleRevisionId={params.get("principleRevision") ?? undefined}
                principleLabel={
                  casePrinciple?.id === params.get("principleRevision")
                    ? casePrinciple.label
                    : undefined
                }
                clearPrinciple={() => {
                  const next = new URLSearchParams(params);
                  next.delete("principleRevision");
                  setParams(next);
                }}
              />
            ) : view === "principles" ? (
              <PrincipleLibrary
                openCases={(filter) => {
                  if (!allowJournalNavigation()) return;
                  setCaseTags(filter.tags ?? []);
                  setCasePrinciple(
                    filter.principleRevisionId && filter.principleLabel
                      ? { id: filter.principleRevisionId, label: filter.principleLabel }
                      : null,
                  );
                  setParams({
                    view: "cases",
                    ...(filter.principleRevisionId
                      ? { principleRevision: filter.principleRevisionId }
                      : {}),
                  });
                }}
                initialSourceReview={sourceReview ?? undefined}
                clearSourceReview={() => setParams({ view: "principles" }, { replace: true })}
              />
            ) : view === "funds" ? (
              <FundsPage
                initialResource={
                  resourceAccount && resourceCurrency
                    ? { kind: "CASH", accountId: resourceAccount, currency: resourceCurrency }
                    : resourceAccount && resourceAsset
                      ? { kind: "SECURITY", accountId: resourceAccount, assetId: resourceAsset }
                      : undefined
                }
              />
            ) : view === "queue" ? (
              <InvestmentWorkQueue
                filters={queueFilters}
                onFiltersChange={setQueueFilters}
                openDecision={(id, item) => {
                  if (allowJournalNavigation())
                    setParams({
                      decision: id,
                      task: item.kind,
                      taskRevision: item.revisionId ?? "",
                      taskDate: item.dueOn ?? "",
                    });
                }}
                openFunds={(resource) => {
                  if (allowJournalNavigation())
                    setParams({
                      view: "funds",
                      ...(resource
                        ? {
                            resourceAccount: resource.accountId,
                            ...(resource.kind === "CASH"
                              ? { resourceCurrency: resource.currency }
                              : { resourceAsset: resource.assetId }),
                          }
                        : {}),
                    });
                }}
                openDecisions={(execution, historical) => {
                  if (allowJournalNavigation())
                    setParams({ execution, ...(historical ? { history: "1" } : {}) });
                }}
              />
            ) : (
              <>
                {pendingExecution && (
                  <div className="flex flex-wrap items-center gap-3">
                    <p className="text-sm">{t("queue_pick_decision_hint")}</p>
                    <Button variant="ghost" onClick={() => setParams({ view: "queue" })}>
                      {t("back_to_work_queue")}
                    </Button>
                  </div>
                )}
                <DecisionList
                  confirmedOnly={!!pendingExecution}
                  open={(id) =>
                    setParams({
                      decision: id,
                      ...(pendingExecution ? { execution: pendingExecution } : {}),
                      ...(historicalExecution ? { history: "1" } : {}),
                    })
                  }
                />
              </>
            ))
          )}
        </div>
      </PageContent>
    </Page>
  );
}

export default function InvestmentPage() {
  if (!isTauri) return <Navigate to="/activities" replace />;
  return (
    <JournalLocaleProvider>
      <InvestmentWorkspace />
    </JournalLocaleProvider>
  );
}
