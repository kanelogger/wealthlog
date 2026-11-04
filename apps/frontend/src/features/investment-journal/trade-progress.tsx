import { ExactValue } from "./controls";
import { useJournalLocale } from "./locale-context";
import type { TradeBinding, ExecutionProgress } from "./trade-types";

export function TradeProgress({
  binding,
  progress,
  beforeTrade = false,
}: {
  binding: TradeBinding;
  progress?: ExecutionProgress;
  beforeTrade?: boolean;
}) {
  const { t } = useJournalLocale();
  return (
    <section className="space-y-4 border-t pt-5">
      <h3 className="font-semibold">
        {t(beforeTrade ? "trade_progress_before_recording" : "trade_plan_progress")}
      </h3>
      <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
        {(
          [
            ["planned_quantity", binding.body.plannedQuantity, undefined],
            ["planned_amount", binding.body.plannedAmount, binding.body.currency ?? undefined],
            ["linked_quantity", progress?.quantityForPlannedAsset, undefined],
            [
              "linked_cash",
              progress?.settlementCashForPlanCurrency,
              binding.body.currency ?? undefined,
            ],
            ["remaining_quantity", progress?.remainingQuantity, undefined],
            ["remaining_cash", progress?.remainingCashAmount, binding.body.currency ?? undefined],
          ] as const
        ).map(([key, value, currency]) => (
          <div key={key} className="flex flex-wrap justify-between gap-3">
            <dt className="text-muted-foreground">{t(key)}</dt>
            <dd>
              <ExactValue value={value} currency={currency} />
            </dd>
          </div>
        ))}
      </dl>
      {progress?.quantityUnitDate && (
        <p className="text-muted-foreground text-xs">
          {t("plan_unit_date", { date: progress.quantityUnitDate })}
        </p>
      )}
      {progress?.quantityUnitStatus && progress.quantityUnitStatus !== "VERIFIED_UNCHANGED" && (
        <p className="text-muted-foreground text-sm leading-relaxed">{t("plan_unit_unknown")}</p>
      )}
      <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
        {t("progress_no_auto_close")}
      </p>
    </section>
  );
}
