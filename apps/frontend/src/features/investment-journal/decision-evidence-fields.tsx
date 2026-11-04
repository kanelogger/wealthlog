import { useState } from "react";
import { cn } from "@/lib/utils";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { useJournalLocale } from "./locale-context";
import { ExactInput, Field, selectClass, textAreaClass } from "./controls";
import type { DecisionBody, Evidence, PrincipleSummary, Purpose } from "./types";

function PercentInput({
  id,
  value,
  onChange,
}: {
  id: string;
  value: number | null;
  onChange: (value: number | null) => void;
}) {
  const [text, setText] = useState(value == null ? "" : String(value / 100));
  return (
    <Input
      id={id}
      type="text"
      inputMode="decimal"
      value={text}
      pattern="(100(\.0{0,2})?|[0-9]{1,2}(\.[0-9]{0,2})?)"
      onChange={(event) => {
        const raw = event.target.value;
        setText(raw);
        if (!/^(100(\.0{0,2})?|[0-9]{1,2}(\.[0-9]{0,2})?)$/.test(raw)) {
          onChange(null);
          return;
        }
        const [whole, fraction = ""] = raw.split(".");
        onChange(Number(BigInt(whole) * 100n + BigInt(fraction.padEnd(2, "0"))));
      }}
    />
  );
}
function EvidenceRow({
  value,
  update,
  purposes,
}: {
  value: Evidence;
  update: (value: Evidence) => void;
  purposes: Purpose[];
}) {
  const { t } = useJournalLocale();
  return (
    <div className="space-y-4">
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t("information_kind")}>
          {(id) => (
            <select
              id={id}
              className={selectClass}
              value={value.informationKind}
              onChange={(e) =>
                update({ ...value, informationKind: e.target.value as Evidence["informationKind"] })
              }
            >
              {(["FACT", "HYPOTHESIS", "UNKNOWN"] as const).map((kind) => (
                <option key={kind} value={kind}>
                  {t(kind)}
                </option>
              ))}
            </select>
          )}
        </Field>
        <Field label={t("source")}>
          {(id) => (
            <Input
              id={id}
              value={value.source ?? ""}
              onChange={(e) => update({ ...value, source: e.target.value || null })}
            />
          )}
        </Field>
      </div>
      <Field label={t("content")}>
        {(id) => (
          <textarea
            id={id}
            className={textAreaClass}
            value={value.content}
            onChange={(e) => update({ ...value, content: e.target.value })}
          />
        )}
      </Field>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t("prob_lower")}>
          {(id) => (
            <PercentInput
              id={id}
              value={value.probabilityLowerBps}
              onChange={(v) => update({ ...value, probabilityLowerBps: v })}
            />
          )}
        </Field>
        <Field label={t("prob_upper")}>
          {(id) => (
            <PercentInput
              id={id}
              value={value.probabilityUpperBps}
              onChange={(v) => update({ ...value, probabilityUpperBps: v })}
            />
          )}
        </Field>
      </div>
      <Field label={t("prob_basis")}>
        {(id) => (
          <Input
            id={id}
            value={value.probabilityBasis ?? ""}
            onChange={(e) => update({ ...value, probabilityBasis: e.target.value || null })}
          />
        )}
      </Field>
      <details className="text-sm">
        <summary className="focus-visible:ring-ring cursor-pointer rounded py-2 font-medium focus-visible:ring-2">
          {t("loss_scenario")}
        </summary>
        <div className="mt-4 space-y-4">
          <p className="text-muted-foreground leading-relaxed">{t("loss_hint")}</p>
          <Field label={t("purpose")}>
            {(id) => (
              <select
                id={id}
                className={selectClass}
                value={value.purposeId ?? ""}
                onChange={(e) =>
                  update({
                    ...value,
                    purposeId: e.target.value || null,
                    lossCurrency: purposes.find((p) => p.id === e.target.value)?.currency ?? null,
                  })
                }
              >
                <option value="">{t("choose")}</option>
                {purposes.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} · {p.currency}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={t("loss_amount")}>
              {(id) => (
                <ExactInput
                  id={id}
                  value={value.assumedLossAmount ?? ""}
                  onChange={(e) => update({ ...value, assumedLossAmount: e.target.value || null })}
                />
              )}
            </Field>
            <Field label={t("currency")}>
              {(id) => (
                <Input
                  id={id}
                  value={value.lossCurrency ?? ""}
                  maxLength={3}
                  onChange={(e) =>
                    update({ ...value, lossCurrency: e.target.value.toUpperCase() || null })
                  }
                />
              )}
            </Field>
          </div>
          <Field label={t("loss_basis")}>
            {(id) => (
              <textarea
                id={id}
                className={textAreaClass}
                value={value.lossBasis ?? ""}
                onChange={(e) => update({ ...value, lossBasis: e.target.value || null })}
              />
            )}
          </Field>
        </div>
      </details>
    </div>
  );
}
export function DecisionEvidenceFields({
  body,
  update,
  purposes,
  principles,
  alternativeErrorId,
}: {
  body: DecisionBody;
  update: (body: DecisionBody) => void;
  purposes: Purpose[];
  principles: PrincipleSummary[];
  alternativeErrorId?: string;
}) {
  const { t } = useJournalLocale();
  const [evidenceKeys, setEvidenceKeys] = useState(() =>
    body.evidence.map(() => crypto.randomUUID()),
  );
  const [optionKeys, setOptionKeys] = useState(() => body.options.map(() => crypto.randomUUID()));
  const refs = body.principleRevisionIds ?? [];
  return (
    <section className="space-y-6 border-t pt-7">
      <h2 className="text-lg font-semibold">{t("part_evidence")}</h2>
      <div className="space-y-4">
        <h3 className="font-medium">{t("evidence")}</h3>
        {body.evidence.map((item, index) => (
          <fieldset key={evidenceKeys[index]} className="space-y-3 rounded-xl border p-4">
            <legend className="px-2 text-sm">
              {t("evidence")} {index + 1}
            </legend>
            <EvidenceRow
              value={item}
              purposes={purposes}
              update={(value) =>
                update({
                  ...body,
                  evidence: body.evidence.map((old, i) => (i === index ? value : old)),
                })
              }
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                setEvidenceKeys((keys) => keys.filter((_, i) => i !== index));
                update({ ...body, evidence: body.evidence.filter((_, i) => i !== index) });
              }}
            >
              {t("remove")}
            </Button>
          </fieldset>
        ))}
        <Button
          type="button"
          variant="outline"
          onClick={() => {
            setEvidenceKeys((keys) => [...keys, crypto.randomUUID()]);
            update({
              ...body,
              evidence: [
                ...body.evidence,
                {
                  informationKind: "UNKNOWN",
                  content: "",
                  source: null,
                  probabilityLowerBps: null,
                  probabilityUpperBps: null,
                  probabilityBasis: null,
                  purposeId: null,
                  assumedLossAmount: null,
                  lossCurrency: null,
                  lossBasis: null,
                },
              ],
            });
          }}
        >
          {t("add_evidence")}
        </Button>
      </div>
      <div className="space-y-4">
        <h3 className="font-medium">{t("alternatives")}</h3>
        {body.options.map((option, index) => (
          <fieldset key={optionKeys[index]} className="space-y-4 rounded-xl border p-4">
            <legend className="px-2 text-sm">
              {t("alternative_action")} {index + 1}
            </legend>
            {(
              [
                ["action", "alternative_action"],
                ["description", "alternative_description"],
                ["advantages", "advantages"],
                ["disadvantages", "disadvantages"],
                ["notSelectedReason", "not_selected_reason"],
              ] as const
            ).map(([key, label]) => (
              <Field key={key} label={t(label)}>
                {(id) => (
                  <textarea
                    id={id}
                    className={key === "action" ? cn(textAreaClass, "min-h-10") : textAreaClass}
                    rows={key === "action" ? 1 : 2}
                    data-alternative-action={key === "action" ? "true" : undefined}
                    aria-invalid={key === "action" && index === 0 && !!alternativeErrorId}
                    aria-describedby={
                      key === "action" && index === 0 ? alternativeErrorId : undefined
                    }
                    value={option[key] ?? ""}
                    onChange={(e) =>
                      update({
                        ...body,
                        options: body.options.map((old, i) =>
                          i === index ? { ...old, [key]: e.target.value } : old,
                        ),
                      })
                    }
                  />
                )}
              </Field>
            ))}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                setOptionKeys((keys) => keys.filter((_, i) => i !== index));
                update({ ...body, options: body.options.filter((_, i) => i !== index) });
              }}
            >
              {t("remove")}
            </Button>
          </fieldset>
        ))}
        <Button
          type="button"
          variant="outline"
          data-add-alternative="true"
          aria-describedby={body.options.length === 0 ? alternativeErrorId : undefined}
          onClick={() => {
            setOptionKeys((keys) => [...keys, crypto.randomUUID()]);
            update({
              ...body,
              options: [
                ...body.options,
                {
                  action: "",
                  description: "",
                  advantages: null,
                  disadvantages: null,
                  notSelectedReason: null,
                },
              ],
            });
          }}
        >
          {t("add_alternative")}
        </Button>
      </div>
      <fieldset className="space-y-3">
        <legend className="mb-3 text-sm font-medium">{t("optional_principles")}</legend>
        {principles
          .filter((p) => p.currentRevisionId)
          .map((p) => (
            <label key={p.principleId} className="flex items-center gap-3 text-sm">
              <input
                className="accent-primary size-4"
                type="checkbox"
                checked={refs.includes(p.currentRevisionId!)}
                onChange={(e) =>
                  update({
                    ...body,
                    principleRevisionIds: e.target.checked
                      ? [...refs, p.currentRevisionId!]
                      : refs.filter((id) => id !== p.currentRevisionId),
                  })
                }
              />
              <span>
                {p.name} · {t("version", { number: p.currentRevisionNumber ?? 1 })}
              </span>
            </label>
          ))}
        {refs
          .filter((id) => !principles.some((p) => p.currentRevisionId === id))
          .map((id) => (
            <label key={id} className="flex items-center gap-3 text-sm">
              <input
                type="checkbox"
                className="accent-primary size-4"
                checked
                onChange={() =>
                  update({ ...body, principleRevisionIds: refs.filter((old) => old !== id) })
                }
              />
              <span>{t("saved_principle")}</span>
            </label>
          ))}
      </fieldset>
    </section>
  );
}
