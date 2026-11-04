import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { executeInvestmentJournal } from "@/adapters";
import { Field, JournalErrorNotice, selectClass, textAreaClass } from "./controls";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation, useJournalNavigationGuard } from "./navigation-guard";
import { PrincipleBodyView } from "./principle-body-view";
import {
  blankPrinciple,
  type PrincipleBody,
  type PrincipleDetail,
  type PrinciplePreview,
  type PrincipleRevision,
  type PrincipleSaveResult,
} from "./principle-types";
import { useInvalidateJournal, useJournalQuery } from "./queries";
import type { ReviewRecord } from "./review-types";
import { ScenarioTagsField } from "./scenario-tags-field";
import { useJournalWrite } from "./use-journal-write";

const textFields = [
  ["content", "principle_rule"],
  ["applicableConditions", "principle_conditions"],
  ["exceptions", "principle_exceptions"],
  ["revisionReason", "principle_revision_reason"],
  ["sourceReference", "principle_source"],
] as const;

export function PrincipleEditor({
  detail,
  initial,
  sourceReviewId,
  close,
}: {
  detail?: PrincipleDetail;
  initial?: PrincipleRevision;
  sourceReviewId?: string;
  close: (id?: string) => void;
}) {
  const { t } = useJournalLocale();
  const [body, setBody] = useState(() => initial?.body ?? blankPrinciple(sourceReviewId));
  const [tags, setTags] = useState(body.scenarioTags.join(", "));
  const [target, setTarget] = useState<PrincipleSaveResult | null>(() =>
    detail && initial?.id === detail.draftRevisionId
      ? {
          principleId: detail.principleId,
          draftRevisionId: initial.id,
          revisionNumber: initial.revisionNumber,
          version: detail.version,
        }
      : null,
  );
  const [dirty, setDirty] = useState(false);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [preview, setPreview] = useState<PrinciplePreview | null>(null);
  const [published, setPublished] = useState(false);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const heading = useRef<HTMLHeadingElement>(null);
  const stage = published ? "published" : preview ? "preview" : "edit";
  useEffect(() => heading.current?.focus(), [stage]);
  const locked = reading || write.busy || write.uncertain;
  useJournalNavigationGuard(dirty || reading, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  const sourceId = body.sourceReviewId?.trim() ?? "";
  const source = useJournalQuery<ReviewRecord>(
    "get_investment_review",
    { reviewId: sourceId },
    !!sourceId && !preview && !published,
  );
  const tagSuggestions = useJournalQuery<string[]>(
    "list_investment_scenario_tags",
    {},
    !preview && !published,
  );
  function edit(patch: Partial<PrincipleBody>) {
    setBody((current) => ({ ...current, ...patch }));
    setDirty(true);
  }
  function save(event: FormEvent) {
    event.preventDefault();
    if (locked || !body.name.trim()) return;
    setError(null);
    void write.run<PrincipleSaveResult>(
      "save_investment_principle_draft",
      {
        principleId: target?.principleId ?? detail?.principleId ?? null,
        expectedDraftRevisionId: target?.draftRevisionId ?? detail?.draftRevisionId ?? null,
        expectedVersion: target?.version ?? detail?.version ?? null,
        body: {
          ...body,
          sourceReviewId: sourceId || null,
          scenarioTags: tags
            .split(/[,，]/)
            .map((tag) => tag.trim())
            .filter(Boolean),
        },
        requestKey: crypto.randomUUID(),
      },
      (result) => {
        setTarget(result);
        setDirty(false);
        void invalidate();
      },
    );
  }
  const canPreview =
    !!target &&
    !dirty &&
    !!body.name.trim() &&
    !!body.content.trim() &&
    (!detail?.currentRevisionId || !!body.revisionReason?.trim());
  async function readPreview() {
    if (!target || locked || !canPreview) return;
    setReading(true);
    setError(null);
    write.clearError();
    const args = {
      principleId: target.principleId,
      draftRevisionId: target.draftRevisionId,
      expectedVersion: target.version,
    };
    try {
      const next = await executeInvestmentJournal<PrinciplePreview>(
        "preview_investment_principle_publication",
        args,
      );
      if (
        next.target.principleId !== args.principleId ||
        next.target.draftRevisionId !== args.draftRevisionId ||
        next.target.expectedVersion !== args.expectedVersion ||
        !next.expectedContentHash ||
        !next.expectedSourceContextHash ||
        !next.ledgerId ||
        !next.sourceContext
      )
        throw new Error(t("records_error"));
      setPreview(next);
    } catch (cause) {
      setError(cause);
    } finally {
      setReading(false);
    }
  }
  function publish() {
    if (!preview || locked || write.error) return;
    const { ledgerId, target, expectedContentHash, expectedSourceContextHash, sourceContext } =
      preview;
    void write.run(
      "publish_investment_principle",
      {
        ledgerId,
        target,
        expectedContentHash,
        expectedSourceContextHash,
        sourceContext,
        requestKey: crypto.randomUUID(),
      },
      () => {
        setPublished(true);
        void invalidate();
      },
    );
  }
  function discard() {
    if (!target || locked || !window.confirm(t("principle_discard_confirm"))) return;
    void write.run(
      "discard_investment_principle_draft",
      {
        target: {
          principleId: target.principleId,
          draftRevisionId: target.draftRevisionId,
          expectedVersion: target.version,
        },
        requestKey: crypto.randomUUID(),
      },
      () => {
        setDirty(false);
        void invalidate();
        close(target.principleId);
      },
    );
  }
  return (
    <section className="max-w-3xl space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <h2
          ref={heading}
          tabIndex={-1}
          role={published ? "status" : undefined}
          className="text-xl font-semibold"
        >
          {t(
            published
              ? "principle_published"
              : preview
                ? "principle_publication_preview"
                : "principle_editor",
          )}
          {target &&
            ` · ${t("version", { number: preview?.revisionNumber ?? target.revisionNumber })}`}
        </h2>
        <Button
          variant="ghost"
          disabled={locked}
          onClick={() => {
            if (allowJournalNavigation()) {
              write.clearError();
              close(target?.principleId);
            }
          }}
        >
          {t("principle_back")}
        </Button>
      </header>
      {published ? null : preview ? (
        <>
          <PrincipleBodyView body={preview.body} source={preview.sourceContext} />
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || !!write.error} onClick={publish}>
              {t("principle_publish")}
            </Button>
            <Button
              variant="outline"
              disabled={locked}
              onClick={() => {
                write.clearError();
                setPreview(null);
              }}
            >
              {t("return_edit")}
            </Button>
          </div>
        </>
      ) : (
        <form onSubmit={save} className="space-y-5">
          <p className="text-muted-foreground text-sm">{t("principle_draft_hint")}</p>
          <fieldset disabled={locked} className="space-y-4">
            <Field label={t("principle_name")}>
              {(id) => (
                <Input
                  id={id}
                  required
                  value={body.name}
                  onChange={(event) => edit({ name: event.target.value })}
                />
              )}
            </Field>
            {textFields.map(([key, label]) => (
              <Field key={key} label={t(label)}>
                {(id) => (
                  <textarea
                    id={id}
                    className={textAreaClass}
                    value={body[key] ?? ""}
                    onChange={(event) => edit({ [key]: event.target.value })}
                  />
                )}
              </Field>
            ))}
            <Field label={t("principle_source_review")} hint={t("principle_source_review_hint")}>
              {(id) => (
                <Input
                  id={id}
                  value={body.sourceReviewId ?? ""}
                  onChange={(event) => edit({ sourceReviewId: event.target.value })}
                />
              )}
            </Field>
            {source.isLoading && (
              <p role="status" className="text-sm">
                {t("loading")}
              </p>
            )}
            {source.isError && (
              <div className="space-y-2">
                <p role="alert" className="text-sm">
                  {t("records_error")}
                </p>
                <Button type="button" variant="outline" onClick={() => void source.refetch()}>
                  {t("retry")}
                </Button>
              </div>
            )}
            {source.data && (
              <p className="text-sm">
                {t("review_record_version", { number: source.data.revisionNumber })} ·{" "}
                {source.data.observationCutoff} ·{" "}
                {t(
                  source.data.status === "DRAFT"
                    ? "draft"
                    : source.data.status === "SUBMITTED"
                      ? "review_submitted"
                      : "review_discarded",
                )}
              </p>
            )}
            <ScenarioTagsField
              tags={tags}
              setTags={(value) => {
                setTags(value);
                setDirty(true);
              }}
              suggestions={tagSuggestions.data ?? []}
            />
            <Field label={t("content_origin")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={body.contentOrigin}
                  onChange={(event) =>
                    edit({ contentOrigin: event.target.value as PrincipleBody["contentOrigin"] })
                  }
                >
                  {(["UNSPECIFIED", "USER_PROVIDED", "ASSISTED_DRAFT"] as const).map((origin) => (
                    <option key={origin} value={origin}>
                      {t(`origin_${origin}`)}
                    </option>
                  ))}
                </select>
              )}
            </Field>
          </fieldset>
          {target && !dirty && (
            <p role="status" className="text-sm">
              {t("saved")}
            </p>
          )}
          {!canPreview && (
            <p className="text-muted-foreground text-sm">{t("principle_preview_hint")}</p>
          )}
          <div className="flex flex-wrap gap-3">
            <Button
              variant={canPreview ? "outline" : "default"}
              disabled={locked || !body.name.trim() || (!!target && !dirty)}
            >
              {t("save_draft")}
            </Button>
            <Button
              type="button"
              variant={canPreview ? "default" : "outline"}
              disabled={locked || !canPreview}
              onClick={() => void readPreview()}
            >
              {t("principle_publication_preview")}
            </Button>
            {target && (
              <Button type="button" variant="ghost" disabled={locked} onClick={discard}>
                {t("principle_discard")}
              </Button>
            )}
          </div>
        </form>
      )}
      {(Boolean(error) || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
    </section>
  );
}
