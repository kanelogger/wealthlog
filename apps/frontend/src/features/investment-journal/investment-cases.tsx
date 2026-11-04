import { useEffect, useRef, useState } from "react";
import { Field } from "./controls";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Button } from "@wealthfolio/ui";
import { ContextPanel } from "./context-panel";
import { DecisionBodyView } from "./decision-body-view";
import { useJournalLocale, type JournalTextKey } from "./locale-context";
import { useJournalQuery } from "./queries";
import { ReviewBodyView } from "./review-fields";
import { ReviewFactsView } from "./review-facts-view";
import type { ReviewBody, ReviewRecord } from "./review-types";
import type { DecisionBody, DecisionDetail } from "./types";

interface CaseBase {
  id: string;
  revisionNumber: number;
  scenarioTags: string[];
  principleRelations: {
    principleRevisionId: string | null;
    newPrincipleRevisionId?: string | null;
    relation: string;
  }[];
}
type InvestmentCase = CaseBase &
  (
    | {
        kind: "DECISION";
        decisionId: string;
        title: string;
        body: DecisionBody;
        confirmedAt: string;
        archived: boolean;
      }
    | {
        kind: "REVIEW";
        decisionTitle?: string | null;
        decisionRevisionNumber?: number | null;
        body: ReviewBody;
        observationCutoff: string;
        submittedAt: string;
        derivedPrincipleRevisions: string[];
      }
  );

const relationLabels: Record<string, JournalTextKey> = {
  REFERENCED: "case_referenced",
  KEEP: "review_principle_keep",
  REVISE: "review_principle_revise",
  RETIRE: "review_principle_retire",
  NEW: "review_principle_new",
  UNDECIDED: "review_principle_undecided",
};
const pageSize = 40;

function CaseDetail({
  item,
  back,
  principleRevisionId,
  principleLabel,
}: {
  item: InvestmentCase;
  back: () => void;
  principleRevisionId?: string;
  principleLabel?: string;
}) {
  const { t } = useJournalLocale();
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  const decision = useJournalQuery<DecisionDetail>(
    "get_investment_decision",
    { decisionId: item.kind === "DECISION" ? item.decisionId : "" },
    item.kind === "DECISION",
  );
  const review = useJournalQuery<ReviewRecord>(
    "get_investment_review",
    { reviewId: item.id },
    item.kind === "REVIEW",
  );
  const revision = decision.data?.revisions.find(
    (row) => row.id === item.id && row.status === "CONFIRMED",
  );
  const reviewed =
    review.data?.id === item.id && review.data.status === "SUBMITTED" ? review.data : undefined;
  const label = (id: string) => {
    const pinned =
      revision?.context?.response.funding.data.principleVersions?.find((row) => row.id === id) ??
      reviewed?.factsSnapshot?.observations.principles.find((row) => row.id === id);
    if (pinned?.body?.name && pinned.revisionNumber != null)
      return `${pinned.body.name} · ${t("version", { number: pinned.revisionNumber })}`;
    return id === principleRevisionId && principleLabel ? principleLabel : id;
  };
  const read = item.kind === "DECISION" ? decision : review;
  return (
    <section className="max-w-4xl space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <h2 ref={heading} tabIndex={-1} className="text-xl font-semibold [overflow-wrap:anywhere]">
          {item.kind === "DECISION" ? item.title : item.decisionTitle || t("case_review")} ·{" "}
          {t(item.kind === "DECISION" ? "review_decision_version" : "review_record_version", {
            number: item.revisionNumber,
          })}
        </h2>
        <Button variant="ghost" onClick={back}>
          {t("case_back")}
        </Button>
      </header>
      {item.kind === "REVIEW" && item.decisionRevisionNumber != null && (
        <p className="text-sm">
          {t("review_decision_version", { number: item.decisionRevisionNumber })}
        </p>
      )}
      <p className="text-sm">
        {item.kind === "DECISION"
          ? t("system_confirmed_at", { date: item.confirmedAt })
          : `${t("review_submitted")} · ${item.submittedAt}`}
      </p>
      <p className="text-sm [overflow-wrap:anywhere]">
        {t("scenario_tags")} · {item.scenarioTags.join(" · ") || t("not_recorded")}
      </p>
      <section className="space-y-2 border-y py-4 text-sm">
        <h3 className="font-semibold">{t("case_principle_relations")}</h3>
        {item.principleRelations.length === 0 &&
          !(item.kind === "REVIEW" && item.derivedPrincipleRevisions.length) && (
            <p>{t("case_no_relation")}</p>
          )}
        {item.principleRelations.map((relation, index) => (
          <p key={index} className="[overflow-wrap:anywhere]">
            {t(relationLabels[relation.relation] ?? "unknown")} ·{" "}
            {relation.principleRevisionId ? label(relation.principleRevisionId) : t("not_recorded")}
            {relation.newPrincipleRevisionId && ` → ${label(relation.newPrincipleRevisionId)}`}
          </p>
        ))}
        {item.kind === "REVIEW" &&
          item.derivedPrincipleRevisions.map((id) => (
            <p key={id} className="[overflow-wrap:anywhere]">
              {t("case_derived_principle")} · {label(id)}
            </p>
          ))}
      </section>
      {read.isSuccess && (item.kind === "DECISION" ? !revision : !reviewed) && (
        <p role="status" className="text-sm">
          {t("review_detail_unavailable")}
        </p>
      )}
      {read.isLoading && <p role="status">{t("loading")}</p>}
      {read.isError && (
        <div className="space-y-2">
          <p role="alert">{t("records_error")}</p>
          <Button variant="outline" onClick={() => void read.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {item.kind === "DECISION" ? (
        <>
          <DecisionBodyView body={item.body} capture={revision?.context ?? undefined} />
          {revision?.context && <ContextPanel capture={revision.context} frozen />}
        </>
      ) : (
        <>
          <p className="text-sm">
            {t("review_cutoff")} · {item.observationCutoff}
            {reviewed?.observationTimezone && ` · ${reviewed.observationTimezone}`}
          </p>
          <ReviewBodyView
            body={item.body}
            snapshots={reviewed?.factsSnapshot?.observations.principles}
          />
          {reviewed?.factsSnapshot && <ReviewFactsView facts={reviewed.factsSnapshot} />}
        </>
      )}
    </section>
  );
}

export function InvestmentCases({
  initialTags = [],
  principleRevisionId,
  principleLabel,
  clearPrinciple,
}: {
  initialTags?: string[];
  principleRevisionId?: string;
  principleLabel?: string;
  clearPrinciple: () => void;
}) {
  const { t } = useJournalLocale();
  const [tags, setTags] = useState(initialTags);
  const [tagInput, setTagInput] = useState("");
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<InvestmentCase | null>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const returning = useRef(false);
  useEffect(() => {
    if (!selected && returning.current) {
      returning.current = false;
      title.current?.focus();
    }
  }, [selected]);
  const suggestions = useJournalQuery<string[]>("list_investment_scenario_tags");
  const matchingTags = (suggestions.data ?? [])
    .filter(
      (tag) =>
        !tags.includes(tag) &&
        tag.toLocaleLowerCase().startsWith(tagInput.trim().toLocaleLowerCase()),
    )
    .slice(0, 8);
  function addTag(value: string) {
    const tag = value.trim();
    if (!tag || tags.includes(tag)) return;
    setTags((current) => [...current, tag]);
    setTagInput("");
    setPage(0);
  }
  const cases = useJournalQuery<InvestmentCase[]>(
    "search_investment_cases",
    {
      scenarioTags: tags,
      principleRevisionId: principleRevisionId ?? null,
      limit: pageSize,
      offset: page * pageSize,
    },
    !selected,
  );
  if (selected)
    return (
      <CaseDetail
        item={selected}
        principleRevisionId={principleRevisionId}
        principleLabel={principleLabel}
        back={() => {
          returning.current = true;
          setSelected(null);
        }}
      />
    );
  return (
    <section className="space-y-5">
      <h2 ref={title} tabIndex={-1} className="text-xl font-semibold">
        {t("case_library")}
      </h2>
      <p className="text-muted-foreground max-w-prose text-sm">{t("case_match_hint")}</p>
      <div className="max-w-xl space-y-3">
        {tags.length > 0 && (
          <ul className="flex flex-wrap gap-2">
            {tags.map((tag) => (
              <li key={tag} className="max-w-full">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  className="h-auto min-h-8 whitespace-normal [overflow-wrap:anywhere]"
                  aria-label={t("case_remove_tag", { tag })}
                  onClick={() => {
                    setTags((current) => current.filter((value) => value !== tag));
                    setPage(0);
                  }}
                >
                  {tag} <span aria-hidden="true">×</span>
                </Button>
              </li>
            ))}
          </ul>
        )}
        <form
          className="space-y-2"
          onSubmit={(event) => {
            event.preventDefault();
            addTag(tagInput);
          }}
        >
          <Field label={t("scenario_tags")} hint={t("case_tag_input_hint")}>
            {(id) => (
              <Input
                id={id}
                value={tagInput}
                onChange={(event) => setTagInput(event.target.value)}
              />
            )}
          </Field>
          <div className="flex flex-wrap gap-2">
            {matchingTags.map((tag) => (
              <Button
                key={tag}
                type="button"
                variant="outline"
                size="sm"
                className="h-auto min-h-8 max-w-full whitespace-normal [overflow-wrap:anywhere]"
                onClick={() => addTag(tag)}
              >
                {tag}
              </Button>
            ))}
            {tagInput.trim() &&
              !matchingTags.includes(tagInput.trim()) &&
              !tags.includes(tagInput.trim()) && (
                <Button type="submit" size="sm">
                  {t("case_add_tag")}
                </Button>
              )}
          </div>
        </form>
      </div>
      {principleRevisionId && (
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <p className="[overflow-wrap:anywhere]">
            {t("case_principle_filter")} · {principleLabel || principleRevisionId}
          </p>
          <Button
            variant="outline"
            onClick={() => {
              setPage(0);
              clearPrinciple();
            }}
          >
            {t("case_clear_principle")}
          </Button>
        </div>
      )}
      {cases.isLoading && <p role="status">{t("loading")}</p>}
      {cases.isError && (
        <div className="space-y-2">
          <p role="alert">{t("records_error")}</p>
          <Button variant="outline" onClick={() => void cases.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {cases.data?.length === 0 && (
        <p className="text-muted-foreground text-sm">{t("case_empty")}</p>
      )}
      <ul className="divide-y">
        {cases.data?.map((item) => (
          <li key={item.id} className="flex flex-wrap items-start justify-between gap-3 py-4">
            <div className="min-w-0 space-y-2 [overflow-wrap:anywhere]">
              <h3 className="font-semibold">
                {item.kind === "DECISION" ? item.title : item.decisionTitle || t("case_review")}
              </h3>
              <p className="text-sm">
                {t(item.kind === "DECISION" ? "review_decision_version" : "review_record_version", {
                  number: item.revisionNumber,
                })}{" "}
                · {item.kind === "DECISION" ? item.confirmedAt : item.observationCutoff}
                {item.kind === "REVIEW" &&
                  item.decisionRevisionNumber != null &&
                  ` · ${t("review_decision_version", { number: item.decisionRevisionNumber })}`}
                {item.kind === "DECISION" && item.archived && ` · ${t("archived")}`}
              </p>
              <p className="text-muted-foreground text-sm">
                {item.scenarioTags.join(" · ") || t("not_recorded")}
              </p>
            </div>
            <Button
              variant="outline"
              onClick={() => setSelected(item)}
              aria-label={t("case_open_named", {
                name:
                  (item.kind === "DECISION" ? item.title : item.decisionTitle) || t("case_review"),
                version: t(
                  item.kind === "DECISION" ? "review_decision_version" : "review_record_version",
                  { number: item.revisionNumber },
                ),
                date: item.kind === "DECISION" ? item.confirmedAt : item.observationCutoff,
              })}
            >
              {t("case_open")}
            </Button>
          </li>
        ))}
      </ul>
      {(page > 0 || cases.data?.length === pageSize) && (
        <div className="flex gap-2">
          <Button
            variant="outline"
            disabled={page === 0 || cases.isFetching}
            onClick={() => setPage(page - 1)}
          >
            {t("previous")}
          </Button>
          <Button
            variant="outline"
            disabled={cases.data?.length !== pageSize || cases.isFetching}
            onClick={() => setPage(page + 1)}
          >
            {t("next")}
          </Button>
        </div>
      )}
    </section>
  );
}
