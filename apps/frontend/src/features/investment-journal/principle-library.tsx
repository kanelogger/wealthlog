import { useEffect, useRef, useState } from "react";
import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Field, JournalErrorNotice, selectClass } from "./controls";
import { useJournalLocale } from "./locale-context";
import { allowJournalNavigation } from "./navigation-guard";
import { PrincipleBodyView } from "./principle-body-view";
import { PrincipleEditor } from "./principle-editor";
import type { PrincipleDetail, PrincipleRevision, PrincipleSummary } from "./principle-types";
import { useInvalidateJournal, useJournalQuery } from "./queries";
import { useJournalWrite } from "./use-journal-write";

const pageSize = 40;
export function PrincipleLibrary({
  initialSourceReview,
  clearSourceReview,
  openCases,
}: {
  initialSourceReview?: string;
  clearSourceReview: () => void;
  openCases?: (filter: {
    tags?: string[];
    principleRevisionId?: string;
    principleLabel?: string;
  }) => void;
}) {
  const { t } = useJournalLocale();
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<{
    detail?: PrincipleDetail;
    initial?: PrincipleRevision;
    source?: string;
  } | null>(initialSourceReview ? { source: initialSourceReview } : null);
  const [revisionId, setRevisionId] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [tags, setTags] = useState("");
  const [archived, setArchived] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const focus = useRef(false);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const locked = write.busy || write.uncertain;
  const list = useJournalQuery<PrincipleSummary[]>(
    "list_investment_principles",
    {
      includeArchived: archived,
      scenarioTags: tags
        .split(/[,，]/)
        .map((tag) => tag.trim())
        .filter(Boolean),
      limit: pageSize,
      offset: page * pageSize,
    },
    !selected && !editing,
  );
  const detail = useJournalQuery<PrincipleDetail>(
    "get_investment_principle",
    { principleId: selected ?? "" },
    !!selected && !editing,
  );
  useEffect(() => {
    if (!editing && focus.current) heading.current?.focus();
  }, [editing, selected]);
  function open(id: string | null) {
    if (!allowJournalNavigation()) return;
    focus.current = true;
    write.clearError();
    setSelected(id);
    setRevisionId(null);
  }
  if (editing)
    return (
      <PrincipleEditor
        detail={editing.detail}
        initial={editing.initial}
        sourceReviewId={editing.source}
        close={(id) => {
          focus.current = true;
          setEditing(null);
          setSelected(id ?? selected);
          setRevisionId(null);
          if (initialSourceReview) clearSourceReview();
        }}
      />
    );
  if (selected) {
    const root = detail.data;
    const current = root?.revisions.find((row) => row.id === root.currentRevisionId);
    const draft = root?.revisions.find((row) => row.id === root.draftRevisionId);
    const revision =
      root?.revisions.find((row) => row.id === revisionId) ??
      current ??
      draft ??
      root?.revisions.at(-1);
    const displayName = current?.body.name ?? draft?.body.name ?? root?.name;
    return (
      <section className="max-w-4xl space-y-5">
        <header className="flex flex-wrap items-start justify-between gap-3">
          <h2
            ref={heading}
            tabIndex={-1}
            className="text-xl font-semibold [overflow-wrap:anywhere]"
          >
            {displayName || t("principle_library")}
          </h2>
          <Button variant="ghost" disabled={locked} onClick={() => open(null)}>
            {t("principle_back")}
          </Button>
        </header>
        {detail.isLoading && <p role="status">{t("loading")}</p>}
        {detail.isError && (
          <div className="space-y-2">
            <p role="alert">{t("records_error")}</p>
            <Button variant="outline" onClick={() => void detail.refetch()}>
              {t("retry")}
            </Button>
          </div>
        )}
        {write.error && (
          <JournalErrorNotice
            error={write.error}
            busy={write.busy}
            replay={write.uncertain ? () => void write.retry() : undefined}
          />
        )}
        {root && (
          <>
            <p className="text-sm">
              {root.isArchived
                ? t("archived")
                : current
                  ? t("principle_current", { number: current.revisionNumber })
                  : t("principle_unpublished")}
              {draft && ` · ${t("principle_draft_version", { number: draft.revisionNumber })}`}
            </p>
            <div className="flex flex-wrap items-end gap-3">
              {revision && (
                <Field label={t("principle_version_picker")}>
                  {(id) => (
                    <select
                      id={id}
                      className={selectClass}
                      value={revision.id}
                      onChange={(event) => setRevisionId(event.target.value)}
                    >
                      {root.revisions.map((row) => (
                        <option key={row.id} value={row.id}>
                          {t("version", { number: row.revisionNumber })} ·{" "}
                          {t(
                            row.id === root.currentRevisionId
                              ? "principle_effective"
                              : row.status === "DRAFT"
                                ? "draft"
                                : row.status === "DISCARDED"
                                  ? "review_discarded"
                                  : "principle_historical",
                          )}
                        </option>
                      ))}
                    </select>
                  )}
                </Field>
              )}
              {!root.isArchived && (
                <Button
                  disabled={locked}
                  variant="outline"
                  onClick={() => {
                    if (allowJournalNavigation()) {
                      write.clearError();
                      setEditing({ detail: root, initial: draft ?? current });
                    }
                  }}
                >
                  {t(draft ? "continue_draft" : "principle_amend")}
                </Button>
              )}
              <Button
                variant="ghost"
                disabled={locked}
                onClick={() => {
                  if (!allowJournalNavigation()) return;
                  void write.run(
                    "archive_investment_principle",
                    {
                      principleId: root.principleId,
                      expectedVersion: root.version,
                      isArchived: !root.isArchived,
                      requestKey: crypto.randomUUID(),
                    },
                    () => void invalidate(),
                  );
                }}
              >
                {t(root.isArchived ? "restore" : "archive")}
              </Button>
            </div>
            {revision && (
              <div className="space-y-4 border-t pt-5">
                {revision.publishedAt && (
                  <p className="text-sm">
                    {t("principle_published")} · {revision.publishedAt} ·{" "}
                    {revision.publishedActorKind
                      ? t(revision.publishedActorKind)
                      : t("unavailable")}
                  </p>
                )}
                {openCases && (
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="outline"
                      onClick={() =>
                        openCases({
                          principleRevisionId: revision.id,
                          principleLabel: `${revision.body.name} · ${t("version", { number: revision.revisionNumber })}`,
                        })
                      }
                    >
                      {t("case_find_principle")}
                    </Button>
                    {revision.body.scenarioTags.length > 0 && (
                      <Button
                        variant="outline"
                        onClick={() => openCases({ tags: revision.body.scenarioTags })}
                      >
                        {t("case_find_tags")}
                      </Button>
                    )}
                  </div>
                )}
                <PrincipleBodyView body={revision.body} source={revision.sourceContext} />
              </div>
            )}
          </>
        )}
      </section>
    );
  }
  return (
    <section className="space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <h2 ref={heading} tabIndex={-1} className="text-xl font-semibold">
          {t("principle_library")}
        </h2>
        <Button
          disabled={locked}
          onClick={() => {
            if (allowJournalNavigation()) {
              write.clearError();
              setEditing({});
            }
          }}
        >
          {t("principle_new")}
        </Button>
      </header>
      <div className="max-w-xl space-y-3">
        <Field label={t("filter_tags")}>
          {(id) => (
            <Input
              id={id}
              value={tags}
              onChange={(event) => {
                setTags(event.target.value);
                setPage(0);
              }}
            />
          )}
        </Field>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={archived}
            onChange={(event) => {
              setArchived(event.target.checked);
              setPage(0);
            }}
          />
          {t("principle_include_archived")}
        </label>
      </div>
      {write.error && (
        <JournalErrorNotice
          error={write.error}
          busy={write.busy}
          replay={write.uncertain ? () => void write.retry() : undefined}
        />
      )}
      {list.isLoading && <p role="status">{t("loading")}</p>}
      {list.isError && (
        <div className="space-y-2">
          <p role="alert">{t("records_error")}</p>
          <Button variant="outline" onClick={() => void list.refetch()}>
            {t("retry")}
          </Button>
        </div>
      )}
      {list.data?.length === 0 && (
        <p className="text-muted-foreground text-sm">{t("principle_empty")}</p>
      )}
      <ul className="divide-y">
        {list.data?.map((row) => (
          <li key={row.principleId} className="flex flex-wrap justify-between gap-3 py-4">
            <div className="min-w-0 space-y-2 [overflow-wrap:anywhere]">
              <h3 className="font-semibold">{row.name || t("not_recorded")}</h3>
              <p className="text-sm">
                {row.isArchived
                  ? t("archived")
                  : row.currentRevisionNumber != null
                    ? t("principle_current", { number: row.currentRevisionNumber })
                    : t("principle_unpublished")}
              </p>
              {row.draftRevisionId && (
                <p className="text-muted-foreground text-sm">
                  {t("draft")} · {row.draftName ?? row.name}
                </p>
              )}
            </div>
            <Button variant="outline" disabled={locked} onClick={() => open(row.principleId)}>
              {t("principle_view")}
            </Button>
          </li>
        ))}
      </ul>
      {(page > 0 || list.data?.length === pageSize) && (
        <div className="flex gap-2">
          <Button
            variant="outline"
            disabled={page === 0 || list.isFetching}
            onClick={() => setPage(page - 1)}
          >
            {t("previous")}
          </Button>
          <Button
            variant="outline"
            disabled={list.data?.length !== pageSize || list.isFetching}
            onClick={() => setPage(page + 1)}
          >
            {t("next")}
          </Button>
        </div>
      )}
    </section>
  );
}
