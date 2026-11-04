import { useState } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Field, JournalErrorNotice, selectClass } from "./controls";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { useJournalWrite } from "./use-journal-write";

const templates: Record<string, JournalTextKey> = {
  EMERGENCY: "template_emergency",
  LONG_TERM: "template_long_term",
  NEAR_TERM: "template_near_term",
};
export function PurposeIntroduction({ currency, done }: { currency: string; done: () => void }) {
  const { t } = useJournalLocale();
  const [category, setCategory] = useState("CUSTOM");
  const [name, setName] = useState("");
  const [selectedCurrency, setCode] = useState<string | null>(null);
  const code = selectedCurrency ?? currency;
  const [saved, setSaved] = useState(false);
  const write = useJournalWrite();
  return (
    <details className="max-w-lg rounded-xl border p-4">
      <summary className="focus-visible:ring-ring cursor-pointer rounded font-medium focus-visible:ring-2">
        {t("onboard_template")}
      </summary>
      <form
        className="mt-5 space-y-5"
        onSubmit={(event) => {
          event.preventDefault();
          if (saved) {
            done();
            return;
          }
          void write.run(
            "save_fund_purpose",
            { name, category, currency: code, requestKey: crypto.randomUUID() },
            () => {
              setSaved(true);
              done();
            },
          );
        }}
      >
        <fieldset disabled={write.busy || write.uncertain || saved} className="space-y-5">
          <Field label={t("purpose_template")}>
            {(id) => (
              <select
                id={id}
                className={selectClass}
                value={category}
                onChange={(event) => {
                  setCategory(event.target.value);
                  if (templates[event.target.value]) setName(t(templates[event.target.value]));
                }}
              >
                <option value="CUSTOM">{t("custom_purpose")}</option>
                {Object.entries(templates).map(([value, key]) => (
                  <option key={value} value={value}>
                    {t(key)}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t("purpose_name")}>
            {(id) => (
              <Input id={id} value={name} required onChange={(e) => setName(e.target.value)} />
            )}
          </Field>
          <Field label={t("currency")}>
            {(id) => (
              <Input
                id={id}
                value={code}
                required
                pattern="[A-Z]{3}"
                maxLength={3}
                onChange={(e) => setCode(e.target.value.toUpperCase())}
              />
            )}
          </Field>
        </fieldset>
        <p className="text-muted-foreground text-sm leading-relaxed">
          {t("no_automatic_allocation")}
        </p>
        {write.error && (
          <JournalErrorNotice
            error={write.error}
            replay={write.uncertain ? () => void write.retry() : undefined}
            busy={write.busy}
          />
        )}
        <Button type="submit" disabled={write.busy || write.uncertain}>
          {t("save_purpose")}
        </Button>
      </form>
    </details>
  );
}
