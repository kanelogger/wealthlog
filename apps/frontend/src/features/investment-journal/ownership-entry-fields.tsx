import { blankLine, resourceIdentity, type CorrectionLine } from "./ownership-entry-model";
import { Button } from "@wealthfolio/ui";
import { ExactInput, Field, selectClass } from "./controls";
import { resourceLabel } from "./context-labels";
import { useJournalLocale } from "./locale-context";
import type { ContextCapture, Purpose, Resource } from "./types";

export function OwnershipEntryFields({
  lines,
  setLines,
  resources,
  purposes,
  capture,
}: {
  lines: CorrectionLine[];
  setLines: (lines: CorrectionLine[]) => void;
  resources: Resource[];
  purposes: Purpose[];
  capture: ContextCapture;
}) {
  const { t } = useJournalLocale();
  const updateLine = (index: number, patch: Partial<CorrectionLine>) =>
    setLines(lines.map((line, i) => (i === index ? { ...line, ...patch } : line)));
  return (
    <>
      {lines.map((line, index) => (
        <fieldset key={index} className="grid gap-3 border-t pt-4 sm:grid-cols-2">
          <legend className="pr-2 text-sm">
            {t("correction_entry")} {index + 1}
          </legend>
          <Field label={t("purpose")}>
            {(id) => (
              <select
                required
                id={id}
                className={selectClass}
                value={line.purposeId}
                onChange={(event) => updateLine(index, { purposeId: event.target.value })}
              >
                <option value="">{t("choose")}</option>
                {purposes.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                    {p.isArchived ? ` · ${t("archived")}` : ""}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t("correction_resource")}>
            {(id) => (
              <select
                required
                id={id}
                className={selectClass}
                value={line.resourceKey}
                onChange={(event) =>
                  updateLine(index, { resourceKey: event.target.value, delta: "" })
                }
              >
                <option value="">{t("choose")}</option>
                {resources.map((resource) => (
                  <option key={resourceIdentity(resource)} value={resourceIdentity(resource)}>
                    {resourceLabel(capture, resource, t)}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t("correction_delta")}>
            {(id) => (
              <ExactInput
                required
                id={id}
                pattern={"-?([0-9]+(\\.[0-9]*)?|\\.[0-9]+)"}
                value={line.delta}
                onChange={(event) => updateLine(index, { delta: event.target.value })}
              />
            )}
          </Field>
          <Button
            type="button"
            variant="ghost"
            className="self-end justify-self-start"
            disabled={lines.length === 1}
            onClick={() => setLines(lines.filter((_, i) => i !== index))}
          >
            {t("remove")}
          </Button>
        </fieldset>
      ))}
      <Button type="button" variant="outline" onClick={() => setLines([...lines, blankLine])}>
        {t("add_correction_entry")}
      </Button>
    </>
  );
}
