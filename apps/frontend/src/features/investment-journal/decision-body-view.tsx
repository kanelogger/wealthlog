import { ExactValue } from "./controls";
import { useJournalLocale } from "./locale-context";
import type { ContextCapture, DecisionBody } from "./types";

export function DecisionBodyView({
  body,
  capture,
}: {
  body: DecisionBody;
  capture?: ContextCapture;
}) {
  const { t } = useJournalLocale();
  const funding = capture?.response.funding.data;
  const assetName = (id: string) =>
    funding?.decisionReferences?.assets.find((asset) => asset.id === id)?.name ?? id;
  const accountName = (id: string) =>
    funding?.decisionReferences?.accounts.find((account) => account.id === id)?.name ?? id;
  const purposeName = (id: string) =>
    funding?.purposes?.find((purpose) => purpose.id === id)?.name ?? id;
  return (
    <article className="min-w-0 space-y-7">
      <section className="space-y-4">
        <h2 className="break-words text-xl font-semibold">{body.title || t("decisions")}</h2>
        <p className="font-medium">{body.action ? t(`action_${body.action}`) : t("unknown")}</p>
        <p className="text-sm">
          {t("asset")}: {body.assetId ? assetName(body.assetId) : t("no_asset")}
        </p>
        <dl className="grid gap-4 sm:grid-cols-2">
          <div>
            <dt className="text-muted-foreground text-sm">{t("planned_amount")}</dt>
            <dd className="mt-1">
              <ExactValue value={body.plannedAmount} currency={body.currency ?? undefined} />
            </dd>
          </div>
          <div>
            <dt className="text-muted-foreground text-sm">{t("planned_quantity")}</dt>
            <dd className="mt-1">
              <ExactValue value={body.plannedQuantity} />
            </dd>
          </div>
        </dl>
        <h3 className="font-medium">{t("thesis")}</h3>
        <p className="max-w-prose whitespace-pre-wrap break-words text-sm leading-relaxed">
          {body.thesis || t("not_recorded")}
        </p>
        <h3 className="font-medium">{t("accepted_disadvantages")}</h3>
        <p className="max-w-prose whitespace-pre-wrap break-words text-sm leading-relaxed">
          {body.acceptedDisadvantages || t("not_recorded")}
        </p>
      </section>
      <section className="space-y-4 border-t pt-6">
        <h3 className="font-semibold">{t("funding_status")}</h3>
        <p className="text-sm">{t(`funding_${body.fundingStatus}`)}</p>
        <p className="text-sm">
          {t("purpose_selection")}:{" "}
          {body.purposeIds.map(purposeName).join(" · ") || t("not_recorded")}
        </p>
        {body.funding.map((line, i) => (
          <p key={i} className="text-sm">
            {purposeName(line.purposeId)} · {accountName(line.resource.accountId)} ·{" "}
            {line.resource.kind === "SECURITY" ? `${assetName(line.resource.assetId)} · ` : ""}
            <ExactValue
              value={line.planned}
              currency={line.resource.kind === "CASH" ? line.resource.currency : undefined}
            />{" "}
            · {line.resource.kind === "CASH" ? t("cash") : t("security")}
          </p>
        ))}
      </section>
      <section className="space-y-4 border-t pt-6">
        <h3 className="font-semibold">{t("evidence")}</h3>
        {body.evidence.length === 0 && (
          <p className="text-muted-foreground text-sm">{t("not_recorded")}</p>
        )}
        {body.evidence.map((item, i) => (
          <div key={i} className="space-y-2 text-sm">
            <p className="font-medium">{t(item.informationKind)}</p>
            <p className="max-w-prose whitespace-pre-wrap break-words leading-relaxed">
              {item.content || t("not_recorded")}
            </p>
            <p className="text-muted-foreground break-words">{item.source || t("not_recorded")}</p>
            {item.probabilityLowerBps !== null && item.probabilityUpperBps !== null && (
              <p>
                {item.probabilityLowerBps / 100}% – {item.probabilityUpperBps / 100}% ·{" "}
                {item.probabilityBasis || t("not_recorded")}
              </p>
            )}
            {item.assumedLossAmount !== null && (
              <p>
                {t("loss_scenario")}:{" "}
                <ExactValue
                  value={item.assumedLossAmount}
                  currency={item.lossCurrency ?? undefined}
                />{" "}
                · {item.lossBasis || t("not_recorded")}
              </p>
            )}
          </div>
        ))}
      </section>
      <section className="space-y-4 border-t pt-6">
        <h3 className="font-semibold">{t("alternatives")}</h3>
        {body.options.map((option, i) => (
          <dl key={i} className="space-y-3 rounded-xl border p-4 text-sm">
            <dt className="break-words font-medium">{option.action}</dt>
            <dd className="whitespace-pre-wrap break-words">
              {option.description || t("not_recorded")}
            </dd>
            {(
              [
                ["advantages", "advantages"],
                ["disadvantages", "disadvantages"],
                ["notSelectedReason", "not_selected_reason"],
              ] as const
            ).map(([key, label]) => (
              <div key={key}>
                <dt className="text-muted-foreground">{t(label)}</dt>
                <dd className="mt-1 whitespace-pre-wrap break-words">
                  {option[key] || t("not_recorded")}
                </dd>
              </div>
            ))}
          </dl>
        ))}
      </section>
      <section className="space-y-4 border-t pt-6">
        {(
          [
            ["informationValue", "information_value"],
            ["waitingCost", "waiting_cost"],
          ] as const
        ).map(([key, label]) => (
          <div key={key}>
            <h3 className="font-medium">{t(label)}</h3>
            <p className="mt-2 max-w-prose whitespace-pre-wrap break-words text-sm leading-relaxed">
              {body[key] || t("unknown")}
            </p>
          </div>
        ))}
        <p className="break-words text-sm">
          {t("scenario_tags")}: {body.scenarioTags.join(" · ") || t("not_recorded")}
        </p>
        <h3 className="font-medium">{t("optional_principles")}</h3>
        <ul className="space-y-2 text-sm">
          {(body.principleRevisionIds ?? []).map((id) => {
            const version = funding?.principleVersions?.find((item) => item.id === id);
            return (
              <li key={id}>
                {version
                  ? `${version.body.name} · ${t("version", { number: version.revisionNumber })}`
                  : t("saved_principle")}
              </li>
            );
          })}
        </ul>
        <p className="break-words text-sm">
          {t("declared_at")}: {body.declaredAt || t("not_recorded")}
        </p>
        <p className="text-muted-foreground text-sm">
          {t("content_origin")}: {t(`origin_${body.contentOrigin}`)}
        </p>
      </section>
    </article>
  );
}
