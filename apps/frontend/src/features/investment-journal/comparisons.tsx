import { ExactValue } from "./controls";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import type { ContextCapture } from "./types";
import { resourceLabel } from "./context-labels";

const statusKeys: Record<string, JournalTextKey> = {
  UNABLE_TO_DETERMINE: "comparison_unknown",
  NOT_CONFIGURED: "comparison_unconfigured",
  NO_SCENARIO_PROVIDED: "comparison_no_scenario",
  PURPOSE_SHORTFALL: "comparison_shortfall",
  WITHIN_PURPOSE_OWNERSHIP: "comparison_owned",
  SCENARIO_EXCEEDS_BUDGET: "comparison_loss_exceeds",
  SCENARIO_WITHIN_BUDGET: "comparison_loss_within",
  BELOW_MINIMUM: "comparison_cash_below",
  AT_OR_ABOVE_MINIMUM: "comparison_cash_within",
  ABOVE_LIMIT: "comparison_asset_above",
  WITHIN_LIMIT: "comparison_asset_within",
};
export function Comparisons({ capture }: { capture: ContextCapture }) {
  const { t } = useJournalLocale();
  const data = capture.response.funding.data;
  const purpose = (id: string) =>
    data.purposes?.find((item) => item.id === id)?.name ?? t("data_unavailable_name");
  const status = (value: string) => t(statusKeys[value] ?? "comparison_unknown");
  return (
    <div className="space-y-5 text-sm">
      {data.decisionAssessment?.plannedFunding.map((line, i) => (
        <section key={i} className="space-y-2 border-t pt-4">
          <h4 className="font-medium">
            {purpose(line.purposeId)} · {t("funds")}
          </h4>
          <p className="break-words">{resourceLabel(capture, line.resource, t)}</p>
          <p>{status(line.status)}</p>
          <dl className="space-y-2">
            {(
              [
                ["planned", "planned_resource"],
                ["purposeAmountBefore", "allocated"],
                ["purposeAmountAfter", "simulated_remaining"],
                ["shortfall", "shortfall"],
                ["unallocatedAvailable", "unallocated"],
              ] as const
            ).map(([key, label]) => (
              <div key={key} className="flex flex-wrap justify-between gap-2">
                <dt className="text-muted-foreground">{t(label)}</dt>
                <dd>
                  <ExactValue
                    value={line[key]}
                    currency={line.resource.kind === "CASH" ? line.resource.currency : undefined}
                  />
                </dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
      {data.risks?.map((risk) => (
        <section key={risk.purposeId} className="space-y-2 border-t pt-4">
          <h4 className="font-medium">{purpose(risk.purposeId)}</h4>
          <p>
            {t("minimum_cash")}: {status(risk.minimumCashStatus)}
          </p>
          <p>
            {t("asset_limit")}: {status(risk.singleAssetLimitStatus)}
          </p>
          {risk.fxSourceDate && (
            <p className="text-muted-foreground text-xs">
              {t("fx_source_date", { date: risk.fxSourceDate })}
            </p>
          )}
        </section>
      ))}
      {data.decisionAssessment?.lossScenarios.map((loss) => (
        <section key={loss.purposeId} className="space-y-2 border-t pt-4">
          <h4 className="font-medium">
            {purpose(loss.purposeId)} · {t("loss_scenario")}
          </h4>
          <p>{status(loss.status)}</p>
          <p>
            {t("loss_budget")}:{" "}
            <ExactValue value={loss.lossBudget} currency={loss.currency ?? undefined} />
          </p>
          <p>
            {t("loss_amount")}:{" "}
            <ExactValue value={loss.assumedLossAmount} currency={loss.currency ?? undefined} />
          </p>
          <p className="text-muted-foreground leading-relaxed">
            {loss.hypothesisBasis || t("not_recorded")}
          </p>
          <p className="text-muted-foreground leading-relaxed">{t("loss_hint")}</p>
        </section>
      ))}
    </div>
  );
}
