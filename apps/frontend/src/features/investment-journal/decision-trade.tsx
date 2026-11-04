import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@wealthfolio/ui";
import {
  executeInvestmentJournal,
  getAccounts,
  getAssets,
  InvestmentJournalError,
} from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import type { Account, Asset } from "@/lib/types";
import { ActualTradeFieldsForm } from "./actual-trade-fields";
import { ExactValue, Field, JournalErrorNotice, selectClass } from "./controls";
import { sameResource } from "./fund-types";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { journalKey, useInvalidateJournal, useJournalQuery } from "./queries";
import { OwnershipImpact, TradeImpact } from "./trade-review";
import { TradeShares } from "./trade-shares";
import type {
  ActualTradeFields,
  ManualAccountState,
  PreparedTrade,
  TradeBinding,
  TradePreview,
  TradeReceipt,
  TradeShare,
} from "./trade-types";
import type { JournalStatus, Purpose } from "./types";
import { useJournalWrite } from "./use-journal-write";

type Ownership = { mode: "APPLY"; shares: TradeShare[] } | { mode: "RECONCILE_LATER" };
const deferred: Ownership = { mode: "RECONCILE_LATER" };

export function DecisionTradeComposer({
  binding,
  close,
}: {
  binding: TradeBinding;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const [account, setAccount] = useState<Account | null>(null);
  const [chosen, setChosen] = useState("");
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
  const status = useJournalQuery<JournalStatus>("get_investment_journal_status");
  const purposes = useJournalQuery<Purpose[]>("list_fund_purposes");
  const manual = account?.trackingMode === "HOLDINGS";
  const state = useJournalQuery<ManualAccountState>(
    "get_investment_journal_account_state",
    { accountId: account?.id ?? "" },
    manual,
  );
  const eligible =
    accounts.data?.filter(
      (row) => row.isActive && !row.isArchived && row.accountType === "SECURITIES",
    ) ?? [];
  const exit = () => {
    if (allowJournalNavigation()) close();
  };
  const chooseAccount = () => {
    if (allowJournalNavigation()) setAccount(null);
  };
  return (
    <section className="max-w-4xl space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-2">
          <h2 className="text-xl font-semibold">
            {t("record_confirmed_trade", { number: binding.revisionNumber })}
          </h2>
          <p className="break-words text-sm font-medium">{binding.body.title}</p>
          {binding.body.purposeIds.length > 0 && (
            <p className="text-muted-foreground break-words text-sm">
              {t("trade_planned_purposes")}:{" "}
              {binding.body.purposeIds
                .map((id) => purposes.data?.find((p) => p.id === id)?.name ?? id)
                .join(" · ")}
            </p>
          )}
          <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
            {t("completed_trade_only")}
          </p>
        </div>
        <Button variant="ghost" onClick={exit}>
          {t("back_to_decision")}
        </Button>
      </div>
      {!account ? (
        <div className="max-w-lg space-y-5">
          <Field label={t("trade_account")} hint={t("trade_account_modes")}>
            {(id) => (
              <select
                id={id}
                className={selectClass}
                value={chosen}
                onChange={(e) => setChosen(e.target.value)}
              >
                <option value="">{t("choose")}</option>
                {eligible.map((row) => (
                  <option key={row.id} value={row.id}>
                    {row.name} · {row.currency} ·{" "}
                    {t(row.trackingMode === "HOLDINGS" ? "manual_account" : "transaction_account")}
                  </option>
                ))}
              </select>
            )}
          </Field>
          {accounts.isLoading && <p role="status">{t("loading")}</p>}
          {accounts.isError && (
            <div className="space-y-3">
              <p role="alert">{t("records_error")}</p>
              <Button variant="outline" onClick={() => void accounts.refetch()}>
                {t("retry")}
              </Button>
            </div>
          )}
          {accounts.data && eligible.length === 0 && (
            <p className="text-muted-foreground text-sm">{t("no_trade_accounts")}</p>
          )}
          <Button
            disabled={!eligible.some((row) => row.id === chosen)}
            onClick={() => setAccount(eligible.find((row) => row.id === chosen) ?? null)}
          >
            {t("continue_trade")}
          </Button>
        </div>
      ) : (
        <>
          <div className="flex flex-wrap items-center justify-between gap-3 border-y py-4 text-sm">
            <span className="break-words font-medium">
              {account.name} · {t(manual ? "manual_account" : "transaction_account")}
            </span>
            <Button variant="outline" onClick={chooseAccount}>
              {t("change_trade_account")}
            </Button>
          </div>
          {(assets.isError || status.isError || state.isError) && (
            <div className="space-y-3">
              <p role="alert">{t("records_error")}</p>
              <Button
                variant="outline"
                onClick={() => {
                  if (assets.isError) void assets.refetch();
                  if (status.isError) void status.refetch();
                  if (state.isError) void state.refetch();
                }}
              >
                {t("retry")}
              </Button>
            </div>
          )}
          {(assets.isLoading || status.isLoading || (manual && state.isLoading)) && (
            <p role="status">{t("loading")}</p>
          )}
          {manual && state.data && !state.data.hasSnapshot && (
            <p role="alert" className="max-w-prose text-sm leading-relaxed">
              {t("initialize_manual_before_trade")}
            </p>
          )}
          {assets.data && status.data && (!manual || state.data?.hasSnapshot) && (
            <TradeRecorder
              key={account.id}
              binding={binding}
              account={account}
              assets={assets.data}
              purposes={purposes.data ?? []}
              purposesError={purposes.isError}
              retryPurposes={() => void purposes.refetch()}
              initialState={state.data}
              day={state.data?.asOfDate ?? status.data.asOfDate}
              close={exit}
            />
          )}
        </>
      )}
    </section>
  );
}

function TradeRecorder({
  binding,
  account,
  assets,
  purposes,
  purposesError,
  retryPurposes,
  initialState,
  day,
  close,
}: {
  binding: TradeBinding;
  account: Account;
  assets: Asset[];
  purposes: Purpose[];
  purposesError: boolean;
  retryPurposes: () => void;
  initialState?: ManualAccountState;
  day: string;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const manual = account.trackingMode === "HOLDINGS";
  const [fields, setFields] = useState<ActualTradeFields>(() => ({
    assetId: binding.body.assetId ?? "",
    side: binding.body.action === "SELL" ? "SELL" : "BUY",
    activityDate: day,
    quantity: "",
    price: "",
    amount: "",
    fee: "",
    tax: "",
    currency: initialState?.currency ?? account.currency,
    fxRate: "",
    observedCashBalance: "",
    notes: "",
    reason: "",
    deviationReason: "",
  }));
  const [observed, setObserved] = useState(initialState);
  const [stage, setStage] = useState<"facts" | "ownership" | "review" | "receipt">("facts");
  const [prepared, setPrepared] = useState<PreparedTrade | null>(null);
  const [review, setReview] = useState<{ preview: TradePreview; ownership: Ownership } | null>(
    null,
  );
  const [receipt, setReceipt] = useState<TradeReceipt | null>(null);
  const [mode, setMode] = useState<Ownership["mode"] | null>(null);
  const [shares, setShares] = useState<TradeShare[]>([]);
  const [changedShares, setChangedShares] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (stage !== "facts") heading.current?.focus();
  }, [stage]);
  useJournalNavigationGuard(dirty || reading, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  const locked = reading || write.busy || write.uncertain;
  const previewCommand = manual ? "preview_decision_manual_trade" : "preview_decision_ledger_trade";
  const writeCommand = manual ? "record_decision_manual_trade" : "record_decision_ledger_trade";

  async function readFacts() {
    const asset = assets.find((row) => row.id === fields.assetId);
    if (!asset) return;
    setReading(true);
    setError(null);
    write.clearError();
    try {
      let state: ManualAccountState | undefined;
      if (manual) {
        state = await executeInvestmentJournal<ManualAccountState>(
          "get_investment_journal_account_state",
          { accountId: account.id },
        );
        setObserved(state);
        if (
          !state.hasSnapshot ||
          !state.stateVersion ||
          state.accountId !== account.id ||
          state.currency !== fields.currency ||
          state.trackingMode !== "HOLDINGS"
        )
          throw new InvestmentJournalError("state_conflict", "Manual account changed");
      }
      const common = {
        accountId: account.id,
        assetId: asset.id,
        side: fields.side,
        activityDate: fields.activityDate,
        quantity: fields.quantity,
        currency: fields.currency,
        fee: fields.fee || null,
        tax: fields.tax || null,
        notes: fields.notes.trim() || null,
        requestKey: crypto.randomUUID(),
      };
      const trade = manual
        ? {
            ...common,
            price: fields.price,
            observedCashBalance: fields.observedCashBalance || null,
            expectedStateVersion: state?.stateVersion,
          }
        : {
            ...common,
            unitPrice: fields.price || null,
            amount: fields.amount || null,
            fxRate: fields.fxRate || null,
          };
      const args = {
        decision: {
          decisionId: binding.decisionId,
          revisionId: binding.revisionId,
          deviationReason: fields.deviationReason.trim() || null,
        },
        trade,
      };
      const facts = await executeInvestmentJournal<TradePreview>(previewCommand, {
        ...args,
        ownership: deferred,
      });
      if (
        facts.decision.id !== binding.revisionId ||
        facts.ownershipMode !== "RECONCILE_LATER" ||
        !facts.previewHash
      )
        throw new InvestmentJournalError("state_conflict", "Preview target changed");
      if (
        prepared &&
        (prepared.fields.assetId !== fields.assetId ||
          prepared.fields.side !== fields.side ||
          prepared.facts.trade.settlementCurrency !== facts.trade.settlementCurrency)
      ) {
        setShares((current) =>
          current.map((line) => ({ ...line, quantity: "", netCash: "", topUps: [] })),
        );
        setChangedShares(true);
      } else {
        setShares((current) =>
          current.map((line) => ({
            ...line,
            topUps: line.topUps.filter((topUp) =>
              facts.trade.changes.some(
                (change) =>
                  change.delta.startsWith("-") && sameResource(change.resource, topUp.resource),
              ),
            ),
          })),
        );
      }
      if (!facts.trade.automaticAllocationEligible && mode === "APPLY") setMode(null);
      setPrepared({ account, asset, fields: { ...fields }, args, facts });
      setStage("ownership");
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }
  async function readOwnership() {
    if (!prepared || !mode) return;
    setError(null);
    write.clearError();
    if (mode === "RECONCILE_LATER") {
      setReview({ preview: prepared.facts, ownership: deferred });
      setStage("review");
      return;
    }
    setReading(true);
    const ownership: Ownership = { mode: "APPLY", shares: structuredClone(shares) };
    try {
      const preview = await executeInvestmentJournal<TradePreview>(previewCommand, {
        ...prepared.args,
        ownership,
      });
      if (
        preview.decision.id !== binding.revisionId ||
        preview.ownershipMode !== "APPLY" ||
        !preview.funding ||
        !preview.ownership ||
        !preview.previewHash
      )
        throw new InvestmentJournalError("state_conflict", "Ownership preview changed");
      setReview({ preview, ownership });
      setStage("review");
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }
  function editActual() {
    write.clearError();
    setError(null);
    setStage("facts");
  }
  function commit() {
    if (!prepared || !review || write.error) return;
    void write.run<TradeReceipt>(
      writeCommand,
      {
        ...prepared.args,
        ownership: review.ownership,
        reason: prepared.fields.reason,
        expectedPreviewHash: review.preview.previewHash,
      },
      (result) => {
        setDirty(false);
        setReceipt(result);
        setStage("receipt");
        void invalidate();
      },
    );
  }
  const sourcesMissing =
    review?.ownership.mode === "APPLY" &&
    (!Array.isArray(review.preview.funding?.sourceReferences) ||
      !review.preview.funding?.resources.every((row) => Array.isArray(row.observation.sources)));
  return (
    <div className="space-y-6">
      {stage === "facts" && (
        <form
          className="space-y-6"
          onSubmit={(event) => {
            event.preventDefault();
            void readFacts();
          }}
        >
          {observed && (
            <section className="space-y-2 text-sm">
              <h3 className="font-medium">{t("manual_baseline")}</h3>
              <p className="text-muted-foreground">
                {t("as_of", { date: observed.asOfDate })} ·{" "}
                {observed.snapshotDate
                  ? t("source_date", { date: observed.snapshotDate })
                  : t("source_unknown")}
              </p>
              <dl className="flex flex-wrap gap-4">
                {Object.entries(observed.cashBalances).map(([currency, amount]) => (
                  <div key={currency}>
                    <dt className="text-muted-foreground">{currency}</dt>
                    <dd>
                      <ExactValue value={amount} />
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          )}
          <fieldset disabled={locked}>
            <ActualTradeFieldsForm
              value={fields}
              update={(value) => {
                setFields(value);
                setDirty(true);
              }}
              account={account}
              assets={assets}
            />
          </fieldset>
          {isBalanceHidden && (
            <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
          )}
          <Button disabled={locked || isBalanceHidden}>
            {t(reading ? "loading" : "preview_actual_trade")}
          </Button>
        </form>
      )}
      {stage === "ownership" && prepared && (
        <div className="space-y-6">
          <h3 ref={heading} tabIndex={-1} className="text-lg font-semibold outline-none">
            {t("trade_ownership_step")}
          </h3>
          <TradeImpact prepared={prepared} preview={prepared.facts} binding={binding} />
          <form
            className="space-y-5 border-t pt-5"
            onSubmit={(event) => {
              event.preventDefault();
              void readOwnership();
            }}
          >
            <fieldset disabled={locked} className="space-y-4">
              <legend className="mb-3 font-semibold">{t("trade_ownership_mode")}</legend>
              {(["APPLY", "RECONCILE_LATER"] as const).map((option) => (
                <label key={option} className="flex items-start gap-3 text-sm">
                  <input
                    type="radio"
                    name="tradeOwnershipMode"
                    className="accent-primary mt-0.5 size-4"
                    checked={mode === option}
                    disabled={
                      option === "APPLY" &&
                      (!prepared.facts.trade.automaticAllocationEligible ||
                        !purposes.some((p) => !p.isArchived))
                    }
                    onChange={() => {
                      setMode(option);
                      setDirty(true);
                    }}
                  />
                  {t(option === "APPLY" ? "apply_trade_ownership" : "defer_trade_ownership")}
                </label>
              ))}
              {!prepared.facts.trade.automaticAllocationEligible && (
                <p className="text-muted-foreground text-sm">
                  {t("trade_requires_reconciliation")}
                </p>
              )}
              {purposesError && (
                <div className="space-y-2">
                  <p role="alert">{t("records_error")}</p>
                  <Button type="button" variant="outline" onClick={retryPurposes}>
                    {t("retry")}
                  </Button>
                </div>
              )}
              {prepared.facts.trade.automaticAllocationEligible &&
                purposes.length > 0 &&
                !purposes.some((p) => !p.isArchived) && (
                  <p className="text-muted-foreground text-sm">{t("trade_no_active_purposes")}</p>
                )}
              {mode === "APPLY" && (
                <>
                  {isBalanceHidden && (
                    <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
                  )}
                  {changedShares && (
                    <p role="status" className="text-sm">
                      {t("recheck_changed_shares")}
                    </p>
                  )}
                  <TradeShares
                    prepared={prepared}
                    purposes={purposes}
                    shares={shares}
                    update={(value) => {
                      setShares(value);
                      setDirty(true);
                    }}
                  />
                </>
              )}
            </fieldset>
            <div className="flex flex-wrap gap-3">
              <Button
                disabled={
                  locked || !mode || (mode === "APPLY" && (shares.length === 0 || isBalanceHidden))
                }
              >
                {t(reading ? "loading" : "review_trade_submission")}
              </Button>
              <Button type="button" variant="outline" disabled={locked} onClick={editActual}>
                {t("return_edit")}
              </Button>
            </div>
          </form>
        </div>
      )}
      {stage === "review" && prepared && review && (
        <div className="space-y-6">
          <h3 ref={heading} tabIndex={-1} className="text-lg font-semibold outline-none">
            {t("trade_submission_title", { number: review.preview.decision.revisionNumber })}
          </h3>
          <TradeImpact prepared={prepared} preview={review.preview} binding={binding} />
          <OwnershipImpact prepared={prepared} preview={review.preview} purposes={purposes} />
          <div className="space-y-2 border-t pt-5 text-sm">
            <p className="whitespace-pre-wrap break-words">
              {t("operation_reason")}: {prepared.fields.reason}
            </p>
            {prepared.fields.deviationReason && (
              <p className="whitespace-pre-wrap break-words">
                {t("deviation_reason")}: {prepared.fields.deviationReason}
              </p>
            )}
            {prepared.fields.notes && (
              <p className="whitespace-pre-wrap break-words">
                {t("trade_notes")}: {prepared.fields.notes}
              </p>
            )}
          </div>
          {sourcesMissing && <p role="alert">{t("source_list_unavailable")}</p>}
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !!write.error || sourcesMissing} onClick={commit}>
              {t("submit_actual_trade")}
            </Button>
            <Button
              variant="outline"
              disabled={locked || !!write.error}
              onClick={() => {
                write.clearError();
                setStage("ownership");
              }}
            >
              {t("return_trade_ownership")}
            </Button>
            <Button variant="ghost" disabled={locked} onClick={editActual}>
              {t("return_edit")}
            </Button>
          </div>
        </div>
      )}
      {stage === "receipt" && receipt && (
        <div role="status" className="space-y-4 border-t pt-6">
          <h3 ref={heading} tabIndex={-1} className="text-lg font-semibold outline-none">
            {t("trade_recorded", { number: binding.revisionNumber })}
          </h3>
          <p className="text-sm leading-relaxed">
            {t(
              receipt.ownershipStatus === "APPLIED"
                ? "trade_ownership_recorded"
                : "deferred_ownership_hint",
            )}
          </p>
          <Button onClick={close}>{t("back_to_decision")}</Button>
        </div>
      )}
      {(error || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
    </div>
  );
}
