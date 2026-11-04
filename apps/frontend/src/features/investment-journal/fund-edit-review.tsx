import { ExactValue } from "./controls";
import { resourceLabel } from "./context-labels";
import { ResourceBalances } from "./fund-allocation-editor";
import type { FundReversalPreview } from "./fund-types";
import { useJournalLocale } from "./locale-context";
import type { ContextCapture, Purpose } from "./types";

export function FundEditReview({
  preview,
  capture,
  purposes,
  correction = false,
}: {
  preview: Pick<FundReversalPreview, "funding" | "entries" | "balancesAfter">;
  capture: ContextCapture;
  purposes: Purpose[];
  correction?: boolean;
}) {
  const { t } = useJournalLocale();
  return (
    <>
      {preview.funding.resources.map((row) => (
        <section key={JSON.stringify(row.observation.resource)} className="space-y-3 border-t pt-5">
          <h3 className="font-semibold [overflow-wrap:anywhere]">
            {resourceLabel(capture, row.observation.resource, t)}
          </h3>
          <ResourceBalances row={row} />
        </section>
      ))}
      <section className="space-y-3 border-t pt-5">
        <h3 className="font-semibold">{t("ownership_changes")}</h3>
        <ul className="space-y-3">
          {preview.entries.map((entry, index) => (
            <li key={index} className="flex flex-wrap justify-between gap-3 text-sm">
              <span className="min-w-0 [overflow-wrap:anywhere]">
                {purposes.find((p) => p.id === entry.purposeId)?.name ?? entry.purposeId} ·{" "}
                {resourceLabel(capture, entry.resource, t)}
              </span>
              <ExactValue
                value={entry.delta}
                currency={entry.resource.kind === "CASH" ? entry.resource.currency : undefined}
              />
            </li>
          ))}
        </ul>
      </section>
      <section className="space-y-3 border-t pt-5">
        <h3 className="font-semibold">
          {t(correction ? "correction_ownership_after" : "ownership_after")}
        </h3>
        <ul className="space-y-3">
          {preview.balancesAfter.map((row, index) => (
            <li key={index} className="flex flex-wrap justify-between gap-3 text-sm">
              <span className="min-w-0 [overflow-wrap:anywhere]">
                {purposes.find((p) => p.id === row.purposeId)?.name ?? row.purposeId} ·{" "}
                {resourceLabel(capture, row.resource, t)}
              </span>
              <ExactValue
                value={row.amount}
                currency={row.resource.kind === "CASH" ? row.resource.currency : undefined}
              />
            </li>
          ))}
        </ul>
      </section>
    </>
  );
}
