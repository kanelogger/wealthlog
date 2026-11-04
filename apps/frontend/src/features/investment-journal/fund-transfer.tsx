import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { executeInvestmentJournal, getTransferPairForActivity, searchActivities } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { isSecuritiesTransfer } from "@/lib/activity-utils";
import { QueryKeys } from "@/lib/query-keys";
import type { Account } from "@/lib/types";
import { ExactInput, ExactValue, Field, JournalErrorNotice, selectClass } from "./controls";
import { ResourceBalances } from "./fund-allocation-editor";
import { FundSources } from "./fund-sources";
import type { FundReversalPreview } from "./fund-types";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { journalKey, useInvalidateJournal } from "./queries";
import type { Purpose, Resource } from "./types";
import { useJournalWrite } from "./use-journal-write";

interface Share {
  purposeId: string;
  sourceAmount: string;
}
type Cash = Extract<Resource, { kind: "CASH" }>;
interface TransferPreview extends FundReversalPreview {
  transfer: {
    sourceResource: Cash;
    destinationResource: Cash;
    sourceAmount: string;
    destinationAmount: string;
    occurredAt: string;
  };
  shares: (Share & { destinationAmount: string })[];
}

const pageSize = 40;
const amountPattern = "([0-9]+(\\.[0-9]*)?|\\.[0-9]+)";

export function FundTransfer({
  accounts,
  purposes,
  day,
  close,
}: {
  accounts: Account[];
  purposes: Purpose[];
  day: string;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const [mode, setMode] = useState<"existing" | "new">("existing");
  const [page, setPage] = useState(0);
  const [activityId, setActivityId] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [date, setDate] = useState(day);
  const [sourceCurrency, setSourceCurrency] = useState("");
  const [destinationCurrency, setDestinationCurrency] = useState("");
  const [sourceAmount, setSourceAmount] = useState("");
  const [destinationAmount, setDestinationAmount] = useState("");
  const [reason, setReason] = useState("");
  const [shares, setShares] = useState<Share[]>([{ purposeId: "", sourceAmount: "" }]);
  const [dirty, setDirty] = useState(false);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [review, setReview] = useState<{
    preview: TransferPreview;
    args: Record<string, unknown>;
    mode: typeof mode;
    reason: string;
  } | null>(null);
  const [saved, setSaved] = useState(false);
  const focus = useRef<HTMLHeadingElement>(null);
  const step = saved ? "receipt" : review ? "review" : "edit";
  useEffect(() => {
    focus.current?.focus();
  }, [step]);
  const write = useJournalWrite();
  const client = useQueryClient();
  const invalidate = useInvalidateJournal();
  const existing = useQuery({
    queryKey: [...journalKey(), "cash-transfer-candidates", page],
    queryFn: () => searchActivities(page, pageSize, { activityTypes: ["TRANSFER_OUT"] }, ""),
    enabled: mode === "existing" && !review && !saved,
    retry: false,
  });
  const pair = useQuery({
    queryKey: [...journalKey(), "cash-transfer-pair", activityId],
    queryFn: () => getTransferPairForActivity(activityId),
    enabled: mode === "existing" && !!activityId && !review && !saved,
    retry: false,
  });
  const candidates =
    existing.data?.data.filter(
      (row) =>
        row.status === "POSTED" &&
        row.sourceGroupId &&
        !isSecuritiesTransfer(row.activityType, row.assetSymbol, row.assetId),
    ) ?? [];
  const active = accounts.filter(
    (account) =>
      account.isActive &&
      !account.isArchived &&
      ["CASH", "SECURITIES", "CRYPTOCURRENCY"].includes(account.accountType),
  );
  const availablePurposes = purposes.filter((purpose) => !purpose.isArchived);
  const locked = reading || write.busy || write.uncertain;
  useJournalNavigationGuard(dirty || reading, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  const accountName = (id: string) => accounts.find((account) => account.id === id)?.name ?? id;
  const purposeName = (id: string) => purposes.find((purpose) => purpose.id === id)?.name ?? id;
  const resourceName = (resource: Resource) =>
    `${accountName(resource.accountId)} · ${resource.kind === "CASH" ? resource.currency : resource.assetId}`;
  const crossCurrency =
    sourceCurrency.trim().toUpperCase() !== destinationCurrency.trim().toUpperCase();

  async function preview(event: FormEvent) {
    event.preventDefault();
    if (
      locked ||
      isBalanceHidden ||
      !reason.trim() ||
      shares.length === 0 ||
      (mode === "existing" && !pair.data)
    )
      return;
    setReading(true);
    setError(null);
    write.clearError();
    const requestKey = crypto.randomUUID();
    const transfer = {
      fromAccountId: from,
      toAccountId: to,
      activityDate: date,
      sourceCurrency: sourceCurrency.trim().toUpperCase(),
      destinationCurrency: destinationCurrency.trim().toUpperCase(),
      sourceAmount,
      destinationAmount: crossCurrency ? destinationAmount : null,
      requestKey,
    };
    const args = mode === "new" ? { transfer, shares } : { activityId, shares };
    try {
      const result = await executeInvestmentJournal<TransferPreview>(
        mode === "new" ? "preview_cash_transfer_ownership" : "preview_existing_transfer_ownership",
        args,
      );
      setReview({
        preview: result,
        args: mode === "new" ? args : { ...args, requestKey },
        mode,
        reason: reason.trim(),
      });
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }

  function commit() {
    if (!review || locked || write.error) return;
    void write.run(
      review.mode === "new"
        ? "create_cash_transfer_with_ownership"
        : "apply_existing_transfer_ownership",
      { ...review.args, expectedPreviewHash: review.preview.previewHash, reason: review.reason },
      () => {
        setDirty(false);
        setSaved(true);
        void invalidate();
        if (review.mode === "new")
          void client.invalidateQueries({ queryKey: [QueryKeys.ACTIVITY_DATA] });
      },
    );
  }

  return (
    <section className="max-w-4xl space-y-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h2
          ref={step === "edit" ? focus : undefined}
          tabIndex={-1}
          className="text-xl font-semibold"
        >
          {t("operation_TRANSFER")}
        </h2>
        <Button
          variant="ghost"
          disabled={locked}
          onClick={() => {
            if (allowJournalNavigation()) close();
          }}
        >
          {t("transfer_back")}
        </Button>
      </header>
      {saved ? (
        <h3 ref={focus} tabIndex={-1} role="status" className="font-semibold">
          {t("transfer_saved")}
        </h3>
      ) : review ? (
        <>
          <h3 ref={focus} tabIndex={-1} className="font-semibold">
            {t("allocation_preview")}
          </h3>
          <p className="text-muted-foreground text-sm">
            {t(review.mode === "new" ? "transfer_new_hint" : "transfer_existing_hint")}
          </p>
          <p className="text-sm">{review.preview.transfer.occurredAt}</p>
          <dl className="grid gap-4 border-y py-4 text-sm sm:grid-cols-2">
            {(
              [
                [
                  review.preview.transfer.sourceResource,
                  review.preview.transfer.sourceAmount,
                  "transfer_out",
                ],
                [
                  review.preview.transfer.destinationResource,
                  review.preview.transfer.destinationAmount,
                  "transfer_in",
                ],
              ] as const
            ).map(([resource, amount, label]) => (
              <div key={label} className="space-y-1">
                <dt>
                  {t(label)} · {accountName(resource.accountId)}
                </dt>
                <dd>
                  <ExactValue value={amount} currency={resource.currency} />
                </dd>
              </div>
            ))}
          </dl>
          <ul className="space-y-3 text-sm">
            {review.preview.shares.map((share) => (
              <li
                key={share.purposeId}
                className="flex flex-wrap items-center justify-between gap-3"
              >
                <span>{purposeName(share.purposeId)}</span>
                <span className="flex flex-wrap items-center gap-2">
                  <ExactValue
                    value={share.sourceAmount}
                    currency={review.preview.transfer.sourceResource.currency}
                  />{" "}
                  →{" "}
                  <ExactValue
                    value={share.destinationAmount}
                    currency={review.preview.transfer.destinationResource.currency}
                  />
                </span>
              </li>
            ))}
          </ul>
          <h4 className="font-medium">{t("transfer_balances_before")}</h4>
          {review.preview.funding.resources.map((row) => (
            <section key={JSON.stringify(row.observation.resource)} className="space-y-2">
              <h4 className="font-medium">{resourceName(row.observation.resource)}</h4>
              <ResourceBalances row={row} />
            </section>
          ))}
          <FundSources preview={review.preview.funding} describeResource={resourceName} trade />
          <section className="space-y-3 border-t pt-4">
            <h4 className="font-medium">{t("transfer_balances_after")}</h4>
            <ul className="space-y-2 text-sm">
              {review.preview.balancesAfter.map((row) => (
                <li
                  key={JSON.stringify([row.purposeId, row.resource])}
                  className="flex flex-wrap justify-between gap-3"
                >
                  <span>
                    {purposeName(row.purposeId)} · {resourceName(row.resource)}
                  </span>
                  <ExactValue
                    value={row.amount}
                    currency={row.resource.kind === "CASH" ? row.resource.currency : undefined}
                  />
                </li>
              ))}
            </ul>
            {review.preview.pendingReconciliation.length > 0 && (
              <p className="text-muted-foreground text-sm">{t("trade_existing_reconciliation")}</p>
            )}
          </section>
          <p className="whitespace-pre-wrap break-words text-sm">
            {t("operation_reason")} · {review.reason}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !!write.error} onClick={commit}>
              {t(review.mode === "new" ? "transfer_submit_new" : "transfer_submit")}
            </Button>
            <Button
              variant="outline"
              disabled={locked}
              onClick={() => {
                write.clearError();
                setReview(null);
              }}
            >
              {t("return_edit")}
            </Button>
          </div>
        </>
      ) : (
        <form className="space-y-5" onSubmit={preview} onChange={() => setDirty(true)}>
          <fieldset disabled={locked} className="space-y-5">
            <Field
              label={t("transfer_mode")}
              hint={t(mode === "new" ? "transfer_new_hint" : "transfer_existing_hint")}
            >
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={mode}
                  onChange={(e) => {
                    setMode(e.target.value === "new" ? "new" : "existing");
                    setError(null);
                  }}
                >
                  <option value="existing">{t("transfer_existing")}</option>
                  <option value="new">{t("transfer_new")}</option>
                </select>
              )}
            </Field>
            {mode === "existing" ? (
              <>
                <Field label={t("transfer_select")}>
                  {(id) => (
                    <select
                      id={id}
                      className={selectClass}
                      required
                      value={activityId}
                      onChange={(e) => setActivityId(e.target.value)}
                    >
                      <option value="">{t("choose")}</option>
                      {candidates.map((row) => (
                        <option key={row.id} value={row.id}>
                          {String(row.date).slice(0, 10)} · {row.accountName} · {row.currency}
                          {!isBalanceHidden && row.amount != null ? ` · ${row.amount}` : ""}
                          {row.comment ? ` · ${row.comment}` : ""}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                {existing.data && !candidates.length && (
                  <p className="text-muted-foreground text-sm">{t("transfer_no_candidates")}</p>
                )}
                {activityId && pair.data && (
                  <p className="break-words text-sm">
                    {accountName(pair.data.transferOut.accountId)} →{" "}
                    {accountName(pair.data.transferIn.accountId)}
                  </p>
                )}
                {activityId && pair.data === null && (
                  <p role="alert">{t("transfer_invalid_pair")}</p>
                )}
                {pair.isLoading && <p role="status">{t("loading")}</p>}
                {pair.isError && (
                  <div className="space-y-2">
                    <p role="alert">{t("records_error")}</p>
                    <Button type="button" variant="outline" onClick={() => void pair.refetch()}>
                      {t("retry")}
                    </Button>
                  </div>
                )}
                {existing.isLoading && <p role="status">{t("loading")}</p>}
                {existing.isError && (
                  <>
                    <p role="alert">{t("records_error")}</p>
                    <Button type="button" variant="outline" onClick={() => void existing.refetch()}>
                      {t("retry")}
                    </Button>
                  </>
                )}
                <div className="flex gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    disabled={page === 0}
                    onClick={() => {
                      setActivityId("");
                      setPage(page - 1);
                    }}
                  >
                    {t("previous")}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    disabled={(page + 1) * pageSize >= (existing.data?.meta.totalRowCount ?? 0)}
                    onClick={() => {
                      setActivityId("");
                      setPage(page + 1);
                    }}
                  >
                    {t("next")}
                  </Button>
                </div>
              </>
            ) : (
              <div className="grid gap-4 sm:grid-cols-2">
                {(
                  [
                    [from, setFrom, setSourceCurrency, "transfer_out"],
                    [to, setTo, setDestinationCurrency, "transfer_in"],
                  ] as const
                ).map(([value, update, currency, label]) => (
                  <Field key={label} label={t(label)}>
                    {(id) => (
                      <select
                        id={id}
                        required
                        className={selectClass}
                        value={value}
                        onChange={(e) => {
                          update(e.target.value);
                          currency(active.find((a) => a.id === e.target.value)?.currency ?? "");
                        }}
                      >
                        <option value="">{t("choose")}</option>
                        {active.map((a) => (
                          <option key={a.id} value={a.id}>
                            {a.name} · {a.currency}
                          </option>
                        ))}
                      </select>
                    )}
                  </Field>
                ))}
                <Field label={t("transfer_date")}>
                  {(id) => (
                    <Input
                      id={id}
                      type="date"
                      required
                      value={date}
                      onChange={(e) => setDate(e.target.value)}
                    />
                  )}
                </Field>
                <div />
                <Field label={t("transfer_source_currency")}>
                  {(id) => (
                    <Input
                      id={id}
                      pattern="[A-Za-z]{3}"
                      maxLength={3}
                      required
                      value={sourceCurrency}
                      onChange={(e) => setSourceCurrency(e.target.value)}
                    />
                  )}
                </Field>
                <Field label={t("transfer_destination_currency")}>
                  {(id) => (
                    <Input
                      id={id}
                      pattern="[A-Za-z]{3}"
                      maxLength={3}
                      required
                      value={destinationCurrency}
                      onChange={(e) => setDestinationCurrency(e.target.value)}
                    />
                  )}
                </Field>
                <Field label={t("transfer_source_amount")}>
                  {(id) => (
                    <ExactInput
                      id={id}
                      required
                      pattern={amountPattern}
                      value={sourceAmount}
                      onChange={(e) => setSourceAmount(e.target.value)}
                    />
                  )}
                </Field>
                {crossCurrency && (
                  <Field label={t("transfer_destination_amount")}>
                    {(id) => (
                      <ExactInput
                        id={id}
                        required
                        pattern={amountPattern}
                        value={destinationAmount}
                        onChange={(e) => setDestinationAmount(e.target.value)}
                      />
                    )}
                  </Field>
                )}
              </div>
            )}
            <p className="text-muted-foreground text-sm">{t("transfer_share_hint")}</p>
            {availablePurposes.length === 0 && (
              <p className="text-muted-foreground text-sm">{t("trade_no_active_purposes")}</p>
            )}
            {shares.map((share, index) => (
              <fieldset
                key={index}
                className="grid items-end gap-3 border-t pt-3 sm:grid-cols-[1fr_1fr_auto]"
              >
                <legend className="pr-2 text-sm">
                  {t("purpose")} {index + 1}
                </legend>
                <Field label={t("purpose")}>
                  {(id) => (
                    <select
                      id={id}
                      className={selectClass}
                      required
                      value={share.purposeId}
                      onChange={(e) =>
                        setShares(
                          shares.map((s, i) =>
                            i === index ? { ...s, purposeId: e.target.value } : s,
                          ),
                        )
                      }
                    >
                      <option value="">{t("choose")}</option>
                      {availablePurposes.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
                <Field label={t("transfer_share_amount")}>
                  {(id) => (
                    <ExactInput
                      id={id}
                      required
                      pattern={amountPattern}
                      value={share.sourceAmount}
                      onChange={(e) =>
                        setShares(
                          shares.map((s, i) =>
                            i === index ? { ...s, sourceAmount: e.target.value } : s,
                          ),
                        )
                      }
                    />
                  )}
                </Field>
                <Button
                  type="button"
                  variant="ghost"
                  disabled={shares.length === 1}
                  onClick={() => {
                    setShares(shares.filter((_, i) => i !== index));
                    setDirty(true);
                  }}
                >
                  {t("remove")}
                </Button>
              </fieldset>
            ))}
            <Button
              type="button"
              variant="outline"
              disabled={availablePurposes.length <= shares.length}
              onClick={() => {
                setShares([...shares, { purposeId: "", sourceAmount: "" }]);
                setDirty(true);
              }}
            >
              {t("add_purpose_share")}
            </Button>
            <Field label={t("operation_reason")}>
              {(id) => (
                <Input
                  id={id}
                  required
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              )}
            </Field>
          </fieldset>
          {isBalanceHidden && (
            <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
          )}
          <Button
            disabled={
              locked ||
              isBalanceHidden ||
              !availablePurposes.length ||
              (mode === "existing" ? !pair.data : !from || !to || from === to)
            }
          >
            {t(reading ? "loading" : "preview_ownership")}
          </Button>
        </form>
      )}
      {(error || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
    </section>
  );
}
