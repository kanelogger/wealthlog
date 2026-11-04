import type { AllocationPreview } from "./fund-types";
import type { Resource } from "./types";
import { useJournalLocale, type JournalTextKey } from "./locale-context";

const stateLabels: Record<string, JournalTextKey> = {
  VALID: "source_valid",
  INVALID: "source_invalid",
  DELETED: "source_deleted",
  UNAVAILABLE: "unavailable",
};
const effectLabels: Record<string, JournalTextKey> = {
  ORDINARY: "source_ordinary",
  TRADE: "source_trade",
  CASH_TRANSFER: "operation_TRANSFER",
  SECURITY_TRANSFER: "source_security_transfer",
  CORPORATE_ACTION: "fund_issue_corporate",
  CALIBRATION: "fund_issue_calibration",
  NONE: "source_no_effect",
  UNKNOWN: "unknown",
};
const kindLabels: Record<string, JournalTextKey> = {
  ACTIVITY: "source_activity",
  MANUAL_TRADE: "source_manual_trade",
  SNAPSHOT: "source_snapshot",
  TRANSFER: "operation_TRANSFER",
};
export function FundSources({
  preview,
  describeResource,
  trade = false,
}: {
  preview: AllocationPreview;
  describeResource: (resource: Resource) => string;
  trade?: boolean;
}) {
  const { t } = useJournalLocale();
  const complete =
    Array.isArray(preview.sourceReferences) &&
    preview.resources.every((row) => Array.isArray(row.observation.sources));
  return (
    <section className="space-y-4 border-t pt-5">
      <h3 className="font-semibold">
        {t(trade ? "trade_observed_sources" : "sources_to_acknowledge")}
      </h3>
      <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
        {t(trade ? "trade_observed_sources_hint" : "sources_acknowledgement_hint")}
      </p>
      {!complete && (
        <p role="alert" className="text-sm">
          {t("source_list_unavailable")}
        </p>
      )}
      {preview.resources.map((row) => (
        <div key={JSON.stringify(row.observation.resource)} className="space-y-3">
          <h4 className="text-sm font-medium [overflow-wrap:anywhere]">
            {describeResource(row.observation.resource)}
          </h4>
          {row.observation.sources?.length === 0 && (
            <p className="text-muted-foreground text-sm">{t("no_accounting_sources")}</p>
          )}
          <ul className="divide-y">
            {row.observation.sources?.map((source) => (
              <li key={`${source.kind}:${source.id}`} className="space-y-2 py-3 text-sm">
                <p className="font-medium">{t(kindLabels[source.kind] ?? "source_record")}</p>
                <p className="break-all">
                  {t("source_record")}: {source.id}
                </p>
                <p>
                  {t(source.applied ? "source_applied" : "source_not_applied")} ·{" "}
                  {t(effectLabels[source.effect] ?? "issue_unknown")}
                </p>
                <p className="text-muted-foreground break-all text-xs">
                  {t("source_current_version")}: {source.version}
                </p>
              </li>
            ))}
          </ul>
        </div>
      ))}
      <details open>
        <summary className="focus-visible:ring-ring cursor-pointer rounded py-2 text-sm font-medium focus-visible:ring-2">
          {t("previously_used_sources", { count: preview.sourceReferences?.length ?? 0 })}
        </summary>
        {preview.sourceReferences?.length === 0 && (
          <p className="text-muted-foreground py-2 text-sm">{t("no_used_sources")}</p>
        )}
        <ul className="divide-y">
          {preview.sourceReferences?.map((source, index) => (
            <li key={index} className="space-y-2 py-3 text-sm">
              <p className="font-medium">{t(kindLabels[source.sourceKind] ?? "source_record")}</p>
              <p className="font-medium [overflow-wrap:anywhere]">
                {describeResource(source.resource)} ·{" "}
                {t(stateLabels[source.state] ?? "issue_unknown")}
              </p>
              <p className="break-all">
                {t("source_record")}: {source.sourceId}
              </p>
              {source.sourceCounterpartyId && (
                <p className="break-all">
                  {t("source_counterparty")}: {source.sourceCounterpartyId}
                </p>
              )}
              <p className="text-muted-foreground break-all text-xs">
                {t("source_original_version")}: {source.originVersion ?? t("not_recorded")}
              </p>
              <p className="text-muted-foreground break-all text-xs">
                {t("source_current_version")}: {source.currentVersion ?? t("not_recorded")}
              </p>
            </li>
          ))}
        </ul>
      </details>
    </section>
  );
}
