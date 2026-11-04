import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { Button } from "@wealthfolio/ui";
import { ExactInput, Field, selectClass } from "./controls";
import { useJournalLocale } from "./locale-context";
import type { PreparedTrade, TradeShare } from "./trade-types";
import type { Purpose, Resource } from "./types";
import { sameResource } from "./fund-types";

export function TradeShares({
  prepared,
  purposes,
  shares,
  update,
}: {
  prepared: PreparedTrade;
  purposes: Purpose[];
  shares: TradeShare[];
  update: (shares: TradeShare[]) => void;
}) {
  const { t } = useJournalLocale();
  const { isBalanceHidden } = useBalancePrivacy();
  const netCash = prepared.facts.trade.netCash;
  const consumed = prepared.facts.trade.changes.filter((change) => change.delta.startsWith("-"));
  const change = (index: number, patch: Partial<TradeShare>) =>
    update(shares.map((line, i) => (i === index ? { ...line, ...patch } : line)));
  function topUp(index: number, resource: Resource, amount: string) {
    const line = shares[index];
    change(index, {
      topUps: [
        ...line.topUps.filter((item) => !sameResource(item.resource, resource)),
        ...(amount ? [{ resource, amount }] : []),
      ],
    });
  }
  const quantity = prepared.facts.trade.changes
    .find((change) => change.resource.kind === "SECURITY")
    ?.delta.replace(/^-/, "");
  return (
    <div className="space-y-5">
      <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
        {t("purpose_share_hint")}
      </p>
      {shares.map((line, index) => (
        <fieldset key={index} className="space-y-4 border-t pt-5">
          <legend className="pr-3 text-sm font-medium">
            {t("purpose")} {index + 1}
          </legend>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={t("purpose")}>
              {(id) => (
                <select
                  id={id}
                  required
                  className={selectClass}
                  value={line.purposeId}
                  onChange={(e) => change(index, { purposeId: e.target.value })}
                >
                  <option value="">{t("choose")}</option>
                  {purposes
                    .filter((p) => !p.isArchived)
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} · {p.currency}
                      </option>
                    ))}
                </select>
              )}
            </Field>
            <Field label={t("share_quantity")}>
              {(id) => (
                <ExactInput
                  id={id}
                  required
                  value={line.quantity}
                  pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                  onChange={(e) => change(index, { quantity: e.target.value })}
                />
              )}
            </Field>
            <Field
              label={t("share_cash", { currency: prepared.facts.trade.settlementCurrency ?? "" })}
            >
              {(id) => (
                <ExactInput
                  id={id}
                  required
                  value={line.netCash}
                  pattern="-?([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                  onChange={(e) => change(index, { netCash: e.target.value })}
                />
              )}
            </Field>
            {consumed.map((entry) => (
              <Field
                key={JSON.stringify(entry.resource)}
                label={t(entry.resource.kind === "CASH" ? "top_up_cash" : "top_up_security", {
                  currency: entry.resource.kind === "CASH" ? entry.resource.currency : "",
                })}
                hint={t("explicit_top_up_hint")}
              >
                {(id) => (
                  <ExactInput
                    id={id}
                    value={
                      line.topUps.find((item) => sameResource(item.resource, entry.resource))
                        ?.amount ?? ""
                    }
                    pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                    onChange={(e) => topUp(index, entry.resource, e.target.value)}
                  />
                )}
              </Field>
            ))}
          </div>
          <div className="flex flex-wrap gap-3">
            {shares.length === 1 && quantity && netCash != null && (
              <Button
                type="button"
                variant="outline"
                disabled={isBalanceHidden}
                onClick={() => change(index, { quantity, netCash })}
              >
                {t("copy_full_trade_share")}
              </Button>
            )}
            <Button
              type="button"
              variant="ghost"
              onClick={() => update(shares.filter((_, i) => i !== index))}
            >
              {t("remove")}
            </Button>
          </div>
        </fieldset>
      ))}
      <Button
        type="button"
        variant="outline"
        onClick={() =>
          update([...shares, { purposeId: "", quantity: "", netCash: "", topUps: [] }])
        }
      >
        {t("add_purpose_share")}
      </Button>
    </div>
  );
}
