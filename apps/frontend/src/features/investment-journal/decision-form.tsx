import { hasOptionContent } from "./types";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import type { Account, Asset } from "@/lib/types";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { useJournalLocale } from "./locale-context";
import { ExactInput, Field, selectClass, textAreaClass } from "./controls";
import type { Action, DecisionBody, Origin, Purpose, PrincipleSummary } from "./types";
import { DecisionFundingFields } from "./decision-funding-fields";
import { DecisionEvidenceFields } from "./decision-evidence-fields";

export type CheckSchedule =
  | { operation: "KEEP" }
  | { operation: "SET"; date: string }
  | { operation: "CANCEL" };
export function DecisionForm({
  initial,
  nextCheckAt,
  busy,
  saved = false,
  locked,
  accounts,
  assets,
  purposes,
  purposesReady = true,
  principles,
  tags,
  save,
  changed,
}: {
  initial: DecisionBody;
  nextCheckAt: string | null;
  busy: boolean;
  saved?: boolean;
  locked: boolean;
  accounts: Account[];
  assets: Asset[];
  purposes: Purpose[];
  purposesReady?: boolean;
  principles: PrincipleSummary[];
  tags: string[];
  save: (body: DecisionBody, schedule: CheckSchedule, preview: boolean) => void;
  changed: (body: DecisionBody) => void;
}) {
  const { t } = useJournalLocale();
  const form = useRef<HTMLFormElement>(null);
  const errorId = useId();
  const { isBalanceHidden } = useBalancePrivacy();
  const [body, setBody] = useState(initial);
  const [checkDate, setCheckDate] = useState(nextCheckAt ?? "");
  const [schedule, setSchedule] = useState<CheckSchedule>({ operation: "KEEP" });
  const [tagText, setTagText] = useState(initial.scenarioTags.join(", "));
  const [validation, setValidation] = useState<
    "required_action" | "required_thesis" | "required_alternative" | null
  >(null);
  useEffect(() => {
    if (!validation) return;
    const selector =
      validation === "required_action"
        ? '[name="action"]'
        : validation === "required_thesis"
          ? '[name="thesis"]'
          : "[data-alternative-action], [data-add-alternative]";
    form.current?.querySelector<HTMLElement>(selector)?.focus();
  }, [validation]);
  function update(next: DecisionBody) {
    setBody(next);
    setValidation(null);
    changed(next);
  }
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const submitter = (event.nativeEvent as SubmitEvent).submitter;
    const prepare = submitter instanceof HTMLButtonElement && submitter.value === "preview";
    const next = {
      ...body,
      // Completely untouched placeholder rows do not create synthetic evidence.
      options: body.options.filter(hasOptionContent),
    };
    if (prepare) {
      if (!next.action) {
        setValidation("required_action");
        return;
      }
      if (!next.thesis.trim()) {
        setValidation("required_thesis");
        return;
      }
      if (!next.options.some((option) => option.action.trim())) {
        setValidation("required_alternative");
        return;
      }
    }
    save(next, schedule, prepare);
  }
  return (
    <form
      ref={form}
      onSubmit={submit}
      className="min-w-0 space-y-7"
      aria-describedby={validation ? errorId : undefined}
    >
      <fieldset disabled={busy || locked} className="min-w-0 space-y-7 disabled:opacity-70">
        <section className="space-y-5">
          <h2 className="text-lg font-semibold">{t("part_action")}</h2>
          <Field label={t("title")}>
            {(id) => (
              <Input
                id={id}
                value={body.title}
                placeholder={t("title_placeholder")}
                onChange={(e) => update({ ...body, title: e.target.value })}
              />
            )}
          </Field>
          <div className="grid gap-5 sm:grid-cols-2">
            <Field label={t("action")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={body.action ?? ""}
                  name="action"
                  aria-invalid={validation === "required_action"}
                  aria-describedby={validation === "required_action" ? errorId : undefined}
                  onChange={(e) =>
                    update({ ...body, action: e.target.value ? (e.target.value as Action) : null })
                  }
                >
                  <option value="">{t("choose_action")}</option>
                  {(["BUY", "SELL", "HOLD", "WAIT", "RESEARCH", "OTHER"] as const).map((action) => (
                    <option key={action} value={action}>
                      {t(`action_${action}`)}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label={t("asset")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={body.assetId ?? ""}
                  onChange={(e) => update({ ...body, assetId: e.target.value || null })}
                >
                  <option value="">{t("no_asset")}</option>
                  {body.assetId && !assets.some((asset) => asset.id === body.assetId) && (
                    <option value={body.assetId}>{t("saved_asset")}</option>
                  )}
                  {assets.map((asset) => (
                    <option key={asset.id} value={asset.id}>
                      {asset.name ?? asset.displayCode ?? asset.id}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label={t("planned_amount")}>
              {(id) => (
                <ExactInput
                  id={id}
                  value={body.plannedAmount ?? ""}
                  pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                  onChange={(e) => update({ ...body, plannedAmount: e.target.value || null })}
                />
              )}
            </Field>
            <Field label={t("currency")}>
              {(id) => (
                <Input
                  id={id}
                  value={body.currency ?? ""}
                  maxLength={3}
                  pattern="[A-Z]{3}"
                  required={body.plannedAmount !== null}
                  onChange={(e) =>
                    update({ ...body, currency: e.target.value.toUpperCase() || null })
                  }
                />
              )}
            </Field>
            <Field label={t("planned_quantity")}>
              {(id) => (
                <ExactInput
                  id={id}
                  value={body.plannedQuantity ?? ""}
                  pattern="([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                  onChange={(e) => update({ ...body, plannedQuantity: e.target.value || null })}
                />
              )}
            </Field>
          </div>
          {isBalanceHidden && (
            <p className="text-muted-foreground text-sm">{t("no_values_edit")}</p>
          )}
          <Field label={t("thesis")}>
            {(id) => (
              <textarea
                id={id}
                className={textAreaClass}
                value={body.thesis}
                name="thesis"
                aria-invalid={validation === "required_thesis"}
                aria-describedby={validation === "required_thesis" ? errorId : undefined}
                onChange={(e) => update({ ...body, thesis: e.target.value })}
              />
            )}
          </Field>
          <Field label={t("accepted_disadvantages")}>
            {(id) => (
              <textarea
                id={id}
                className={textAreaClass}
                value={body.acceptedDisadvantages ?? ""}
                onChange={(e) => update({ ...body, acceptedDisadvantages: e.target.value || null })}
              />
            )}
          </Field>
        </section>
        <DecisionFundingFields
          body={body}
          update={update}
          purposes={purposes}
          purposesReady={purposesReady}
          accounts={accounts}
          assets={assets}
        />
        <DecisionEvidenceFields
          body={body}
          update={update}
          purposes={purposes}
          principles={principles}
          alternativeErrorId={validation === "required_alternative" ? errorId : undefined}
        />
        <section className="space-y-5 border-t pt-7">
          <h2 className="text-lg font-semibold">{t("part_follow_up")}</h2>
          {body.action === "RESEARCH" && (
            <p className="text-muted-foreground text-sm leading-relaxed">{t("research_hint")}</p>
          )}
          <Field label={t("information_value")}>
            {(id) => (
              <textarea
                id={id}
                className={textAreaClass}
                value={body.informationValue ?? ""}
                onChange={(e) => update({ ...body, informationValue: e.target.value || null })}
              />
            )}
          </Field>
          <Field label={t("waiting_cost")}>
            {(id) => (
              <textarea
                id={id}
                className={textAreaClass}
                value={body.waitingCost ?? ""}
                onChange={(e) => update({ ...body, waitingCost: e.target.value || null })}
              />
            )}
          </Field>
          <Field label={t("next_check")} hint={t("check_hint")}>
            {(id) => (
              <Input
                id={id}
                type="date"
                value={checkDate}
                onChange={(e) => {
                  const date = e.target.value;
                  setCheckDate(date);
                  setSchedule(date ? { operation: "SET", date } : { operation: "CANCEL" });
                  changed(body);
                }}
              />
            )}
          </Field>
          <Field label={t("declared_at")} hint={t("declared_hint")}>
            {(id) => (
              <Input
                id={id}
                value={body.declaredAt ?? ""}
                onChange={(e) => update({ ...body, declaredAt: e.target.value || null })}
              />
            )}
          </Field>
          <Field label={t("scenario_tags")} hint={t("scenario_tags_hint")}>
            {(id) => (
              <>
                <Input
                  id={id}
                  list={`${id}-tags`}
                  value={tagText}
                  onChange={(e) => {
                    setTagText(e.target.value);
                    update({
                      ...body,
                      scenarioTags: e.target.value
                        .split(/[,，]/)
                        .map((tag) => tag.trim())
                        .filter(Boolean),
                    });
                  }}
                />
                <datalist id={`${id}-tags`}>
                  {tags.map((tag) => (
                    <option key={tag} value={tag} />
                  ))}
                </datalist>
              </>
            )}
          </Field>
          <Field label={t("content_origin")} hint={t("origin_hint")}>
            {(id) => (
              <select
                id={id}
                className={selectClass}
                value={body.contentOrigin}
                onChange={(e) => update({ ...body, contentOrigin: e.target.value as Origin })}
              >
                {(["UNSPECIFIED", "USER_PROVIDED", "ASSISTED_DRAFT"] as const).map((origin) => (
                  <option key={origin} value={origin}>
                    {t(`origin_${origin}`)}
                  </option>
                ))}
              </select>
            )}
          </Field>
        </section>
      </fieldset>
      {validation && (
        <p id={errorId} role="alert" className="text-destructive text-sm">
          {t(validation)}
        </p>
      )}
      <div className="bg-background sticky bottom-0 z-10 flex flex-wrap justify-end gap-3 border-t py-4">
        {saved && (
          <p role="status" className="text-muted-foreground mr-auto self-center text-sm">
            {t("saved")}
          </p>
        )}
        <Button
          type="submit"
          name="intent"
          value="save"
          variant="outline"
          disabled={busy || locked}
        >
          {t("save_draft")}
        </Button>
        <Button type="submit" name="intent" value="preview" disabled={busy || locked}>
          {t("save_preview")}
        </Button>
      </div>
    </form>
  );
}
