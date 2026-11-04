import { useJournalLocale } from "./locale-context";
import type { PrincipleBody, PrincipleSource } from "./principle-types";
import { ReviewBodyView } from "./review-fields";
import { ReviewFactsView } from "./review-facts-view";

export function PrincipleBodyView({
  body,
  source,
}: {
  body: PrincipleBody;
  source?: PrincipleSource | null;
}) {
  const { t } = useJournalLocale();
  const review = source?.sourceReview;
  const reference = source?.sourceReference ?? body.sourceReference;
  return (
    <div className="space-y-5">
      <h3 className="text-lg font-semibold [overflow-wrap:anywhere]">
        {body.name || t("not_recorded")}
      </h3>
      {(
        [
          ["principle_rule", body.content],
          ["principle_conditions", body.applicableConditions],
          ["principle_exceptions", body.exceptions],
          ["principle_revision_reason", body.revisionReason],
        ] as const
      ).map(([label, value]) => (
        <div key={label} className="space-y-1 text-sm">
          <h4 className="font-semibold">{t(label)}</h4>
          <p className="whitespace-pre-wrap [overflow-wrap:anywhere]">
            {value || t("not_recorded")}
          </p>
        </div>
      ))}
      <p className="text-sm [overflow-wrap:anywhere]">
        {t("scenario_tags")} · {body.scenarioTags.join(" · ") || t("not_recorded")}
      </p>
      <p className="text-sm">
        {t("content_origin")} · {t(`origin_${body.contentOrigin}`)}
      </p>
      <section className="space-y-3 border-t pt-5">
        <h4 className="font-semibold">{t("principle_sources")}</h4>
        {reference && (
          <div className="space-y-1 text-sm">
            <p className="font-medium">{t("principle_source")}</p>
            <p className="whitespace-pre-wrap [overflow-wrap:anywhere]">{reference}</p>
          </div>
        )}
        {!reference && !review && !body.sourceReviewId && (
          <p className="text-sm">{t("not_recorded")}</p>
        )}
        {review ? (
          <details open className="space-y-3 rounded-lg border p-4">
            <summary className="cursor-pointer text-sm font-semibold">
              {t("principle_source_review")} ·{" "}
              {t("review_record_version", { number: review.revisionNumber })} ·{" "}
              {t(
                review.statusAtCapture === "DRAFT"
                  ? "draft"
                  : review.statusAtCapture === "SUBMITTED"
                    ? "review_submitted"
                    : "review_discarded",
              )}
            </summary>
            <p className="text-muted-foreground text-sm">{t("principle_source_status_hint")}</p>
            <p className="text-sm">
              {t("review_cutoff")} · {review.observationCutoff} · {review.observationTimezone}
            </p>
            <ReviewBodyView
              body={review.body}
              snapshots={review.factsSnapshot?.observations.principles}
            />
            {review.factsSnapshot && <ReviewFactsView facts={review.factsSnapshot} />}
          </details>
        ) : (
          body.sourceReviewId && (
            <p className="break-all text-sm">
              {t("principle_source_review")} · {body.sourceReviewId}
            </p>
          )
        )}
      </section>
    </div>
  );
}
