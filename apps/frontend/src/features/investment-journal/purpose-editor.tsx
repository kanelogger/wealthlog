import { useState } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { ExactInput, Field, JournalErrorNotice, selectClass } from "./controls";
import { useJournalLocale } from "./locale-context";
import { useJournalNavigationGuard } from "./navigation-guard";
import { useJournalWrite } from "./use-journal-write";
import { useInvalidateJournal } from "./queries";
import type { Purpose } from "./types";

export function PurposeEditor({
  initial,
  currency,
  close,
}: {
  initial?: Purpose;
  currency: string;
  close: () => void;
}) {
  const { t } = useJournalLocale();
  const [name, setName] = useState(initial?.name ?? "");
  const [category, setCategory] = useState(initial?.category ?? "CUSTOM");
  const [code, setCode] = useState(initial?.currency ?? currency);
  const [targetAmount, setTarget] = useState(initial?.targetAmount ?? "");
  const [minimumCash, setMinimum] = useState(initial?.minimumCash ?? "");
  const [lossBudget, setLoss] = useState(initial?.lossBudget ?? "");
  const [limit, setLimit] = useState(initial?.singleAssetLimitBps?.toString() ?? "");
  const [useOn, setUseOn] = useState(initial?.useOn ?? "");
  const [liquidity, setLiquidity] = useState(initial?.liquidity ?? "");
  const [archived, setArchived] = useState(initial?.isArchived ?? false);
  const [dirty, setDirty] = useState(false);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  useJournalNavigationGuard(dirty, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  return (
    <form
      className="max-w-3xl space-y-6"
      onChange={() => setDirty(true)}
      onSubmit={(event) => {
        event.preventDefault();
        void write.run(
          "save_fund_purpose",
          {
            id: initial?.id ?? null,
            expectedVersion: initial?.version ?? null,
            name,
            category,
            currency: code,
            targetAmount: targetAmount || null,
            minimumCash: minimumCash || null,
            lossBudget: lossBudget || null,
            singleAssetLimitBps: limit === "" ? null : Number(limit),
            useOn: useOn || null,
            liquidity: liquidity || null,
            isArchived: archived,
            requestKey: crypto.randomUUID(),
          },
          () => {
            setDirty(false);
            void invalidate();
            close();
          },
        );
      }}
    >
      <h2 className="text-xl font-semibold">{t(initial ? "edit_purpose" : "new_purpose")}</h2>
      <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
        {t("purpose_definition_hint")}
      </p>
      <fieldset disabled={write.busy || write.uncertain} className="grid gap-5 sm:grid-cols-2">
        <Field label={t("purpose_name")}>
          {(id) => (
            <Input id={id} required value={name} onChange={(e) => setName(e.target.value)} />
          )}
        </Field>
        <Field label={t("purpose_template")}>
          {(id) => (
            <select
              id={id}
              className={selectClass}
              value={category}
              onChange={(e) => setCategory(e.target.value)}
            >
              <option value="CUSTOM">{t("custom_purpose")}</option>
              <option value="EMERGENCY">{t("template_emergency")}</option>
              <option value="LONG_TERM">{t("template_long_term")}</option>
              <option value="NEAR_TERM">{t("template_near_term")}</option>
              {!["CUSTOM", "EMERGENCY", "LONG_TERM", "NEAR_TERM"].includes(category) && (
                <option value={category}>{category}</option>
              )}
            </select>
          )}
        </Field>
        <Field label={t("valuation_currency")} hint={t("valuation_currency_hint")}>
          {(id) => (
            <Input
              id={id}
              required
              value={code}
              pattern="[A-Z]{3}"
              maxLength={3}
              onChange={(e) => setCode(e.target.value.toUpperCase())}
            />
          )}
        </Field>
        <Field label={t("target_amount")}>
          {(id) => (
            <ExactInput
              id={id}
              value={targetAmount}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => setTarget(e.target.value)}
            />
          )}
        </Field>
        <Field label={t("use_on")}>
          {(id) => (
            <Input id={id} type="date" value={useOn} onChange={(e) => setUseOn(e.target.value)} />
          )}
        </Field>
        <Field label={t("liquidity")}>
          {(id) => (
            <Input id={id} value={liquidity} onChange={(e) => setLiquidity(e.target.value)} />
          )}
        </Field>
        <Field label={t("minimum_cash")}>
          {(id) => (
            <ExactInput
              id={id}
              value={minimumCash}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => setMinimum(e.target.value)}
            />
          )}
        </Field>
        <Field label={t("loss_budget")}>
          {(id) => (
            <ExactInput
              id={id}
              value={lossBudget}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => setLoss(e.target.value)}
            />
          )}
        </Field>
        <Field label={t("asset_limit_bps")} hint={t("asset_limit_bps_hint")}>
          {(id) => (
            <Input
              id={id}
              type="number"
              min="0"
              max="10000"
              step="1"
              value={limit}
              onChange={(e) => setLimit(e.target.value)}
            />
          )}
        </Field>
        <label className="flex items-center gap-3 self-end py-3 text-sm">
          <input
            type="checkbox"
            className="accent-primary size-4"
            checked={archived}
            onChange={(e) => setArchived(e.target.checked)}
          />
          {t("archive_purpose")}
        </label>
      </fieldset>
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
      <div className="flex flex-wrap gap-3">
        <Button disabled={write.busy || write.uncertain}>{t("save_purpose_only")}</Button>
        <Button
          type="button"
          variant="outline"
          disabled={write.busy || write.uncertain}
          onClick={() => {
            if (!dirty || window.confirm(t("discard_navigation"))) close();
          }}
        >
          {t("cancel")}
        </Button>
      </div>
    </form>
  );
}
