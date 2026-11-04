import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Field, selectClass, textAreaClass } from "./controls";
import { ScenarioTagsField } from "./scenario-tags-field";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import type {
  PrincipleAssessment,
  ReviewBody,
  ReviewEvidence,
  ReviewFacts,
  ReviewPrinciple,
} from "./review-types";

const texts = [
  ["judgment", "review_judgment"],
  ["executionAssessment", "review_execution_assessment"],
  ["deviationReason", "deviation_reason"],
  ["otherExplanations", "review_explanations"],
  ["openQuestions", "review_open_questions"],
] as const;
const dispositions: Record<PrincipleAssessment["disposition"], JournalTextKey> = {
  KEEP: "review_principle_keep",
  REVISE: "review_principle_revise",
  RETIRE: "review_principle_retire",
  NEW: "review_principle_new",
  UNDECIDED: "review_principle_undecided",
};

export function ReviewFields({
  body,
  change,
  principles,
  tags,
  setTags,
  suggestions,
}: {
  body: ReviewBody;
  change: (body: ReviewBody) => void;
  principles: ReviewPrinciple[];
  tags: string;
  setTags: (tags: string) => void;
  suggestions: string[];
}) {
  const { t } = useJournalLocale();
  const evidence = (index: number, patch: Partial<ReviewEvidence>) =>
    change({
      ...body,
      outcomeEvidence: body.outcomeEvidence.map((row, i) =>
        i === index ? { ...row, ...patch } : row,
      ),
    });
  const assessment = (index: number, patch: Partial<PrincipleAssessment>) =>
    change({
      ...body,
      principles: body.principles.map((row, i) => (i === index ? { ...row, ...patch } : row)),
    });
  const versions = principles.flatMap((row) => [
    ...(row.currentRevisionId
      ? [
          {
            id: row.currentRevisionId,
            label: `${row.name} · ${t("version", { number: row.currentRevisionNumber ?? 1 })}`,
          },
        ]
      : []),
    ...(row.draftRevisionId
      ? [{ id: row.draftRevisionId, label: `${row.draftName ?? row.name} · ${t("draft")}` }]
      : []),
  ]);
  return (
    <div className="space-y-5">
      {texts.map(([key, label]) => (
        <Field key={key} label={t(label)}>
          {(id) => (
            <textarea
              id={id}
              className={textAreaClass}
              value={body[key] ?? ""}
              onChange={(event) => change({ ...body, [key]: event.target.value })}
            />
          )}
        </Field>
      ))}
      <section className="space-y-4 border-t pt-4">
        <h3 className="font-semibold">{t("review_outcome_evidence")}</h3>
        {body.outcomeEvidence.map((row, index) => (
          <fieldset key={index} className="space-y-3 border-t pt-3">
            <legend className="pr-2 text-sm">
              {t("review_outcome_evidence")} {index + 1}
            </legend>
            <Field label={t("information_kind")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={row.informationKind}
                  onChange={(event) =>
                    evidence(index, {
                      informationKind: event.target.value as ReviewEvidence["informationKind"],
                    })
                  }
                >
                  {(["FACT", "HYPOTHESIS", "UNKNOWN"] as const).map((kind) => (
                    <option value={kind} key={kind}>
                      {t(kind)}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label={t("review_evidence_content")}>
              {(id) => (
                <textarea
                  id={id}
                  className={textAreaClass}
                  value={row.content}
                  onChange={(event) => evidence(index, { content: event.target.value })}
                />
              )}
            </Field>
            <Field label={t("review_evidence_source")}>
              {(id) => (
                <Input
                  id={id}
                  value={row.source ?? ""}
                  onChange={(event) => evidence(index, { source: event.target.value || null })}
                />
              )}
            </Field>
            <Button
              type="button"
              variant="ghost"
              onClick={() =>
                change({
                  ...body,
                  outcomeEvidence: body.outcomeEvidence.filter((_, i) => i !== index),
                })
              }
            >
              {t("remove")}
            </Button>
          </fieldset>
        ))}
        <Button
          type="button"
          variant="outline"
          onClick={() =>
            change({
              ...body,
              outcomeEvidence: [
                ...body.outcomeEvidence,
                { informationKind: "UNKNOWN", content: "", source: null },
              ],
            })
          }
        >
          {t("review_add_evidence")}
        </Button>
      </section>
      <ScenarioTagsField tags={tags} setTags={setTags} suggestions={suggestions} />
      <section className="space-y-4 border-t pt-4">
        <h3 className="font-semibold">{t("review_principles")}</h3>
        {body.principles.map((row, index) => (
          <fieldset key={index} className="space-y-3 border-t pt-3">
            <legend className="pr-2 text-sm">
              {t("review_principles")} {index + 1}
            </legend>
            <Field label={t("review_disposition")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={row.disposition}
                  onChange={(event) =>
                    assessment(index, {
                      disposition: event.target.value as PrincipleAssessment["disposition"],
                    })
                  }
                >
                  {Object.entries(dispositions).map(([key, label]) => (
                    <option key={key} value={key}>
                      {t(label)}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            {(["principleRevisionId", "newPrincipleRevisionId"] as const).map((key) => (
              <Field
                key={key}
                label={t(
                  key === "principleRevisionId"
                    ? "review_original_principle"
                    : "review_proposed_principle",
                )}
              >
                {(id) => (
                  <select
                    id={id}
                    className={selectClass}
                    value={row[key] ?? ""}
                    onChange={(event) => assessment(index, { [key]: event.target.value || null })}
                  >
                    <option value="">{t("not_recorded")}</option>
                    {row[key] && !versions.some((v) => v.id === row[key]) && (
                      <option value={row[key]}>{row[key]}</option>
                    )}
                    {versions.map((version) => (
                      <option key={version.id} value={version.id}>
                        {version.label}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
            ))}
            <Field label={t("operation_reason")}>
              {(id) => (
                <Input
                  id={id}
                  value={row.reason ?? ""}
                  onChange={(event) => assessment(index, { reason: event.target.value || null })}
                />
              )}
            </Field>
            <Button
              type="button"
              variant="ghost"
              onClick={() =>
                change({ ...body, principles: body.principles.filter((_, i) => i !== index) })
              }
            >
              {t("remove")}
            </Button>
          </fieldset>
        ))}
        <Button
          type="button"
          variant="outline"
          onClick={() =>
            change({
              ...body,
              principles: [
                ...body.principles,
                {
                  principleRevisionId: null,
                  newPrincipleRevisionId: null,
                  disposition: "UNDECIDED",
                  reason: null,
                },
              ],
            })
          }
        >
          {t("review_add_principle")}
        </Button>
      </section>
      <Field label={t("content_origin")}>
        {(id) => (
          <select
            id={id}
            className={selectClass}
            value={body.contentOrigin}
            onChange={(event) =>
              change({ ...body, contentOrigin: event.target.value as ReviewBody["contentOrigin"] })
            }
          >
            {(["UNSPECIFIED", "USER_PROVIDED", "ASSISTED_DRAFT"] as const).map((origin) => (
              <option value={origin} key={origin}>
                {t(`origin_${origin}`)}
              </option>
            ))}
          </select>
        )}
      </Field>
    </div>
  );
}

export function ReviewBodyView({
  body,
  snapshots = [],
}: {
  body: ReviewBody;
  snapshots?: ReviewFacts["observations"]["principles"];
}) {
  const { t } = useJournalLocale();
  const principleLabel = (id: string) => {
    const pinned = snapshots.find((row) => row.id === id || row.revisionId === id);
    const name = pinned?.body?.name ?? pinned?.name;
    return name
      ? `${name}${pinned?.revisionNumber != null ? ` · ${t("version", { number: pinned.revisionNumber })}` : ""}${pinned?.statusAtCapture === "DRAFT" ? ` · ${t("draft")}` : ""}`
      : `${t("review_principle_unavailable")} · ${id}`;
  };
  return (
    <div className="space-y-4">
      {texts.map(([key, label]) => (
        <div key={key} className="space-y-1 text-sm">
          <h4 className="font-semibold">{t(label)}</h4>
          <p className="whitespace-pre-wrap [overflow-wrap:anywhere]">
            {body[key] || t("not_recorded")}
          </p>
        </div>
      ))}
      <section className="space-y-2 text-sm">
        <h4 className="font-semibold">{t("review_outcome_evidence")}</h4>
        {!body.outcomeEvidence.length && <p>{t("not_recorded")}</p>}
        {body.outcomeEvidence.map((row, i) => (
          <div key={i} className="space-y-1 border-t pt-2 [overflow-wrap:anywhere]">
            <p>{t(row.informationKind)}</p>
            <p className="whitespace-pre-wrap">{row.content}</p>
            {row.source && (
              <p>
                {t("review_evidence_source")} · {row.source}
              </p>
            )}
          </div>
        ))}
      </section>
      <p className="text-sm [overflow-wrap:anywhere]">
        {t("scenario_tags")} · {body.scenarioTags.join(" · ") || t("not_recorded")}
      </p>
      {body.principles.map((row, i) => (
        <div key={i} className="space-y-1 text-sm [overflow-wrap:anywhere]">
          <p>
            {t("review_principles")} · {t(dispositions[row.disposition])}
          </p>
          {row.principleRevisionId && (
            <p>
              {t("review_original_principle")} · {principleLabel(row.principleRevisionId)}
            </p>
          )}
          {row.newPrincipleRevisionId && (
            <p>
              {t("review_proposed_principle")} · {principleLabel(row.newPrincipleRevisionId)}
            </p>
          )}
          {row.reason && <p>{row.reason}</p>}
        </div>
      ))}
      <p className="text-sm">
        {t("content_origin")} · {t(`origin_${body.contentOrigin}`)}
      </p>
    </div>
  );
}
