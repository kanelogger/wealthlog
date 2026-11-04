import { useEffect, useRef, useState } from "react";
import { Button } from "@wealthfolio/ui";
import { executeInvestmentJournal } from "@/adapters";
import {
  ExactInput,
  ExactValue,
  Field,
  JournalErrorNotice,
  selectClass,
  textAreaClass,
} from "./controls";
import { resourceLabel } from "./context-labels";
import { useJournalLocale } from "./locale-context";
import { useJournalNavigationGuard } from "./navigation-guard";
import { useInvalidateJournal } from "./queries";
import { useJournalWrite } from "./use-journal-write";
import type { ContextCapture, Purpose, Resource, ResourceStatus } from "./types";
import { sameResource, type AllocationDelta, type AllocationPreview } from "./fund-types";
import { FundSources } from "./fund-sources";

export function ResourceBalances({ row }: { row: ResourceStatus }) {
  const { t } = useJournalLocale();
  return (
    <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
      {(["actual", "allocated", "unallocated", "excess", "negativeActual"] as const).map((key) => (
        <div key={key} className="flex flex-wrap justify-between gap-2">
          <dt className="text-muted-foreground">
            {t(key === "negativeActual" ? "liability" : key)}
          </dt>
          <dd>
            <ExactValue
              value={row.balance[key]}
              currency={
                row.observation.resource.kind === "CASH"
                  ? row.observation.resource.currency
                  : undefined
              }
            />
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function FundAllocationEditor({
  resource,
  capture,
  purposes,
  close,
  reconcileFirst = false,
}: {
  resource: Resource;
  capture: ContextCapture;
  purposes: Purpose[];
  close: () => void;
  reconcileFirst?: boolean;
}) {
  const { t } = useJournalLocale();
  type Operation = "ALLOCATE" | "REALLOCATE" | "RECONCILE";
  const [operationKind, setKind] = useState<Operation>(reconcileFirst ? "RECONCILE" : "ALLOCATE");
  const [reason, setReason] = useState("");
  const [lines, setLines] = useState<{ purposeId: string; delta: string }[]>([]);
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [preview, setPreview] = useState<{
    observed: AllocationPreview;
    entries: AllocationDelta[];
    operationKind: Operation;
    reason: string;
  } | null>(null);
  const write = useJournalWrite();
  const invalidate = useInvalidateJournal();
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    if (preview) heading.current?.focus();
  }, [preview]);
  useJournalNavigationGuard(dirty || loading, {
    pending: t("pending_navigation"),
    discard: t("discard_navigation"),
  });
  async function inspect() {
    setLoading(true);
    setError(null);
    try {
      const entries = lines.map((line) => ({ ...line, resource }));
      const observed = await executeInvestmentJournal<AllocationPreview>(
        "preview_fund_allocations",
        { resources: [resource] },
      );
      setPreview({ observed, entries, operationKind, reason });
    } catch (cause) {
      setError(cause);
    } finally {
      setLoading(false);
    }
  }
  const locked = loading || write.busy || write.uncertain;
  const sourcesReady =
    preview &&
    Array.isArray(preview.observed.sourceReferences) &&
    preview.observed.resources.every((row) => Array.isArray(row.observation.sources)) &&
    preview.observed.resources.some((row) => sameResource(row.observation.resource, resource));
  return (
    <div className="max-w-3xl space-y-6">
      <div className="space-y-2">
        <h2 ref={heading} tabIndex={-1} className="text-xl font-semibold outline-none">
          {t(preview ? "allocation_preview" : "change_ownership")}
        </h2>
        <p className="break-words font-medium">{resourceLabel(capture, resource, t)}</p>
        <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
          {t("ownership_only")}
        </p>
      </div>
      {preview ? (
        <div className="space-y-6">
          <p className="text-muted-foreground text-sm">
            {t("as_of", { date: preview.observed.asOfDate })}
          </p>
          {preview.observed.resources.map((row) => (
            <ResourceBalances key={JSON.stringify(row.observation.resource)} row={row} />
          ))}
          <FundSources
            preview={preview.observed}
            describeResource={(resource) => resourceLabel(capture, resource, t)}
          />
          <section className="space-y-3 border-t pt-5">
            <h3 className="font-semibold">{t("ownership_changes")}</h3>
            <p className="text-sm">{t(`operation_${preview.operationKind}`)}</p>
            {preview.entries.length === 0 ? (
              <p className="text-sm">{t("acknowledge_sources_only")}</p>
            ) : (
              <ul className="divide-y">
                {preview.entries.map((line, index) => (
                  <li key={index} className="flex flex-wrap justify-between gap-3 py-3 text-sm">
                    <span>
                      {purposes.find((p) => p.id === line.purposeId)?.name ?? line.purposeId}
                    </span>
                    <ExactValue
                      value={line.delta}
                      currency={resource.kind === "CASH" ? resource.currency : undefined}
                    />
                  </li>
                ))}
              </ul>
            )}
            <p className="whitespace-pre-wrap break-words text-sm">{preview.reason}</p>
            <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
              {t("allocation_validation_hint")}
            </p>
          </section>
          <div className="flex flex-wrap gap-3">
            <Button
              disabled={locked || !sourcesReady}
              onClick={() =>
                void write.run(
                  "apply_fund_allocation",
                  {
                    operationKind: preview.operationKind,
                    reason: preview.reason,
                    entries: preview.entries,
                    resources: [resource],
                    expectedPreviewHash: preview.observed.previewHash,
                    requestKey: crypto.randomUUID(),
                  },
                  () => {
                    setDirty(false);
                    void invalidate();
                    close();
                  },
                )
              }
            >
              {t("apply_ownership")}
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
        </div>
      ) : (
        <form
          className="space-y-5"
          onChange={() => setDirty(true)}
          onSubmit={(event) => {
            event.preventDefault();
            void inspect();
          }}
        >
          <fieldset disabled={locked} className="space-y-5">
            <Field label={t("ownership_operation")}>
              {(id) => (
                <select
                  id={id}
                  className={selectClass}
                  value={operationKind}
                  onChange={(e) => setKind(e.target.value as Operation)}
                >
                  <option value="ALLOCATE">{t("operation_ALLOCATE")}</option>
                  <option value="REALLOCATE">{t("operation_REALLOCATE")}</option>
                  <option value="RECONCILE">{t("operation_RECONCILE")}</option>
                </select>
              )}
            </Field>
            <p className="text-muted-foreground max-w-prose text-sm leading-relaxed">
              {t("signed_delta_hint")}
            </p>
            {lines.map((line, index) => (
              <div key={index} className="grid items-end gap-4 sm:grid-cols-[1fr_1fr_auto]">
                <Field label={t("purpose")}>
                  {(id) => (
                    <select
                      id={id}
                      required
                      className={selectClass}
                      value={line.purposeId}
                      onChange={(e) =>
                        setLines(
                          lines.map((row, i) =>
                            i === index ? { ...row, purposeId: e.target.value } : row,
                          ),
                        )
                      }
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
                <Field label={t(resource.kind === "CASH" ? "cash_delta" : "quantity_delta")}>
                  {(id) => (
                    <ExactInput
                      id={id}
                      required
                      pattern="-?([0-9]+(\.[0-9]*)?|\.[0-9]+)"
                      value={line.delta}
                      onChange={(e) =>
                        setLines(
                          lines.map((row, i) =>
                            i === index ? { ...row, delta: e.target.value } : row,
                          ),
                        )
                      }
                    />
                  )}
                </Field>
                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => {
                    setLines(lines.filter((_, i) => i !== index));
                    setDirty(true);
                  }}
                >
                  {t("remove")}
                </Button>
              </div>
            ))}
            <Button
              type="button"
              variant="outline"
              disabled={purposes.length === 0}
              onClick={() => {
                setLines([...lines, { purposeId: "", delta: "" }]);
                setDirty(true);
              }}
            >
              {t("add_ownership_line")}
            </Button>
            <Field label={t("operation_reason")}>
              {(id) => (
                <textarea
                  id={id}
                  required
                  className={textAreaClass}
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              )}
            </Field>
          </fieldset>
          <div className="flex flex-wrap gap-3">
            <Button disabled={locked || (lines.length === 0 && operationKind !== "RECONCILE")}>
              {t(loading ? "loading" : "preview_ownership")}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={locked}
              onClick={() => {
                if (!dirty || window.confirm(t("discard_navigation"))) close();
              }}
            >
              {t("cancel")}
            </Button>
          </div>
        </form>
      )}
      {(error || write.error) && (
        <JournalErrorNotice
          error={error ?? write.error}
          replay={write.uncertain ? () => void write.retry() : undefined}
          busy={write.busy}
        />
      )}
    </div>
  );
}
