import { Input } from "@wealthfolio/ui/components/ui/input";
import type { Account, Asset } from "@/lib/types";
import { ExactInput, Field, selectClass, textAreaClass } from "./controls";
import { useJournalLocale } from "./locale-context";
import type { ActualTradeFields } from "./trade-types";

export function ActualTradeFieldsForm({
  value,
  update,
  account,
  assets,
}: {
  value: ActualTradeFields;
  update: (value: ActualTradeFields) => void;
  account: Account;
  assets: Asset[];
}) {
  const { t } = useJournalLocale();
  const manual = account.trackingMode === "HOLDINGS";
  const change = (patch: Partial<ActualTradeFields>) => update({ ...value, ...patch });
  return (
    <div className="space-y-6">
      <div className="grid gap-5 sm:grid-cols-2">
        <Field label={t("trade_asset")} hint={t("trade_equity_scope")}>
          {(id) => (
            <select
              id={id}
              required
              className={selectClass}
              value={value.assetId}
              onChange={(e) => change({ assetId: e.target.value })}
            >
              <option value="">{t("choose")}</option>
              {assets
                .filter(
                  (asset) =>
                    asset.kind === "INVESTMENT" &&
                    (!asset.instrumentType || asset.instrumentType === "EQUITY"),
                )
                .map((asset) => (
                  <option key={asset.id} value={asset.id}>
                    {asset.name ?? asset.displayCode ?? asset.id}
                  </option>
                ))}
            </select>
          )}
        </Field>
        <Field label={t("trade_side")}>
          {(id) => (
            <select
              id={id}
              className={selectClass}
              value={value.side}
              onChange={(e) => change({ side: e.target.value as "BUY" | "SELL" })}
            >
              <option value="BUY">{t("action_BUY")}</option>
              <option value="SELL">{t("action_SELL")}</option>
            </select>
          )}
        </Field>
        <Field
          label={t("trade_date")}
          hint={t(manual ? "manual_trade_today" : "ledger_trade_date_hint")}
        >
          {(id) => (
            <Input
              id={id}
              type="date"
              required
              value={value.activityDate}
              onChange={(e) => change({ activityDate: e.target.value })}
            />
          )}
        </Field>
        <Field label={t("trade_currency")} hint={manual ? t("manual_trade_currency") : undefined}>
          {(id) => (
            <Input
              id={id}
              required
              value={value.currency}
              readOnly={manual}
              maxLength={3}
              pattern="[A-Za-z]{3}"
              onChange={(e) => change({ currency: e.target.value })}
            />
          )}
        </Field>
        <Field label={t("actual_quantity")}>
          {(id) => (
            <ExactInput
              id={id}
              required
              value={value.quantity}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => change({ quantity: e.target.value })}
            />
          )}
        </Field>
        <Field label={t("actual_price")}>
          {(id) => (
            <ExactInput
              id={id}
              required={manual}
              value={value.price}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => change({ price: e.target.value })}
            />
          )}
        </Field>
        {!manual && (
          <Field label={t("final_posted_cash")} hint={t("final_posted_cash_hint")}>
            {(id) => (
              <ExactInput
                id={id}
                value={value.amount}
                pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                onChange={(e) => change({ amount: e.target.value })}
              />
            )}
          </Field>
        )}
        <Field label={t("trade_fee")} hint={t("blank_fee_unknown")}>
          {(id) => (
            <ExactInput
              id={id}
              value={value.fee}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => change({ fee: e.target.value })}
            />
          )}
        </Field>
        <Field label={t("trade_tax")}>
          {(id) => (
            <ExactInput
              id={id}
              value={value.tax}
              pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
              onChange={(e) => change({ tax: e.target.value })}
            />
          )}
        </Field>
        {manual ? (
          <Field label={t("observed_cash_balance")} hint={t("cash_calibration_hint")}>
            {(id) => (
              <ExactInput
                id={id}
                value={value.observedCashBalance}
                pattern="-?([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                onChange={(e) => change({ observedCashBalance: e.target.value })}
              />
            )}
          </Field>
        ) : (
          <Field
            label={t("trade_fx_rate")}
            hint={t("trade_fx_hint", { currency: account.currency })}
          >
            {(id) => (
              <ExactInput
                id={id}
                value={value.fxRate}
                pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                onChange={(e) => change({ fxRate: e.target.value })}
              />
            )}
          </Field>
        )}
      </div>
      <Field label={t("deviation_reason")}>
        {(id) => (
          <textarea
            id={id}
            className={textAreaClass}
            value={value.deviationReason}
            onChange={(e) => change({ deviationReason: e.target.value })}
          />
        )}
      </Field>
      <Field label={t("trade_notes")}>
        {(id) => (
          <textarea
            id={id}
            className={textAreaClass}
            value={value.notes}
            onChange={(e) => change({ notes: e.target.value })}
          />
        )}
      </Field>
      <Field label={t("operation_reason")}>
        {(id) => (
          <textarea
            id={id}
            required
            className={textAreaClass}
            value={value.reason}
            onChange={(e) => change({ reason: e.target.value })}
          />
        )}
      </Field>
    </div>
  );
}
