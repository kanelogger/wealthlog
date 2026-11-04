import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import type { Account, Asset } from "@/lib/types";
import { useJournalLocale } from "./locale-context";
import type { DecisionBody, Purpose, Resource } from "./types";
import { ExactInput, Field, selectClass } from "./controls";

export function DecisionFundingFields({
  body,
  update,
  purposes,
  purposesReady = true,
  accounts,
  assets,
}: {
  body: DecisionBody;
  update: (body: DecisionBody) => void;
  purposes: Purpose[];
  purposesReady?: boolean;
  accounts: Account[];
  assets: Asset[];
}) {
  const { t } = useJournalLocale();
  function changeLine(index: number, patch: Partial<DecisionBody["funding"][number]>) {
    update({
      ...body,
      funding: body.funding.map((line, i) => (i === index ? { ...line, ...patch } : line)),
    });
  }
  return (
    <section className="space-y-5 border-t pt-7">
      <h2 className="text-lg font-semibold">{t("part_funding")}</h2>
      <fieldset className="space-y-3">
        <legend className="mb-2 text-sm font-medium">{t("purpose_selection")}</legend>
        {purposesReady && purposes.length === 0 && (
          <p className="text-muted-foreground text-sm">{t("no_purposes")}</p>
        )}
        {purposes
          .filter((p) => !p.isArchived || body.purposeIds.includes(p.id))
          .map((p) => (
            <label key={p.id} className="flex items-center gap-3 text-sm">
              <input
                type="checkbox"
                className="accent-primary size-4"
                checked={body.purposeIds.includes(p.id)}
                onChange={(event) =>
                  update({
                    ...body,
                    purposeIds: event.target.checked
                      ? [...body.purposeIds, p.id]
                      : body.purposeIds.filter((id) => id !== p.id),
                  })
                }
              />
              <span>
                {p.name} · {p.currency}
                {p.isArchived ? ` · ${t("archived")}` : ""}
              </span>
            </label>
          ))}
      </fieldset>
      <Field label={t("funding_status")}>
        {(id) => (
          <select
            id={id}
            className={selectClass}
            value={body.fundingStatus}
            onChange={(event) =>
              update({
                ...body,
                fundingStatus: event.target.value as DecisionBody["fundingStatus"],
              })
            }
          >
            <option value="UNKNOWN">{t("funding_UNKNOWN")}</option>
            <option value="PLANNED">{t("funding_PLANNED")}</option>
          </select>
        )}
      </Field>
      <p className="text-muted-foreground text-sm leading-relaxed">
        {t("no_automatic_allocation")}
      </p>
      {body.funding.map((line, index) => (
        <fieldset key={index} className="space-y-4 rounded-xl border p-4">
          <legend className="px-2 text-sm font-medium">
            {t("funds")} {index + 1}
          </legend>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={t("purpose")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={line.purposeId}
                  onChange={(event) => changeLine(index, { purposeId: event.target.value })}
                >
                  <option value="">{t("choose")}</option>
                  {purposes
                    .filter((p) => !p.isArchived || p.id === line.purposeId)
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} · {p.currency}
                      </option>
                    ))}
                </select>
              )}
            </Field>
            <Field label={t("account")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={line.resource.accountId}
                  onChange={(event) =>
                    changeLine(index, {
                      resource: { ...line.resource, accountId: event.target.value },
                    })
                  }
                >
                  <option value="">{t("choose")}</option>
                  {accounts
                    .filter(
                      (a) => (a.isActive && !a.isArchived) || a.id === line.resource.accountId,
                    )
                    .map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.name} · {a.currency}
                      </option>
                    ))}
                </select>
              )}
            </Field>
            <Field label={t("resource_kind")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={line.resource.kind}
                  onChange={(event) =>
                    changeLine(index, {
                      planned: "",
                      resource:
                        event.target.value === "CASH"
                          ? { kind: "CASH", accountId: line.resource.accountId, currency: "" }
                          : { kind: "SECURITY", accountId: line.resource.accountId, assetId: "" },
                    })
                  }
                >
                  <option value="CASH">{t("cash")}</option>
                  <option value="SECURITY">{t("security")}</option>
                </select>
              )}
            </Field>
            {line.resource.kind === "CASH" ? (
              <Field label={t("currency")}>
                {(id) => (
                  <Input
                    id={id}
                    value={line.resource.kind === "CASH" ? line.resource.currency : ""}
                    maxLength={3}
                    pattern="[A-Z]{3}"
                    onChange={(event) =>
                      changeLine(index, {
                        resource: {
                          ...line.resource,
                          currency: event.target.value.toUpperCase(),
                        } as Resource,
                      })
                    }
                  />
                )}
              </Field>
            ) : (
              <Field label={t("asset")}>
                {(id) => (
                  <select
                    id={id}
                    className={selectClass}
                    value={line.resource.kind === "SECURITY" ? line.resource.assetId : ""}
                    onChange={(event) =>
                      changeLine(index, {
                        resource: { ...line.resource, assetId: event.target.value } as Resource,
                      })
                    }
                  >
                    <option value="">{t("choose")}</option>
                    {assets.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.name ?? a.displayCode ?? a.id}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
            )}
            <Field label={t("planned_resource")}>
              {(id) => (
                <ExactInput
                  id={id}
                  value={line.planned}
                  pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                  onChange={(event) => changeLine(index, { planned: event.target.value })}
                />
              )}
            </Field>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => update({ ...body, funding: body.funding.filter((_, i) => i !== index) })}
          >
            {t("remove")}
          </Button>
        </fieldset>
      ))}
      <Button
        type="button"
        variant="outline"
        onClick={() =>
          update({
            ...body,
            funding: [
              ...body.funding,
              {
                purposeId: "",
                resource: { kind: "CASH", accountId: "", currency: "" },
                planned: "",
              },
            ],
          })
        }
      >
        {t("add_funding")}
      </Button>
    </section>
  );
}
